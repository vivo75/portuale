//! The phase shell backend, the calling-environment filter, and sandbox/isolation.

use crate::ebuild_phases::*;

/// Which real shell executes a phase, and every real `bin/*.sh` this
/// portuale sources unmodified along with it.
///
/// `Bash` (**the default**) is a genuine `bash <bin_dir>/ebuild.sh
/// <phase>` subprocess -- matching real portage's own `_doebuild_spawn()`
/// invocation shape almost exactly (`lib/portage/package/ebuild/
/// doebuild.py`'s own `cmd = "{ebuild.sh} {phase}"`, spawned via
/// `portage.process.spawn()`; real `bin/ebuild.sh:153`'s own
/// `EBUILD_SH_ARGS="$*"` picks up `<phase>` from the subprocess's own
/// positional args, which its own tail, `bin/ebuild.sh:830-843`, then
/// really uses to call `__ebuild_main ${EBUILD_SH_ARGS}` and `exit`).
///
/// `Brush` is an embedded `brush_core::Shell` (see this module's own doc
/// comment, and `portuale/Cargo.toml`'s, for the pinned commit and how
/// the embedding works). It deliberately never sets `EBUILD_SH_ARGS`,
/// since a bare `exit` inside an *embedded* shell would kill the whole
/// hosting Rust process rather than just return control -- so it uses
/// brush's own "source, then separately `invoke_function`" two-step
/// instead of `__ebuild_main`.
///
/// **Why `Bash` is the default** (it was `Brush` originally, for the
/// zero-dependency / minimal-Linux fit -- hard goal 3): brush's `declare
/// -f` function serializer corrupts any function body containing a
/// redirected here-document (the redirect is torn off the `cat` line and
/// re-emitted after the body with its `"${var}"` target mangled; a `<<-`
/// body is re-indented with spaces so its terminator no longer matches).
/// `__save_ebuild_env` runs `declare -f` on every in-scope function
/// between phases, and `toolchain-funcs.eclass`'s `_tc-has-openmp` (plus
/// others) trips this -- the written `${T}/environment` then fails to
/// parse and the next phase's `source "${T}/environment" || die` aborts
/// the build. That breaks a real `emerge <atom>` for essentially every
/// compiled package. `Bash` has no such problem; `Brush` stays available
/// via `--shell brush` / `--shell=brush` (a portuale-only flag on both
/// `emerge` and `ebuild`, deliberately NOT in `ebuild_options::OPTIONS`
/// -- that table transcribes real `bin/ebuild`'s argparse only). The
/// brush `declare -f` bug is tracked in `docs/brush-pin.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellBackend {
    #[default]
    Bash,
    Brush,
}

/// The real `src_*` phases portuale puts inside a sandbox -- both the
/// `FEATURES=network-sandbox` net namespace and the `FEATURES=sandbox`
/// `sys-apps/sandbox` filesystem confinement apply to exactly this set.
/// Real portage: `_doebuild_spawn` sandboxes every phase not in
/// `_unsandboxed_phases` (`clean`/`config`/`setup`/`pre|post*`/…), and
/// network-unshares every phase not in `_ipc_phases`; for the phases
/// portuale actually runs as real bash both come out to this list.
/// `nofetch` is deliberately excluded (real `spawn_nofetch` uses its own
/// private tmpdir and neither unshares nor `sandbox`-wraps).
pub(crate) const SANDBOXED_SRC_PHASES: &[&str] = &[
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
];

/// Real `special_env_vars.environ_whitelist` + its `environ_whitelist_re`
/// (`^(CCACHE_|DISTCC_).*`): the calling-environment variables real
/// portage's `config.environ()` carries into a phase's environment.
/// portuale spawns phases with the full inherited process env plus its
/// `phase_env_vars` overrides; real never inherits wholesale, so vars
/// like `EMERGE_DEFAULT_OPTS` (an `emerge` CLI option, not a build var)
/// or a portuale-only `PORTAGE_RUNNING_ROOT` leaked through and, once
/// L1-c started regenerating the vdb `environment` from the live phase
/// env, into the vdb where real's has nothing (L1-f). `run_one_phase_*`
/// now filters the inherited env to this list; `phase_env_vars` still
/// sets everything portuale actually needs on top. Copied verbatim from
/// a live `special_env_vars.environ_whitelist` -- entries portuale never
/// sets simply won't be present to pass through.
pub(crate) const ENVIRON_WHITELIST: &[&str] = &[
    "A",
    "AA",
    "ACCEPT_LICENSE",
    "BASH_ENV",
    "BINPKG_FORMAT",
    "BROOT",
    "BUILD_ID",
    "BUILD_PREFIX",
    "CATEGORY",
    "COLORTERM",
    "COLUMNS",
    "CVS_RSH",
    "D",
    "DISPLAY",
    "DISTDIR",
    "DOC_SYMLINKS_DIR",
    "EAPI",
    "EBUILD",
    "EBUILD_FORCE_TEST",
    "EBUILD_PHASE",
    "EBUILD_PHASE_FUNC",
    "ECHANGELOG_USER",
    "ECLASSDIR",
    "ECLASS_DEPTH",
    "ED",
    "EDITOR",
    "EMERGE_FROM",
    "ENV_UNSET",
    "EPREFIX",
    "EROOT",
    "ESYSROOT",
    "FEATURES",
    "FILESDIR",
    "GPG_AGENT_INFO",
    "HOME",
    "INSTALL_MASK",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_PAPER",
    "LC_TIME",
    "LD_PRELOAD",
    "LESS",
    "LESSOPEN",
    "LOGNAME",
    "LS_COLORS",
    "MAKEFLAGS",
    "MAKEOPTS",
    "MERGE_TYPE",
    "NINJAOPTS",
    "NOCOLOR",
    "NO_COLOR",
    "P",
    "PAGER",
    "PATH",
    "PF",
    "PKGDIR",
    "PKGUSE",
    "PKG_INSTALL_MASK",
    "PKG_LOGDIR",
    "PKG_TMPDIR",
    "PM_EBUILD_HOOK_DIR",
    "PN",
    "PORTAGE_ACTUAL_DISTDIR",
    "PORTAGE_ARCHLIST",
    "PORTAGE_BASHRC",
    "PORTAGE_BASHRC_FILES",
    "PORTAGE_BINPKG_FILE",
    "PORTAGE_BINPKG_TAR_OPTS",
    "PORTAGE_BINPKG_TMPFILE",
    "PORTAGE_BIN_PATH",
    "PORTAGE_BUILDDIR",
    "PORTAGE_BUILD_GROUP",
    "PORTAGE_BUILD_USER",
    "PORTAGE_BUNZIP2_COMMAND",
    "PORTAGE_BZIP2_COMMAND",
    "PORTAGE_COLORMAP",
    "PORTAGE_COMPRESS",
    "PORTAGE_COMPRESSION_COMMAND",
    "PORTAGE_COMPRESS_EXCLUDE_SUFFIXES",
    "PORTAGE_CONFIGROOT",
    "PORTAGE_DEBUG",
    "PORTAGE_DEPCACHEDIR",
    "PORTAGE_DOHTML_UNWARNED_SKIPPED_EXTENSIONS",
    "PORTAGE_DOHTML_UNWARNED_SKIPPED_FILES",
    "PORTAGE_DOHTML_WARN_ON_SKIPPED_FILES",
    "PORTAGE_EBUILD_EXIT_FILE",
    "PORTAGE_ECLASS_LOCATIONS",
    "PORTAGE_FEATURES",
    "PORTAGE_GID",
    "PORTAGE_GRPNAME",
    "PORTAGE_INST_GID",
    "PORTAGE_INST_UID",
    "PORTAGE_INTERNAL_CALLER",
    "PORTAGE_IPC_DAEMON",
    "PORTAGE_IUSE",
    "PORTAGE_LOG_FILE",
    "PORTAGE_OVERRIDE_EPREFIX",
    "PORTAGE_PIPE_FD",
    "PORTAGE_PROPERTIES",
    "PORTAGE_PYM_PATH",
    "PORTAGE_PYTHON",
    "PORTAGE_PYTHONPATH",
    "PORTAGE_QUIET",
    "PORTAGE_REPOSITORIES",
    "PORTAGE_REPO_NAME",
    "PORTAGE_REPO_REVISIONS",
    "PORTAGE_RESTRICT",
    "PORTAGE_SOCKS5_PROXY",
    "PORTAGE_TMPDIR",
    "PORTAGE_UPDATE_ENV",
    "PORTAGE_USERNAME",
    "PORTAGE_VERBOSE",
    "PORTAGE_WORKDIR_MODE",
    "PORTAGE_XATTR_EXCLUDE",
    "PORTDIR",
    "PORTDIR_OVERLAY",
    "PR",
    "PREROOTPATH",
    "PV",
    "PVR",
    "PYTHONDONTWRITEBYTECODE",
    "REPLACED_BY_VERSION",
    "REPLACING_VERSIONS",
    "ROOT",
    "ROOTPATH",
    "SANDBOX_LOG",
    "SSH_AGENT_PID",
    "SSH_AUTH_SOCK",
    "STY",
    "SYSROOT",
    "T",
    "TEMP",
    "TERM",
    "TERMCAP",
    "TMP",
    "TMPDIR",
    "USER",
    "USE_EXPAND",
    "USE_ORDER",
    "WINDOW",
    "WORKDIR",
    "XARGS",
    "XAUTHORITY",
    "__PORTAGE_TEST_HARDLINK_LOCKS",
    "ftp_proxy",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// `key` survives real portage's `config.environ()` calling-env filter.
pub(crate) fn environ_whitelisted(key: &str) -> bool {
    ENVIRON_WHITELIST.contains(&key) || key.starts_with("CCACHE_") || key.starts_with("DISTCC_")
}

/// The settings the native `gpkg-helper.py compress` (#326 S4) reads from
/// its environment: `BINPKG_COMPRESS`, `BINPKG_COMPRESS_FLAGS[_<NAME>]`
/// and `BINPKG_GPG_SIGNING_*`. Real's helper rebuilds `portage.settings`
/// from the config files; the native one has no config loader (D7), so
/// `ebuild_package::invoke_dyn_package` exports the already-resolved
/// values (files-only for `gpkg`, the Q6 rule) and they must survive the
/// calling-env filter below in the `package` phase -- the one phase that
/// runs `__dyn_package`, hence the one place real's own helper would
/// have read them. The compression three are on real's `environ_filter`
/// (so absent from a real phase env); they never reach any other phase
/// and `bin/phase-functions.sh` saves no `package`-phase environment.
pub(crate) fn gpkg_helper_setting(phase: &str, key: &str) -> bool {
    phase == "package"
        && (key == "BINPKG_COMPRESS"
            || key == "BINPKG_COMPRESS_FLAGS"
            || key.starts_with("BINPKG_COMPRESS_FLAGS_")
            || key.starts_with("BINPKG_GPG_SIGNING_"))
}

/// Real `config.environ()`'s `filter_calling_env`
/// (`lib/portage/package/ebuild/config.py:3275-3305`, bug #189417): once a
/// phase's own `${T}/environment` exists -- i.e. every phase after the
/// first real one -- the process it spawns carries only the
/// `special_env_vars.environ_whitelist` keys of the config/calling env;
/// every other config variable is supplied by the saved environment
/// instead, so a variable the ebuild itself `unset` in an earlier phase
/// (bug #189417's own wording: "variables unset by the ebuild remain
/// unset") stays unset. Real exempts the init/fetch phases and a `setup`
/// phase run from a standalone `ebuild` (`EMERGE_FROM == "ebuild"`).
///
/// Without this, portuale re-injected the config `build_env` (`CFLAGS`
/// and friends) into every later phase: `dev-build/ninja-1.13.2-r1`'s
/// `src_compile` `unset CFLAGS` never reached `src_install`, so real
/// `__dyn_install`'s write-if-present loop wrote a `build-info/CFLAGS`
/// real never writes and the vdb carried a `CFLAGS` row real omits (#160,
/// l3-20260925T074707Z Class 4; `findings/l3.md` § #160 S0).
pub(crate) fn filter_calling_env(
    phase: &str,
    t_dir: &Path,
    extra_env: &[(String, String)],
) -> bool {
    if matches!(phase, "clean" | "cleanrm" | "depend" | "fetch") {
        return false;
    }
    // `EMERGE_FROM` lives in `extra_env` for the binary/saved-env paths
    // (the literal in `vars` below is the source path's own default).
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

/// The `FEATURES` string a phase-execution decision should consult:
/// the **last** `FEATURES` pair on `extra_env` when present (the
/// resolved incremental list #37 S2 threads), else the process
/// environment (a standalone `ebuild <file>` run, where no resolved
/// config exists). Real semantics: every Rust-side gate below reads
/// `settings.features`, i.e. the resolved list, never the raw calling
/// env.
pub(crate) fn features_string(extra_env: &[(String, String)]) -> String {
    extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "FEATURES")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("FEATURES").unwrap_or_default())
}

/// A `FEATURES` token check over the given resolved features string
/// (`features_string`'s output). The `pretend.rs`/`ebuild_merge.rs`
/// siblings keep their own process-env reads: they are CLI-boundary
/// fallbacks (`ebuild <file>`), not phase execution.
pub(crate) fn feature_token_present(features: &str, token: &str) -> bool {
    features.split_whitespace().any(|t| t == token)
}

/// Real `FEATURES` for the phase's own bash environment
/// (`doebuild_environment()`'s own `mysettings["FEATURES"]`, the same
/// source every isolation check above already reads via
/// `feature_token_present`) -- passed straight through rather than
/// blanked, so real, unmodified bash that gates its own behavior on a
/// `contains_word <token> "${FEATURES}"` check runs correctly: real
/// `bin/estrip`'s own `compressdebug`/`installsources`/`nostrip`/
/// `splitdebug`/`xattr` handling (called from `prepstrip`/`prepallstrip`
/// during `install`); `phase-functions.sh`'s own `__dyn_test` (`FEATURES=
/// test` is what actually makes `src_test` run at all -- previously a
/// silent, unconditional no-op here regardless of what the invoker
/// asked for) and `nostrip`/`ccache`/`distcc`/`noauto` checks;
/// `misc-functions.sh`'s own `noclean`/`keepwork`/`sfperms`/`suidctl`/
/// `selinux`/`packdebug`/`chflags`/`binpkg-do{compress,strip}` handling.
/// None of these are also interpreted by portuale's own Rust side (the
/// tokens Rust *does* independently act on --
/// `sandbox`/`usersandbox`/`{network,ipc,mount,pid}-sandbox`,
/// `distlocks`, `buildpkg-live`, `binpkg-multi-instance` -- are never
/// gated on again by any real bash this module runs, confirmed by
/// grepping every `contains_word … "${FEATURES}"` site in `bin/*.sh`),
/// so there's no double-handling risk; a token real bash doesn't
/// understand, or whose supporting tool/env var isn't present
/// (`ccache`/`distcc`/`selinux` all require their own binaries), is
/// exactly as inert as it always was, and degrades exactly the way real
/// portage's own bash would under the same conditions.
pub(crate) fn phase_features_value() -> String {
    std::env::var("FEATURES").unwrap_or_default()
}

/// `FEATURES=network-sandbox` present?
pub(crate) fn network_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "network-sandbox")
}

/// Real `_doebuild_spawn`'s own `networked` exemption formula
/// (`doebuild.py:241-251`), the half of `phase_isolation`'s own
/// network-unshare decision that doesn't depend on `FEATURES` itself
/// (that part is `network_sandbox_requested()`, a process-env read
/// tests can't safely mutate in parallel -- see `run_commands`'s own
/// doc comment) -- kept as its own pure function purely so it can be
/// unit-tested directly. `restrict`/`properties` are already
/// USE-reduced flat token strings (`restrict_and_properties`). `true`
/// when: `phase == "unpack"` and the ebuild's own `PROPERTIES` says
/// `live` (a live/VCS package's checkout step needs the network by
/// definition); `phase == "test"` and `PROPERTIES` says `test_network`
/// (an ebuild that declares its own test suite needs network access);
/// or the ebuild's own `RESTRICT` says `network-sandbox` (an explicit
/// per-package opt-out of the whole feature, regardless of phase).
pub(crate) fn network_sandbox_exempt(phase: &str, restrict: &str, properties: &str) -> bool {
    (phase == "unpack" && flat_field_has_token(properties, &["live"]))
        || (phase == "test" && flat_field_has_token(properties, &["test_network"]))
        || flat_field_has_token(restrict, &["network-sandbox"])
}

/// `FEATURES=sandbox` or `FEATURES=usersandbox` present -- real
/// `_spawn`'s own `"sandbox" not in features and "usersandbox" not in
/// features` gate. (Portuale does no `userpriv`, so `sandbox` and
/// `usersandbox` are equivalent here.)
pub(crate) fn fs_sandbox_requested(features: &str) -> bool {
    feature_token_present(features, "sandbox") || feature_token_present(features, "usersandbox")
}

/// Real `portage.const.SANDBOX_BINARY` (`/usr/bin/sandbox`), and real
/// `portage.process.sandbox_capable` (the file exists and is
/// executable). `None` -> `FEATURES=sandbox` silently degrades to an
/// unsandboxed run, exactly real `_spawn`'s own
/// `if not free and not (fakeroot or sandbox_capable): free = True`.
pub(crate) fn sandbox_binary() -> Option<&'static Path> {
    use std::sync::OnceLock;
    static BIN: OnceLock<Option<PathBuf>> = OnceLock::new();
    BIN.get_or_init(|| {
        let p = PathBuf::from("/usr/bin/sandbox");
        // `X_OK` is good enough -- matches real `os.access(_, os.X_OK)`.
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

/// Real `FEATURES=sandbox` for one phase: `true` when the feature is
/// requested, the phase is a sandboxed `src_*` phase, and the `sandbox`
/// binary is available. A single warning is printed (real `_spawn`'s own
/// silent degrade is matched with an explicit note) when the feature is
/// on but the binary is missing.
pub(crate) fn fs_sandbox_for_phase(phase: &str, features: &str) -> bool {
    use std::sync::OnceLock;
    if !fs_sandbox_requested(features) || !SANDBOXED_SRC_PHASES.contains(&phase) {
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

/// Every namespace/confinement wrapper to apply to one phase's real bash
/// subprocess -- real `_doebuild_spawn`'s `unshare_{net,ipc,mount,pid}`
/// + `spawn_sandbox`, collapsed to what portuale models.
#[derive(Clone, Copy, Default)]
pub(crate) struct Isolation {
    /// `FEATURES=network-sandbox` -> `unshare --net` (real `CLONE_NEWNET`).
    pub(crate) net: bool,
    /// `FEATURES=ipc-sandbox` -> `unshare --ipc` (real `CLONE_NEWIPC`).
    pub(crate) ipc: bool,
    /// `FEATURES=mount-sandbox` -> `unshare --mount` + `mount --make-rslave /`
    /// (real `CLONE_NEWNS` + real `_exec2`'s own `mount --make-rslave /`).
    pub(crate) mount: bool,
    /// `FEATURES=pid-sandbox` -> `unshare --pid --fork --mount-proc`
    /// (real `CLONE_NEWPID` + `pid-ns-init`; `unshare --fork` stands in
    /// for the full init, `--mount-proc` for real `_exec2`'s own new
    /// `/proc` mount).
    pub(crate) pid: bool,
    /// `FEATURES=sandbox`/`usersandbox` -> `sandbox <cmd>` (real
    /// `spawn_sandbox`).
    pub(crate) fs_sandbox: bool,
}

impl Isolation {
    pub(crate) fn any_unshare(&self) -> bool {
        self.net || self.ipc || self.mount || self.pid
    }
    pub(crate) fn any(&self) -> bool {
        self.any_unshare() || self.fs_sandbox
    }
    /// The `unshare(1)` flags for this combination (always with
    /// `--map-root-user` first, so it works from portuale's non-root
    /// context -- real portage only unshares when `uid == 0`).
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

/// Whether `unshare <flags> -- true` actually succeeds here (cached per
/// distinct flag combination). Real portage validates the `unshare(2)`
/// call in a short-lived subprocess before relying on it
/// (`_unshare_validator`); this is the same idea via the `unshare(1)`
/// CLI, which portuale already assumes is present the way it assumes
/// `tar`/`wget`/`bash`. A `false` result (unprivileged user namespaces
/// disabled, or no `unshare` binary) drops the wrappers with a warning
/// -- real portage's own non-fatal "Unable to unshare" degrade.
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

/// The isolation wrappers to apply to one phase, from `FEATURES` -- real
/// `_doebuild_spawn`. Only the `src_*` phases (`SANDBOXED_SRC_PHASES`)
/// are wrapped; a requested-but-unusable `unshare` combination degrades
/// to no unshare with one warning (real "Unable to unshare").
///
/// `net` also carries real `_doebuild_spawn`'s own `networked` exemption
/// formula (`doebuild.py:241-251`): `FEATURES=network-sandbox` is
/// requested but this call still isn't network-unshared when `phase ==
/// "unpack"` and the ebuild's own `PROPERTIES` says `live` (a live/VCS
/// package's checkout step needs the network by definition), when
/// `phase == "test"` and `PROPERTIES` says `test_network` (an ebuild
/// that declares its own test suite needs network access), or when the
/// ebuild's own `RESTRICT` says `network-sandbox` (an explicit ebuild
/// opt-out of the whole feature, regardless of phase). `_ipc_phases`
/// (`setup`/`pretend`/`config`/`info`/`pre|postinst`/`pre|postrm`) is
/// real's own third exemption clause, but every one of those is already
/// outside `SANDBOXED_SRC_PHASES` here, so it never needs its own check.
pub(crate) fn phase_isolation(env: &Environment, phase: &str, features: &str) -> Isolation {
    use std::sync::OnceLock;
    if !SANDBOXED_SRC_PHASES.contains(&phase) {
        return Isolation::default();
    }
    let mut net = network_sandbox_requested(features);
    if net {
        // Exemption checks run for sandboxable phases only (`depend`
        // never reaches here -- it is outside `SANDBOXED_SRC_PHASES`
        // above), so the empty-set reduction is the whole story.
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

/// Build a blocking `Command` that runs `bash <script> <arg>` (real
/// `_doebuild_spawn`'s own `EBUILD_SH_BINARY <arg>` shape) wrapped per
/// `iso`:
///
///   `unshare <flags> -- sh -c '<config>; exec "$@"' _ [sandbox] bash <script> <arg>`
///
/// - `FEATURES=sandbox` prepends the `sys-apps/sandbox` binary (real
///   `spawn_sandbox`: `args = [SANDBOX_BINARY, mycommand]`). `bin/*.sh`
///   itself does the `SANDBOX_ON=1` / `addread /` / `addwrite
///   "${PORTAGE_TMPDIR}/portage"` setup (given `SANDBOX_LOG` /
///   `SANDBOX_DISABLED=0` from `phase_env_vars`); `sandbox` exits
///   non-zero once its log gains a violation.
/// - `FEATURES={network,ipc,mount,pid}-sandbox` wrap the command in the
///   matching `unshare(1)` namespaces (real `_exec`'s
///   `unshare(CLONE_NEW*)`). The `sh -c` shim configures what real
///   `_exec2` configures inside each namespace -- `ip link set lo up`
///   plus the real `AI_ADDRCONFIG`-workaround addresses (real
///   `_configure_loopback_interface`, `process.py:838-871`; bug #690758
///   -- some glibc `getaddrinfo()` calls with `AI_ADDRCONFIG` set return
///   no results at all when a family has zero non-loopback addresses
///   configured, so real adds one anyway) for `--net`, `mount
///   --make-rslave /` for `--mount` (real `_exec2`'s own call) -- then
///   `exec "$@"` runs the real (possibly `sandbox`-prefixed) command.
///   The IPv6 address add is allowed to fail silently (`2>/dev/null`,
///   same as every other command in this shim) exactly the way real's
///   own `has_ipv6()` guard skips it outright on an IPv6-less host --
///   the net effect (no IPv6 loopback address) is identical either way.
pub(crate) fn sandbox_wrapped_command(
    script: &Path,
    arg: &str,
    iso: Isolation,
) -> std::process::Command {
    use std::ffi::OsString;

    let mut argv: Vec<OsString> = Vec::new();
    if iso.fs_sandbox {
        // Presence was already confirmed by `fs_sandbox_for_phase` /
        // the misc-functions caller before `iso.fs_sandbox` was set.
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
