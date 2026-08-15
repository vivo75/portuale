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

/// A `FEATURES` token check over `features_string`'s output. CLI fallbacks elsewhere
/// keep their own process-env reads.
pub(crate) fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
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
