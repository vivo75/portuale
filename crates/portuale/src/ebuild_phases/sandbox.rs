//! The phase shell backend, the calling-environment filter, and sandbox/isolation.

use crate::ebuild_phases::*;

/// Which shell executes a phase, and every `bin/*.sh` sourced along with it.
///
/// `Bash` (**the default**) is a genuine `bash <bin_dir>/ebuild.sh <phase>`
/// subprocess — Portage's `_doebuild_spawn` shape (`EBUILD_SH_ARGS="$*"` picks up
/// `<phase>`, and the tail really calls `__ebuild_main ${EBUILD_SH_ARGS}` and exits).
///
/// `Brush` is an embedded `brush_core::Shell` (see `Cargo.toml` for the pinned commit).
/// It never sets `EBUILD_SH_ARGS` — a bare `exit` inside an embedded shell would kill
/// the host process — so it uses "source, then separately `invoke_function`" instead.
///
/// `Bash` is the default because brush's `declare -f` serializer corrupts function
/// bodies containing redirected here-documents; `__save_ebuild_env` runs `declare -f`
/// between phases, so essentially every compiled package broke. `Brush` stays via
/// `--shell brush` (a portuale-only flag, not in `ebuild_options::OPTIONS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellBackend {
    #[default]
    Bash,
    Brush,
}

/// The `src_*` phases sandboxing applies to. Portage sandboxes every phase outside
/// `_unsandboxed_phases` and unshares every phase outside `_ipc_phases`; for the
/// phases run here both come out to this list. `nofetch` is excluded (its own
/// private tmpdir, neither unshared nor wrapped).
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) const SANDBOXED_SRC_PHASES: &str = include_str!("sandboxed_src_phases.txt");

/// The calling-env vars Portage's `config.environ()` carries into a phase (plus
/// `CCACHE_*`/`DISTCC_*`). Phases spawn with the inherited env plus `phase_env_vars`
/// overrides; without this filter CLI-only vars (e.g. `EMERGE_DEFAULT_OPTS`) would
/// leak into the regenerated vdb `environment`.
/// Portage: special_env_vars.py. One name per line.
pub(crate) const ENVIRON_WHITELIST: &str = include_str!("environ_whitelist.txt");

/// Whether `name` is a line of the one-name-per-line `table`.
pub(crate) fn listed(table: &str, name: &str) -> bool {
    table.lines().any(|line| line == name)
}

/// Whether `key` survives Portage's `config.environ()` calling-env filter.
pub(crate) fn environ_whitelisted(key: &str) -> bool {
    listed(ENVIRON_WHITELIST, key) || key.starts_with("CCACHE_") || key.starts_with("DISTCC_")
}

/// Settings the native `gpkg-helper compress` reads from its env. The native helper
/// has no config loader, so `invoke_dyn_package` exports the resolved values and they
/// must survive the calling-env filter in the `package` phase — the one phase running
/// `__dyn_package`. They never reach any other phase.
/// Portage: gpkg-helper.py
pub(crate) fn gpkg_helper_setting(phase: &str, key: &str) -> bool {
    phase == "package"
        && (key == "BINPKG_COMPRESS"
            || key == "BINPKG_COMPRESS_FLAGS"
            || key.starts_with("BINPKG_COMPRESS_FLAGS_")
            || key.starts_with("BINPKG_GPG_SIGNING_"))
}

/// Portage's `filter_calling_env`: once `${T}/environment` exists, only whitelisted
/// calling-env keys pass; the saved environment supplies the rest, so an ebuild's
/// `unset` sticks. Exempts the init/fetch phases and standalone-`ebuild` `setup`.
/// Portage: config.py
pub(crate) fn filter_calling_env(
    phase: &str,
    t_dir: &Path,
    extra_env: &[(String, String)],
) -> bool {
    if matches!(phase, "clean" | "cleanrm" | "depend" | "fetch") {
        return false;
    }
    // `EMERGE_FROM` rides in `extra_env` for the binary/saved-env paths.
    let emerge_from = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "EMERGE_FROM")
        .map(|(_, v)| v.as_str())
        .unwrap_or("ebuild");
    if phase == "setup" && emerge_from == "ebuild" {
        return false;
    }
    t_dir.join("environment").is_file()
}

/// The `FEATURES` string phase decisions consult: the last `extra_env` pair when
/// present (the resolved list), else the process env (standalone runs).
/// Portage: config.py, settings.features
pub(crate) fn features_string(extra_env: &[(String, String)]) -> String {
    extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("FEATURES").unwrap_or_default())
}

/// A `FEATURES` token check over `features_string`'s output. CLI fallbacks elsewhere
/// keep their own process-env reads.
pub(crate) fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
}

/// `FEATURES` for the phase's bash: passed straight through so unmodified bash gating
/// on `contains_word <token> "${FEATURES}"` works (estrip handling, `__dyn_test`'s test
/// gate, `misc-functions.sh` handling). Tokens Rust acts on independently are never
/// re-gated by any bash run here (verified by grep), so no double-handling.
/// Portage: doebuild.py, doebuild_environment
pub(crate) fn phase_features_value() -> String {
    std::env::var("FEATURES").unwrap_or_default()
}

/// `FEATURES=network-sandbox` present?
pub(crate) fn network_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "network-sandbox")
}

/// The non-`FEATURES` half of the net-unshare decision (pure, so unit-testable):
/// unpack of a `live` package, `test` of a `test_network` package, or
/// `RESTRICT=network-sandbox`. Inputs are already USE-reduced token strings.
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) fn network_sandbox_exempt(phase: &str, restrict: &str, properties: &str) -> bool {
    (phase == "unpack" && flat_field_has_token(properties, &["live"]))
        || (phase == "test" && flat_field_has_token(properties, &["test_network"]))
        || flat_field_has_token(restrict, &["network-sandbox"])
}

/// `FEATURES=sandbox` or `usersandbox` present (equivalent here: no `userpriv`).
/// Portage: process.py, _spawn
pub(crate) fn fs_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "sandbox") || feature_token_present(features, "usersandbox")
}

/// `/usr/bin/sandbox` when executable (Portage's `sandbox_capable`). `None`
/// degrades to an unsandboxed run.
/// Portage: const.py, process.py
pub(crate) fn sandbox_binary() -> Option<&'static Path> {
    use std::sync::OnceLock;
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let p = PathBuf::from("/usr/bin/sandbox");
        // Matches Portage's `os.access(_, os.X_OK)`.
        if p.is_file()
            && std::fs::metadata(&p)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt as _;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
        {
            Some(p)
        } else {
            None
        }
    })
    .as_deref()
}

/// True when sandboxing is requested, the phase is a `src_*` phase, and the binary
/// exists (else a one-shot warning and an unsandboxed run).
pub(crate) fn fs_sandbox_for_phase(phase: &str, features: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested(features) || !listed(SANDBOXED_SRC_PHASES, phase) {
        return false;
    }
    if sandbox_binary().is_some() {
        return true;
    }
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "!!! /usr/bin/sandbox not found (for FEATURES=\"sandbox\"); \
             src_* phases run without filesystem confinement"
        );
    });
    false
}

/// Namespace/confinement wrappers for one phase's bash subprocess.
/// Portage: doebuild.py, _doebuild_spawn
#[derive(Clone, Copy, Default)]
pub(crate) struct Isolation {
    /// `FEATURES=network-sandbox` (`CLONE_NEWNET`).
    pub(crate) net: bool,
    /// `FEATURES=ipc-sandbox` (`CLONE_NEWIPC`).
    pub(crate) ipc: bool,
    /// `FEATURES=mount-sandbox` (`CLONE_NEWNS` + `mount --make-rslave /`).
    pub(crate) mount: bool,
    /// `FEATURES=pid-sandbox` (`CLONE_NEWPID`; `--fork` for the init, `--mount-proc`
    /// for the new `/proc` mount).
    pub(crate) pid: bool,
    /// `FEATURES=sandbox`/`usersandbox` (Portage's `spawn_sandbox`).
    pub(crate) fs_sandbox: bool,
}

impl Isolation {
    pub(crate) fn any_unshare(&self) -> bool {
        self.net || self.ipc || self.mount || self.pid
    }
    pub(crate) fn any(&self) -> bool {
        self.any_unshare() || self.fs_sandbox
    }
    /// `unshare(1)` flags, `--map-root-user` first so it works non-root (Portage
    /// unshares only as `uid == 0`).
    pub(crate) fn unshare_flags(&self) -> Vec<&'static str> {
        let mut f = vec!["--map-root-user"];
        if self.net {
            f.push("--net");
        }
        if self.ipc {
            f.push("--ipc");
        }
        if self.mount {
            f.push("--mount");
        }
        if self.pid {
            f.extend(["--pid", "--fork", "--mount-proc"]);
        }
        f
    }
}

/// Whether `unshare <flags> -- true` succeeds here (cached per flag set; Portage
/// validates the same way via `_unshare_validator`). `false` drops the wrappers
/// with a warning — Portage's non-fatal "Unable to unshare".
pub(crate) fn unshare_combo_usable(flags: &[&str]) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let key = flags.join(" ");
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(&v) = cache.lock().unwrap().get(&key) {
        return v;
    }
    let ok = std::process::Command::new("unshare")
        .args(flags)
        .args(["--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    cache.lock().unwrap().insert(key, ok);
    ok
}

/// Wrappers for one phase from `FEATURES`: only `src_*` phases; an unusable unshare
/// combination degrades with one warning. `net` also carries the `networked`
/// exemption (live-`unpack`, `test_network`-`test`, `RESTRICT` opt-out).
/// Portage: doebuild.py, _doebuild_spawn
pub(crate) fn phase_isolation(env: &Environment, phase: &str, features: &str) -> Isolation {
    use std::sync::OnceLock;
    if !listed(SANDBOXED_SRC_PHASES, phase) {
        return Isolation::default();
    }
    let mut net = network_sandbox_requested(features);
    if net {
        // Exemptions run for sandboxable phases only, so the empty-set reduction suffices.
        let (restrict, properties) =
            restrict_and_properties(env, &std::collections::HashSet::new());
        if network_sandbox_exempt(phase, &restrict, &properties) {
            net = false;
        }
    }
    let mut iso = Isolation {
        net,
        ipc: feature_token_present(features, "ipc-sandbox"),
        mount: feature_token_present(features, "mount-sandbox"),
        pid: feature_token_present(features, "pid-sandbox"),
        fs_sandbox: fs_sandbox_for_phase(phase, features),
    };
    if iso.any_unshare() && !unshare_combo_usable(&iso.unshare_flags()) {
        static WARNED: OnceLock<()> = OnceLock::new();
        WARNED.get_or_init(|| {
            eprintln!(
                "!!! Unable to unshare (for FEATURES=\"network-sandbox / ipc-sandbox / \
                 mount-sandbox / pid-sandbox\"); src_* phases run without namespace isolation"
            );
        });
        iso.net = false;
        iso.ipc = false;
        iso.mount = false;
        iso.pid = false;
    }
    iso
}

/// A blocking `Command` running `bash <script> <arg>` (Portage's `EBUILD_SH_BINARY`
/// shape) wrapped per `iso`:
/// `unshare <flags> -- sh -c '<config>; exec "$@"' _ [sandbox] bash <script> <arg>`.
/// The `sh` shim configures inside each namespace what Portage's `_exec2` does
/// (loopback + workaround addresses for `--net`, `--make-rslave` for `--mount`);
/// `bin/*.sh` itself does the `SANDBOX_ON`/`addread`/`addwrite` setup from the env.
/// Portage: doebuild.py, process.py
pub(crate) fn sandbox_wrapped_command(
    script: &Path,
    arg: &str,
    iso: Isolation,
) -> std::process::Command {
    use std::ffi::OsString;

    let mut argv: Vec<OsString> = Vec::new();
    if iso.fs_sandbox {
        // Presence was already confirmed before `iso.fs_sandbox` was set.
        if let Some(bin) = sandbox_binary() {
            argv.push(bin.into());
        }
    }
    argv.push("bash".into());
    argv.push(script.into());
    argv.push(arg.into());

    if !iso.any_unshare() {
        let mut c = std::process::Command::new(&argv[0]);
        c.args(&argv[1..]);
        return c;
    }

    let mut shim = String::new();
    if iso.net {
        shim.push_str(
            "ip link set lo up 2>/dev/null; \
             ip addr add 10.0.0.1/8 dev lo 2>/dev/null; \
             ip -6 addr add fd::1/8 dev lo 2>/dev/null; ",
        );
    }
    if iso.mount {
        shim.push_str("mount --make-rslave / 2>/dev/null; ");
    }
    shim.push_str("exec \"$@\"");

    let mut c = std::process::Command::new("unshare");
    c.args(iso.unshare_flags())
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(shim)
        .arg("portuale-sandbox")
        .args(&argv);
    c
}
