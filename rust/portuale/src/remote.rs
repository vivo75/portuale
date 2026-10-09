//! Remote binary-package merge over SSH (`mrg --remote-*`).
//!
//! Plan: `docs/remote-merge.md` (slice 1 covers this module's surface --
//! option validation, the multiplexed `ssh` transport, and the read-only
//! preflight; no payload streaming yet).
//!
//! Shape, mirroring Ansible's `ansible.builtin.ssh` (a wrapper around the
//! system `ssh` binary, multiplexed, rc 255 = transport error): the server
//! shells out to OpenSSH, never links an SSH library, so the musl-static
//! story and the near-zero-dependency discipline both survive. The client
//! needs only bash ≥ 5.3 and POSIX `/bin` -- generated bash goes over
//! `ssh … bash -s` (stdin), machine-readable `KEY=VALUE` lines come back
//! on stdout, human log on stderr. No Python anywhere near the client.
//!
//! Binary-format scope: gpkg (`.gpkg.tar`) only. xpak (`.tbz2`) is out of
//! scope for `mrg` -- it is the old format and `mrg` does not target full
//! binary-format compatibility (the shared server-side `binpkg` reader may
//! open `.tbz2` incidentally, but that is never a tested or guaranteed
//! `mrg` path; `emerge`/`ebuild` keep their own local xpak support, see
//! `docs/remote-merge.md` §1 / §14).

use clap::ArgMatches;
use portage_util::TempDir;
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

// --- Option validation -----------------------------------------------------

/// Host-key policy (`--remote-strict-host-key-checking`). Default is
/// trust-on-first-use: a plain `yes` would fail on every new machine,
/// which is the common provisioning case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictHostKeyChecking {
    /// `accept-new`: add unknown keys (fingerprint printed), abort on
    /// changed keys.
    AcceptNew,
    /// `yes`: reject unknown keys outright.
    Yes,
    /// `no`: no checking (throwaway labs only).
    No,
}

impl StrictHostKeyChecking {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "accept-new" => Some(Self::AcceptNew),
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            _ => None,
        }
    }

    fn as_ssh_value(self) -> &'static str {
        match self {
            Self::AcceptNew => "accept-new",
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}

/// How driver scripts and files reach the client (`--remote-transport`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTransport {
    /// Real SSH client (default).
    Ssh,
    /// Execute the *same generated driver* against local paths (no ssh):
    /// offline debugging and SSH-free driver tests. The hostname stays a
    /// label; key/port/user/timeout/host-key options are inert.
    Local,
}

impl RemoteTransport {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "ssh" => Some(Self::Ssh),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

/// Where a config tree lives (`--remote-etc-portage server:<path>` /
/// `client:<path>`). The vdb/edb placements of the plan table land in
/// slice 6 with the vdb shadow; only etc-portage is placed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPlacement {
    /// Read directly off the server filesystem.
    Server(String),
    /// Pulled once over the multiplexed connection at plan start.
    Client(String),
}

impl ConfigPlacement {
    /// Parse `server:<path>` / `client:<path>`; anything else is a usage
    /// error naming the option.
    pub fn parse(option: &str, value: &str) -> Result<Self, String> {
        match value.split_once(':') {
            Some(("server", path)) if !path.is_empty() => Ok(Self::Server(path.to_string())),
            Some(("client", path)) if !path.is_empty() => Ok(Self::Client(path.to_string())),
            _ => Err(format!(
                "mrg: {option} must be server:<path> or client:<path>, got {value:?}"
            )),
        }
    }
}

/// mrg -> pretend handoff for remote resolve runs: `mrg` sets it from the
/// validated CLI surface, `pretend::run`'s getbinpkg dispatch takes it.
/// Same process-global-options precedent as portage-repo's own
/// `USEOLDPKG_ATOMS`/`BINPKG_CHANGED_DEPS_OVERRIDE` statics (plain
/// `RwLock`, `unwrap()` like theirs): set immediately before the
/// in-process `pretend::run` call, taken (cleared) at the dispatch site,
/// so nothing leaks across calls. Only `mrg` remote-with-atoms ever sets
/// it; the `emerge` applet never does.
static REMOTE_EXEC: std::sync::RwLock<Option<RemoteContext>> = std::sync::RwLock::new(None);

/// Publish a remote execution for the upcoming `pretend::run` call.
pub fn set_remote_exec(ctx: RemoteContext) {
    *REMOTE_EXEC.write().unwrap() = Some(ctx);
}

/// Take a published remote execution, if any (clears the slot).
pub fn take_remote_exec() -> Option<RemoteContext> {
    REMOTE_EXEC.write().unwrap().take()
}

/// Validated remote target: Ansible's `play_context` split -- connection
/// parameters in one struct, validated once, threaded through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteContext {
    /// Client hostname / IP (never `user@host` -- see `check_remote`).
    pub hostname: String,
    /// SSH login user; `None` lets ssh pick (its own default).
    pub user: Option<String>,
    /// SSH port.
    pub port: u16,
    /// Private key file; `None` defers to ssh-agent/defaults.
    pub key_file: Option<String>,
    /// Seconds to wait establishing the connection.
    pub timeout_secs: u64,
    /// Extra argv for every ssh invocation (whitespace-split, v1 cut:
    /// no quoting -- pass one flat string of simple tokens).
    pub ssh_args: Option<String>,
    /// Host-key policy.
    pub strict_host_key_checking: StrictHostKeyChecking,
    /// Abort when `|server - client|` clock differs by more seconds;
    /// `0` disables.
    pub max_clock_skew_secs: u64,
    /// Target `${ROOT}` on the client.
    pub root: String,
    /// Per-unit work area on the client.
    pub workdir: String,
    /// How driver scripts + files reach the client.
    pub transport: RemoteTransport,
    /// One explicit binpkg file to bundle, stream and unpack (bypasses
    /// resolution; slices 2-4 trials, later an escape hatch).
    pub binpkg: Option<String>,
    /// Where `/etc/portage` resolves from (default
    /// `client:/etc/portage`).
    pub etc_portage: ConfigPlacement,
    /// Where the client installed-db lives (default
    /// `client:<root>/var/db/pkg`). `server:` = stateless client: files
    /// merge with no vdb entry, no old hooks, and fail-closed collisions
    /// (slice 6).
    pub vdb: ConfigPlacement,
    /// Binhost-index cache side, informational (default
    /// `server:<root>/var/cache/edb`): the client has no binhosts, so
    /// only `server:` parses -- the resolve reads its own cache.
    pub edb: ConfigPlacement,
    /// Server ledger directory override (`None` =
    /// `<placed-PKGDIR>/remote-ledger`).
    pub ledger_dir: Option<String>,
    /// Strict-mode ledger provenance enforcement
    /// (`--remote-require-ledger-match`, plan §8): abort before merging
    /// anything unless the client's and server's newest ledger line agree.
    pub require_ledger_match: bool,
    /// Binary the server ships to the client (`--remote-portuale-binary`,
    /// #326 D5/Q3): `None` = this process's own binary (`current_exe()`).
    pub portuale_binary: Option<String>,
    /// Client directory holding the portuale binary
    /// (`--remote-portuale-dir`, #326 Q1): `None` = the default pair
    /// (`/opt/bin`, then `/usr/local/bin`). Must be absolute when given.
    pub portuale_dir: Option<String>,
    /// Resolved client binary placement (post-preflight, #326 S8.1):
    /// `Unresolved` until the preflight facts decide it. Local transport
    /// without the override never resolves past `Unresolved` -- it keeps
    /// `current_exe()` and installs nothing (#326 D5).
    pub bin_plan: ClientBinPlan,
    /// Space-separated CONFIG_PROTECT list for the client merge (real
    /// default `/etc`; the resolve path derives it from the placed
    /// config unless explicitly flagged -- slice 6).
    pub config_protect: String,
    /// Whether `--remote-config-protect` was passed explicitly.
    pub config_protect_explicit: bool,
    /// Space-separated CONFIG_PROTECT_MASK list (real default
    /// `/etc/env.d`).
    pub config_protect_mask: String,
    /// Whether `--remote-config-protect-mask` was passed explicitly.
    pub config_protect_mask_explicit: bool,
}

/// Resolved client binary placement (#326 S8.1 gates): the absolute client
/// path every generated script exports as `PORTUALE_BIN`, however it got
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientBinPlan {
    /// No preflight ran yet (fresh CLI parse, or local transport without
    /// `--remote-portuale-dir`, which skips the gates by design).
    Unresolved,
    /// The running binary itself (local transport without the override,
    /// #326 D5): no preflight checks, no install.
    Local(String),
    /// A client path whose full SHA-256 already matches: use it, install
    /// nothing.
    Use(String),
    /// No candidate matches: install there (as `portuale-<hash>`).
    Install(String),
}

impl ClientBinPlan {
    /// The absolute path the plan settled on, if any.
    pub fn abs(&self) -> Option<&str> {
        match self {
            Self::Unresolved => None,
            Self::Local(path) | Self::Use(path) | Self::Install(path) => Some(path),
        }
    }
}

/// `--remote-*` ids besides `remote_hostname`, in OPTIONS-table order --
/// any of these without `--remote-hostname` is a usage error.
const REMOTE_OPTION_IDS: &[&str] = &[
    "remote_user",
    "remote_port",
    "remote_key_file",
    "remote_timeout",
    "remote_ssh_args",
    "remote_strict_host_key_checking",
    "remote_max_clock_skew",
    "remote_root",
    "remote_workdir",
    "remote_transport",
    "remote_binpkg",
    "remote_config_protect",
    "remote_config_protect_mask",
    "remote_etc_portage",
    "remote_vdb",
    "remote_edb",
    "remote_ledger_dir",
    "remote_require_ledger_match",
    "remote_portuale_binary",
    "remote_portuale_dir",
];

fn get(matches: &ArgMatches, id: &str) -> Option<String> {
    matches.get_one::<String>(id).cloned()
}

/// Whether `id` was provided on the command line, type-agnostic -- the
/// `REMOTE_OPTION_IDS` presence check must see both `Value` string
/// options and `Flag` bools (which `get_one::<String>` would panic on).
fn provided(matches: &ArgMatches, id: &str) -> bool {
    matches.contains_id(id)
        && matches
            .value_source(id)
            .is_some_and(|src| src == clap::parser::ValueSource::CommandLine)
}

/// Validate the `--remote-*` surface: `Ok(None)` = local mode,
/// `Ok(Some(ctx))` = remote mode, `Err(message)` = usage error (exit 2).
pub fn check_remote(matches: &ArgMatches) -> Result<Option<RemoteContext>, String> {
    let hostname = get(matches, "remote_hostname");
    let Some(hostname) = hostname else {
        if let Some(offender) = REMOTE_OPTION_IDS.iter().find(|id| provided(matches, id)) {
            let long = format!("--{}", offender.replace('_', "-"));
            return Err(format!("mrg: {long} requires --remote-hostname"));
        }
        return Ok(None);
    };
    if hostname.is_empty() {
        return Err("mrg: --remote-hostname requires a non-empty value".to_string());
    }
    if hostname.contains('@') {
        return Err("mrg: put the login user in --remote-user, not --remote-hostname".to_string());
    }
    let port = match get(matches, "remote_port") {
        None => 22,
        Some(raw) => raw.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
            format!("mrg: --remote-port: {raw:?} is not a valid TCP port (1-65535)")
        })?,
    };
    let timeout_secs = match get(matches, "remote_timeout") {
        None => 10,
        Some(raw) => raw.parse::<u64>().ok().ok_or_else(|| {
            format!("mrg: --remote-timeout: {raw:?} is not a non-negative integer")
        })?,
    };
    let max_clock_skew_secs = match get(matches, "remote_max_clock_skew") {
        None => 900,
        Some(raw) => raw.parse::<u64>().ok().ok_or_else(|| {
            format!("mrg: --remote-max-clock-skew: {raw:?} is not a non-negative integer")
        })?,
    };
    // Clap's `choices` already restrict these; the fallbacks keep the
    // validation total if the table ever drifts.
    let strict_host_key_checking = match get(matches, "remote_strict_host_key_checking") {
        None => StrictHostKeyChecking::AcceptNew,
        Some(raw) => StrictHostKeyChecking::parse(&raw).ok_or_else(|| {
            format!("mrg: --remote-strict-host-key-checking: {raw:?} is not accept-new, yes or no")
        })?,
    };
    let transport = match get(matches, "remote_transport") {
        None => RemoteTransport::Ssh,
        Some(raw) => RemoteTransport::parse(&raw)
            .ok_or_else(|| format!("mrg: --remote-transport: {raw:?} is not ssh or local"))?,
    };
    let root = get(matches, "remote_root").unwrap_or_else(|| "/".to_string());
    let vdb = match get(matches, "remote_vdb") {
        None => ConfigPlacement::Client(format!("{}/var/db/pkg", root.trim_end_matches('/'))),
        Some(raw) => ConfigPlacement::parse("--remote-vdb", &raw)?,
    };
    // Informational (plan §7/§10): the client has no binhosts -- only
    // `server:` parses. The resolve reads its own cache; this records
    // intent for a future slice that threads it through.
    let edb = match get(matches, "remote_edb") {
        None => ConfigPlacement::Server(format!("{}/var/cache/edb", root.trim_end_matches('/'))),
        Some(raw) => match ConfigPlacement::parse("--remote-edb", &raw)? {
            placement @ ConfigPlacement::Server(_) => placement,
            ConfigPlacement::Client(_) => {
                return Err(
                    "mrg: --remote-edb must be server:<path> (the client has no binhosts)"
                        .to_string(),
                );
            }
        },
    };
    let (config_protect, config_protect_explicit) = match get(matches, "remote_config_protect") {
        Some(raw) => (raw, true),
        None => ("/etc".to_string(), false),
    };
    let (config_protect_mask, config_protect_mask_explicit) =
        match get(matches, "remote_config_protect_mask") {
            Some(raw) => (raw, true),
            None => ("/etc/env.d".to_string(), false),
        };
    let portuale_binary = get(matches, "remote_portuale_binary").filter(|b| !b.is_empty());
    let portuale_dir = match get(matches, "remote_portuale_dir").filter(|d| !d.is_empty()) {
        Some(raw) if !raw.starts_with('/') => {
            return Err(format!(
                "mrg: --remote-portuale-dir must be an absolute client path, got {raw:?}"
            ));
        }
        other => other,
    };
    Ok(Some(RemoteContext {
        hostname,
        user: get(matches, "remote_user").filter(|u| !u.is_empty()),
        port,
        key_file: get(matches, "remote_key_file").filter(|k| !k.is_empty()),
        timeout_secs,
        ssh_args: get(matches, "remote_ssh_args").filter(|a| !a.is_empty()),
        strict_host_key_checking,
        max_clock_skew_secs,
        root,
        workdir: get(matches, "remote_workdir")
            .unwrap_or_else(|| "/var/tmp/portage-remote".to_string()),
        transport,
        binpkg: get(matches, "remote_binpkg").filter(|b| !b.is_empty()),
        etc_portage: match get(matches, "remote_etc_portage") {
            None => ConfigPlacement::Client("/etc/portage".to_string()),
            Some(raw) => ConfigPlacement::parse("--remote-etc-portage", &raw)?,
        },
        vdb,
        edb,
        ledger_dir: get(matches, "remote_ledger_dir").filter(|d| !d.is_empty()),
        require_ledger_match: matches.get_flag("remote_require_ledger_match"),
        config_protect,
        config_protect_explicit,
        config_protect_mask,
        config_protect_mask_explicit,
        portuale_binary,
        portuale_dir,
        bin_plan: ClientBinPlan::Unresolved,
    }))
}

// --- Shell quoting ---------------------------------------------------------

/// Quote one argv word for `bash -s` payloads: single-quote, escaping
/// embedded quotes as `'\''`. Server-side, unit-tested -- client paths
/// are never string-interpolated raw.
pub fn sh_quote(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('\'');
    for chunk in word.split('\'') {
        // Rejoin with the close-escape-reopen sequence; the first split
        // boundary needs no opener (already inside quotes).
        if out.len() > 1 {
            out.push_str("'\\''");
        }
        out.push_str(chunk);
    }
    out.push('\'');
    out
}

// --- SSH transport ---------------------------------------------------------

/// Directory for ControlPersist sockets (`0700`). Unix-domain socket
/// paths cap at ~108 bytes, and `%C` alone still needs ~60 of those, so
/// candidates are tried shortest-first and any base that cannot fit is
/// skipped: `$XDG_RUNTIME_DIR` (usually `/run/user/<uid>`, the shortest
/// stable per-user dir), then `$HOME/.portuale/cp`. `None` when neither
/// exists nor fits -- the caller then skips multiplexing entirely
/// (slower, still correct).
fn control_dir() -> Option<std::path::PathBuf> {
    control_dir_for(
        std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from),
        std::env::var_os("HOME").map(std::path::PathBuf::from),
    )
}

/// Worst-case `%C` expansion plus ssh's own multiplex suffix, with
/// margin under the 108-byte `sun_path` cap.
const CONTROL_PATH_BUDGET: usize = 64;

fn control_dir_for(
    xdg_runtime: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(xdg) = xdg_runtime {
        candidates.push(xdg.join(".portuale-cp"));
    }
    if let Some(home) = home {
        candidates.push(home.join(".portuale/cp"));
    }
    candidates.into_iter().find(|dir| {
        if dir.to_string_lossy().len() + CONTROL_PATH_BUDGET >= 108 {
            return false;
        }
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        true
    })
}

/// Build the `ssh` argv (without the remote command): multiplexed,
/// non-interactive (`BatchMode=yes` fails fast instead of prompting),
/// rc 255 = transport error per the ansible convention.
fn ssh_argv(ctx: &RemoteContext, control: Option<&std::path::Path>) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-p".to_string(), ctx.port.to_string()];
    if let Some(key) = &ctx.key_file {
        argv.push("-i".to_string());
        argv.push(key.clone());
    }
    argv.push("-o".to_string());
    argv.push("BatchMode=yes".to_string());
    argv.push("-o".to_string());
    argv.push(format!("ConnectTimeout={}", ctx.timeout_secs));
    argv.push("-o".to_string());
    argv.push(format!(
        "StrictHostKeyChecking={}",
        ctx.strict_host_key_checking.as_ssh_value()
    ));
    if let Some(dir) = control {
        // `%C` hashes %l%h%p%r -- safe under the ~108-byte unix-socket
        // limit no matter how long the hostname is.
        let socket = dir.join("%C");
        argv.push("-o".to_string());
        argv.push("ControlMaster=auto".to_string());
        argv.push("-o".to_string());
        argv.push("ControlPersist=60s".to_string());
        argv.push("-o".to_string());
        argv.push(format!("ControlPath={}", socket.display()));
    }
    if let Some(extra) = &ctx.ssh_args {
        argv.extend(extra.split_whitespace().map(String::from));
    }
    if let Some(user) = &ctx.user {
        argv.push("-l".to_string());
        argv.push(user.clone());
    }
    argv.push(ctx.hostname.clone());
    argv
}

/// True when an ssh failure is a *transport* problem (vs. a remote
/// command failure): exit 255, spawn failure, or the classic fatal
/// signatures on stderr.
fn is_transport_error(code: Option<i32>, stderr: &str) -> bool {
    if code == Some(255) {
        return true;
    }
    code.is_none()
        && (stderr.contains("Connection refused")
            || stderr.contains("Connection timed out")
            || stderr.contains("No route to host")
            || stderr.contains("Could not resolve hostname")
            || stderr.contains("Permission denied"))
}

/// The local transport's `bash -s`: the client side of a `local` run is
/// this same host, so the child would inherit the server process's
/// environment. Two things must not ride along or be missing (#313):
/// the server's own VDB selection (`PORTUALE_VDB_*`, set by `mrg` for a
/// `--remote-vdb=server:` copy) -- a client phase's `has_version` /
/// `best_version` must read the *client's* VDB, which stays a files tree
/// (`mrg.rs` `setup_vdb`) -- and `PORTUALE_BIN`, which the vendored
/// `portageq-wrapper` shim execs (`#326 S8.3`: every generated script
/// exports the resolved absolute path, and local transport keeps
/// `current_exe()` with no install). Over ssh neither is forwarded, so
/// only this transport needs it.
fn local_client_bash() -> std::process::Command {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-s");
    for var in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        crate::vdb_ipc::IPC_VAR,
    ] {
        cmd.env_remove(var);
    }
    if let Ok(exe) = std::env::current_exe() {
        cmd.env("PORTUALE_BIN", exe);
    }
    cmd
}

/// Run `bash -s` with `script` on stdin; stdout/stderr captured
/// separately (machine log vs. human log). Over ssh, or as a local
/// subprocess when the transport is `local` (same driver, no network).
fn run_script_stdin(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    script: &str,
) -> Result<std::process::Output, String> {
    use std::io::Write as _;
    let mut child = match ctx.transport {
        RemoteTransport::Ssh => {
            let mut argv = ssh_argv(ctx, control);
            argv.push("bash".to_string());
            argv.push("-s".to_string());
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))?
        }
        RemoteTransport::Local => local_client_bash()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("mrg: cannot spawn local bash: {e}"))?,
    };
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: child stdin unavailable".to_string())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("mrg: writing to child stdin: {e}"))?;
    child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for child: {e}"))
}

// --- Resolve-path support (slice 5) --------------------------------------------

/// Temporarily point `PORTAGE_CONFIGROOT` at `dir` (pulled client config
/// or an explicit server path) for a resolve run, restoring the previous
/// value on drop. `unsafe` because `std::env::set_var` is (edition 2024):
/// SAFETY: the guarded region is `mrg`'s synchronous resolve path --
/// binary-only, so no build threads spawn, and every env read inside
/// observes one constant value; nothing else in the process writes this
/// variable concurrently.
pub(crate) struct ConfigRootOverride {
    previous: Option<std::ffi::OsString>,
}

impl ConfigRootOverride {
    pub(crate) fn set(dir: &std::path::Path) -> Self {
        let previous = std::env::var_os("PORTAGE_CONFIGROOT");
        // SAFETY: see the struct doc comment.
        unsafe {
            std::env::set_var("PORTAGE_CONFIGROOT", dir);
        }
        Self { previous }
    }
}

impl Drop for ConfigRootOverride {
    fn drop(&mut self) {
        // SAFETY: see the struct doc comment.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
        }
    }
}

/// A placed `/etc/portage` config root for a resolve run: `Server`
/// paths are used directly, `Client` trees are pulled once into a temp
/// dir re-rooted as `<tmp>/etc/portage` (a valid `PORTAGE_CONFIGROOT`,
/// real-root layout) with `ConfigRootOverride` held for the whole
/// resolve. The pulled temp dir is removed best-effort on drop, once
/// the resolve is done (backlog #171c review: both the resolve site
/// and `--remote-binpkg` leaked one
/// `/tmp/portuale-remote-etc-<pid>-<nanos>` per run). The pulled tree
/// additionally carries the server's own `make.globals` seeded under
/// `<tmp>/usr/share/portage/config/` (backlog #171 follow-up 3) -- the
/// pull only carries the client's `/etc/portage` contents, so without
/// the seed the resolve would stack no base layer at all. Shared by
/// `run_remote_resolve` and `run_bundle_stage` -- one placement match,
/// no duplicated pull logic. A dangling pulled `make.profile` symlink is
/// additionally remapped onto the server's repos
/// (`remap_client_make_profile`, backlog #171 follow-up 4) -- same
/// helper, so both paths resolve the client profile the same way.
pub(crate) struct PlacedConfig {
    /// The dir to resolve from (the server path, or the pulled temp root).
    pub(crate) dir: std::path::PathBuf,
    _config_guard: ConfigRootOverride,
    /// Temp dir holding a pulled client tree (`None` for `Server`).
    tmp: Option<std::path::PathBuf>,
}

impl Drop for PlacedConfig {
    fn drop(&mut self) {
        if let Some(tmp) = &self.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
    }
}

/// Lexically normalize an absolute path (fold `.`/`..` without touching
/// the filesystem -- the client tree isn't here, only pulled symlink
/// text, so `canonicalize` would resolve the wrong machine's paths).
fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

/// Resolve a pulled client `make.profile` symlink against the server's
/// repos (backlog #171 follow-up 4, owner Q10 = b: the client may hold no
/// repo at all, so a symlink into the client's repo path dangles here and
/// the profile chain would silently resolve to `[]`).
///
/// Only dangling symlinks whose target text names a repo profile (a
/// `/profiles/` component in the client-absolute target) are rewritten:
/// to the same-named server repo's `<location>/profiles/<rel>` when the
/// pulled client `repos.conf` (if any) has a repo whose `location` is the
/// link prefix, else to the first server repo -- in `find_repos` priority
/// order, main repo first -- providing `<location>/profiles/<rel>`. A
/// link that already resolves inside the pulled tree (e.g. a relative
/// link to a custom profile shipped under `/etc/portage`, or a loopback
/// absolute link), a directory `make.profile` (its `parent` entries ride
/// the normal profile machinery), a missing one, and a target naming no
/// repo profile are all left alone. Nothing found is a hard `mrg: !!! …`
/// error naming the target, never a silent empty chain.
///
/// Real grounding (`3rdparty/portage` = portage-3.0.82.2): the profile
/// root is `<config_root>/etc/make.profile` followed as a link
/// (`package/ebuild/_config/LocationsManager.py:119-149`, `realpath`ed
/// at `:154-157` for repo matching), and real warns it "should point
/// into a profile within $PORTDIR/profiles/"
/// (`package/ebuild/config.py:1444-1450`) -- the rewrite keeps exactly
/// that shape, server-side, then reuses the existing resolver.
fn remap_client_make_profile(
    tmp: &std::path::Path,
    client_etc_portage: &str,
    server_config_root: &std::path::Path,
) -> Result<(), String> {
    let link = tmp.join("etc/portage/make.profile");
    let meta = match link.symlink_metadata() {
        Ok(meta) => meta,
        Err(_) => return Ok(()),
    };
    if !meta.file_type().is_symlink() {
        return Ok(());
    }
    // Already resolving server-side (a shipped in-tree profile, or a
    // loopback absolute link): leave it -- only dangling links need the
    // server-repo mapping.
    if link.is_dir() {
        return Ok(());
    }
    // The link target *text* -- never followed on the client (the pull is
    // tar-only); relative targets resolve against the client's own
    // `/etc/portage` dir, lexically.
    let target = std::fs::read_link(&link)
        .map_err(|e| format!("mrg: reading the pulled client make.profile link: {e}"))?;
    let target_text = target.display().to_string();
    let client_abs = if target.is_absolute() {
        target.clone()
    } else {
        lexical_normalize(&std::path::Path::new(client_etc_portage).join(&target))
    };
    let client_abs_text = client_abs.to_string_lossy().into_owned();
    let Some(split) = client_abs_text.find("/profiles/") else {
        return Ok(());
    };
    let prefix = lexical_normalize(std::path::Path::new(&client_abs_text[..split]));
    let rel = client_abs_text[split + "/profiles/".len()..].to_string();
    if rel.is_empty() {
        return Err(format!(
            "mrg: !!! the pulled client make.profile points at {target_text}, which names no profile under profiles/"
        ));
    }
    // Step 2: the pulled client `repos.conf` (if any) names the repo
    // whose `location` is the link prefix; the same-named server repo
    // provides the profile.
    let client_repo_name: Option<String> = if tmp
        .join("etc/portage/repos.conf")
        .symlink_metadata()
        .is_ok()
    {
        portage_repo::find_repos(tmp).ok().and_then(|repos| {
            repos.into_iter().find_map(|repo| {
                (lexical_normalize(&repo.location) == prefix).then(|| repo.name.clone())
            })
        })
    } else {
        None
    };
    let server_repos = portage_repo::find_repos(server_config_root).map_err(|e| {
        format!(
            "mrg: !!! the pulled client make.profile points at {target_text}, and the server repos cannot be listed: {e}"
        )
    })?;
    // The same-named server repo first (a step-2 hit), then every server
    // repo in priority order with main first (`find_repos` already sorts
    // that way): the first `<location>/profiles/<rel>` that exists wins.
    let mut ordered: Vec<&portage_repo::RepoConfig> = Vec::new();
    if let Some(name) = client_repo_name.as_deref() {
        ordered.extend(server_repos.iter().filter(|repo| repo.name == name));
    }
    ordered.extend(server_repos.iter().filter(|repo| {
        client_repo_name
            .as_deref()
            .is_none_or(|name| repo.name != name)
    }));
    for repo in ordered {
        let candidate = repo.location.join("profiles").join(&rel);
        if candidate.is_dir() {
            std::fs::remove_file(&link)
                .map_err(|e| format!("mrg: replacing the pulled client make.profile link: {e}"))?;
            std::os::unix::fs::symlink(&candidate, &link).map_err(|e| {
                format!(
                    "mrg: pointing the pulled client make.profile at {}: {e}",
                    candidate.display()
                )
            })?;
            return Ok(());
        }
    }
    // Step 5: nothing found -- fail loudly, naming the target.
    let name_note = client_repo_name
        .as_deref()
        .map_or(String::new(), |name| format!(" for client repo {name:?}"));
    Err(format!(
        "mrg: !!! the pulled client make.profile points at {target_text}: no server repo provides profiles/{rel}{name_note} ({} server repo(s) checked)",
        server_repos.len()
    ))
}

/// Place `/etc/portage` per `ConfigPlacement` (see `PlacedConfig`).
/// `Err` is a pull failure (already a full `mrg: …` line); callers
/// print it and exit 1, like the resolve path's own errors.
pub(crate) fn place_config_root(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<PlacedConfig, String> {
    // Backlog #171 follow-up 3 (owner Q9 = b): the server's own
    // `make.globals` -- located exactly the way the local path locates
    // it (`resolve_config` reads
    // `<config_root>/usr/share/portage/config/make.globals`, which is
    // the live `/usr/share/portage/config/make.globals` for a `/` run;
    // real sources it from `global_config_path` regardless of
    // `config_root`, `config.py:446-499,569` +
    // `_config/LocationsManager.py:413-415`). Read before the
    // `ConfigRootOverride` below repoints `PORTAGE_CONFIGROOT` at the
    // placed dir, so this still names the server file. `Server`
    // placements need no seed (the placed root is server-side already);
    // the pulled `Client` tree gets it as the resolve's bottom layer,
    // under the client profile chain and the pulled `make.conf` --
    // real's `config` stacking order. Absent on the server (tests):
    // contributes nothing, deterministically.
    let server_config_root = portage_repo::config_root_from_env();
    let server_globals = server_config_root.join("usr/share/portage/config/make.globals");
    let (dir, tmp): (std::path::PathBuf, Option<std::path::PathBuf>) = match &ctx.etc_portage {
        ConfigPlacement::Server(path) => (std::path::PathBuf::from(path), None),
        ConfigPlacement::Client(path) => {
            let tmp = TempDir::new("portuale-remote-etc").keep();
            // The pulled tree is the *contents* of the client's
            // `/etc/portage`; re-root it as `<tmp>/etc/portage` so the
            // dir is a valid `PORTAGE_CONFIGROOT` (real-root layout).
            let pulled_portage = tmp.join("etc/portage");
            if let Err(message) = pull_dir(ctx, control, path, &pulled_portage) {
                // Best-effort: don't leave a partial pull behind.
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(message);
            }
            if server_globals.is_file() {
                let dest = tmp.join("usr/share/portage/config/make.globals");
                if let Some(parent) = dest.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(format!(
                        "mrg: staging the server make.globals under {}: {e}",
                        tmp.display()
                    ));
                }
                if let Err(e) = std::fs::copy(&server_globals, &dest) {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(format!(
                        "mrg: staging the server make.globals under {}: {e}",
                        tmp.display()
                    ));
                }
            }
            // Backlog #171 follow-up 4 (owner Q10 = b): the pulled
            // `make.profile` symlink dangles when the client holds its
            // repos elsewhere (or none at all) -- resolve its target
            // text against the server's repos before the
            // `ConfigRootOverride` below repoints `PORTAGE_CONFIGROOT`
            // at the pulled root (server repos must be listed from the
            // server root, like the seed above). `Server` placements
            // are untouched.
            if let Err(message) = remap_client_make_profile(&tmp, path, &server_config_root) {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(message);
            }
            // Backlog #171 follow-up 5 (l171f; review of l171d/l171e):
            // the pulled client tree may hold no `repos.conf` at all
            // (owner Q10: the client operates without the gentoo
            // repository) while the server holds the repos -- but
            // `find_repos` on the placed root fails `NoReposConf`
            // without the user `etc/portage/repos.conf` path, so both
            // resolving paths hard-errored after a successful remap.
            // Seed the server's own global `repos.conf` (read from the
            // server root like the `make.globals` seed above, before
            // the `ConfigRootOverride`), but only when the pulled
            // client tree has none: a client `repos.conf`, if present,
            // still wins -- real layers the global file before the
            // user's in one parser, so the user wins per key
            // (`repository/config.py:1488-1508`), and the user slot is
            // never clobbered here. Absent on the server (or a
            // directory rather than a file): contributes nothing, and
            // the loud `NoReposConf` error is kept -- never a silent
            // empty repo set. Same file-copy mechanism (with tmp
            // cleanup) as the `make.globals` seed.
            let server_repos_conf = server_config_root.join("usr/share/portage/config/repos.conf");
            if tmp
                .join("etc/portage/repos.conf")
                .symlink_metadata()
                .is_err()
                && server_repos_conf.is_file()
                && let Err(e) =
                    std::fs::copy(&server_repos_conf, tmp.join("etc/portage/repos.conf"))
            {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(format!(
                    "mrg: staging the server repos.conf under {}: {e}",
                    tmp.display()
                ));
            }
            (tmp.clone(), Some(tmp.clone()))
        }
    };
    let guard = ConfigRootOverride::set(&dir);
    Ok(PlacedConfig {
        dir,
        _config_guard: guard,
        tmp,
    })
}

/// Run an arbitrary remote command (`tar`, …), not just `bash -s`.
/// stdout bytes come back to the caller (used by the config pull).
/// `remote_argv` is an argv, never shell text: over ssh, which joins its
/// trailing words with spaces for the remote shell, each word is
/// `sh_quote`d, so a client path with spaces or metacharacters (e.g. a
/// `--remote-portuale-dir`) stays one word (#326 S8 review).
fn run_raw_command(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_argv: &[String],
) -> Result<std::process::Output, String> {
    match ctx.transport {
        RemoteTransport::Local => {
            let (head, tail) = remote_argv
                .split_first()
                .ok_or_else(|| "mrg: empty remote command".to_string())?;
            std::process::Command::new(head)
                .args(tail)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("mrg: cannot spawn local command: {e}"))
        }
        RemoteTransport::Ssh => {
            let mut argv = ssh_argv(ctx, control);
            argv.extend(remote_argv.iter().map(|word| sh_quote(word)));
            std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))
        }
    }
}

/// Pull a remote directory to a local one (`tar -c` remotely, `tar -x`
/// locally). Used for `/etc/portage` under the `client:` placement.
fn pull_dir(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_dir: &str,
    local_dir: &std::path::Path,
) -> Result<(), String> {
    let output = run_raw_command(
        ctx,
        control,
        &[
            "tar".to_string(),
            "-c".to_string(),
            "-C".to_string(),
            remote_dir.to_string(),
            ".".to_string(),
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "mrg: pulling {remote_dir} from {} failed (exit {})",
            ctx.hostname,
            output.status.code().unwrap_or(-1)
        ));
    }
    std::fs::create_dir_all(local_dir).map_err(|e| format!("{}: {e}", local_dir.display()))?;
    let mut child = std::process::Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(local_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("mrg: cannot spawn local tar: {e}"))?;
    use std::io::Write as _;
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: tar stdin unavailable".to_string())?
        .write_all(&output.stdout)
        .map_err(|e| format!("mrg: feeding local tar: {e}"))?;
    let unpack = child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for local tar: {e}"))?;
    if !unpack.status.success() {
        return Err("mrg: unpacking pulled config failed".to_string());
    }
    Ok(())
}

/// Ship local file bytes to `<workdir>/<name>` on the client: `sh -c
/// 'cat > …'` over ssh (the `>` must be remote-side, hence the explicit
/// `sh -c` with one pre-quoted word), plain `fs::copy` for local.
fn send_file(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    local_path: &std::path::Path,
    dest: &str,
) -> Result<(), String> {
    let bytes = std::fs::read(local_path).map_err(|e| format!("{}: {e}", local_path.display()))?;
    send_bytes(ctx, control, &bytes, dest)
}

/// Ship in-memory bytes to a client path: the `send_file` transport
/// with bytes the server never wrote to disk (old-hook envs staged for
/// the merge driver, backlog #171).
fn send_bytes(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    bytes: &[u8],
    dest: &str,
) -> Result<(), String> {
    match ctx.transport {
        RemoteTransport::Local => {
            if let Some(parent) = std::path::Path::new(dest).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(dest, bytes).map_err(|e| format!("mrg: writing {dest} failed: {e}"))?;
            Ok(())
        }
        RemoteTransport::Ssh => {
            use std::io::Write as _;
            let mut argv = ssh_argv(ctx, control);
            argv.push("sh".to_string());
            argv.push("-c".to_string());
            argv.push(format!("cat > {}", sh_quote(dest)));
            let mut child = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("mrg: cannot spawn ssh: {e}"))?;
            child
                .stdin
                .take()
                .ok_or_else(|| "mrg: ssh stdin unavailable".to_string())?
                .write_all(bytes)
                .map_err(|e| format!("mrg: writing to ssh stdin: {e}"))?;
            let output = child
                .wait_with_output()
                .map_err(|e| format!("mrg: waiting for ssh: {e}"))?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() && is_transport_error(code, &stderr) {
                return Err(format!(
                    "mrg: client {} unreachable:\n{stderr}",
                    ctx.hostname
                ));
            }
            if !output.status.success() {
                return Err(format!(
                    "mrg: sending {dest} failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            Ok(())
        }
    }
}

// --- Preflight -------------------------------------------------------------

/// Read-only gates, generated bash (no arrays, no `set -e` -- every gate
/// reports instead of aborting). `KEY=VALUE` on stdout, human log on
/// stderr. `@ROOT@`/`@WORKDIR@` are substituted server-side, pre-quoted.
/// `bin` is `(dirs, candidates)` from the S8.1 search (#326 D5): per
/// candidate the full digest or `missing`, plus `uname -m` and each
/// directory's existence, writability and octal mode. `None` skips the
/// binary section (local transport without the override installs
/// nothing, so there is nothing to report).
fn preflight_script(root: &str, workdir: &str, bin: Option<(&[String], &[String])>) -> String {
    let mut script = format!(
        r#"echo "PREFLIGHT=1"
echo "BASH_MAJOR=${{BASH_VERSINFO[0]}}"
echo "BASH_MINOR=${{BASH_VERSINFO[1]}}"
for t in tar mkdir rm cat chmod ln find grep sed cmp stat readlink id tail sha256sum mv uname mktemp; do
  if command -v "$t" >/dev/null 2>&1; then echo "TOOL_$t=yes"; else echo "TOOL_$t=no"; fi
done
ROOT={root}
WORKDIR={workdir}
if [ -w "$ROOT" ]; then echo "ROOT_WRITABLE=yes"; else echo "ROOT_WRITABLE=no"; fi
if [ -d "$ROOT/var/db/pkg" ]; then echo "VDB_DIR=yes"; else echo "VDB_DIR=no"; fi
if [ -d "$WORKDIR" ]; then
  if [ -w "$WORKDIR" ]; then echo "WORKDIR=writable"; else echo "WORKDIR=unwritable"; fi
elif mkdir -p "$WORKDIR" 2>/dev/null; then
  rmdir "$WORKDIR" 2>/dev/null
  echo "WORKDIR=creatable"
else
  echo "WORKDIR=uncreatable"
fi
echo "CLIENT_TIME=$(date +%s)"
"#,
        root = sh_quote(root),
        workdir = sh_quote(workdir),
    );
    if let Some((dirs, candidates)) = bin {
        script.push_str("echo \"UNAME_M=$(uname -m)\"\n");
        for (i, candidate) in candidates.iter().enumerate() {
            script.push_str(&format!(
                concat!(
                    "CAND={cand}\n",
                    "if [ -f \"$CAND\" ]; then echo \"CAND_{i}_SHA=$(sha256sum -- \"$CAND\" 2>/dev/null | cut -d' ' -f1)\"; ",
                    "else echo \"CAND_{i}_SHA=missing\"; fi\n",
                ),
                cand = sh_quote(candidate),
                i = i,
            ));
        }
        for (j, dir) in dirs.iter().enumerate() {
            script.push_str(&format!(
                concat!(
                    "BINDIR={dir}\n",
                    "if [ -d \"$BINDIR\" ]; then echo \"DIR_{j}_EXISTS=yes\"; else echo \"DIR_{j}_EXISTS=no\"; fi\n",
                    "if [ -w \"$BINDIR\" ]; then echo \"DIR_{j}_WRITABLE=yes\"; else echo \"DIR_{j}_WRITABLE=no\"; fi\n",
                    "if [ -d \"$BINDIR\" ]; then echo \"DIR_{j}_MODE=$(stat -c %a -- \"$BINDIR\" 2>/dev/null || echo unknown)\"; fi\n",
                ),
                dir = sh_quote(dir),
                j = j,
            ));
        }
    }
    script
}

/// Parse `KEY=VALUE` stdout lines into a map (anything else ignored).
fn parse_kv(stdout: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=')
            && !key.is_empty()
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

/// Evaluate the preflight gates: `(failures, warnings)`. Hard gates fail
/// the run; a missing vdb only warns in slice 1 (placement matrix and
/// the stateless degrade land in slice 5 -- see docs/remote-merge.md §7).
fn preflight_gates(values: &HashMap<String, String>) -> (Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let major: u32 = values
        .get("BASH_MAJOR")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let minor: u32 = values
        .get("BASH_MINOR")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if major < 5 || (major == 5 && minor < 3) {
        failures.push(format!(
            "client bash is {major}.{minor}, need 5.3 or better"
        ));
    }
    for tool in [
        "tar",
        "mkdir",
        "rm",
        "cat",
        "chmod",
        "ln",
        "find",
        "grep",
        "sed",
        "cmp",
        "stat",
        "readlink",
        "id",
        "tail",
        "sha256sum",
        "mv",
        "uname",
        "mktemp",
    ] {
        if values
            .get(format!("TOOL_{tool}").as_str())
            .map(String::as_str)
            != Some("yes")
        {
            failures.push(format!("client is missing /bin/{tool}"));
        }
    }
    if values.get("ROOT_WRITABLE").map(String::as_str) != Some("yes") {
        failures.push("client target ROOT is not writable".to_string());
    }
    match values.get("WORKDIR").map(String::as_str) {
        Some("writable") | Some("creatable") => {}
        Some(other) => failures.push(format!("client workdir is {other}")),
        None => failures.push("client workdir check missing from preflight output".to_string()),
    }
    if values.get("VDB_DIR").map(String::as_str) != Some("yes") {
        warnings.push(
            "client vdb not found under the target ROOT (stateless degrade, slice 5)".to_string(),
        );
    }
    (failures, warnings)
}

/// Clock gate: `|server - client| <= max_skew` (`0` disables). Returns
/// the failure message, if any.
fn clock_gate(client_time: Option<&str>, server_time: u64, max_skew: u64) -> Option<String> {
    if max_skew == 0 {
        return None;
    }
    let client: u64 = client_time?.parse().ok()?;
    let skew = server_time.abs_diff(client);
    if skew > max_skew {
        Some(format!(
            "client/server clocks differ by {skew}s (limit {max_skew}s)"
        ))
    } else {
        None
    }
}

// --- Client portuale binary (slice 8, #326 D5/Q1-Q3/Q5) ----------------------
//
// The server installs and verifies its own binary on the client: the file
// it ships is `current_exe()`, or `--remote-portuale-binary=<path>` (e.g.
// a static musl build when the server itself runs a dynamic dev build).
// The client must hold a file with the same full SHA-256; whatever file
// is found, the full digest is always what gets compared. `<hash>` in
// `portuale-<hash>` is the first 16 hex digits of that digest (Q5).
// Different server builds get different names, so they never overwrite
// each other; a plain `portuale` whose digest differs is never touched
// (it may be an operator's own install).

/// Default client search directories, in D5 order.
pub(crate) fn default_bin_dirs() -> Vec<String> {
    vec!["/opt/bin".to_string(), "/usr/local/bin".to_string()]
}

/// Candidate directories: only the `--remote-portuale-dir=<abs>` pair
/// when that option is given (Q1), else the default pair.
pub(crate) fn bin_search_dirs(ctx: &RemoteContext) -> Vec<String> {
    match &ctx.portuale_dir {
        Some(dir) => vec![dir.clone()],
        None => default_bin_dirs(),
    }
}

/// Candidate files in D5 order: per directory, `portuale` then
/// `portuale-<hash>`. The first file whose full digest matches is used.
pub(crate) fn bin_candidates(dirs: &[String], short_hash: &str) -> Vec<String> {
    let mut out = Vec::new();
    for dir in dirs {
        out.push(format!("{dir}/portuale"));
        out.push(format!("{dir}/portuale-{short_hash}"));
    }
    out
}

/// The server-side identity of the binary to ship.
pub(crate) struct ServerBinInfo {
    /// The file to stream (`--remote-portuale-binary`, or `current_exe()`).
    pub path: std::path::PathBuf,
    /// Full SHA-256 hex of the file.
    pub digest_hex: String,
    /// First 16 hex digits of the digest (the `<hash>` in file names).
    pub short_hash: String,
    /// ELF `e_machine` of the file.
    pub machine: u16,
}

/// Read the shipped binary's ELF `e_machine`: bytes 18-19, in the
/// endianness byte 5 names. Header bytes are read directly; no new crate.
pub(crate) fn elf_machine(bytes: &[u8]) -> Result<u16, String> {
    if bytes.len() < 20 {
        return Err("not an ELF binary (shorter than the 20-byte header prefix)".to_string());
    }
    if &bytes[0..4] != b"\x7fELF" {
        return Err("not an ELF binary (bad magic)".to_string());
    }
    match bytes[5] {
        1 => Ok(u16::from_le_bytes([bytes[18], bytes[19]])),
        2 => Ok(u16::from_be_bytes([bytes[18], bytes[19]])),
        other => Err(format!("unknown ELF data encoding {other}")),
    }
}

/// Hash the binary the server will ship and read its ELF machine (Q3).
/// Errors name `--remote-portuale-binary` when that option selected the
/// file.
pub(crate) fn server_bin_info(ctx: &RemoteContext) -> Result<ServerBinInfo, String> {
    let (path, named) = match &ctx.portuale_binary {
        Some(custom) => (std::path::PathBuf::from(custom), true),
        None => (
            std::env::current_exe().map_err(|e| {
                format!("mrg: cannot locate the running binary for the client install: {e}")
            })?,
            false,
        ),
    };
    let bytes = std::fs::read(&path).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!(
                "mrg: cannot read the running binary {} for the client install: {e}",
                path.display()
            )
        }
    })?;
    use sha2::Digest as _;
    let digest_hex = format!("{:x}", sha2::Sha256::digest(&bytes));
    let machine = elf_machine(&bytes).map_err(|e| {
        if named {
            format!("mrg: --remote-portuale-binary {}: {e}", path.display())
        } else {
            format!("mrg: the running binary {}: {e}", path.display())
        }
    })?;
    Ok(ServerBinInfo {
        short_hash: digest_hex[..16].to_string(),
        digest_hex,
        machine,
        path,
    })
}

/// `e_machine` values the gate knows, with the `uname -m` names each
/// maps to.
fn machine_unames(machine: u16) -> Option<&'static [&'static str]> {
    match machine {
        // EM_386: the 32-bit x86 spellings `uname -m` prints.
        3 => Some(&["i386", "i486", "i586", "i686"]),
        // EM_PPC64: both endians run the same instruction set name here.
        21 => Some(&["ppc64", "ppc64le"]),
        // EM_S390.
        22 => Some(&["s390x"]),
        // EM_ARM.
        40 => Some(&["armv5tel", "armv6l", "armv7l", "armv8l", "arm"]),
        // EM_X86_64 (`amd64` is the FreeBSD `uname -m` spelling).
        62 => Some(&["x86_64", "amd64"]),
        // EM_AARCH64 (`arm64` is the macOS spelling).
        183 => Some(&["aarch64", "arm64"]),
        // EM_RISCV.
        243 => Some(&["riscv64"]),
        _ => None,
    }
}

/// Human name for an `e_machine` in gate messages.
pub(crate) fn machine_name(machine: u16) -> String {
    match machine_unames(machine) {
        Some(names) => names[0].to_string(),
        None => format!("e_machine {machine}"),
    }
}

/// Whether the shipped binary runs on the client (Q3): `Some(true)` when
/// the client's `uname -m` is one of the machine's names, `Some(false)`
/// on a definite mismatch, `None` when either side is unknown -- an
/// unknown pair is a mismatch only when both are known.
pub(crate) fn arch_matches(machine: u16, uname_m: &str) -> Option<bool> {
    let names = machine_unames(machine)?;
    if uname_m.is_empty() {
        return None;
    }
    Some(names.contains(&uname_m))
}

/// Server-side preflight facts for the binary gates: what to look for
/// and what to compare against. `None` for local transport without
/// `--remote-portuale-dir`, which keeps `current_exe()` and installs
/// nothing (D5).
pub(crate) struct BinPreflight {
    /// `bin_search_dirs(ctx)`.
    pub dirs: Vec<String>,
    /// `bin_candidates(&dirs, &short)`, in D5 order.
    pub candidates: Vec<String>,
    /// Full expected SHA-256 hex.
    pub digest: String,
    /// The file the digest (and bytes) come from, for messages.
    pub bin_path: String,
    /// ELF `e_machine` of that file.
    pub machine: u16,
}

/// Compute the binary preflight facts, unless this run skips the gates
/// by design (local transport without the override).
pub(crate) fn bin_preflight(ctx: &RemoteContext) -> Result<Option<BinPreflight>, String> {
    if ctx.transport == RemoteTransport::Local && ctx.portuale_dir.is_none() {
        return Ok(None);
    }
    let info = server_bin_info(ctx)?;
    let dirs = bin_search_dirs(ctx);
    let candidates = bin_candidates(&dirs, &info.short_hash);
    Ok(Some(BinPreflight {
        dirs,
        candidates,
        digest: info.digest_hex,
        bin_path: info.path.display().to_string(),
        machine: info.machine,
    }))
}

/// Whether `dir` (preflight index `j`) may receive the install: it must
/// exist and be writable, and a world-writable directory without the
/// sticky bit is refused (D5). `Err` names the reason for the report.
fn dir_writable_for_install(
    values: &HashMap<String, String>,
    j: usize,
    dir: &str,
) -> Result<(), String> {
    let yes = "yes".to_string();
    if values.get(&format!("DIR_{j}_EXISTS")) != Some(&yes) {
        return Err(format!("client directory {dir} does not exist"));
    }
    if values.get(&format!("DIR_{j}_WRITABLE")) != Some(&yes) {
        return Err(format!("client directory {dir} is not writable"));
    }
    let mode_text = values
        .get(&format!("DIR_{j}_MODE"))
        .map(String::as_str)
        .unwrap_or("");
    let mode = u32::from_str_radix(mode_text.trim(), 8)
        .map_err(|_| format!("client directory {dir}: cannot read its mode ({mode_text:?})"))?;
    if mode & 0o002 != 0 && mode & 0o1000 == 0 {
        return Err(format!(
            "client directory {dir} is world-writable without the sticky bit (refused)"
        ));
    }
    Ok(())
}

/// Evaluate the S8.1 gates over parsed preflight values: `(plan,
/// failures, notes)`. The first digest match wins; no match plans an
/// install as `<dir>/portuale-<hash>` in the first usable directory; a
/// mismatched plain `portuale` is a note (reported, left alone); an
/// arch mismatch or no usable directory fails.
pub(crate) fn evaluate_bin_gates(
    values: &HashMap<String, String>,
    pre: &BinPreflight,
    override_given: bool,
) -> (ClientBinPlan, Vec<String>, Vec<String>) {
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let uname_m = values.get("UNAME_M").map(String::as_str).unwrap_or("");
    if arch_matches(pre.machine, uname_m) == Some(false) {
        failures.push(format!(
            "client architecture is {uname_m:?} but the shipped binary (--remote-portuale-binary {}) is built for {}: build the client architecture and pass it with --remote-portuale-binary",
            pre.bin_path,
            machine_name(pre.machine),
        ));
        return (ClientBinPlan::Unresolved, failures, notes);
    }
    let short: String = pre.digest.chars().take(16).collect();
    let mut corrupted_hashed: Vec<String> = Vec::new();
    for (i, path) in pre.candidates.iter().enumerate() {
        let sha = values
            .get(&format!("CAND_{i}_SHA"))
            .map(String::as_str)
            .unwrap_or("missing");
        if sha == pre.digest {
            return (ClientBinPlan::Use(path.clone()), failures, notes);
        }
        let plain = path.rsplit('/').next().unwrap_or(path) == "portuale";
        if !plain && sha != "missing" && !sha.is_empty() {
            corrupted_hashed.push(path.clone());
        }
        if plain && sha != "missing" && !sha.is_empty() {
            notes.push(format!(
                "client {path} has a different SHA-256 (an operator install?); left alone"
            ));
        }
    }
    // No match: the first usable directory takes `<dir>/portuale-<hash>`.
    let mut reasons = Vec::new();
    for (j, dir) in pre.dirs.iter().enumerate() {
        match dir_writable_for_install(values, j, dir) {
            Ok(()) => {
                let target = format!("{dir}/portuale-{short}");
                if corrupted_hashed.contains(&target) {
                    notes.push(format!(
                        "client {target} does not match its name (corrupted); planned for replacement"
                    ));
                }
                return (ClientBinPlan::Install(target), failures, notes);
            }
            Err(reason) => reasons.push(reason),
        }
    }
    if override_given {
        failures.push(format!(
            "mrg: --remote-portuale-dir {}: {}",
            pre.dirs.first().map(String::as_str).unwrap_or("?"),
            reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "no usable directory".to_string()),
        ));
    } else {
        failures.push(
            "mrg: no writable client directory for the portuale binary (/opt/bin and /usr/local/bin are missing, unwritable, or world-writable without the sticky bit): pass --remote-portuale-dir=<abs dir> pointing at a directory the client login can write, or run as a root login".to_string(),
        );
    }
    (ClientBinPlan::Unresolved, failures, notes)
}

/// Run the install stage when the plan says so (S8.2): `Use`/`Local`
/// are no-ops (no status line -- a reuse prints nothing);
/// `Unresolved` is an internal error (preflight never ran).
pub(crate) fn ensure_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<(), String> {
    match &ctx.bin_plan {
        ClientBinPlan::Install(target) => {
            let pre = bin_preflight(ctx)?.ok_or_else(|| {
                "mrg: internal error: the client binary plan needs an install but preflight skipped the binary gates".to_string()
            })?;
            install_client_binary(ctx, control, target, &pre.digest, &pre.bin_path)
        }
        ClientBinPlan::Use(_) | ClientBinPlan::Local(_) => Ok(()),
        ClientBinPlan::Unresolved => Err(
            "mrg: internal error: the client binary was never resolved (preflight did not run)"
                .to_string(),
        ),
    }
}

/// Digest of a client path, if it exists: `None` when missing. Only a
/// transport failure is an `Err`; a missing file is not one.
fn client_file_digest(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    path: &str,
) -> Result<Option<String>, String> {
    match ctx.transport {
        RemoteTransport::Local => {
            let bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(format!("mrg: reading {path}: {e}")),
            };
            use sha2::Digest as _;
            Ok(Some(format!("{:x}", sha2::Sha256::digest(&bytes))))
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &["sha256sum".to_string(), "--".to_string(), path.to_string()],
            )?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                if is_transport_error(code, &stderr) {
                    return Err(format!(
                        "mrg: client {} unreachable:\n{stderr}",
                        ctx.hostname
                    ));
                }
                return Ok(None);
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout
                .split_whitespace()
                .next()
                .map(String::from)
                .filter(|hex| hex.len() == 64))
        }
    }
}

/// Best-effort removal of a client temp path (the install's failure
/// cleanup, S8.2).
fn remove_client_path(ctx: &RemoteContext, control: Option<&std::path::Path>, path: &str) {
    match ctx.transport {
        RemoteTransport::Local => {
            let _ = std::fs::remove_file(path);
        }
        RemoteTransport::Ssh => {
            let _ = run_raw_command(
                ctx,
                control,
                &[
                    "rm".to_string(),
                    "-f".to_string(),
                    "--".to_string(),
                    path.to_string(),
                ],
            );
        }
    }
}

/// Run `"<abs>" __helper ping` on the client and require the D8 token on
/// stdout: the installed binary really executes there.
fn ping_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    abs: &str,
) -> Result<(), String> {
    let output = run_raw_command(
        ctx,
        control,
        &[abs.to_string(), "__helper".to_string(), "ping".to_string()],
    )
    .map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local client binary ping failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if ctx.transport == RemoteTransport::Ssh && is_transport_error(code, &stderr) {
            return Err(format!(
                "mrg: client {} unreachable:\n{stderr}",
                ctx.hostname
            ));
        }
        return Err(format!(
            "mrg: client binary {abs} failed its __helper ping (exit {}):\n{stderr}",
            code.unwrap_or(-1)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim() != crate::helpers::PING_TOKEN {
        return Err(format!(
            "mrg: client binary {abs} did not answer the __helper ping (no D8 token)"
        ));
    }
    Ok(())
}

/// The S8.2 install: `mktemp <dir>/.portuale.XXXXXX`, stream the server
/// binary over the existing connection (the way bundles stream), `chmod
/// 0755`, re-check the digest, `mv` to `<dir>/portuale-<hash>`, then
/// `"<abs>" __helper ping` (which must print the D8 token). On any
/// failure the temp file is removed and the run fails before any unit
/// is consumed. Prints the status line `portuale-remote: install-bin
/// <rc>`.
fn install_client_binary(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    target: &str,
    expected: &str,
    bin_path: &str,
) -> Result<(), String> {
    // Idempotent: a matching target only needs its ping (a reuse, so no
    // status line -- the second run reports no `install-bin` line).
    match client_file_digest(ctx, control, target)? {
        Some(digest) if digest == expected => {
            ping_client_binary(ctx, control, target)?;
            return Ok(());
        }
        _ => {}
    }
    let bytes = std::fs::read(bin_path)
        .map_err(|e| format!("mrg: reading the shipped binary {bin_path}: {e}"))?;
    let dir = std::path::Path::new(target)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| format!("mrg: install target {target} has no parent directory"))?;
    // `mktemp <dir>/.portuale.XXXXXX`.
    let tmp: String = match ctx.transport {
        RemoteTransport::Local => {
            let mut attempt = 0;
            loop {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|t| t.as_nanos())
                    .unwrap_or(0);
                let candidate = format!(
                    "{dir}/.portuale.{}.{attempt}.{nanos}.tmp",
                    std::process::id()
                );
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(_) => break candidate,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => {
                        attempt += 1;
                    }
                    Err(e) => {
                        return Err(format!("mrg: creating the client temp file: {e}"));
                    }
                }
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &["mktemp".to_string(), format!("{dir}/.portuale.XXXXXX")],
            )?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() {
                if is_transport_error(code, &stderr) {
                    return Err(format!(
                        "mrg: client {} unreachable:\n{stderr}",
                        ctx.hostname
                    ));
                }
                return Err(format!(
                    "mrg: client mktemp in {dir} failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if name.is_empty() || !name.starts_with(&dir) {
                remove_client_path(ctx, control, &name);
                return Err(format!(
                    "mrg: client mktemp answered unexpectedly: {name:?}"
                ));
            }
            name
        }
    };
    let fail = |step: &str, detail: String| {
        remove_client_path(ctx, control, &tmp);
        format!("mrg: client binary install failed at {step}: {detail}")
    };
    // Write from stdin (streamed over the existing connection).
    if let Err(message) = send_bytes(ctx, control, &bytes, &tmp) {
        return Err(fail("stream", message));
    }
    // `chmod 0755`.
    match ctx.transport {
        RemoteTransport::Local => {
            use std::os::unix::fs::PermissionsExt as _;
            if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)) {
                return Err(fail("chmod", e.to_string()));
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &[
                    "chmod".to_string(),
                    "0755".to_string(),
                    "--".to_string(),
                    tmp.clone(),
                ],
            )
            .map_err(|message| fail("chmod", message))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(fail(
                    "chmod",
                    format!("exit {}:\n{stderr}", output.status.code().unwrap_or(-1)),
                ));
            }
        }
    }
    // Re-check the digest before the rename (never an in-place write).
    match client_file_digest(ctx, control, &tmp)? {
        Some(digest) if digest == expected => {}
        other => {
            return Err(fail(
                "digest",
                format!("the streamed bytes do not match (got {other:?})"),
            ));
        }
    }
    // `mv` to `<dir>/portuale-<hash>`.
    match ctx.transport {
        RemoteTransport::Local => {
            if let Err(e) = std::fs::rename(&tmp, target) {
                return Err(fail("mv", e.to_string()));
            }
        }
        RemoteTransport::Ssh => {
            let output = run_raw_command(
                ctx,
                control,
                &[
                    "mv".to_string(),
                    "--".to_string(),
                    tmp.clone(),
                    target.to_string(),
                ],
            )
            .map_err(|message| fail("mv", message))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(fail(
                    "mv",
                    format!("exit {}:\n{stderr}", output.status.code().unwrap_or(-1)),
                ));
            }
        }
    }
    // The install must execute: `"<abs>" __helper ping` prints D8's token.
    if let Err(message) = ping_client_binary(ctx, control, target) {
        remove_client_path(ctx, control, target);
        return Err(message);
    }
    println!("portuale-remote: install-bin 0");
    Ok(())
}

/// `ssh-keygen -F <id>` output, if the id is already known (empty =
/// first contact). Best-effort: failures mean "unknown", never fatal.
/// Follows a custom `UserKnownHostsFile` from `--remote-ssh-args` --
/// without it ssh-keygen would read the default file while ssh used the
/// custom one (and vice versa).
fn known_host_lines(ctx: &RemoteContext, id: &str) -> String {
    let mut cmd = std::process::Command::new("ssh-keygen");
    if let Some(file) = known_hosts_file(ctx) {
        cmd.args(["-f", &file]);
    }
    cmd.args(["-F", id])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Fingerprint lines for a known id (`ssh-keygen -l -F`), best-effort.
fn host_fingerprint(ctx: &RemoteContext, id: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("ssh-keygen");
    if let Some(file) = known_hosts_file(ctx) {
        cmd.args(["-f", &file]);
    }
    let out = cmd.args(["-l", "-F", id]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A `-o Name=Value` ssh option's value inside `--remote-ssh-args`
/// (joined `-oName=V` and split `-o Name=V` spellings), if present.
fn ssh_option_value(ctx: &RemoteContext, name: &str) -> Option<String> {
    let extra = ctx.ssh_args.as_ref()?;
    let prefix = format!("{name}=");
    let mut tokens = extra.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if let Some(rest) = token.strip_prefix("-o") {
            if rest.is_empty() {
                if let Some(next) = tokens.next()
                    && let Some(value) = next.strip_prefix(&prefix)
                    && !value.is_empty()
                {
                    return Some(value.to_string());
                }
            } else if let Some(value) = rest.strip_prefix(&prefix)
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        } else if let Some(value) = token.strip_prefix(&prefix)
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Custom known-hosts file for both ssh and ssh-keygen lookups, if the
/// caller set one (tests isolate it; production defaults to ssh's own).
fn known_hosts_file(ctx: &RemoteContext) -> Option<String> {
    ssh_option_value(ctx, "UserKnownHostsFile")
}

/// The known_hosts id(s) this connection is stored under: a
/// `-o HostKeyAlias=X` inside `--remote-ssh-args` redirects it (else the
/// hostname itself, plus the `[host]:port` spelling for non-default
/// ports, which plain `-F host` does not match).
fn lookup_ids(ctx: &RemoteContext) -> Vec<String> {
    if let Some(alias) = ssh_option_value(ctx, "HostKeyAlias") {
        return vec![alias];
    }
    vec![
        ctx.hostname.clone(),
        format!("[{}]:{}", ctx.hostname, ctx.port),
    ]
}

/// True when none of the connection's lookup ids is already known.
fn is_first_contact(ctx: &RemoteContext) -> bool {
    lookup_ids(ctx)
        .iter()
        .all(|id| known_host_lines(ctx, id).trim().is_empty())
}

/// Fingerprint of the key used for this connection, if known now.
fn connection_fingerprint(ctx: &RemoteContext) -> Option<String> {
    lookup_ids(ctx)
        .iter()
        .find_map(|id| host_fingerprint(ctx, id))
}

/// mrg entry point for remote mode: `--remote-binpkg` runs the
/// single-file flow, target atoms run the resolve flow (which enforces
/// `--getbinpkgonly`). `--pretend` never reaches here (mrg routes it
/// locally with the remote options dropped).
pub fn run_remote_cli(matches: &ArgMatches, ctx: RemoteContext, argv: Vec<String>) -> ExitCode {
    if ctx.binpkg.is_some() {
        if matches.get_many::<String>("package").is_some() {
            eprintln!("mrg: --remote-binpkg cannot be combined with target atoms");
            return ExitCode::from(2);
        }
        return run_remote(&ctx);
    }
    // No binpkg and no atoms: the slice-1 preflight-only run (no payload,
    // no resolve) -- `--getbinpkgonly` is only forced once targets exist.
    if matches.get_many::<String>("package").is_none() {
        return run_remote(&ctx);
    }
    run_remote_resolve(matches, ctx, argv)
}

// --- Resolve-path support (slice 5) --------------------------------------------

/// Repo position for one merged entry: `(repo name, commit hash)` from
/// the repo checkout the entry resolved from (`unknown` when the repo is
/// not git or git is unavailable -- same honesty rule as elsewhere).
fn repo_position(
    repos: &[portage_repo::RepoConfig],
    repo_name: &Option<String>,
) -> (String, String) {
    let name = repo_name
        .clone()
        .unwrap_or_else(|| "__unknown__".to_string());
    let commit = repos
        .iter()
        .find(|repo| repo.name == name)
        .and_then(|repo| {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(&repo.location)
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|hash| hash.trim().to_string())
                .filter(|hash| !hash.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    (name, commit)
}

/// mrg atoms-mode entry point: enforce `--getbinpkgonly`, place
/// `/etc/portage`, hand the resolve to `pretend::run` with the remote
/// execution published. Exit 2 = usage error, else pretend's own code.
pub fn run_remote_resolve(
    matches: &ArgMatches,
    mut ctx: RemoteContext,
    argv: Vec<String>,
) -> ExitCode {
    if !matches.get_flag("getbinpkgonly") {
        eprintln!("mrg: remote execution requires --getbinpkgonly (no source build on client)");
        return ExitCode::from(2);
    }
    if matches.get_flag("buildpkgonly") {
        eprintln!(
            "mrg: --remote-* cannot be combined with --buildpkgonly (remote runs install, not build)"
        );
        return ExitCode::from(2);
    }
    if let Some(action) = crate::mrg::action_selected(matches) {
        eprintln!(
            "mrg: --remote-hostname cannot be combined with --{action} (remote runs install, not actions)"
        );
        return ExitCode::from(2);
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Fail-early stage 2 (plan §5.5): client sanity before resolving.
    let (values, bin) = match run_preflight_inner(&ctx, control) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (mut failures, mut warnings) = evaluate_preflight(&ctx, &values);
    // S8.1 binary gates (read-only): the plan rides the handoff into
    // `run_remote_plan`, which installs only when units ship. Local
    // transport without the override skips the gates and keeps
    // `current_exe()` (#326 D5).
    match bin {
        Some(pre) => {
            let override_given = ctx.portuale_dir.is_some();
            let (plan, mut bin_failures, bin_notes) =
                evaluate_bin_gates(&values, &pre, override_given);
            failures.append(&mut bin_failures);
            for note in bin_notes {
                warnings.push(note);
            }
            ctx.bin_plan = plan;
        }
        None => {
            let exe = std::env::current_exe().map(|p| p.display().to_string());
            match exe {
                Ok(path) => ctx.bin_plan = ClientBinPlan::Local(path),
                Err(e) => failures.push(format!("mrg: cannot locate the running binary: {e}")),
            }
        }
    }
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(&ctx);
    if !failures.is_empty() {
        return print_preflight_report(&ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(&ctx, &failures, &warnings, first_contact);
    // etc-portage placement → config root for the resolve (shared
    // helper: `Server` direct, `Client` pulled). `_placed` stays alive
    // across the resolve (config root + env override); its temp pull
    // is removed best-effort on drop at scope end.
    let _placed = match place_config_root(&ctx, control) {
        Ok(placed) => placed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    set_remote_exec(ctx);
    let code = crate::pretend::run(&argv);
    // Defensive: the dispatch site takes the handoff, but an early return
    // (resolve error, unexpected action) must not leak it in-process.
    let _ = take_remote_exec();
    code
}

/// BFS-drop the transitive dependents of a failed package (real
/// `_calc_resume_list`): every skipped cp records the failed cpv that
/// doomed it, for the `skipped (… failed)` trailer.
fn drop_dependents(
    dependents: &std::collections::HashMap<(String, String), Vec<(String, String)>>,
    skip: &mut std::collections::HashMap<(String, String), String>,
    failed_cp: (String, String),
    failed_cpv: &str,
) {
    let mut queue = vec![failed_cp];
    while let Some(x) = queue.pop() {
        if let Some(deps) = dependents.get(&x) {
            for p in deps {
                if skip.insert(p.clone(), failed_cpv.to_string()).is_none() {
                    queue.push(p.clone());
                }
            }
        }
    }
}

/// One resolved binary plan, executed remotely entry by entry in merge
/// order (sequential; `--remote-jobs` stays a reserved knob). Report is
/// per-unit trailers (the flow's `>>> Remote merged <cpv>` line, plus
/// `!!! Remote <cpv>: failed: …` and `>>> Remote <cpv>: skipped (…)` from
/// the loop) with a closing `>>> Remote summary: …` line (plan §11). Without `--keep-going` the
/// first unit error aborts the plan; with it, the error fails just that
/// unit and BFS-drops its transitive dependents (the `run_merge_loop`
/// `_calc_resume_list` policy), merging the rest and returning a
/// combined failed+skipped report. `AlreadyInstalled` stays a silent
/// no-op like the local plan. Anything else non-`Binary` cannot occur
/// (`check_binary_plan` gates first) and fails loudly.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_remote_plan(
    entries: &[portage_repo::GraphEntry],
    config: &portage_profile::Config,
    repos: &[portage_repo::RepoConfig],
    root: &std::path::Path,
    pkgdir: &std::path::Path,
    portage_tmpdir: &std::path::Path,
    ctx: &RemoteContext,
    keep_going: bool,
) -> Result<(), String> {
    use portage_repo::PretendOutcome;
    use std::collections::HashMap;
    // Backlog #322: every shipped unit carries the vendored phase runtime
    // (`build_bundle`), so a binary that cannot find it fails here -- the
    // gate stage, before the vdb shadow, the ledger gate, any ssh and any
    // write -- instead of panicking after the plan is printed.
    if entries.iter().any(|e| {
        !matches!(
            e.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        )
    }) {
        crate::ebuild_phases::require_phase_runtime()?;
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Vdb shadow for the pre-ship pre-check (plan §7): pulled once for
    // client placement, read directly for server placement.
    let shadow = load_vdb_shadow(ctx, control)?;
    if matches!(ctx.vdb, ConfigPlacement::Server(_)) {
        println!(
            "mrg: note: --remote-vdb is server-side: client merges run stateless (no old hooks, fail-closed collisions)"
        );
    }
    // Server ledger base: `--remote-ledger-dir` wins, else the placed
    // config's own `<PKGDIR>/remote-ledger` (plan §8).
    let server_ledger_base: std::path::PathBuf = match &ctx.ledger_dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => pkgdir.join("remote-ledger"),
    };
    // Strict-mode provenance gate (plan §8): fail early, before anything
    // ships, when the client and server ledger record disagree.
    if ctx.require_ledger_match {
        check_ledger_match(ctx, control, &server_ledger_base)?;
    }
    // cp -> the cps that depend on it (each entry's own `required_by`),
    // the same edge set `run_merge_loop` drops on.
    let dependents: HashMap<(String, String), Vec<(String, String)>> = entries
        .iter()
        .map(|e| {
            (
                (e.category.clone(), e.package.clone()),
                e.required_by.clone(),
            )
        })
        .collect();
    // Skipped cp -> the failed cpv that doomed it (for the trailer).
    let mut skip: HashMap<(String, String), String> = HashMap::new();
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut merged: u32 = 0;
    // Real `preinst_mask()` (`bin/misc-functions.sh`): the placed
    // config's resolved `INSTALL_MASK` + the `no{man,info,doc}` FEATURES
    // fold, through the same `pretend::config_install_mask` resolver the
    // local merge uses -- so a remote merge masks exactly what a local
    // one would (backlog #170). `ConfigPlacement::Client` resolves the
    // pulled client `/etc/portage`; `ConfigPlacement::Server` the
    // server's own (both are `config` here: the resolve already ran
    // under `ConfigRootOverride` for the placed root).
    let (install_mask, install_mask_prunes_usr_share) = crate::pretend::config_install_mask(config);
    // Backlog #171: the same placed config's resolved `FEATURES` list
    // rides every unit's regen postinst run (the local
    // `refresh_features` rule), so the vdb env carries the merge-time
    // features, not the binpkg's build-time ones.
    let regen_features = crate::pretend::config_features_string(config);
    // Backlog #171 review: the same placed config's resolved
    // `PORTAGE_BZIP2_COMMAND` rides the regen install too, so the
    // scrubbed vdb env records the client's configured compressor
    // (e.g. `lbzip2`), not always the `make.globals` default.
    let regen_bzip2 = crate::pretend::config_bzip2_command(config);
    // S8.2 install (a client mutation, so after the read-only stages --
    // the shadow, the ledger gate and the resolve above): one stage per
    // invocation, only when at least one unit ships, never with an
    // empty plan. `--pretend` never reaches here (`mrg` routes it
    // locally with the remote options dropped).
    let ships = entries.iter().any(|e| {
        matches!(
            e.outcome,
            PretendOutcome::New { .. }
                | PretendOutcome::Reinstall { .. }
                | PretendOutcome::Upgrade { .. }
                | PretendOutcome::Downgrade { .. }
        )
    });
    if ships {
        ensure_client_binary(ctx, control)?;
    }
    for entry in entries {
        let version = match &entry.outcome {
            // #72 B3: a removal is not remotely merged (execution is a
            // non-goal).
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::Uninstall { .. } => continue,
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                version.clone()
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
            PretendOutcome::NoVisibleCandidate => {
                return Err(format!(
                    "no binary package available for {}/{}",
                    entry.category, entry.package
                ));
            }
        };
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let cp = (entry.category.clone(), entry.package.clone());
        if let Some(culprit) = skip.get(&cp) {
            skipped.push(cpv.clone());
            println!(">>> Remote {cpv}: skipped ({culprit} failed)");
            continue;
        }
        let unit = run_one_remote_unit(
            entry,
            &version,
            &cpv,
            config,
            repos,
            root,
            pkgdir,
            ctx,
            control,
            &shadow,
            &server_ledger_base,
            &install_mask,
            install_mask_prunes_usr_share,
            Some(&regen_features),
            Some(&regen_bzip2),
        );
        match unit {
            Ok(()) => {
                // The flow's own `>>> Remote merged <cpv>` line is this
                // unit's trailer; only failed/skipped need plan-level
                // trailers here.
                merged += 1;
            }
            Err(message) => {
                let short = message.lines().next().unwrap_or(&message).to_string();
                println!("!!! Remote {cpv}: failed: {short}");
                if !keep_going {
                    return Err(message);
                }
                failures.push((cpv.clone(), message));
                drop_dependents(&dependents, &mut skip, cp, &cpv);
            }
        }
    }
    println!(
        ">>> Remote summary: {merged} merged, {} failed, {} skipped",
        failures.len(),
        skipped.len()
    );
    let _ = portage_tmpdir;
    if failures.is_empty() {
        return Ok(());
    }
    let mut msg = format!(
        "Remote plan finished with {} failed unit(s) (--keep-going):\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(cpv, e)| format!("  {cpv}: {}", e.lines().next().unwrap_or(e)))
            .collect::<Vec<_>>()
            .join("\n")
    );
    if !skipped.is_empty() {
        msg.push_str(&format!(
            "\n{} dependent package(s) not merged:\n{}",
            skipped.len(),
            skipped
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Err(msg)
}

/// One merge-bound plan entry end to end: locate the binpkg (binhost
/// download or local `$PKGDIR`), run it through the client
/// (bundle → unpack → phases → merge → postinst, shadow pre-check
/// before anything ships), then record the server ledger. `Err` is the
/// unit's failure; the caller decides abort vs. keep-going.
#[allow(clippy::too_many_arguments)]
fn run_one_remote_unit(
    entry: &portage_repo::GraphEntry,
    version: &str,
    cpv: &str,
    config: &portage_profile::Config,
    repos: &[portage_repo::RepoConfig],
    root: &std::path::Path,
    pkgdir: &std::path::Path,
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    shadow: &VdbShadow,
    server_ledger_base: &std::path::Path,
    install_mask: &str,
    install_mask_prunes_usr_share: bool,
    regen_features: Option<&str>,
    regen_bzip2: Option<&str>,
) -> Result<(), String> {
    let binpkg_path = if entry.remote_binary {
        let (binrepo, record) = portage_repo::find_remote_binpkg_instance(
            &config.binrepos,
            root,
            &entry.category,
            &entry.package,
            version,
            entry.build_id.as_deref(),
        )
        .ok_or_else(|| {
            format!(
                "{}/{}-{version}: not found in any binhost `Packages` index",
                entry.category, entry.package
            )
        })?;
        crate::emerge_getbinpkg::download_and_verify(
            &binrepo.sync_uri,
            &record,
            &entry.category,
            &entry.package,
            version,
            pkgdir,
        )?
    } else {
        crate::emerge_getbinpkg::resolve_local_binpkg(
            pkgdir,
            &entry.category,
            &entry.package,
            version,
            entry.build_id.as_deref(),
        )
        .ok_or_else(|| {
            format!(
                "{}/{}-{version}: no binpkg file under {}",
                entry.category,
                entry.package,
                pkgdir.display()
            )
        })?
    };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs())
        .unwrap_or(0);
    let (repo, commit) = repo_position(repos, &entry.repo_name);
    let ledger_entry = LedgerEntry {
        ts,
        repo,
        commit,
        cpv: cpv.to_string(),
    };
    let ledger = LedgerSpec {
        file: client_ledger_file(ctx),
        line: ledger_entry.line(),
    };
    run_binpkg_flow(
        ctx,
        control,
        &binpkg_path,
        Some(&ledger),
        Some(&ledger_entry.repo),
        Some(shadow),
        install_mask,
        install_mask_prunes_usr_share,
        regen_features,
        regen_bzip2,
    )?;
    record_server_ledger(server_ledger_base, &ctx.hostname, &ledger_entry)?;
    Ok(())
}

/// Slice-1 entry point: connect, preflight, report. Exit 0 = every hard
/// gate passed; 1 = a gate failed or the client is unreachable.
/// Slice 2+ entry point with `--remote-binpkg`: preflight, then bundle,
/// stream, unpack and verify one explicit binpkg file.
pub fn run_remote(ctx: &RemoteContext) -> ExitCode {
    // Backlog #322: an explicit `--remote-binpkg` bundles the phase
    // runtime into the unit, so a binary with no runtime fails here, with
    // one message, before the first connection -- not after the preflight.
    if ctx.binpkg.is_some()
        && let Err(message) = crate::ebuild_phases::require_phase_runtime()
    {
        eprintln!("mrg: {message}");
        return ExitCode::from(1);
    }
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Before the first connection: afterwards the key is present even on
    // first contact, so this decision cannot be made later.
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(ctx);
    let (values, bin) = match run_preflight_inner(ctx, control) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (mut failures, mut warnings) = evaluate_preflight(ctx, &values);
    // The trial path ships one unit, so it enforces the S8.1 binary
    // gates like the resolve path; the preflight-only run ships nothing
    // and skips them (like `--pretend`, which installs nothing).
    let mut ctx = ctx.clone();
    if ctx.binpkg.is_some() {
        match bin {
            Some(pre) => {
                let override_given = ctx.portuale_dir.is_some();
                let (plan, mut bin_failures, bin_notes) =
                    evaluate_bin_gates(&values, &pre, override_given);
                failures.append(&mut bin_failures);
                for note in bin_notes {
                    warnings.push(note);
                }
                ctx.bin_plan = plan;
            }
            None => match std::env::current_exe().map(|p| p.display().to_string()) {
                Ok(path) => ctx.bin_plan = ClientBinPlan::Local(path),
                Err(e) => failures.push(format!("mrg: cannot locate the running binary: {e}")),
            },
        }
    }
    if !failures.is_empty() {
        return print_preflight_report(&ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(&ctx, &failures, &warnings, first_contact);
    match &ctx.binpkg {
        None => ExitCode::from(0),
        Some(path) => {
            let path = Path::new(path).to_path_buf();
            run_bundle_stage(&ctx, control, &path)
        }
    }
}

/// Connect and run the preflight script: parsed `KEY=VALUE` gates on
/// success, `mrg: …`-prefixed message on transport or command failure.
/// Human log lines (client stderr) print straight through on success.
/// The binary facts (`bin_preflight`) are computed before the first
/// connection, so an unreadable `--remote-portuale-binary` fails fast.
/// Returns the parsed values plus the binary facts the S8.1 gates decide
/// on (`None` when the run skips those gates by design).
fn run_preflight_inner(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<(HashMap<String, String>, Option<BinPreflight>), String> {
    let bin = bin_preflight(ctx)?;
    let script = match &bin {
        Some(pre) => preflight_script(&ctx.root, &ctx.workdir, Some((&pre.dirs, &pre.candidates))),
        None => preflight_script(&ctx.root, &ctx.workdir, None),
    };
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local preflight command failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if ctx.transport == RemoteTransport::Ssh && is_transport_error(code, &stderr) {
            let mut message = format!("mrg: client {} unreachable:", ctx.hostname);
            for line in stderr.lines().take(5) {
                message.push_str(&format!("\nmrg:   {line}"));
            }
            return Err(message);
        }
        let mut message = format!(
            "mrg: client preflight command failed (exit {}):",
            code.unwrap_or(-1)
        );
        for line in stderr.lines().take(5) {
            message.push_str(&format!("\nmrg:   {line}"));
        }
        return Err(message);
    }
    for line in stderr.lines() {
        println!("{line}");
    }
    Ok((parse_kv(&String::from_utf8_lossy(&output.stdout)), bin))
}

/// Gates + clock over parsed preflight values. Pure (unit-tested).
fn evaluate_preflight(
    ctx: &RemoteContext,
    values: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let (mut failures, warnings) = preflight_gates(values);
    let server_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(message) = clock_gate(
        values.get("CLIENT_TIME").map(String::as_str),
        server_time,
        ctx.max_clock_skew_secs,
    ) {
        failures.push(message);
    }
    (failures, warnings)
}

/// TOFU receipt + warnings + ok/failed report. Returns the exit code.
fn print_preflight_report(
    ctx: &RemoteContext,
    failures: &[String],
    warnings: &[String],
    first_contact: bool,
) -> ExitCode {
    // Trust-on-first-use receipt: the key was added during this very
    // connection -- print its fingerprint. SSH-only: local transport
    // never touches known_hosts.
    if first_contact
        && ctx.transport == RemoteTransport::Ssh
        && let Some(fingerprint) = connection_fingerprint(ctx)
    {
        println!("mrg: added new host key for {}:", ctx.hostname);
        for line in fingerprint.lines() {
            println!("mrg:   {line}");
        }
    }

    for warning in warnings {
        println!("mrg: warning: {warning}");
    }
    if failures.is_empty() {
        println!(">>> Remote preflight {}: ok", ctx.hostname);
        ExitCode::from(0)
    } else {
        eprintln!("mrg: remote preflight {} failed:", ctx.hostname);
        for failure in failures {
            eprintln!("mrg:   {failure}");
        }
        ExitCode::from(1)
    }
}

// --- Bundle streaming (slice 2) --------------------------------------------

/// Unpack driver: byte-count gate, `tar -xpf`, member + manifest sanity.
/// Values baked in server-side, pre-quoted. The tarball itself arrived
/// earlier via `send_file` (`$WORKDIR/<pf>/bundle.tar`). `UNPACK=...` on
/// stdout, log on stderr; any gate prints its reason and exits 1.
fn unpack_script(workdir: &str, pf: &str, expected_bytes: u64) -> String {
    format!(
        r#"UNIT_DIR={unit}
BUNDLE="$UNIT_DIR/bundle.tar"
if [ ! -f "$BUNDLE" ]; then echo "UNPACK=missing-bundle"; exit 1; fi
ACTUAL=$(wc -c < "$BUNDLE")
if [ "$ACTUAL" != "{expected}" ]; then echo "UNPACK=byte-count-mismatch expected={expected} actual=$ACTUAL"; exit 1; fi
if ! tar -xpf "$BUNDLE" -C {workdir}; then echo "UNPACK=tar-failed"; exit 1; fi
rm -f "$BUNDLE"
for member in "$UNIT_DIR/image" "$UNIT_DIR/build-info" "$UNIT_DIR/remote-manifest"; do
  if [ ! -e "$member" ]; then echo "UNPACK=missing-member member=$member"; exit 1; fi
done
# NOTE: build-info/CONTENTS is NOT gated here (fixture binpkgs lack it;
# real ones carry it) -- slice 4's merge owns that check, where a
# missing CONTENTS is genuinely unmergeable.
FORMAT=$(sed -n 's/^FORMAT=//p' "$UNIT_DIR/remote-manifest" | head -n 1)
if [ "$FORMAT" != "1" ]; then echo "UNPACK=bad-manifest format=$FORMAT"; exit 1; fi
echo "UNPACK=ok"
"#,
        unit = sh_quote(&format!("{workdir}/{pf}")),
        workdir = sh_quote(workdir),
        expected = expected_bytes,
    )
}

/// Slice-2 stage for `--remote-binpkg <file>`: build the bundle
/// (server-side, `remote_bundle`), stream it to
/// `$WORKDIR/<pf>/bundle.tar`, unpack + verify there. Exit 0 = `UNPACK=ok`.
/// One repo-ledger line, shared by both sides: `<unix-ts> <repo> <commit> <cpv>`
/// (docs/remote-merge.md §8).
pub(crate) struct LedgerEntry {
    pub ts: u64,
    pub repo: String,
    pub commit: String,
    pub cpv: String,
}

impl LedgerEntry {
    fn line(&self) -> String {
        format!("{} {} {} {}", self.ts, self.repo, self.commit, self.cpv)
    }
}

/// Append one line, keeping the last 10 (rotation both sides share).
fn ledger_append(path: &std::path::Path, line: &str) -> Result<(), String> {
    use std::io::Write as _;
    let mut lines: Vec<String> = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    lines.push(line.to_string());
    while lines.len() > 10 {
        lines.remove(0);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for kept in &lines {
        writeln!(file, "{kept}").map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

/// Hostname made filename-safe for the server ledger file.
fn sanitize_hostname(hostname: &str) -> String {
    hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Server-side ledger record after one merged entry:
/// `<pkgdir>/remote-ledger/<hostname>`, last 10 kept. Pass the ledger
/// *directory* (`<placed-PKGDIR>/remote-ledger`, or `--remote-ledger-dir`
/// when set) -- the hostname file hangs off it.
pub(crate) fn record_server_ledger(
    ledger_dir: &std::path::Path,
    hostname: &str,
    entry: &LedgerEntry,
) -> Result<(), String> {
    ledger_append(&ledger_dir.join(sanitize_hostname(hostname)), &entry.line())
}

/// The client-side ledger destination baked into the merge driver:
/// `<ctx.root>/var/db/remote-repos` (a plain file, not the vdb).
fn client_ledger_file(ctx: &RemoteContext) -> String {
    format!("{}/var/db/remote-repos", ctx.root.trim_end_matches('/'))
}

/// The newest (last non-empty) line of a last-10 ledger file: the plan's
/// append-ordered `<unix-ts> <repo> <commit> <cpv>` line (`docs/
/// remote-merge.md` §8). `None` for an absent/empty ledger -- a client
/// (or server) that has never recorded provenance.
fn newest_ledger_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(String::from)
}

/// The server-side ledger record for one hostname:
/// `<base>/<hostname>` (last 10 kept). `None` when absent/empty.
fn server_latest_ledger(base: &std::path::Path, hostname: &str) -> Option<String> {
    std::fs::read_to_string(base.join(sanitize_hostname(hostname)))
        .ok()
        .and_then(|text| newest_ledger_line(&text))
}

/// Strict-mode provenance comparison (`--remote-require-ledger-match`,
/// plan §8): the client's and server's *newest* ledger line must agree --
/// both absent (fresh client + no server record) or byte-identical. Any
/// asymmetry is drift: a client that lost its ledger, was reimaged or
/// hand-edited, or was last merged by a different server.
fn ledger_lines_match(client: Option<&str>, server: Option<&str>) -> bool {
    match (client, server) {
        (None, None) => true,
        (Some(c), Some(s)) => c == s,
        _ => false,
    }
}

/// Read the client ledger's newest line over the transport (plan §8's
/// "provenance story"). Missing/empty reads as `None`; only a transport
/// or command failure is an `Err`.
fn read_client_latest_ledger(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<Option<String>, String> {
    let file = client_ledger_file(ctx);
    let script = format!(
        "f={};\nif [ -f \"$f\" ]; then tail -n 1 \"$f\"; fi\n",
        sh_quote(&file)
    );
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local ledger read failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        if ctx.transport == RemoteTransport::Ssh && is_transport_error(code, &stderr) {
            let mut message = format!("mrg: client {} unreachable:", ctx.hostname);
            for line in stderr.lines().take(5) {
                message.push_str(&format!("\nmrg:   {line}"));
            }
            return Err(message);
        }
        let mut message = format!(
            "mrg: reading the client ledger failed (exit {}):",
            code.unwrap_or(-1)
        );
        for line in stderr.lines().take(5) {
            message.push_str(&format!("\nmrg:   {line}"));
        }
        return Err(message);
    }
    Ok(newest_ledger_line(&String::from_utf8_lossy(&output.stdout)))
}

/// The strict-mode gate: abort with no client writes unless the client's
/// and server's newest ledger line agree (both-absent = fresh = ok).
fn check_ledger_match(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    server_ledger_base: &std::path::Path,
) -> Result<(), String> {
    let client = read_client_latest_ledger(ctx, control)?;
    let server = server_latest_ledger(server_ledger_base, &ctx.hostname);
    if ledger_lines_match(client.as_deref(), server.as_deref()) {
        return Ok(());
    }
    let show = |line: Option<&str>| line.unwrap_or("(no ledger)").to_string();
    Err(format!(
        "mrg: --remote-require-ledger-match: the client ledger provenance disagrees with the server record for {}:\nmrg:   client: {}\nmrg:   server: {}",
        ctx.hostname,
        show(client.as_deref()),
        show(server.as_deref()),
    ))
}

// --- Vdb shadow + pre-ship collision pre-check (slice 6) ----------------------
///
/// The server reasons about installed-file ownership *before* shipping a
/// unit's tarball (plan §7): a unit doomed by a foreign-owned collision
/// fails here, with zero client writes. The shadow is the placed vdb
/// read directly (`server:`) or pulled once at plan start (`client:`,
/// same `tar -c`/`tar -x` shape as the etc-portage pull).
///
/// Deliberately lenient: only ownership by a *different* package fails
/// the pre-check. Same-package ownership (any version/slot) passes --
/// the client driver refines those (same-slot replace vs. collision)
/// against the live vdb, or fail-closed without one. A passed pre-check
/// is therefore necessary but not sufficient; a failed one is final.
///
// Installed-file ownership: absolute path -> owning `(category, pf)`.
pub(crate) struct VdbShadow {
    owners: std::collections::HashMap<String, (String, String)>,
}

/// `obj`/`sym` paths out of one vdb `CONTENTS` file (real
/// `dblink`/`vartree` shape: `obj <path> <md5> <mtime>`,
/// `sym <path> -> <target> <mtime>`). `dir` lines merge freely
/// client-side (`mkdir -p`), so they never gate a pre-check.
fn contents_owned_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("obj"), Some(path)) | (Some("sym"), Some(path)) => Some(path.to_string()),
                _ => None,
            }
        })
        .collect()
}

impl VdbShadow {
    /// Read `<dir>/<cat>/<pf>/CONTENTS` (two-level walk, no new
    /// dependencies -- the vdb is exactly that deep). Missing/unreadable
    /// files are skipped, not fatal: an empty shadow pre-checks nothing,
    /// and the client driver still gates every collision for real.
    fn load(dir: &std::path::Path) -> Self {
        Self::from_db(&portage_vdb::FilesDb::open_vdb_dir(dir))
    }

    /// The shadow of any installed-package database: entries in listing
    /// order (in-progress `-MERGING-<pf>` entries are never listed), the
    /// first owner of a path wins. An unlistable database or an
    /// unreadable / non-UTF-8 `CONTENTS` contributes nothing.
    fn from_db(db: &dyn portage_vdb::InstalledDb) -> Self {
        let mut owners = std::collections::HashMap::new();
        let Ok(keys) = db.entries() else {
            return Self { owners };
        };
        for key in keys {
            let Ok(Some(bytes)) = db.read_file(&key, "CONTENTS") else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for path in contents_owned_paths(&text) {
                owners
                    .entry(path)
                    .or_insert_with(|| (key.category.clone(), key.pf.clone()));
            }
        }
        Self { owners }
    }

    /// Owner `(category, pf)` of an absolute image path, if recorded.
    fn owner(&self, abspath: &str) -> Option<&(String, String)> {
        self.owners.get(abspath)
    }

    /// True when `pf` belongs to package `pn` (real `_pkgsplit`
    /// longest-trailing-version rule via `portage_repo::split_pf`).
    fn same_package(pf: &str, pn: &str) -> bool {
        portage_repo::split_pf(pf).is_some_and(|(name, _)| name == pn)
    }
}

/// `(kind, rel)` image paths out of a staged `filemeta` file (the exact
/// bytes the client will gate on: `<kind> <md5> <mtime> <rel>
/// [<target>]`, whitespace-free by bundle-build construction).
fn parse_filemeta_paths(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let kind = words.next()?.to_string();
            if !matches!(kind.as_str(), "obj" | "sym" | "dir") {
                return None;
            }
            words.next()?;
            words.next()?;
            let rel = words.next()?.to_string();
            Some((kind, rel))
        })
        .collect()
}

/// Pre-ship gate for one unit: every `obj`/`sym` image path owned in the
/// shadow by a *different* package fails with a `not shipping` error
/// (zero client writes so far). Same-package ownership passes for the
/// client driver to refine.
fn shadow_precheck(
    shadow: &VdbShadow,
    cpv: &str,
    category: &str,
    pn: &str,
    staged_filemeta: &str,
) -> Result<(), String> {
    for (kind, rel) in parse_filemeta_paths(staged_filemeta) {
        if kind == "dir" {
            continue;
        }
        let abspath = format!("/{rel}");
        if let Some((cat, pf)) = shadow.owner(&abspath)
            && (cat != category || !VdbShadow::same_package(pf, pn))
        {
            return Err(format!(
                "mrg: not shipping {cpv}: {abspath} owned by {cat}/{pf} (vdb shadow)"
            ));
        }
    }
    Ok(())
}

/// Load the shadow for this run: read directly for `server:` placement,
/// pull once for `client:` placement (plan §7's pull-once-per-run).
fn load_vdb_shadow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<VdbShadow, String> {
    match &ctx.vdb {
        ConfigPlacement::Server(path) => Ok(VdbShadow::load(std::path::Path::new(path))),
        ConfigPlacement::Client(path) => {
            let tmp = TempDir::new("portuale-remote-vdb").keep();
            // Like the etc-portage pull, the tmp dir is left for
            // forensics; it carries no secrets beyond file lists.
            pull_dir(ctx, control, path, &tmp)?;
            Ok(VdbShadow::load(&tmp))
        }
    }
}

/// Every merge-bound entry must be `Binary` in a remote plan (the
/// `--getbinpkgonly` resolve guarantees it); anything else is a usage
/// error naming the offenders (exit 2 at the call site).
pub(crate) fn check_binary_plan(entries: &[portage_repo::GraphEntry]) -> Result<(), String> {
    use portage_repo::{CandidateSource, PretendOutcome};
    let mut offenders = Vec::new();
    for entry in entries {
        let merge_bound = !matches!(
            entry.outcome,
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
        );
        if merge_bound && entry.source != CandidateSource::Binary {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version.clone()
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                    to.clone()
                }
                _ => String::new(),
            };
            offenders.push(format!("{}/{}-{version}", entry.category, entry.package));
        }
    }
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "remote plan is not binary-only (needs --getbinpkgonly): {}",
            offenders.join(" ")
        ))
    }
}

/// One binpkg end to end (bundle, stream, unpack, phases, merge,
/// postinst): shared by the `--remote-binpkg` path (no ledger, no
/// shadow) and the resolve path (ledger per merged entry, shadow
/// pre-check before anything ships). `install_mask` /
/// `install_mask_prunes_usr_share` are the resolve's own
/// `config_install_mask` values for the placed config -- both paths
/// resolve the client config now (backlog #170/#171b: the trial path's
/// `run_bundle_stage` places and loads it like `run_remote_resolve`
/// does). `regen_features` is the resolve's own
/// `config_features_string` for the placed config (the merge-time
/// `FEATURES` the regen'd vdb env carries, backlog #171);
/// `regen_bzip2` is the resolve's own `config_bzip2_command` for the
/// placed config (the merge-time `PORTAGE_BZIP2_COMMAND` the scrubbed
/// vdb env records). Prints the
/// stage report lines; `Ok(cpv)` is the merged `category/package-version`.
#[allow(clippy::too_many_arguments)]
fn run_binpkg_flow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
    ledger: Option<&LedgerSpec>,
    repo_override: Option<&str>,
    shadow: Option<&VdbShadow>,
    install_mask: &str,
    install_mask_prunes_usr_share: bool,
    regen_features: Option<&str>,
    regen_bzip2: Option<&str>,
) -> Result<String, String> {
    let staging = TempDir::new("portuale-remote-bundle").keep();
    if let Err(message) = std::fs::create_dir_all(&staging) {
        return Err(format!("mrg: staging dir {}: {message}", staging.display()));
    }
    let staged = match crate::remote_bundle::build_bundle(
        binpkg_path,
        &staging,
        repo_override,
        &crate::binpkg::GpgVerify::from_env(),
        install_mask,
        install_mask_prunes_usr_share,
    ) {
        Ok(staged) => staged,
        Err(message) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("mrg: bundle build failed: {message}"));
        }
    };
    let pf = staged
        .manifest
        .cpv
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    // Pre-ship gate (slice 6): the shadow fails a foreign-owned unit
    // before its tarball streams anywhere -- zero client writes so far.
    // The staged `filemeta` is the exact bytes the client will gate on.
    if let Some(shadow) = shadow {
        let filemeta_path = staging.join(&pf).join("filemeta");
        match std::fs::read_to_string(&filemeta_path) {
            Ok(text) => {
                if let Err(message) = shadow_precheck(
                    shadow,
                    &staged.manifest.cpv,
                    &staged.category,
                    &staged.pn,
                    &text,
                ) {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(message);
                }
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!("mrg: reading staged filemeta: {e}"));
            }
        }
    }
    let dest_tar = format!("{}/{pf}/bundle.tar", ctx.workdir);
    // Fail-early: the stream lands (or fails) before the driver runs.
    if ctx.transport == RemoteTransport::Ssh {
        // The unit dir must exist for `cat >` (no `mkdir -p` hiding a
        // wrong workdir -- preflight already proved creatability).
        let mkdir = format!("mkdir -p {}", sh_quote(&format!("{}/{pf}", ctx.workdir)));
        match run_script_stdin(ctx, control, &mkdir) {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!(
                    "mrg: creating unit dir failed (exit {}):\n{stderr}",
                    output.status.code().unwrap_or(-1)
                ));
            }
            Err(message) => {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(message);
            }
        }
    }
    if let Err(message) = send_file(ctx, control, &staged.tarball, &dest_tar) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(message);
    }
    let script = unpack_script(&ctx.workdir, &pf, staged.byte_count);
    let output = match run_script_stdin(ctx, control, &script) {
        Ok(output) => output,
        Err(message) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(message);
        }
    };
    let _ = std::fs::remove_dir_all(&staging);
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        println!("{line}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let unpack = parse_kv(&stdout).get("UNPACK").cloned().unwrap_or_default();
    if !(output.status.success() && unpack == "ok") {
        let mut message = format!(
            "mrg: bundle unpack failed (exit {}, UNPACK={unpack}):",
            code.unwrap_or(-1)
        );
        for line in stderr.lines().take(5) {
            message.push_str(&format!("\nmrg:   {line}"));
        }
        return Err(message);
    }
    println!(
        ">>> Remote bundle {}: unpacked ({} bytes, slot {}, repo {})",
        staged.manifest.cpv, staged.byte_count, staged.manifest.slot, staged.manifest.repo,
    );
    let (vdb, stateless) = client_vdb_placement(ctx);
    // Backlog #171: stage the replaced instance's saved env for the
    // merge driver's old hooks (server-decompressed, so the client
    // needs no bzip2). Best-effort: failures warn and the driver falls
    // back to the client vdb. Stateless merges run no old hooks.
    if !stateless {
        let unit_dir = format!("{}/{pf}", ctx.workdir);
        let mainslot = staged.manifest.slot.split('/').next().unwrap_or("0");
        let shipment = ship_old_hook_envs(
            ctx,
            control,
            &unit_dir,
            &vdb,
            &staged.category,
            &staged.pn,
            &staged.pf,
            mainslot,
        );
        for pf in &shipment.shipped {
            println!(
                ">>> Remote old-env {}: staged for {pf}",
                staged.manifest.cpv
            );
        }
        for warning in &shipment.warnings {
            println!("{}", old_hook_warn_message(&staged.manifest.cpv, warning));
        }
    }
    // Slice-3 phases (pretend/setup/preinst, DEFINED_PHASES-gated at
    // bundle time). Postinst waits for the slice-4 merge. An empty phase
    // list (no hooks, or no ebuild/env shipped) is a note, not a failure
    // -- same degrade as the local merge -- and, crucially, not a skip:
    // the copy+vdb merge below runs regardless (a hookless binpkg still
    // installs files; returning early here silently unmerged it).
    if staged.phases.is_empty() {
        println!(
            ">>> Remote phases {}: none defined, skipped",
            staged.manifest.cpv
        );
    } else {
        let unit_dir = format!("{}/{pf}", ctx.workdir);
        match run_phases_stage(ctx, control, &unit_dir, &staged) {
            Ok(done) => {
                println!(
                    ">>> Remote phases {}: {}",
                    staged.manifest.cpv,
                    done.join(", ")
                );
            }
            Err(message) => {
                return Err(message);
            }
        }
    }
    let unit_dir = format!("{}/{pf}", ctx.workdir);
    // Slice-4 merge (copy+vdb+replace); the regen postinst below stays
    // non-fatal like the local merge's own `_postinst_failure` rule.
    match run_merge_stage(ctx, control, &unit_dir, &staged, ledger) {
        Ok(markers) => {
            for marker in &markers {
                println!(">>> Remote merge {}: {marker}", staged.manifest.cpv);
            }
        }
        Err(message) => {
            return Err(message);
        }
    }
    // Backlog #171: the postinst phase runs for every merged unit --
    // defined or not -- as the merge-time environment regeneration
    // (real `vartree.py:5334` + `phase-functions.sh:1072-1082`; an
    // undefined `pkg_postinst` is a no-op that still re-saves the env).
    // Any regen/pull/compress/install failure keeps the build-time env
    // already in the vdb with one `!!!` warning and continues (real
    // `_postinst_failure`: "It's stupid to bail out here").
    let cpv = staged.manifest.cpv.clone();
    // Backlog #171b: the calling environment for the client phase is
    // the server process -- forward its set locale variables (real's
    // `environ_whitelist` rule; never invented defaults).
    let server_locale = collect_server_locale(|name| std::env::var(name).ok());
    match run_postinst_regen_stage(
        ctx,
        control,
        &unit_dir,
        &staged,
        regen_features,
        &server_locale,
    ) {
        Ok(report) => {
            if report.skipped_no_hooks {
                println!(">>> Remote postinst {cpv}: none to run from, skipped");
            } else {
                if !staged.postinst_defined {
                    println!(">>> Remote postinst {cpv}: no pkg_postinst defined, regen only");
                }
                if report.phase_rc == 0 {
                    println!(">>> Remote postinst {cpv}: ok");
                } else {
                    println!(
                        ">>> Remote postinst {cpv}: FAILED (exit {}) -- merge kept (real _postinst_failure)",
                        report.phase_rc
                    );
                }
                if report.regen_present {
                    if stateless {
                        println!(">>> Remote env-regen {cpv}: skipped (stateless, no client vdb)");
                    } else {
                        let vdb_env =
                            format!("{vdb}/{}/{}/environment.bz2", staged.category, staged.pf);
                        match install_regenerated_env(
                            ctx,
                            control,
                            &unit_dir,
                            &vdb_env,
                            regen_bzip2,
                        ) {
                            Ok(()) => println!(
                                ">>> Remote env-regen {cpv}: vdb environment.bz2 regenerated"
                            ),
                            Err((step, detail)) => {
                                println!("{}", regen_warn_message(&cpv, &step));
                                for line in detail.lines().take(3) {
                                    println!("mrg:   {line}");
                                }
                            }
                        }
                    }
                } else {
                    println!("{}", regen_warn_message(&cpv, "postinst"));
                }
            }
        }
        Err(message) => {
            println!(">>> Remote postinst {cpv}: transport failed, merge kept: {message}");
            println!("{}", regen_warn_message(&cpv, "postinst"));
        }
    }
    println!(">>> Remote merged {cpv}");
    Ok(cpv)
}
fn run_bundle_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
) -> ExitCode {
    if !binpkg_path.is_file() {
        eprintln!("mrg: --remote-binpkg {}: not found", binpkg_path.display());
        return ExitCode::from(1);
    }
    // Backlog #171b: the trial path resolves the client config exactly
    // like the plan path does -- place `/etc/portage` per
    // `ConfigPlacement` through the shared `place_config_root` helper
    // (same shape as `run_remote_resolve`), then load the same repos +
    // resolved config `pretend::run` merges with
    // (`load_repos_and_config`: one shared resolver call, no duplicate
    // logic). The placed config's `INSTALL_MASK` (+ the
    // `no{man,info,doc}` fold), resolved `FEATURES`, and
    // `PORTAGE_BZIP2_COMMAND` feed `build_bundle` / the env regen
    // exactly as `run_remote_plan`'s own values do. Both placements
    // always name a path, so there is no config-less mode left here to
    // keep the old empty values for; a pull or resolve failure is a
    // hard error like the resolve path's own. `_placed` stays alive
    // across the flow; its temp pull is removed best-effort on drop.
    let _placed = match place_config_root(ctx, control) {
        Ok(placed) => placed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let config_dir = _placed.dir.to_path_buf();
    let (_repos, config) =
        match crate::pretend::load_repos_and_config(&config_dir, &portage_repo::root_from_env()) {
            Ok(loaded) => loaded,
            Err(message) => {
                eprintln!("mrg: --remote-binpkg: cannot resolve the client config: {message}");
                return ExitCode::from(1);
            }
        };
    let (install_mask, install_mask_prunes_usr_share) =
        crate::pretend::config_install_mask(&config);
    let regen_features = crate::pretend::config_features_string(&config);
    let regen_bzip2 = crate::pretend::config_bzip2_command(&config);
    // S8.2 install, like the resolve path: the trial ships exactly one
    // unit, so the stage always runs here (never under `--pretend`,
    // which never reaches this executor).
    if let Err(message) = ensure_client_binary(ctx, control) {
        eprintln!("{message}");
        return ExitCode::from(1);
    }
    match run_binpkg_flow(
        ctx,
        control,
        binpkg_path,
        None,
        None,
        None,
        &install_mask,
        install_mask_prunes_usr_share,
        Some(&regen_features),
        Some(&regen_bzip2),
    ) {
        Ok(_) => ExitCode::from(0),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

// --- Client merge (slice 4) ----------------------------------------------------

/// Real `eapi_exports_merge_type` (`lib/portage/eapi.py:63`, attrs table
/// `:300`: `exports_merge_type = eapi >= Eapi("4")`): `MERGE_TYPE` reaches
/// the phase env only for EAPI 4+ (real `config.py:3335-3336` pops it
/// otherwise). An unsupported or unparseable EAPI takes real's
/// `_get_eapi_attrs` fallback row (`eapi.py:247-258`), which exports.
fn eapi_exports_merge_type(eapi: &str) -> bool {
    // Real tests the *whole* string against `_supported_eapis`; anything
    // that is not a plain supported number (e.g. `3-foo`) takes the
    // exporting fallback row.
    match eapi.trim().parse::<u32>() {
        Ok(n) => n >= 4,
        Err(_) => true,
    }
}

/// `export` block for the new instance's phase script (`phase_script`;
/// the replaced instance's prerm/postrm go through the merge driver's own
/// `run_old_hook`, which deliberately exports no `MERGE_TYPE`: real's
/// unmerge re-creates the old instance's settings, `vartree.py:4535-4540`,
/// and never passes through the scheduler that sets it): path overrides with
/// caller-side values, everything else from the sourced saved env.
/// `unit_bin` is the shipped runtime dir (`$UNIT/bin`,
/// `$OLD_TMP/bin` symlink or copy -- callers decide).
/// `MERGE_TYPE=binary` rides the same line as `EMERGE_FROM` (real
/// `_emerge/Binpkg.py:92` sets it for every binpkg merge; `mrg` only
/// merges binpkgs), gated on [`eapi_exports_merge_type`].
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5):
/// every client call uses it, never `$PATH` -- `PORTAGE_PYTHON` is the
/// shipped `portuale-python` shim through the script's own `$UNITBIN`
/// (`$UNIT/bin`, defined by each script header), `PORTAGE_PYM_PATH` is
/// `/` (no checkout on the client), and `PORTAGE_IPC_DAEMON` is unset
/// (no IPC daemon, #326 D6 -- this also scrubs a value inherited over
/// the local transport).
#[allow(clippy::too_many_arguments)]
fn phase_exports(
    ebuild: &str,
    build_dir: &str,
    root: &str,
    image_dir: &str,
    temp_dir: &str,
    work_subdir: &str,
    home_dir: &str,
    files_dir: &str,
    bin_dir: &str,
    tmpdir: &str,
    colormap: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    portuale_bin: &str,
) -> String {
    format!(
        concat!(
            "export EAPI={eapi} CATEGORY={category} PN={pn} PV={pv} PR={pr} PVR={pvr} P={p} PF={pf}\n",
            "export EBUILD={ebuild} O={obuild}\n",
            "export ROOT={root} EROOT={root}\n",
            "export PORTAGE_BUILDDIR={builddir}\n",
            "export WORKDIR={workdir}\n",
            "export D={imaged}/ ED={imaged}/\n",
            "export T={temp} HOME={home} FILESDIR={filesdir}\n",
            "export PORTAGE_BIN_PATH={bindir}\n",
            "export PORTAGE_ECLASS_LOCATIONS=\"\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "export PORTAGE_PYTHON=\"$UNITBIN/portuale-python\"\n",
            "export PORTAGE_COLORMAP={colormap}\n",
            "export PORTAGE_TMPDIR={tmpdir}\n",
            "export SANDBOX_LOG={temp}/sandbox.log\n",
            "export EBUILD_PHASE={phase} EMERGE_FROM=binary{merge_type}\n",
        ),
        merge_type = if eapi_exports_merge_type(&staged.eapi) {
            " MERGE_TYPE=binary"
        } else {
            ""
        },
        eapi = sh_quote(&staged.eapi),
        category = sh_quote(&staged.category),
        pn = sh_quote(&staged.pn),
        pv = sh_quote(&staged.pv),
        pr = sh_quote(&staged.pr),
        pvr = sh_quote(&staged.pvr),
        p = sh_quote(&staged.p),
        pf = sh_quote(&staged.pf),
        ebuild = sh_quote(ebuild),
        obuild = sh_quote(&format!("{build_dir}/build-info")),
        root = sh_quote(root),
        builddir = sh_quote(build_dir),
        workdir = sh_quote(work_subdir),
        imaged = sh_quote(image_dir),
        temp = sh_quote(temp_dir),
        home = sh_quote(home_dir),
        filesdir = sh_quote(files_dir),
        bindir = sh_quote(bin_dir),
        tmpdir = sh_quote(tmpdir),
        colormap = sh_quote(colormap),
        phase = phase,
        portuale_bin = sh_quote(portuale_bin),
    )
}

// --- Client phases (slice 3) -------------------------------------------------

/// One phase invocation, generated bash: recreate the local
/// `run_phase_from_saved_env` + `run_one_phase_bash` setup client-side
/// (fresh `bash bin/ebuild.sh <phase>` process per phase -- the readonly
/// `EBUILD_PHASE` semantics demand it, exactly like local `spawnebuild`).
/// Path overrides are exported with client-side values; everything else
/// (EAPI, PN/PV/…, USE) rides the sourced saved environment, whose stale
/// path copies ebuild.sh's own `__preprocess_ebuild_env` strips (the
/// `environment.raw` marker enables that filtering, mirroring local).
/// `S` is deliberately *not* exported: ebuild.sh defaults it to
/// `${WORKDIR}/${P}` with `P` from the saved env (exact local behavior).
/// `PHASE_<phase>=<rc>` on stdout, phase log raw on stdout/stderr.
fn phase_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    portuale_bin: &str,
) -> String {
    // D keeps local's trailing slash. `UNITBIN` is the shipped runtime
    // dir (`$UNIT/bin`): `phase_exports` resolves `PORTAGE_PYTHON`
    // through it, so no shim falls back to `PATH` (#326 D5/S8.3).
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" {phase}\n",
            "rc=$?\n",
            "echo \"PHASE_{phase}=$rc\"\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            phase,
            portuale_bin,
        ),
        phase = phase,
    )
}

/// Locale variables the regen postinst run re-exports into the client
/// phase env: real's saved locale comes from the calling environment
/// through `environ_whitelist` (`special_env_vars.py`: `LANG` plus the
/// `LC_*` list), and for `mrg` the calling environment is the server
/// process. `LANGUAGE` is **not** in real's whitelist -- it rides along
/// here only because the slice brief names it explicitly; see
/// `collect_server_locale`. Only set variables are ever forwarded
/// (never invented defaults).
pub(crate) const REGEN_LOCALE_VARS: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_PAPER",
];

/// The server process's own locale values for the client regen phase
/// env, as `(name, value)` pairs in `REGEN_LOCALE_VARS` order -- one
/// entry per variable the getter reports as set (even empty: a set-but-
/// empty `LANG` is still the calling environment's value, which is what
/// real's whitelist passes through). Production passes
/// `|name| std::env::var(name).ok()`; tests inject a fake map.
fn collect_server_locale(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    REGEN_LOCALE_VARS
        .iter()
        .filter_map(|name| get(name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Real's `posixish_locale` EAPI attribute (`eapi.py:182,309`): true
/// from EAPI 6 on. Only posixish phases run the `LC_ALL` split below
/// (`EbuildPhase.py:51-56`); older EAPIs keep `LC_ALL` as-is.
fn eapi_is_posixish(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    // Real rejects an unknown EAPI outright; anything reaching us parsed
    // off an ebuild, so default an unparseable value to the modern rule
    // (the same direction the local phase path's unconditional split
    // already takes).
    digits.parse::<u64>().map(|n| n >= 6).unwrap_or(true)
}

/// Real's `locale_categories` (`portage/util/locale.py:22-36`): the full
/// set `split_LC_ALL` copies a set `LC_ALL` over -- the six POSIX
/// categories, `LC_PAPER`, and the five GNU extensions
/// (`LC_ADDRESS`, `LC_IDENTIFICATION`, `LC_MEASUREMENT`, `LC_NAME`,
/// `LC_TELEPHONE`). The fan-out below iterates this list, not the
/// forwarding set: real's split operates on settings, and for a
/// binpkg-merge postinst `$T/environment` does not exist yet, so
/// `config.environ()` applies no whitelist filter
/// (`config.py:3286-3299`) and all twelve reach the phase env and the
/// `PORTAGE_UPDATE_ENV` save (`EbuildPhase.py:158,259` runs the env
/// extractor for `pretend`/`prerm` only). Probed against real's own
/// `split_LC_ALL` + `environ()`: with `LC_ALL` set, all twelve export
/// when `$T/environment` is absent, seven when present.
pub(crate) const REAL_LOCALE_CATEGORIES: &[&str] = &[
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_ADDRESS",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_NAME",
    "LC_PAPER",
    "LC_TELEPHONE",
];

/// Port of real's `split_LC_ALL` (`portage/util/locale.py:160`) to the
/// forwarded regen locale set, for a posixish phase: a set `LC_ALL` fans
/// out over all twelve real `locale_categories` (see
/// `REAL_LOCALE_CATEGORIES`; real copies it over the same list,
/// unconditionally overwriting) and itself
/// disappears (real blanks it, then `config.environ()` deletes the
/// placeholder, `config.py:3374-3385`). `LANG` is filled from `LC_ALL`
/// only when the server left it unset -- real's split never touches
/// `LANG`, but with `LC_ALL` set and `LANG` unset the effective locale
/// is `LC_ALL`'s value everywhere, and the bed shows real's saved env
/// carrying `LANG` alongside the split categories. An explicitly set
/// `LANG` is never clobbered; `LANGUAGE` (not a locale category, rides
/// along per the l171b brief) is never touched. A set-but-empty
/// `LC_ALL` is dropped without fanning out (real's `if lc_all:` is
/// falsy, then `environ()` deletes the placeholder). Non-posixish
/// EAPIs return the pairs unchanged (real never calls the split there).
fn split_server_locale(locale: &[(String, String)], eapi: &str) -> Vec<(String, String)> {
    if !eapi_is_posixish(eapi) {
        return locale.to_vec();
    }
    let lc_all = locale
        .iter()
        .rev()
        .find(|(name, _)| name == "LC_ALL")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let mut out: Vec<(String, String)> = locale
        .iter()
        .filter(|(name, _)| name != "LC_ALL")
        .cloned()
        .collect();
    if lc_all.is_empty() {
        return out;
    }
    for name in REAL_LOCALE_CATEGORIES {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some(pair) => pair.1 = lc_all.clone(),
            None => out.push((name.to_string(), lc_all.clone())),
        }
    }
    if !out.iter().any(|(n, _)| n == "LANG") {
        out.push(("LANG".to_string(), lc_all));
    }
    out
}

/// The merge-time environment-regeneration postinst run (backlog #171):
/// real `vartree.py:5334` sets `PORTAGE_UPDATE_ENV=<dbpkgdir>/
/// environment.bz2` around *every* merge's postinst phase, and
/// `bin/phase-functions.sh:1072-1082` re-saves the vdb env from the
/// live phase env through it -- so the vdb env carries the merge-time
/// config, not the binpkg's build-time one. `mrg` runs this phase on
/// the client for every merged unit, **whether or not `pkg_postinst`
/// is defined**: real's `prerm|postrm|preinst|postinst|config|info`
/// case (`phase-functions.sh:1061-1083`) runs
/// `__ebuild_phase_with_hooks pkg_${1}` unconditionally
/// (`__ebuild_phase` is a no-op for an undefined function) and *then*
/// the `PORTAGE_UPDATE_ENV` block -- so an undefined `pkg_postinst`
/// still regenerates the env. `pkg_postinst` itself still runs only if
/// defined (the same no-op rule; nothing faked).
///
/// `PORTAGE_UPDATE_ENV` points at a unit-local file
/// (`$UNIT/environment.regen`) and `PORTAGE_BZIP2_COMMAND` at the
/// bundle's `bzip2-passthrough` (plain text out; the server compresses
/// -- the client must have no `bzip2`, plan §6). `FEATURES` /
/// `PORTAGE_FEATURES` carry the resolved client list (the local
/// `refresh_features` rule), so the env records the merge-time
/// features (e.g. no `buildpkg`). `locale` re-exports the server's own
/// locale values (see `REGEN_LOCALE_VARS` / `collect_server_locale`,
/// split per `split_server_locale` on posixish EAPIs) so
/// the regen'd env carries them exactly as real's calling-environment
/// whitelist does. `PHASE_postinst=<rc>` plus
/// `REGEN_ENV=ok|missing|skip:no-hooks` come back on stdout; the
/// script exits with the phase rc (a non-zero postinst is non-fatal --
/// real `_postinst_failure` -- as long as the regen file exists).
///
/// `O` is unset and `SHELL` unset before the phase runs: real's
/// phase shell never has `O` (`config.environ()` drops it via
/// `special_env_vars.py: environ_filter` even though `doebuild.py:475`
/// sets `mysettings["O"]`) and its saved `SHELL` is `declare --`
/// (bash initializes its own -- `SHELL` is in neither the whitelist
/// nor the config), while `mrg` exported a unit-local `O` and usually
/// inherits an exported `SHELL`. `LC_ALL` is unset on posixish EAPIs
/// and the forwarded locale arrives pre-split (see
/// `split_server_locale`); the unset also kills any `LC_ALL` leaking
/// in from the sourced build-time environment.
#[allow(clippy::too_many_arguments)]
fn postinst_regen_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    features: Option<&str>,
    locale: &[(String, String)],
    portuale_bin: &str,
) -> String {
    let mut extra = String::new();
    if let Some(list) = features.filter(|f| !f.is_empty()) {
        extra.push_str(&format!(
            "export FEATURES={features} PORTAGE_FEATURES={features}\n",
            features = sh_quote(list),
        ));
    }
    extra.push_str(&format!(
        "export PORTAGE_UPDATE_ENV={regen} PORTAGE_BZIP2_COMMAND={passthrough}\n",
        regen = sh_quote(&format!("{unit_dir}/environment.regen")),
        passthrough = sh_quote(&format!("{unit_dir}/bin/bzip2-passthrough")),
    ));
    // Backlog #171b: match real's phase shell (see the fn doc comment)
    // so the `PORTAGE_UPDATE_ENV` save matches real's saved env.
    extra.push_str("unset O\n");
    // Backlog #171c: `unset` (not `export -n`) reproduces real exactly:
    // bash self-initializes an unexported `SHELL`, while `export -n`
    // would keep a non-default server value.
    extra.push_str("unset SHELL\n");
    // Backlog #171c: real's posixish phases never see `LC_ALL`
    // (`split_LC_ALL` + `config.environ()`); the unset also covers an
    // `LC_ALL` leaking in from the sourced build-time environment, and
    // the exports below carry the pre-split values.
    let posixish = eapi_is_posixish(&staged.eapi);
    if posixish {
        extra.push_str("unset LC_ALL\n");
    }
    for (name, value) in split_server_locale(locale, &staged.eapi).iter() {
        extra.push_str(&format!(
            "export {name}={quoted}\n",
            quoted = sh_quote(value)
        ));
    }
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "if [ ! -f \"$UNIT/build-info/{pf}.ebuild\" ] || [ ! -f \"$UNIT/environment\" ]; then\n",
            "  echo \"REGEN_ENV=skip:no-hooks\"\n",
            "  echo \"PHASE_postinst=0\"\n",
            "  exit 0\n",
            "fi\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "{extra}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" postinst\n",
            "rc=$?\n",
            "echo \"PHASE_postinst=$rc\"\n",
            "if [ -s \"$UNIT/environment.regen\" ]; then echo \"REGEN_ENV=ok\"; else echo \"REGEN_ENV=missing\"; fi\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        pf = staged.pf,
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            "postinst",
            portuale_bin,
        ),
        extra = extra,
    )
}

// --- Client merge driver (slice 4): helpers ----------------------------------

/// Longest-prefix `is_protected` + `alloc_cfg` + `env_val` + `run_old_hook`
/// helpers shared by the merge flow. Pure bash, no placeholders except
/// the colormap/PATH roots baked by the caller template below.
const MERGE_HELPERS: &str = r#"mfail() { echo "MERGE_FAIL=$1 $2"; echo "STATUS=failed:$1"; exit 1; }
warn() { echo "MERGE_WARN=$1 $2"; }
env_val() {
  sed -n "s/^declare -x $2=\"\\(.*\\)\"$/\\1/p" "$1" | head -n 1
}
is_protected() {
  dest=$1; best_p=0; best_m=0
  for e in $PROTECT; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len; }
    fi
  done
  for e in $MASK; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len; }
    fi
  done
  [ "$best_p" -gt 0 ] && [ "$best_p" -gt "$best_m" ] && echo yes || echo no
}
alloc_cfg() {
  dir=${1%/*}; base=${1##*/}; n=0
  while [ -e "$dir/._cfg$(printf '%04d' $n)_$base" ]; do n=$((n + 1)); done
  echo "$dir/._cfg$(printf '%04d' $n)_$base"
}
run_old_hook() {
  vdbdir=$1; phase=$2
  set -- "$vdbdir"/*.ebuild
  [ -f "$1" ] || { echo "OLDHOOK=missing-ebuild"; return 2; }
  ebuild=$1
  OTMP="$WORKDIR/oldtmp"
  rm -rf "$OTMP"
  mkdir -p "$OTMP/temp" "$OTMP/work" "$OTMP/homedir" "$OTMP/files" "$OTMP/empty" "$OTMP/image" || return 1
  # Backlog #171: prefer the server-decompressed env the server staged
  # at `$UNIT/old-env/<pf>` (no client bzip2 needed); then the legacy
  # plain `environment` older `mrg` versions left, used only when no
  # `environment.bz2` exists; client `bzip2 -dc` only as a last resort;
  # then any plain `environment` at all (an older-`mrg` entry with both
  # files whose staging failed, on a bzip2-less client).
  pf=${vdbdir##*/}
  if [ -f "$UNIT/old-env/$pf" ]; then
    cp "$UNIT/old-env/$pf" "$OTMP/temp/environment" || return 1
  elif [ ! -f "$vdbdir/environment.bz2" ] && [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment.bz2" ] && command -v bzip2 >/dev/null 2>&1; then
    bzip2 -dc -- "$vdbdir/environment.bz2" > "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  else
    echo "OLDHOOK=no-env-bzip2-missing"; return 2
  fi
  : > "$OTMP/temp/environment.raw"
  EAPI=$(env_val "$OTMP/temp/environment" EAPI)
  [ -n "$EAPI" ] || { echo "OLDHOOK=no-eapi"; return 2; }
  export EAPI
  for v in CATEGORY PN PV PR PVR P PF; do
    val=$(env_val "$OTMP/temp/environment" "$v")
    [ -n "$val" ] || { echo "OLDHOOK=no-$v"; return 2; }
    export "$v=$val"
  done
  export EBUILD="$ebuild" O="$vdbdir"
  export ROOT="$ROOT" EROOT="$ROOT"
  export PORTAGE_BUILDDIR="$OTMP"
  export WORKDIR="$OTMP/work"
  export D="$OTMP/image/" ED="$OTMP/image/"
  export T="$OTMP/temp" HOME="$OTMP/homedir" FILESDIR="$OTMP/files"
  export PORTAGE_BIN_PATH="$UNITBIN"
  export PORTAGE_ECLASS_LOCATIONS=""
  export PORTAGE_PYTHON="$UNITBIN/portuale-python"
  export PORTAGE_PYM_PATH=/
  unset PORTAGE_IPC_DAEMON
  export PORTAGE_COLORMAP="$COLORMAP"
  export PORTAGE_TMPDIR="$WORKDIR"
  export SANDBOX_LOG="$OTMP/temp/sandbox.log"
  export EBUILD_PHASE="$phase" EMERGE_FROM=binary
  export PATH="$UNITBIN/ebuild-helpers:$PATH"
  bash "$UNITBIN/ebuild.sh" "$phase"
  rc=$?
  echo "OLDHOOK_$phase=$rc"
  return $rc
}
"#;

// --- Client merge flow (slice 4) -----------------------------------------------

/// Merge flow: old-prerm → ownership scan → gate+copy → vdb → remove-old
/// → old-postrm → env-update, appended after `MERGE_HELPERS`. Shell vars
/// (`UNIT`, `ROOT`, `VDB`, `VDBROOT`, `NEWPF`, `PKG`, `MAINS`, `PROTECT`,
/// `MASK`, `STATELESS`, …) come from `merge_script`'s header. Machine
/// lines `MERGE_<STEP>=…` plus a final `STATUS=merged` (or
/// `STATUS=failed:<step>` from `mfail`); any `mfail` prints
/// `MERGE_FAIL=<step> <detail>` and exits 1. `STATELESS=1` (server-side
/// vdb placement) skips old-version discovery and the ownership scan --
/// any existing file/symlink destination (outside CONFIG_PROTECT
/// divert) is a fail-closed collision, and old hooks never run.
/// New-postinst runs separately afterwards (see `run_bundle_stage`).
///
/// The `client:<path>` VDB is read and written by this bash on the
/// client, so it stays in the files format by construction: portuale
/// has no code there to call `portage-vdb` from (feat#157 design §6.3).
/// Only the server-side `VdbShadow` read goes through `portage-vdb`.
const MERGE_FLOW: &str = r##"OLD_PF=""; OLD_COUNTER=-1
if [ "$STATELESS" = 1 ]; then :; else
for d in "$VDBROOT"/"$PKG"-*/; do

  [ -d "$d" ] || continue
  cpf=${d%/}; cpf=${cpf##*/}
  [ "$cpf" = "$NEWPF" ] && continue
  rest=${cpf#"$PKG"-}
  case "$rest" in [0-9]*) ;; *) continue;; esac
  slot=$(cat "$d/SLOT" 2>/dev/null | cut -d/ -f1)
  [ "$slot" = "$MAINS" ] || continue
  c=$(cat "$d/COUNTER" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) c=-1;; esac
  if [ "$c" -gt "$OLD_COUNTER" ]; then OLD_COUNTER=$c; OLD_PF=$cpf; fi
done
fi
if [ -n "$OLD_PF" ]; then OLDVDB="$VDBROOT/$OLD_PF"; else OLDVDB=""; fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_PRERM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" prerm; then
    echo "MERGE_PRERM=ok $OLD_PF"
  else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_PRERM=skip $OLD_PF"; else echo "MERGE_PRERM=warn $OLD_PF"; fi
  fi
else
  echo "MERGE_PRERM=skip:none-installed"
fi
: > "$REPLACED_OWN"; : > "$OTHERS_OWN"
if [ "$STATELESS" != 1 ]; then
for c in "$VDB"/*/*/CONTENTS; do
  [ -f "$c" ] || continue
  pfdir=${c%/*}; pfdir=${pfdir##*/}
  awk '$1=="obj"||$1=="sym"{print $2}' "$c" > "$UNIT/scan.list"
  if [ "$pfdir" = "$NEWPF" ] || { [ -n "$OLD_PF" ] && [ "$pfdir" = "$OLD_PF" ]; }; then
    cat "$UNIT/scan.list" >> "$REPLACED_OWN"
  else
    cat "$UNIT/scan.list" >> "$OTHERS_OWN"
  fi
done
rm -f "$UNIT/scan.list"
fi
: > "$NEWCONTENTS"; : > "$NEWPATHS"
ROOTUID=$(id -u)
while read -r line; do
  [ -n "$line" ] || continue
  set -f; set -- $line; set +f; kind=$1
  case "$kind" in
    dir) rel=$4 ;;
    obj) rel=$4; md5=$2; mtime=$3 ;;
    sym) rel=$4; mtime=$3; target=$5 ;;
    *) mfail copy "unknown filemeta kind $kind" ;;
  esac
  src="$IMAGE/$rel"; dest="$ROOT/$rel"; apath="/$rel"
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    if [ "$STATELESS" = 1 ]; then
      # No ownership proof without a client vdb: the shape rules still
      # apply, but any existing file/symlink destination outside
      # CONFIG_PROTECT divert is a fail-closed collision (merging
      # directories and the divert itself need no ownership).
      if [ -d "$dest" ] && [ ! -L "$dest" ]; then
        [ "$kind" = dir ] || mfail collision "file over directory at $apath (stateless)";
      elif [ "$kind" = dir ]; then
        mfail collision "directory over file at $apath (stateless)";
      elif [ "$(is_protected "$dest")" = yes ]; then :;
      else mfail collision "stateless refusing to overwrite $apath"; fi
    elif grep -Fxq "$apath" "$REPLACED_OWN"; then :;
    elif grep -Fxq "$apath" "$OTHERS_OWN"; then mfail collision "$apath owned by another package";
    elif [ -d "$dest" ] && [ ! -L "$dest" ]; then
      [ "$kind" = dir ] || mfail collision "file over directory at $apath";
    elif [ "$kind" = dir ]; then
      mfail collision "directory over file at $apath";
    else :; fi
  fi
  case "$kind" in
    dir)
      if [ ! -d "$dest" ]; then
        mkdir -p "$dest" || mfail copy "mkdir $apath"
        [ "$ROOTUID" = 0 ] && chown --reference "$src" "$dest" || true
        chmod --reference "$src" "$dest" || mfail copy "chmod $apath"
      fi
      echo "dir $apath" >> "$NEWCONTENTS"
      ;;
    obj)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        if cmp -s "$src" "$dest"; then :;
        else dest=$(alloc_cfg "$dest"); fi
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # A symlink at dest would make `mv` treat a symlink-to-directory as
      # a destination directory (and `cp` follow it): drop the link
      # first, exactly like real movefile's `os.unlink(dest)` for a
      # symlink destination.
      [ -L "$dest" ] && rm -f "$dest"
      # Real movefile / local replace_file_atomic (portuale #96/#97):
      # materialize the new file at a temporary sibling and rename(2) it
      # over the destination. Never `cp` into the live inode -- ETXTBSY
      # protects a running executable, but an mmap'd shared library
      # opens fine, and rewriting it kills every process mapping it.
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      cp -p "$src" "$tmp" || { rm -f "$tmp"; mfail copy "$apath"; }
      [ "$ROOTUID" = 0 ] && chown --reference "$src" "$tmp" || true
      chmod --reference "$src" "$tmp" || { rm -f "$tmp"; mfail copy "chmod $apath"; }
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "obj $apath $md5 $mtime" >> "$NEWCONTENTS"
      ;;
    sym)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        cur=""; [ -L "$dest" ] && cur=$(readlink "$dest")
        [ "$cur" = "$target" ] || dest=$(alloc_cfg "$dest")
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # Same atomic replacement for the link: a symlink destination is
      # unlinked first (so `mv` cannot resolve it as a directory), then
      # the new link is built at a temporary sibling and renamed over
      # the destination -- never `rm` then `ln` on the live path.
      [ -L "$dest" ] && rm -f "$dest"
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      ln -s "$target" "$tmp" || { rm -f "$tmp"; mfail copy "symlink $apath"; }
      touch -h -r "$src" "$tmp" 2>/dev/null || true
      if [ "$ROOTUID" = 0 ]; then chown -h --reference "$src" "$tmp" 2>/dev/null || true; fi
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "sym $apath -> $target $mtime" >> "$NEWCONTENTS"
      ;;
  esac
  echo "$apath" >> "$NEWPATHS"
done < "$UNIT/filemeta"
echo "MERGE_COPY=ok"
# Stateless clients keep no vdb at all (plan §7: the server owns
# installed-db state) -- files merge, but no entry, COUNTER, or
# edb-counter is recorded.
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_VDB=skip:stateless-no-vdb"
else
rm -rf "$TMPVDB"; mkdir -p "$TMPVDB" || mfail vdb "mkdir tmp"
for f in "$UNIT/build-info"/*; do
  # NOTE: `cond && action || fail` misfires when cond is false -- always
  # an explicit if for fallible steps.
  if [ -f "$f" ]; then cp "$f" "$TMPVDB"/ || mfail vdb "copy build-info"; fi
done
# Backlog #171: no plain `environment` in the vdb (real writes
# `environment.bz2` only). The build-time `environment.bz2` rides the
# verbatim `build-info/*` copies above as the fallback until the
# postinst regen phase overwrites it; future unmerge hooks source that
# file (server-decompressed at ship time, never a plain copy).
printf '%s\n' "$CAT" > "$TMPVDB/CATEGORY"
printf '%s\n' "$FULLSLOT" > "$TMPVDB/SLOT"
printf '%s\n' "$REPO" > "$TMPVDB/repository"
cp "$NEWCONTENTS" "$TMPVDB/CONTENTS" || mfail vdb "write CONTENTS"
max_c=-1
for f in "$VDBROOT"/*/COUNTER "$ROOT/var/cache/edb/counter"; do
  [ -f "$f" ] || continue
  c=$(cat "$f" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) continue;; esac
  [ "$c" -gt "$max_c" ] && max_c=$c
done
NEXT=$((max_c + 1))
printf '%s' "$NEXT" > "$TMPVDB/COUNTER"
mkdir -p "$ROOT/var/cache/edb" && printf '%s' "$NEXT" > "$ROOT/var/cache/edb/counter"
tmpmeta="$TMPVDB/metadata.tmp"
: > "$tmpmeta"
for f in BDEPEND BUILD_ID BUILD_TIME CHOST COUNTER DEFINED_PHASES DEPEND DESCRIPTION EAPI HOMEPAGE IDEPEND IUSE KEYWORDS LICENSE PDEPEND PROPERTIES PROVIDES RDEPEND REQUIRES RESTRICT SLOT USE repository; do
  [ -f "$TMPVDB/$f" ] || continue
  printf '%s=%s\n' "$f" "$(tr -s '[:space:]' ' ' < "$TMPVDB/$f" | sed -e 's/^ *//' -e 's/ *$//')" >> "$tmpmeta"
done
if [ -s "$tmpmeta" ]; then
  { echo "#format=1"; LC_ALL=C sort "$tmpmeta"; } > "$TMPVDB/metadata"
  printf '#dir_mtime=%s\n' "$(stat -c %Y "$TMPVDB")" >> "$TMPVDB/metadata"
  rm -f "$tmpmeta"
fi
rm -rf "$NEWVDB"
mv "$TMPVDB" "$NEWVDB" || mfail vdb "rename into place"
echo "MERGE_VDB=ok"
fi
if [ -n "$OLD_PF" ]; then
  if [ -f "$OLDVDB/CONTENTS" ]; then
    : > "$UNIT/olddirs.list"
    while read -r line; do
      [ -n "$line" ] || continue
      set -f; set -- $line; set +f; kind=$1
      case "$kind" in
        obj|sym)
          apath=$2; recorded=${line##* }
          grep -Fxq "$apath" "$NEWPATHS" && continue
          if [ ! -e "$ROOT$apath" ] && [ ! -L "$ROOT$apath" ]; then continue; fi
          if [ "$(stat -c %Y "$ROOT$apath" 2>/dev/null)" = "$recorded" ]; then
            rm -f "$ROOT$apath" || warn remove "cannot remove $apath"
          else
            warn remove "modified-kept $apath"
          fi
          ;;
        dir) echo "$2" >> "$UNIT/olddirs.list" ;;
        *) warn remove "unmerge-skip $line" ;;
      esac
    done < "$OLDVDB/CONTENTS"
    sort -r "$UNIT/olddirs.list" | while read -r d; do
      [ -n "$d" ] && rmdir "$ROOT$d" 2>/dev/null || true
    done
    rm -f "$UNIT/olddirs.list"
  else
    warn remove "no CONTENTS in $OLD_PF, files kept"
  fi
  echo "MERGE_REMOVE=ok $OLD_PF"
elif [ "$STATELESS" = 1 ]; then
  echo "MERGE_REMOVE=skip:stateless-no-vdb"
else
  echo "MERGE_REMOVE=skip:none-installed"
fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_POSTRM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" postrm; then echo "MERGE_POSTRM=ok $OLD_PF"; else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_POSTRM=skip $OLD_PF"; else echo "MERGE_POSTRM=warn $OLD_PF"; fi
  fi
  rm -rf "$OLDVDB"
else
  echo "MERGE_POSTRM=skip:none-installed"
fi
if command -v ldconfig >/dev/null 2>&1; then
  if ldconfig -r "$ROOT" 2>/dev/null; then echo "MERGE_ENVUPDATE=ok ldconfig"; else echo "MERGE_ENVUPDATE=skip:ldconfig-failed"; fi
else
  echo "MERGE_ENVUPDATE=skip:no-ldconfig"
fi
if [ -n "$LEDGER_FILE" ]; then
  mkdir -p "${LEDGER_FILE%/*}" 2>/dev/null || warn ledger "mkdir for $LEDGER_FILE"
  if printf '%s\n' "$LEDGER_LINE" >> "$LEDGER_FILE" 2>/dev/null; then
    tail -n 10 "$LEDGER_FILE" > "$LEDGER_FILE.tmp" 2>/dev/null && mv "$LEDGER_FILE.tmp" "$LEDGER_FILE" 2>/dev/null || warn ledger "rotate $LEDGER_FILE"
    echo "MERGE_LEDGER=ok"
  else
    warn ledger "append $LEDGER_FILE"
  fi
else
  echo "MERGE_LEDGER=skip:none-requested"
fi
echo "MERGE_DONE=ok"
echo "STATUS=merged"
"##;

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver.
/// `None` file skips the client record (the `--remote-binpkg` trial path
/// keeps no ledger; the resolve path always records).
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5),
/// exported for the whole driver (including the old hooks, which inherit
/// the header's exports); `PORTAGE_PYM_PATH` is `/` and
/// `PORTAGE_IPC_DAEMON` is unset, like the phase scripts.
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}

#[allow(clippy::too_many_arguments)]
fn merge_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    vdb: &str,
    stateless: bool,
    protect_list: &str,
    mask_list: &str,
    ledger: Option<&LedgerSpec>,
    portuale_bin: &str,
) -> Result<String, String> {
    // A placed client vdb must stay inside the unit's world: reject the
    // degenerate empty path before it becomes `//var/db/pkg`.
    if vdb.trim().is_empty() {
        return Err("mrg: empty client vdb path".to_string());
    }
    Ok(format!(
        concat!(
            "UNIT={unit}\n",
            "IMAGE=\"$UNIT/image\"\n",
            "VDB={vdb}\n",
            "VDBROOT={vdb}/{category}\n",
            "STATELESS={stateless}\n",
            "CAT={category}\n",
            "PKG={pkg}\n",
            "NEWPF={pf}\n",
            "NEWVDB=\"$VDBROOT/$NEWPF\"\n",
            "TMPVDB=\"$VDBROOT/-MERGING-$NEWPF\"\n",
            "FULLSLOT={slot}\n",
            "MAINS={mainslot}\n",
            "REPO={repo}\n",
            "ROOT={root}\n",
            "WORKDIR={workdir}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "COLORMAP={colormap}\n",
            "PROTECT={protect}\n",
            "MASK={mask}\n",
            "LEDGER_FILE={ledger_file}\n",
            "LEDGER_LINE={ledger_line}\n",
            "NEWCONTENTS=\"$UNIT/CONTENTS.new\"\n",
            "NEWPATHS=\"$UNIT/paths.new\"\n",
            "REPLACED_OWN=\"$UNIT/replaced.own\"\n",
            "OTHERS_OWN=\"$UNIT/others.own\"\n",
            "{helpers}",
            "{flow}",
        ),
        unit = sh_quote(unit_dir),
        root = sh_quote(root),
        vdb = sh_quote(vdb),
        stateless = if stateless { "1" } else { "0" },
        category = sh_quote(&staged.category),
        pkg = sh_quote(&staged.pn),
        pf = sh_quote(&staged.pf),
        slot = sh_quote(&staged.manifest.slot),
        mainslot = sh_quote(staged.manifest.slot.split('/').next().unwrap_or("0")),
        repo = sh_quote(&staged.manifest.repo),
        workdir = sh_quote(
            &std::path::Path::new(unit_dir)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/var/tmp".to_string())
        ),
        colormap = sh_quote(&crate::color::phase_colormap_export()),
        protect = sh_quote(protect_list),
        mask = sh_quote(mask_list),
        ledger_file = sh_quote(&ledger.map(|l| l.file.clone()).unwrap_or_default()),
        ledger_line = sh_quote(&ledger.map(|l| l.line.clone()).unwrap_or_default()),
        helpers = MERGE_HELPERS,
        flow = MERGE_FLOW,
        portuale_bin = sh_quote(portuale_bin),
    ))
}

/// Client vdb destination for this run: the placed path, plus whether
/// the driver runs stateless (server-side vdb placement, plan §7 -- the
/// driver merges files only: no vdb entry, no old hooks, fail-closed
/// collisions). Shared by the merge driver and the #171 env-regen
/// install (a stateless merge has no vdb entry to attach the regen'd
/// env to, so the install is skipped while postinst still runs).
fn client_vdb_placement(ctx: &RemoteContext) -> (String, bool) {
    match &ctx.vdb {
        ConfigPlacement::Client(path) => (path.clone(), false),
        ConfigPlacement::Server(_) => (
            format!("{}/var/db/pkg", ctx.root.trim_end_matches('/')),
            true,
        ),
    }
}

/// Human tail of a merge failure: the `MERGE_FAIL` line plus a few log
/// lines, prefixed for the report.
fn merge_failure_tail(stdout: &str, stderr: &str, code: Option<i32>) -> String {
    let mut message = format!("mrg: client merge failed (exit {}):", code.unwrap_or(-1));
    let mut shown = 0;
    for line in stdout
        .lines()
        .filter(|l| l.starts_with("MERGE_FAIL=") || l.starts_with("MERGE_WARN="))
        .chain(stderr.lines())
        .take(6)
    {
        message.push_str(&format!("\nmrg:   {line}"));
        shown += 1;
        if shown >= 6 {
            break;
        }
    }
    message
}

/// Run the merge driver; `Ok(markers)` are the `MERGE_<STEP>=ok` lines
/// for the report, `Err(message)` the failure tail. The success gate is
/// the driver's own `STATUS=merged` trailer (plan §11), with exit 0 +
/// `MERGE_DONE=ok` kept as the backstop.
fn run_merge_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    ledger: Option<&LedgerSpec>,
) -> Result<Vec<String>, String> {
    let (vdb, stateless) = client_vdb_placement(ctx);
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let script = merge_script(
        unit_dir,
        staged,
        &ctx.root,
        &vdb,
        stateless,
        &ctx.config_protect,
        &ctx.config_protect_mask,
        ledger,
        &portuale_bin,
    )?;
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local merge command failed: {message}")
        } else {
            message
        }
    })?;
    let code = output.status.code();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stderr.lines().chain(stdout.lines()) {
        println!("{line}");
    }
    let values = parse_kv(&stdout);
    if output.status.success()
        && values.get("MERGE_DONE").map(String::as_str) == Some("ok")
        && values.get("STATUS").map(String::as_str) == Some("merged")
    {
        let mut markers = Vec::new();
        for step in [
            "MERGE_PRERM",
            "MERGE_COPY",
            "MERGE_VDB",
            "MERGE_REMOVE",
            "MERGE_POSTRM",
            "MERGE_ENVUPDATE",
        ] {
            if let Some(value) = values.get(step) {
                markers.push(format!("{step}={value}"));
            }
        }
        Ok(markers)
    } else {
        Err(merge_failure_tail(&stdout, &stderr, code))
    }
}

// --- Merge-time vdb environment regeneration (backlog #171) -------------------
//
// Real regenerates the vdb env from the live postinst environment of
// the merge (`vartree.py:5334` `PORTAGE_UPDATE_ENV=<dbpkgdir>/
// environment.bz2`; `bin/phase-functions.sh:1072-1082`), while `mrg`
// kept the binpkg's build-time env. The client regenerates
// uncompressed (no `bzip2` on the client, plan §6/§14.1) and the
// server compresses + installs; old-instance hooks get their env
// server-decompressed the same way.

/// Outcome of the regen postinst run: the phase rc (non-zero is
/// non-fatal, real `_postinst_failure`) plus whether the client left a
/// fresh `$UNIT/environment.regen` behind.
struct RegenReport {
    phase_rc: i32,
    regen_present: bool,
    skipped_no_hooks: bool,
}

/// Run the regen postinst script and print its log straight through.
/// `Err` is a transport/command failure only -- a non-zero phase rc or
/// a missing regen file is a warn-and-continue `RegenReport`, never a
/// unit failure (real `_postinst_failure`). `locale` is the server's
/// own locale pairs for the client phase env (see
/// `collect_server_locale`); the unit flow collects them from the
/// process environment, tests inject them explicitly.
fn run_postinst_regen_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    features: Option<&str>,
    locale: &[(String, String)],
) -> Result<RegenReport, String> {
    let colormap = crate::color::phase_colormap_export();
    let workdir_parent = std::path::Path::new(&ctx.workdir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/tmp".to_string());
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let script = postinst_regen_script(
        unit_dir,
        staged,
        &ctx.root,
        &workdir_parent,
        &colormap,
        features,
        locale,
        &portuale_bin,
    );
    let output = run_script_stdin(ctx, control, &script).map_err(|message| {
        if ctx.transport == RemoteTransport::Local {
            format!("mrg: local postinst regen command failed: {message}")
        } else {
            message
        }
    })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stderr.lines().chain(stdout.lines()) {
        println!("{line}");
    }
    let values = parse_kv(&stdout);
    let phase_rc: i32 = values
        .get("PHASE_postinst")
        .and_then(|rc| rc.parse().ok())
        .unwrap_or(-1);
    let regen = values
        .get("REGEN_ENV")
        .map(String::as_str)
        .unwrap_or("missing");
    Ok(RegenReport {
        phase_rc,
        regen_present: regen == "ok",
        skipped_no_hooks: regen == "skip:no-hooks",
    })
}

/// Pull one remote file's bytes to the server (`cat` remotely; a plain
/// read for the `local` transport, where client paths are server
/// paths). `Err` is the pull failure (the #171 regen warn path).
fn pull_file(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    remote_path: &str,
) -> Result<Vec<u8>, String> {
    match ctx.transport {
        RemoteTransport::Local => std::fs::read(remote_path)
            .map_err(|e| format!("mrg: pulling {remote_path} from the client failed: {e}")),
        RemoteTransport::Ssh => {
            let output =
                run_raw_command(ctx, control, &["cat".to_string(), remote_path.to_string()])?;
            let code = output.status.code();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success() && is_transport_error(code, &stderr) {
                return Err(format!(
                    "mrg: client {} unreachable:\n{stderr}",
                    ctx.hostname
                ));
            }
            if !output.status.success() {
                return Err(format!(
                    "mrg: pulling {remote_path} from the client failed (exit {}):\n{stderr}",
                    code.unwrap_or(-1)
                ));
            }
            Ok(output.stdout)
        }
    }
}

/// Server-side `bzip2` over piped bytes (the server has bzip2; the
/// client must not need it). The local merge shells the same binary
/// (`seed_saved_environment`, `remote_bundle::build_bundle`) -- there
/// is no Rust bzip2 crate in the tree, so this pipes through the
/// system one instead of adding a dependency.
fn bzip2_pipe(args: &[&str], bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new("bzip2")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("mrg: cannot spawn server bzip2: {e}"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| "mrg: bzip2 stdin unavailable".to_string())?
        .write_all(bytes)
        .map_err(|e| format!("mrg: feeding server bzip2: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("mrg: waiting for server bzip2: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "mrg: server bzip2 {} failed (exit {}): {}",
            args.join(" "),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// Server-side compression for the pulled regen'd env: `bzip2 -9`, the
/// same level real's `PORTAGE_UPDATE_ENV` block uses (`-f9`).
fn bzip2_compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-c", "-f9"], bytes)
}

/// Server-side decompression for an old instance's `environment.bz2`.
fn bzip2_decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    bzip2_pipe(&["-d", "-c", "--"], bytes)
}

/// Rewrite the `${PORTAGE_BZIP2_COMMAND}` line in a pulled regen'd
/// environment **only** when its value is the unit-local
/// `bzip2-passthrough` stand-in the regen postinst run used, replacing
/// it with the resolved client command (`bzip2_command` -- the
/// server-resolved client `PORTAGE_BZIP2_COMMAND`, `make.globals`'
/// `bzip2` when nothing is configured). Real's merge-time save records
/// its live value (verified on this host's own
/// `/var/db/pkg/*/environment.bz2`, which all carry
/// `declare -x PORTAGE_BZIP2_COMMAND="bzip2"`), while ours ran with a
/// per-unit workdir path that must not leak into the vdb (and would
/// keep the L1 `environment` rows red: `normalize.py` compares the
/// content verbatim). Any other value -- real's own spelling, a
/// foreign path, anything -- is copied verbatim; a missing line stays
/// missing (never invent it).
fn scrub_bzip2_command(bytes: &[u8], bzip2_command: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let is_passthrough = match line.iter().position(|b| *b == b'=') {
            Some(eq) => {
                let mut name = &line[..eq];
                for prefix in ["declare -x ".as_bytes(), "declare -- ".as_bytes()] {
                    if let Some(rest) = name.strip_prefix(prefix) {
                        name = rest;
                        break;
                    }
                }
                name == b"PORTAGE_BZIP2_COMMAND"
                    && line
                        .windows(b"bzip2-passthrough".len())
                        .any(|w| w == b"bzip2-passthrough")
            }
            None => false,
        };
        if is_passthrough {
            let escaped = bzip2_command.replace('\\', "\\\\").replace('"', "\\\"");
            out.extend_from_slice(
                format!("declare -x PORTAGE_BZIP2_COMMAND=\"{escaped}\"\n").as_bytes(),
            );
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

/// Install `bytes` at `dest` on the client atomically (temp name +
/// `mv`, so a concurrent unmerge never reads a half-written env). A
/// failed `mv` best-effort removes the remote temp name (review #4 --
/// no client tmp litter).
fn install_file_atomic(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    bytes: &[u8],
    dest: &str,
) -> Result<(), String> {
    let tmp = format!("{dest}.portuale-regen-tmp");
    match ctx.transport {
        RemoteTransport::Local => {
            if let Some(parent) = std::path::Path::new(&tmp).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&tmp, bytes).map_err(|e| format!("mrg: writing {tmp} failed: {e}"))?;
            std::fs::rename(&tmp, dest).map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("mrg: installing {dest} failed: {e}")
            })?;
            Ok(())
        }
        RemoteTransport::Ssh => {
            // Backlog #262: `TempDir::new` creates a directory (backlog
            // #260), so the bytes must ride a file *inside* it --
            // staging straight into its path fails every ssh install
            // with `Is a directory`, and the vdb keeps the build-time
            // env (with its stray `declare -- x=""`) instead of the
            // regen'd one.
            let server_dir = TempDir::new("portuale-regen-install").keep();
            let server_tmp = server_dir.join("regen.env");
            std::fs::write(&server_tmp, bytes)
                .map_err(|e| format!("mrg: staging the regen install failed: {e}"))?;
            let result = send_file(ctx, control, &server_tmp, &tmp).and_then(|()| {
                let output = run_raw_command(
                    ctx,
                    control,
                    &["mv".to_string(), tmp.clone(), dest.to_string()],
                )?;
                if output.status.success() {
                    Ok(())
                } else {
                    // Best-effort: don't litter the client tmp name.
                    let _ = run_raw_command(
                        ctx,
                        control,
                        &["rm".to_string(), "-f".to_string(), tmp.clone()],
                    );
                    Err(format!(
                        "mrg: moving {tmp} into place failed (exit {})",
                        output.status.code().unwrap_or(-1)
                    ))
                }
            });
            let _ = std::fs::remove_dir_all(&server_dir);
            result
        }
    }
}

/// Pull the regen'd env, scrub it, compress it server-side and install
/// it over the vdb's build-time `environment.bz2`. `Err` is
/// `(failing step, detail)` -- `pull`, `compress` or `install` -- for
/// the caller's one-`!!!` warn-and-continue line. `regen_bzip2` is the
/// resolved client `PORTAGE_BZIP2_COMMAND` for the scrub (`None` on
/// paths that resolve no config -- the `make.globals` default).
fn install_regenerated_env(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    vdb_env_bz2: &str,
    regen_bzip2: Option<&str>,
) -> Result<(), (String, String)> {
    let regen = format!("{unit_dir}/environment.regen");
    let bytes = pull_file(ctx, control, &regen).map_err(|message| ("pull".to_string(), message))?;
    let scrubbed = scrub_bzip2_command(&bytes, regen_bzip2.unwrap_or("bzip2"));
    let compressed =
        bzip2_compress(&scrubbed).map_err(|message| ("compress".to_string(), message))?;
    install_file_atomic(ctx, control, &compressed, vdb_env_bz2)
        .map_err(|message| ("install".to_string(), message))?;
    Ok(())
}

/// The #171 failure line (coordinator decision): names the package and
/// the failing step (`postinst`, `pull`, `compress`, `install`); the
/// vdb keeps the build-time `environment.bz2` and the merge continues.
fn regen_warn_message(cpv: &str, step: &str) -> String {
    format!(
        "!!! Remote {cpv}: merge-time environment regen failed at {step}, keeping build-time environment.bz2"
    )
}

/// A staged old-hook environment: which replaced-version pfs got a
/// server-decompressed env under `$UNIT/old-env/`, plus per-pf failure
/// details for the report (the driver falls back to the client vdb for
/// those -- the legacy plain file, then client `bzip2`).
struct OldHookShipment {
    shipped: Vec<String>,
    warnings: Vec<String>,
}

/// `OLDPF=<pf> SLOT=<slot>` lines out of the old-version probe
/// (anything else ignored -- the driver owns discovery, this only
/// stages envs).
fn parse_oldpf_probe(stdout: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let rest = match line.trim().strip_prefix("OLDPF=") {
            Some(rest) => rest,
            None => continue,
        };
        let (pf, slot) = match rest.split_once(" SLOT=") {
            Some((pf, slot)) => (pf, slot),
            None => continue,
        };
        if pf.is_empty() {
            continue;
        }
        out.push((pf.to_string(), slot.to_string()));
    }
    out
}

/// The old-hook staging warning: names the package and the failing
/// detail; old hooks for that pf fall back to the client vdb.
fn old_hook_warn_message(cpv: &str, detail: &str) -> String {
    format!(
        "!!! Remote {cpv}: could not stage the saved env for an old instance ({detail}); old hooks fall back to the client vdb"
    )
}

/// Stage every same-slot installed version's saved env for the merge
/// driver's `run_old_hook` (backlog #171): probe the client vdb for
/// same-package versions, pull each `environment.bz2`, decompress it
/// **server-side**, and ship the plain text to `$UNIT/old-env/<pf>`.
/// A version with no bz2 (older `mrg` entries carry only a plain
/// `environment`) ships that file as-is; a version with neither
/// warns with both pull errors (and still hits the driver's own
/// warn-and-skip). Best-effort throughout:
/// every failure lands in `warnings` and the merge continues.
#[allow(clippy::too_many_arguments)]
fn ship_old_hook_envs(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    vdb: &str,
    category: &str,
    pn: &str,
    new_pf: &str,
    new_mainslot: &str,
) -> OldHookShipment {
    let mut shipment = OldHookShipment {
        shipped: Vec::new(),
        warnings: Vec::new(),
    };
    let vdbroot = format!("{vdb}/{category}");
    // Same discovery shape as the driver's own replace loop
    // (`$PKG-<digit...>` names, SLOT first field); the driver still
    // decides, this only stages envs.
    let script = format!(
        concat!(
            "UNIT={unit}\n",
            "VDBROOT={vdbroot}\n",
            "PKG={pkg}\n",
            "mkdir -p \"$UNIT/old-env\" 2>/dev/null || true\n",
            "for d in \"$VDBROOT/\"$PKG-*/; do\n",
            "  [ -d \"$d\" ] || continue\n",
            "  cpf=${{d%/}}; cpf=${{cpf##*/}}\n",
            "  rest=${{cpf#\"$PKG-\"}}\n",
            "  case \"$rest\" in [0-9]*) ;; *) continue;; esac\n",
            "  [ \"$cpf\" = {newpf} ] && continue\n",
            "  slot=$(cat \"$d/SLOT\" 2>/dev/null | cut -d/ -f1)\n",
            "  echo \"OLDPF=$cpf SLOT=${{slot:-unknown}}\"\n",
            "done\n",
        ),
        unit = sh_quote(unit_dir),
        vdbroot = sh_quote(&vdbroot),
        pkg = sh_quote(pn),
        newpf = sh_quote(new_pf),
    );
    let output = match run_script_stdin(ctx, control, &script) {
        Ok(output) => output,
        Err(message) => {
            shipment.warnings.push(format!("probe: {message}"));
            return shipment;
        }
    };
    if !output.status.success() {
        shipment.warnings.push(format!(
            "probe exited {}",
            output.status.code().unwrap_or(-1)
        ));
        return shipment;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    for (pf, slot) in parse_oldpf_probe(&stdout) {
        if pf == new_pf {
            continue;
        }
        // Unknown slots ship anyway (fail-open toward staging; the
        // driver decides what to replace).
        if slot != "unknown" && slot != new_mainslot {
            continue;
        }
        let old_bz2 = format!("{vdbroot}/{pf}/environment.bz2");
        match pull_file(ctx, control, &old_bz2) {
            Ok(compressed) => match bzip2_decompress(&compressed) {
                Ok(plain) => {
                    let dest = format!("{unit_dir}/old-env/{pf}");
                    match send_bytes(ctx, control, &plain, &dest) {
                        Ok(()) => shipment.shipped.push(pf),
                        Err(message) => shipment.warnings.push(format!("{pf}: send: {message}")),
                    }
                }
                Err(message) => shipment
                    .warnings
                    .push(format!("{pf}: decompress: {message}")),
            },
            Err(bz2_message) => {
                // No bz2 (older `mrg` entries carry only a plain file,
                // or nothing at all): ship the plain text as-is. When
                // the plain pull fails too, the original bz2 error goes
                // into `warnings` (review #3 -- silent staging failures
                //); a version with neither file warns here and hits the
                // driver's own warn-and-skip as well.
                let old_plain = format!("{vdbroot}/{pf}/environment");
                match pull_file(ctx, control, &old_plain) {
                    Ok(plain) => {
                        let dest = format!("{unit_dir}/old-env/{pf}");
                        if let Err(message) = send_bytes(ctx, control, &plain, &dest) {
                            shipment.warnings.push(format!("{pf}: send: {message}"));
                        } else {
                            shipment.shipped.push(pf);
                        }
                    }
                    Err(plain_message) => shipment.warnings.push(format!(
                        "{pf}: env pull: {bz2_message}; plain fallback: {plain_message}"
                    )),
                }
            }
        }
    }
    shipment
}

/// The S8.1-resolved absolute client binary for generated scripts
/// (#326 D5/S8.3): every script exports this as `PORTUALE_BIN`.
fn client_bin_abs(ctx: &RemoteContext) -> Result<&str, String> {
    ctx.bin_plan.abs().ok_or_else(|| {
        "mrg: internal error: the client binary was never resolved (preflight did not run)"
            .to_string()
    })
}

/// Run the staged phases in order, stopping at the first non-zero
/// (pretend/setup/preinst are all fatal -- local rule kept). Returns the
/// per-phase `ok` markers for the report line.
fn run_phases_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
) -> Result<Vec<String>, String> {
    let portuale_bin = client_bin_abs(ctx)?.to_string();
    let colormap = crate::color::phase_colormap_export();
    let workdir_parent = std::path::Path::new(&ctx.workdir)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/var/tmp".to_string());
    let mut done = Vec::new();
    for phase in &staged.phases {
        let script = phase_script(
            unit_dir,
            staged,
            phase,
            &ctx.root,
            &workdir_parent,
            &colormap,
            &portuale_bin,
        );
        let output = run_script_stdin(ctx, control, &script).map_err(|message| {
            if ctx.transport == RemoteTransport::Local {
                format!("mrg: local phase {phase} command failed: {message}")
            } else {
                message
            }
        })?;
        let code = output.status.code();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stderr.lines().chain(stdout.lines()) {
            println!("{line}");
        }
        let reported: i32 = parse_kv(&stdout)
            .get(&format!("PHASE_{phase}"))
            .and_then(|rc| rc.parse().ok())
            .unwrap_or(-1);
        if output.status.success() && reported == 0 {
            done.push(format!("{phase} ok"));
        } else {
            let mut message = format!(
                "mrg: client phase {phase} failed (exit {}, PHASE_{phase}={reported}):",
                code.unwrap_or(-1)
            );
            for line in stderr.lines().chain(stdout.lines()).take(5) {
                message.push_str(&format!("\nmrg:   {line}"));
            }
            return Err(message);
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    /// #313: a local-transport client script gets `PORTUALE_BIN` (the shim's
    /// target) and has the server's `PORTUALE_VDB_*` removed from its
    /// inherited environment (`get_envs` reports a removal as `None`).
    #[test]
    fn local_client_bash_exports_the_binary_and_scrubs_the_servers_vdb_selection() {
        let cmd = local_client_bash();
        let envs: std::collections::BTreeMap<_, _> = cmd.get_envs().collect();
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            envs.get(std::ffi::OsStr::new("PORTUALE_BIN")),
            Some(&Some(exe.as_os_str()))
        );
        for var in [
            "PORTUALE_VDB_BACKEND",
            "PORTUALE_VDB_PATH",
            "PORTUALE_VDB_ROOT",
            crate::vdb_ipc::IPC_VAR,
        ] {
            assert_eq!(envs.get(std::ffi::OsStr::new(var)), Some(&None), "{var}");
        }
    }

    #[test]
    fn sh_quote_wraps_and_escapes() {
        assert_eq!(sh_quote("/"), "'/'");
        assert_eq!(sh_quote("/var/tmp/a b"), "'/var/tmp/a b'");
        assert_eq!(sh_quote("o'clock"), "'o'\\''clock'");
        assert_eq!(sh_quote(""), "''");
        assert_eq!(sh_quote("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn strict_host_key_checking_parses() {
        assert_eq!(
            StrictHostKeyChecking::parse("accept-new"),
            Some(StrictHostKeyChecking::AcceptNew)
        );
        assert_eq!(
            StrictHostKeyChecking::parse("yes"),
            Some(StrictHostKeyChecking::Yes)
        );
        assert_eq!(
            StrictHostKeyChecking::parse("no"),
            Some(StrictHostKeyChecking::No)
        );
        assert_eq!(StrictHostKeyChecking::parse("sometimes"), None);
        assert_eq!(
            StrictHostKeyChecking::AcceptNew.as_ssh_value(),
            "accept-new"
        );
    }

    #[test]
    fn clock_gate_bounds_skew_and_disables_at_zero() {
        assert_eq!(clock_gate(Some("1000"), 1000, 900), None);
        assert_eq!(clock_gate(Some("100"), 1000, 900), None);
        assert_eq!(
            clock_gate(Some("99"), 1000, 900),
            Some("client/server clocks differ by 901s (limit 900s)".to_string())
        );
        // `0` disables the check entirely.
        assert_eq!(clock_gate(Some("1"), 1_000_000, 0), None);
        // Unparsable client time fails open here (the bash gate already
        // failed -- no CLIENT_TIME means the script never ran right).
        assert_eq!(clock_gate(None, 1000, 900), None);
        assert_eq!(clock_gate(Some("soon"), 1000, 900), None);
    }

    #[test]
    fn preflight_gates_catch_old_bash_missing_tools_and_roots() {
        let mut ok = HashMap::new();
        for (k, v) in [
            ("BASH_MAJOR", "5"),
            ("BASH_MINOR", "3"),
            ("TOOL_tar", "yes"),
            ("TOOL_mkdir", "yes"),
            ("TOOL_rm", "yes"),
            ("TOOL_cat", "yes"),
            ("TOOL_chmod", "yes"),
            ("TOOL_ln", "yes"),
            ("TOOL_find", "yes"),
            ("TOOL_grep", "yes"),
            ("TOOL_sed", "yes"),
            ("TOOL_cmp", "yes"),
            ("TOOL_stat", "yes"),
            ("TOOL_readlink", "yes"),
            ("TOOL_id", "yes"),
            ("TOOL_tail", "yes"),
            ("TOOL_sha256sum", "yes"),
            ("TOOL_mv", "yes"),
            ("TOOL_uname", "yes"),
            ("TOOL_mktemp", "yes"),
            ("ROOT_WRITABLE", "yes"),
            ("VDB_DIR", "yes"),
            ("WORKDIR", "writable"),
        ] {
            ok.insert(k.to_string(), v.to_string());
        }
        assert_eq!(preflight_gates(&ok), (Vec::new(), Vec::new()));

        let mut old = ok.clone();
        old.insert("BASH_MINOR".to_string(), "2".to_string());
        let (failures, _) = preflight_gates(&old);
        assert!(failures.iter().any(|f| f.contains("5.3")), "{failures:?}");

        let mut missing = ok.clone();
        missing.insert("TOOL_tar".to_string(), "no".to_string());
        missing.insert("TOOL_tail".to_string(), "no".to_string());
        missing.insert("TOOL_sha256sum".to_string(), "no".to_string());
        missing.insert("ROOT_WRITABLE".to_string(), "no".to_string());
        missing.insert("WORKDIR".to_string(), "uncreatable".to_string());
        let (failures, _) = preflight_gates(&missing);
        assert_eq!(failures.len(), 5, "{failures:?}");

        // A missing vdb warns (slice-5 placement matrix owns the degrade),
        // never fails.
        let mut novdb = ok.clone();
        novdb.insert("VDB_DIR".to_string(), "no".to_string());
        let (failures, warnings) = preflight_gates(&novdb);
        assert!(failures.is_empty());
        assert_eq!(warnings.len(), 1);
    }

    // --- #326 S8.1: digest, architecture, binary preflight gates ----------

    /// Fake (but well-formed) full SHA-256 hex for gate tests: the
    /// preflight values carry hex text, so any 64-hex string decides the
    /// same way a real digest would.
    const S8_DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const S8_SHORT: &str = "0123456789abcdef";
    const S8_WRONG: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    fn s8_pre(dirs: &[&str], machine: u16) -> BinPreflight {
        let dirs: Vec<String> = dirs.iter().map(|s| s.to_string()).collect();
        BinPreflight {
            candidates: bin_candidates(&dirs, S8_SHORT),
            digest: S8_DIGEST.to_string(),
            bin_path: "/server/portuale".to_string(),
            machine,
            dirs,
        }
    }

    /// Parsed preflight values for the binary gates: `cands` are the
    /// `CAND_<i>_SHA` facts in D5 order, `dir_states` the
    /// `(EXISTS, WRITABLE, MODE)` triple per directory.
    fn s8_values(
        cands: &[&str],
        dir_states: &[(&str, &str, &str)],
        uname: &str,
    ) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (i, sha) in cands.iter().enumerate() {
            map.insert(format!("CAND_{i}_SHA"), sha.to_string());
        }
        for (j, (exists, writable, mode)) in dir_states.iter().enumerate() {
            map.insert(format!("DIR_{j}_EXISTS"), exists.to_string());
            map.insert(format!("DIR_{j}_WRITABLE"), writable.to_string());
            map.insert(format!("DIR_{j}_MODE"), mode.to_string());
        }
        map.insert("UNAME_M".to_string(), uname.to_string());
        map
    }

    #[test]
    fn elf_machine_reads_both_endians_and_rejects_garbage() {
        // Little-endian x86-64 (EM_X86_64 = 62).
        let mut le = vec![0u8; 24];
        le[0..4].copy_from_slice(b"\x7fELF");
        le[4] = 2;
        le[5] = 1;
        le[18] = 62;
        le[19] = 0;
        assert_eq!(elf_machine(&le), Ok(62));
        // Big-endian short (EM_PPC64 = 21 reads byte-swapped).
        let mut be = vec![0u8; 24];
        be[0..4].copy_from_slice(b"\x7fELF");
        be[4] = 2;
        be[5] = 2;
        be[18] = 0;
        be[19] = 21;
        assert_eq!(elf_machine(&be), Ok(21));
        assert!(elf_machine(&le[..10]).is_err());
        let mut bad = le.clone();
        bad[0] = b'X';
        assert!(
            bad.get(0..4)
                .is_some_and(|magic| elf_machine(&bad).is_err() && magic != b"\x7fELF")
        );
        let mut enc = le.clone();
        enc[5] = 9;
        assert!(elf_machine(&enc).is_err());
    }

    #[test]
    fn arch_matches_maps_machines_to_uname_names() {
        assert_eq!(arch_matches(62, "x86_64"), Some(true));
        assert_eq!(arch_matches(62, "amd64"), Some(true));
        assert_eq!(arch_matches(183, "aarch64"), Some(true));
        assert_eq!(arch_matches(183, "arm64"), Some(true));
        assert_eq!(arch_matches(3, "i686"), Some(true));
        assert_eq!(arch_matches(3, "i386"), Some(true));
        assert_eq!(arch_matches(40, "armv7l"), Some(true));
        assert_eq!(arch_matches(243, "riscv64"), Some(true));
        assert_eq!(arch_matches(21, "ppc64le"), Some(true));
        assert_eq!(arch_matches(22, "s390x"), Some(true));
        // Definite mismatches.
        assert_eq!(arch_matches(62, "aarch64"), Some(false));
        assert_eq!(arch_matches(183, "x86_64"), Some(false));
        assert_eq!(arch_matches(3, "x86_64"), Some(false));
        // Unknown pairs pass (a mismatch only when both are known).
        assert_eq!(arch_matches(9999, "x86_64"), None);
        assert_eq!(arch_matches(62, ""), None);
        assert_eq!(arch_matches(9999, ""), None);
    }

    #[test]
    fn bin_search_dirs_prefers_the_override_pair() {
        let mut ctx = ctx_with_args("h", None);
        assert_eq!(bin_search_dirs(&ctx), vec!["/opt/bin", "/usr/local/bin"]);
        ctx.portuale_dir = Some("/srv/bin".to_string());
        assert_eq!(bin_search_dirs(&ctx), vec!["/srv/bin"]);
        // D5 order: per directory, `portuale` then `portuale-<hash>`.
        assert_eq!(
            bin_candidates(&bin_search_dirs(&ctx), S8_SHORT),
            vec![
                "/srv/bin/portuale".to_string(),
                format!("/srv/bin/portuale-{S8_SHORT}"),
            ]
        );
    }

    #[test]
    fn bin_gates_use_the_first_match_in_d5_order() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
        // Both a plain and a hashed candidate match: the first in D5
        // order (the plain `/opt/bin/portuale`) wins.
        let values = s8_values(
            &[S8_DIGEST, "missing", S8_DIGEST, "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(plan, ClientBinPlan::Use("/opt/bin/portuale".to_string()));
        // Only the hashed name matches.
        let values = s8_values(
            &["missing", S8_DIGEST, "missing", "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Use(format!("/opt/bin/portuale-{S8_SHORT}"))
        );
        // A later directory's match wins when nothing earlier matches.
        let values = s8_values(
            &["missing", "missing", S8_DIGEST, "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Use("/usr/local/bin/portuale".to_string())
        );
    }

    #[test]
    fn bin_gates_plan_an_install_when_nothing_matches() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
        let values = s8_values(
            &["missing", "missing", "missing", "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
        );
        // An unwritable first directory falls through to the second.
        let values = s8_values(
            &["missing", "missing", "missing", "missing"],
            &[("yes", "no", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Install(format!("/usr/local/bin/portuale-{S8_SHORT}"))
        );
    }

    #[test]
    fn bin_gates_report_a_mismatched_plain_portuale_and_leave_it_alone() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
        // A non-matching plain `portuale` is reported and left alone:
        // the install goes next to it as `portuale-<hash>`, never over
        // it.
        let values = s8_values(
            &[S8_WRONG, "missing", "missing", "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
        );
        assert!(
            notes
                .iter()
                .any(|n| n.contains("/opt/bin/portuale") && n.contains("left alone")),
            "{notes:?}"
        );
    }

    #[test]
    fn bin_gates_replace_a_corrupted_hashed_binary() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
        // A `portuale-<hash>` whose content does not match its name is
        // planned for replacement at the same path.
        let values = s8_values(
            &["missing", S8_WRONG, "missing", "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
        );
        assert!(
            notes.iter().any(|n| n.contains("planned for replacement")),
            "{notes:?}"
        );
    }

    #[test]
    fn bin_gates_fail_an_arch_mismatch_naming_the_binary_option() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 183);
        let values = s8_values(
            &["missing", "missing", "missing", "missing"],
            &[("yes", "yes", "755"), ("yes", "yes", "755")],
            "x86_64",
        );
        let (_, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("--remote-portuale-binary"),
            "{failures:?}"
        );
        assert!(failures[0].contains("x86_64"), "{failures:?}");
        // Unknown pairs pass (both sides must be known to mismatch).
        let unknown = s8_pre(&["/opt/bin"], 9999);
        let values = s8_values(&["missing", "missing"], &[("yes", "yes", "755")], "x86_64");
        let (plan, failures, _) = evaluate_bin_gates(&values, &unknown, false);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(matches!(plan, ClientBinPlan::Install(_)), "{plan:?}");
    }

    #[test]
    fn bin_gates_fail_without_a_writable_dir() {
        let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
        // Neither default directory usable, no override: the message
        // names the option and the root-login alternative (Q1).
        let values = s8_values(
            &["missing", "missing", "missing", "missing"],
            &[("no", "no", "unknown"), ("yes", "no", "755")],
            "x86_64",
        );
        let (_, failures, _) = evaluate_bin_gates(&values, &pre, false);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("--remote-portuale-dir"),
            "{failures:?}"
        );
        assert!(failures[0].contains("root"), "{failures:?}");
        // With the override, the failure names the override directory.
        let over = s8_pre(&["/srv/bin"], 62);
        let values = s8_values(
            &["missing", "missing"],
            &[("no", "no", "unknown")],
            "x86_64",
        );
        let (_, failures, _) = evaluate_bin_gates(&values, &over, true);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].contains("--remote-portuale-dir") && failures[0].contains("/srv/bin"),
            "{failures:?}"
        );
    }

    #[test]
    fn bin_gates_refuse_a_world_writable_dir_without_the_sticky_bit() {
        let pre = s8_pre(&["/srv/bin"], 62);
        // `0777`: world-writable, no sticky bit -- refused even though
        // the directory exists and is writable.
        let values = s8_values(&["missing", "missing"], &[("yes", "yes", "777")], "x86_64");
        let (_, failures, _) = evaluate_bin_gates(&values, &pre, true);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].contains("sticky"), "{failures:?}");
        // `1777` (sticky) and `0755` (not world-writable) are usable.
        for mode in ["1777", "755", "775"] {
            let values = s8_values(&["missing", "missing"], &[("yes", "yes", mode)], "x86_64");
            let (plan, failures, _) = evaluate_bin_gates(&values, &pre, true);
            assert!(failures.is_empty(), "{mode}: {failures:?}");
            assert_eq!(
                plan,
                ClientBinPlan::Install(format!("/srv/bin/portuale-{S8_SHORT}")),
                "{mode}"
            );
        }
    }

    #[test]
    fn preflight_script_reports_candidates_dirs_and_uname() {
        // Render level with real temp dirs: the script's candidate
        // digests, writability and mode facts must match `sha256sum` /
        // `stat` run directly.
        let tmp = TempDir::new("portuale-remote-binscript").keep();
        let dir = tmp.join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("portuale");
        std::fs::write(&present, b"client binary bytes").unwrap();
        let missing = dir.join("portuale-0123456789abcdef");
        assert!(!missing.exists());
        let dirs = vec![dir.to_string_lossy().into_owned()];
        let candidates = vec![
            present.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ];
        let script = preflight_script(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
            Some((&dirs, &candidates)),
        );
        assert!(script.contains("sha256sum"), "tool floor:\n{script}");
        assert!(script.contains("UNAME_M"), "uname fact:\n{script}");
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs the preflight script");
        assert!(output.status.success());
        let values = parse_kv(&String::from_utf8_lossy(&output.stdout));
        use sha2::Digest as _;
        let expected = format!("{:x}", sha2::Sha256::digest(b"client binary bytes"));
        assert_eq!(
            values.get("CAND_0_SHA").map(String::as_str),
            Some(expected.as_str())
        );
        assert_eq!(
            values.get("CAND_1_SHA").map(String::as_str),
            Some("missing")
        );
        assert!(
            values.get("UNAME_M").is_some_and(|m| !m.is_empty()),
            "{values:?}"
        );
        assert_eq!(values.get("DIR_0_EXISTS").map(String::as_str), Some("yes"));
        assert_eq!(
            values.get("DIR_0_WRITABLE").map(String::as_str),
            Some("yes")
        );
        let mode = values.get("DIR_0_MODE").map(String::as_str).unwrap_or("?");
        assert!(
            u32::from_str_radix(mode.trim(), 8).is_ok(),
            "DIR_0_MODE must be octal: {values:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `portuale` must never be invoked by bare name in generated client
    /// scripts -- only through `$PORTUALE_BIN`, an absolute path, or a
    /// non-command token (`portuale-python`, `portuale-<hash>`,
    /// `portuale-remote:`, `.portuale.` mktemp names, the ping token,
    /// `(portuale #nn)` code comments).
    /// Returns the offending lines.
    fn bare_portuale_hits(script: &str) -> Vec<String> {
        let mut scrubbed = script.to_string();
        for token in [
            "portuale-python",
            "portuale-remote",
            ".portuale.",
            "portuale-helper-ping-1",
            "(portuale #",
        ] {
            scrubbed = scrubbed.replace(token, "########");
        }
        // `portuale-<hex>` install names.
        while let Some(start) = scrubbed.find("portuale-") {
            let rest = &scrubbed[start + "portuale-".len()..];
            let hex_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
            if hex_len == 0 {
                break;
            }
            scrubbed.replace_range(start..start + "portuale-".len() + hex_len, "########");
        }
        scrubbed
            .lines()
            .filter(|line| line.contains("portuale"))
            .map(str::to_string)
            .collect()
    }

    /// #326 S8.3: every generated script kind exports the resolved
    /// absolute `PORTUALE_BIN` (and `PORTAGE_PYM_PATH=/`, unsetting
    /// `PORTAGE_IPC_DAEMON`), resolves `PORTAGE_PYTHON` through the
    /// bundle's own `$UNITBIN/portuale-python`, and invokes no
    /// `portuale` command by bare name.
    #[test]
    fn generated_scripts_use_absolute_portuale_bin() {
        const ABS: &str = "/opt/bin/portuale-0123456789abcdef";
        let staged = render_test_staged();
        let phase = phase_script("/work/probe-1.0", &staged, "setup", "/", "/work", "", ABS);
        let regen =
            postinst_regen_script("/work/probe-1.0", &staged, "/", "/work", "", None, &[], ABS);
        let merge = merge_script(
            "/work/probe-1.0",
            &staged,
            "/",
            "/var/db/pkg",
            false,
            "/etc",
            "/etc/env.d",
            None,
            ABS,
        )
        .expect("merge script renders");
        let helpers_flow = format!("{MERGE_HELPERS}\n{MERGE_FLOW}");
        let unpack = unpack_script("/work", "probe-1.0", 1234);
        let preflight = preflight_script("/", "/work", None);
        // The three rendered kinds export the absolute path (the old
        // hooks inherit the merge header's export at runtime); the
        // read-only unpack/preflight drivers invoke no binary at all.
        for (name, script) in [
            ("phase", phase.as_str()),
            ("regen", regen.as_str()),
            ("merge", merge.as_str()),
        ] {
            assert!(
                script.contains(&format!("export PORTUALE_BIN='{ABS}'")),
                "{name} must export the absolute PORTUALE_BIN:\n{script}"
            );
        }
        for (name, script) in [
            ("phase", phase.as_str()),
            ("regen", regen.as_str()),
            ("merge", merge.as_str()),
            ("old-hook", helpers_flow.as_str()),
        ] {
            assert!(
                script.contains("export PORTAGE_PYM_PATH=/"),
                "{name} must pin PORTAGE_PYM_PATH=/:\n{script}"
            );
            assert!(
                script.contains("unset PORTAGE_IPC_DAEMON"),
                "{name} must drop PORTAGE_IPC_DAEMON (no IPC daemon):\n{script}"
            );
            assert!(
                !script.contains("PORTAGE_IPC_DAEMON="),
                "{name} must not export PORTAGE_IPC_DAEMON:\n{script}"
            );
            assert!(
                script.contains("PORTAGE_PYTHON=\"$UNITBIN/portuale-python\""),
                "{name} must resolve PORTAGE_PYTHON through $UNITBIN:\n{script}"
            );
            assert!(
                !script.contains("/usr/bin/python"),
                "{name} must not point at a system python:\n{script}"
            );
        }
        assert!(
            !merge.contains(":-portuale"),
            "portageq-wrapper's PATH fallback must be gone:\n{merge}"
        );
        for (name, script) in [
            ("phase", phase.as_str()),
            ("regen", regen.as_str()),
            ("merge", merge.as_str()),
            ("old-hook", helpers_flow.as_str()),
            ("unpack", unpack.as_str()),
            ("preflight", preflight.as_str()),
        ] {
            assert_eq!(
                bare_portuale_hits(script),
                Vec::<String>::new(),
                "{name} invokes portuale by bare name"
            );
        }
    }

    #[test]
    fn transport_errors_are_not_remote_failures() {
        assert!(is_transport_error(Some(255), ""));
        assert!(is_transport_error(
            None,
            "ssh: Could not resolve hostname badhost"
        ));
        assert!(is_transport_error(None, "Connection refused"));
        assert!(!is_transport_error(Some(1), "some remote error"));
        assert!(!is_transport_error(Some(0), ""));
    }

    #[test]
    fn control_dir_prefers_short_dirs_and_skips_unfitting_ones() {
        use std::path::PathBuf;
        // A deep base cannot fit `%C` + suffix under 108 bytes.
        let deep = PathBuf::from("/tmp/pytest-of-vivo/pytest-707/loopback-sshd0/home");
        assert!(control_dir_for(None, Some(deep)).is_none());
        // XDG_RUNTIME_DIR wins when it fits (and gets created 0700).
        // Deliberately NOT `TempDir`: this test is about the 108-byte
        // `sun_path` budget, and `TempDir`'s own `portuale-td-…-{pid}-{nanos}`
        // name already eats too much of it to fit `%C` + suffix.
        let tmp = PathBuf::from("/tmp/cpfit");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let picked = control_dir_for(Some(tmp.clone()), None);
        assert_eq!(picked, Some(tmp.join(".portuale-cp")));
        assert!(tmp.join(".portuale-cp").is_dir());
        let _ = std::fs::remove_dir_all(&tmp);
        // Nothing configured at all: no multiplexing.
        assert!(control_dir_for(None, None).is_none());
    }

    fn ctx_with_args(hostname: &str, ssh_args: Option<&str>) -> RemoteContext {
        RemoteContext {
            hostname: hostname.to_string(),
            user: None,
            port: 22222,
            key_file: None,
            timeout_secs: 10,
            ssh_args: ssh_args.map(String::from),
            strict_host_key_checking: StrictHostKeyChecking::AcceptNew,
            max_clock_skew_secs: 900,
            root: "/".to_string(),
            workdir: "/var/tmp/portage-remote".to_string(),
            transport: RemoteTransport::Ssh,
            binpkg: None,
            config_protect: "/etc".to_string(),
            config_protect_explicit: false,
            config_protect_mask: "/etc/env.d".to_string(),
            config_protect_mask_explicit: false,
            etc_portage: ConfigPlacement::Client("/etc/portage".to_string()),
            vdb: ConfigPlacement::Client("/var/db/pkg".to_string()),
            edb: ConfigPlacement::Server("/var/cache/edb".to_string()),
            ledger_dir: None,
            require_ledger_match: false,
            portuale_binary: None,
            portuale_dir: None,
            bin_plan: ClientBinPlan::Unresolved,
        }
    }

    #[test]
    fn etc_placement_parses_server_and_client_forms() {
        assert_eq!(
            ConfigPlacement::parse("--remote-etc-portage", "server:/etc/portage"),
            Ok(ConfigPlacement::Server("/etc/portage".to_string()))
        );
        assert_eq!(
            ConfigPlacement::parse("--remote-etc-portage", "client:/etc/portage"),
            Ok(ConfigPlacement::Client("/etc/portage".to_string()))
        );
        assert!(ConfigPlacement::parse("--remote-etc-portage", "/etc/portage").is_err());
        assert!(ConfigPlacement::parse("--remote-etc-portage", "server:").is_err());
        assert!(ConfigPlacement::parse("--remote-etc-portage", "bizarre").is_err());
    }

    #[test]
    fn remote_exec_handoff_sets_and_takes() {
        assert!(take_remote_exec().is_none());
        let ctx = ctx_with_args("h", None);
        set_remote_exec(ctx.clone());
        assert_eq!(take_remote_exec(), Some(ctx));
        // Taking clears the slot: no leakage across calls.
        assert!(take_remote_exec().is_none());
    }

    #[test]
    fn ledger_append_keeps_the_last_ten_lines() {
        let dir = TempDir::new("portuale-remote-ledger").keep();
        let path = dir.join("ledger");
        for n in 0..14 {
            ledger_append(&path, &format!("line{n}")).unwrap();
        }
        let kept: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(kept.len(), 10);
        assert_eq!(kept[0], "line4");
        assert_eq!(kept[9], "line13");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newest_ledger_line_reads_the_last_non_empty_line() {
        assert_eq!(newest_ledger_line(""), None);
        assert_eq!(newest_ledger_line("\n\n"), None);
        assert_eq!(
            newest_ledger_line("10 old 0 pkg\n20 new 1 pkg\n"),
            Some("20 new 1 pkg".to_string())
        );
        assert_eq!(newest_ledger_line("a\nb\n\n"), Some("b".to_string()));
    }

    #[test]
    fn ledger_lines_match_requires_byte_identical_newest_lines() {
        let line = "1700000000 testrepo abc123 dev-libs/x-1.0";
        assert!(ledger_lines_match(None, None));
        assert!(ledger_lines_match(Some(line), Some(line)));
        assert!(!ledger_lines_match(
            Some(line),
            Some("1700000001 testrepo def dev-libs/y-2.0")
        ));
        assert!(!ledger_lines_match(None, Some(line)));
        assert!(!ledger_lines_match(Some(line), None));
    }

    #[test]
    fn shadow_parsers_read_contents_and_filemeta_shapes() {
        // Only obj/sym lines own paths; dir lines merge freely.
        assert_eq!(
            contents_owned_paths("obj /a/b 0123 456\ndir /a\nsym /c -> /d 789\njunk\n"),
            vec!["/a/b".to_string(), "/c".to_string()]
        );
        // filemeta keeps kind + rel; garbage lines drop out.
        assert_eq!(
            parse_filemeta_paths(
                "obj abc 123 usr/bin/x\nsym def 456 etc/link tgt\ndir ghi 789 usr/share\nbogus\n"
            ),
            vec![
                ("obj".to_string(), "usr/bin/x".to_string()),
                ("sym".to_string(), "etc/link".to_string()),
                ("dir".to_string(), "usr/share".to_string()),
            ]
        );
        // `_pkgsplit` longest-version rule for the same-package check.
        assert!(VdbShadow::same_package("foo-1bar-2.0", "foo-1bar"));
        assert!(!VdbShadow::same_package("foo-2.0", "foo-1bar"));
        assert!(!VdbShadow::same_package("noversion", "noversion"));
    }

    #[test]
    fn shadow_precheck_fails_only_foreign_owners() {
        let dir = TempDir::new("portuale-remote-shadow").keep();
        let vdb = dir.join("vdb");
        std::fs::create_dir_all(vdb.join("dev-libs/oldpkg-1.0")).unwrap();
        std::fs::write(
            vdb.join("dev-libs/oldpkg-1.0/CONTENTS"),
            "obj /usr/bin/foreign abc 1\nsym /usr/bin/shared -> /x 2\n",
        )
        .unwrap();
        std::fs::create_dir_all(vdb.join("dev-libs/newpkg-2.0")).unwrap();
        std::fs::write(
            vdb.join("dev-libs/newpkg-2.0/CONTENTS"),
            "obj /usr/bin/own def 3\n",
        )
        .unwrap();
        let shadow = VdbShadow::load(&vdb);
        // Same package, any version: passes for the driver to refine.
        assert!(
            shadow_precheck(
                &shadow,
                "dev-libs/newpkg-1.0",
                "dev-libs",
                "newpkg",
                "obj m 1 usr/bin/own\n",
            )
            .is_ok()
        );
        // Foreign owner: fails before shipping.
        let err = shadow_precheck(
            &shadow,
            "dev-libs/newpkg-1.0",
            "dev-libs",
            "newpkg",
            "obj m 1 usr/bin/foreign\n",
        )
        .unwrap_err();
        assert!(err.contains("not shipping"), "{err}");
        assert!(err.contains("oldpkg-1.0"), "{err}");
        // Unowned path: passes.
        assert!(
            shadow_precheck(
                &shadow,
                "dev-libs/newpkg-1.0",
                "dev-libs",
                "newpkg",
                "obj m 1 usr/bin/fresh\ndir m 1 usr/share\n",
            )
            .is_ok()
        );
        // Missing vdb dir: empty shadow, everything passes.
        let empty = VdbShadow::load(&dir.join("absent"));
        assert!(
            shadow_precheck(
                &empty,
                "dev-libs/newpkg-1.0",
                "dev-libs",
                "newpkg",
                "obj m 1 usr/bin/foreign\n",
            )
            .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Backlog #183: the vdb shadow skips in-progress `-MERGING-<pf>`
    /// entries, so a stale half-written `CONTENTS` never blocks a bundle
    /// -- real `vardbapi._excluded_dirs` (`vartree.py`).
    #[test]
    fn shadow_ignores_a_stale_merging_entry() {
        let dir = TempDir::new("portuale-remote-shadow-merging").keep();
        let vdb = dir.join("vdb");
        std::fs::create_dir_all(vdb.join("dev-libs/-MERGING-stalepkg-1.0")).unwrap();
        std::fs::write(
            vdb.join("dev-libs/-MERGING-stalepkg-1.0/CONTENTS"),
            "obj /usr/bin/stale abc 1\n",
        )
        .unwrap();
        let shadow = VdbShadow::load(&vdb);
        assert!(shadow.owner("/usr/bin/stale").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drop_dependents_covers_diamonds_transitively() {
        use std::collections::{HashMap, HashSet};
        let cp = |p: &str| ("dev-libs".to_string(), p.to_string());
        // a <- b <- d, a <- c <- d (diamond): failing a drops b, c, d.
        let dependents: HashMap<(String, String), Vec<(String, String)>> = HashMap::from([
            (cp("a"), vec![cp("b"), cp("c")]),
            (cp("b"), vec![cp("d")]),
            (cp("c"), vec![cp("d")]),
            (cp("d"), vec![]),
        ]);
        let mut skip = HashMap::new();
        drop_dependents(&dependents, &mut skip, cp("a"), "dev-libs/a-1.0");
        assert_eq!(
            skip.keys().collect::<HashSet<_>>(),
            [cp("b"), cp("c"), cp("d")].iter().collect::<HashSet<_>>()
        );
        assert_eq!(skip[&cp("d")], "dev-libs/a-1.0");
        // Unrelated roots never join.
        let mut skip = HashMap::new();
        drop_dependents(&dependents, &mut skip, cp("b"), "dev-libs/b-1.0");
        assert!(skip.contains_key(&cp("d")));
        assert!(!skip.contains_key(&cp("c")));
    }

    fn binary_entry(package: &str, binary: bool) -> portage_repo::GraphEntry {
        use portage_repo::{CandidateSource, PretendOutcome, VisibilityProvenance};
        portage_repo::GraphEntry {
            discovery: 0,
            category: "dev-libs".to_string(),
            package: package.to_string(),
            outcome: PretendOutcome::New {
                version: "1.0".to_string(),
            },
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: Some("testrepo".to_string()),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by: Vec::new(),
            source: if binary {
                CandidateSource::Binary
            } else {
                CandidateSource::Ebuild
            },
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    #[test]
    fn binary_plan_check_accepts_binaries_and_names_sources() {
        use portage_repo::PretendOutcome;
        assert!(check_binary_plan(&[binary_entry("a", true)]).is_ok());
        assert!(check_binary_plan(&[]).is_ok());
        let mut installed = binary_entry("b", true);
        installed.outcome = PretendOutcome::AlreadyInstalled {
            version: "1.0".to_string(),
        };
        installed.source = portage_repo::CandidateSource::Ebuild;
        assert!(check_binary_plan(&[installed]).is_ok());
        let err = check_binary_plan(&[binary_entry("c", false)]).unwrap_err();
        assert!(err.contains("dev-libs/c-1.0"), "{err}");
    }

    #[test]
    fn lookup_ids_follow_host_key_alias_and_ports() {
        // No alias: hostname plus the bracketed host:port spelling (plain
        // `-F host` does not match `[host]:port` entries).
        assert_eq!(
            lookup_ids(&ctx_with_args("client.invalid", None)),
            vec![
                "client.invalid".to_string(),
                "[client.invalid]:22222".to_string()
            ]
        );
        // `-o HostKeyAlias=X` (joined or split) redirects the id.
        assert_eq!(
            lookup_ids(&ctx_with_args("h", Some("-o HostKeyAlias=alias.invalid"))),
            vec!["alias.invalid".to_string()]
        );
        assert_eq!(
            lookup_ids(&ctx_with_args("h", Some("-o HostKeyAlias=alias.invalid"))),
            lookup_ids(&ctx_with_args("h", Some("HostKeyAlias=alias.invalid"))),
        );
        // Unrelated extra args leave the hostname ids alone.
        assert_eq!(
            lookup_ids(&ctx_with_args("h", Some("-o Foo=yes")))[0],
            "h".to_string()
        );
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    /// The unpack driver rejects a truncated stream before touching
    /// anything: byte-count gate first, tar never runs on short input.
    #[test]
    fn unpack_script_rejects_a_truncated_bundle() {
        let tmp = TempDir::new("portuale-remote-trunc").keep();
        let staging = tmp.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let staged = crate::remote_bundle::build_bundle(
            &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &staging,
            None,
            &crate::binpkg::GpgVerify::default(),
            "",
            false,
        )
        .expect("fixture tbz2 stages");
        // Simulate the streamed file, truncated to half its bytes.
        let unit = tmp.join("work/packagepkg-1.0");
        std::fs::create_dir_all(&unit).unwrap();
        let bytes = std::fs::read(&staged.tarball).unwrap();
        std::fs::write(unit.join("bundle.tar"), &bytes[..bytes.len() / 2]).unwrap();

        let script = unpack_script(
            tmp.join("work").to_str().unwrap(),
            "packagepkg-1.0",
            staged.byte_count,
        );
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs the driver");
        assert!(!output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            stdout.contains("UNPACK=byte-count-mismatch"),
            "unexpected driver output:\n{stdout}"
        );
        // Nothing unpacked: the gate runs before tar.
        assert!(!unit.join("image").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #170: end to end through the real `MERGE_FLOW` -- a bundle
    /// built with a non-empty resolved mask merges without the masked
    /// files (not on the root, not in the vdb `CONTENTS`), while the
    /// merge-time aux files land in `var/db/pkg/<cat>/<pf>/`: real
    /// `_emerge/Binpkg.py:374` (`BINPKGMD5`) and `vartree.py:4581`
    /// `preinst_mask` (`INSTALL_MASK`). The fixture image ships
    /// `usr/share/packagepkg/hello.txt`, so the anchored
    /// `/usr/share/packagepkg` mask stands in for the bed's
    /// `/usr/share/porttest/im/...` mask.
    #[test]
    fn remote_merge_masks_image_and_lands_aux_files_in_vdb() {
        use md5::Digest as _;

        let tmp = TempDir::new("portuale-remote-mask").keep();
        let binpkg = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
        let staging = tmp.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let staged = crate::remote_bundle::build_bundle(
            &binpkg,
            &staging,
            None,
            &crate::binpkg::GpgVerify::default(),
            "/usr/share/packagepkg",
            false,
        )
        .expect("fixture tbz2 stages");
        // Deliver the bundle the way the stream would, then run the real
        // unpack driver.
        let work = tmp.join("work");
        let unit = work.join("packagepkg-1.0");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::copy(&staged.tarball, unit.join("bundle.tar")).unwrap();
        let script = unpack_script(work.to_str().unwrap(), "packagepkg-1.0", staged.byte_count);
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs the driver");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success() && stdout.contains("UNPACK=ok"),
            "unpack driver failed:\n{stdout}"
        );
        // Run the real merge driver against a scratch root.
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
        let markers = run_merge_stage(&ctx, None, unit.to_str().unwrap(), &staged, None)
            .expect("merge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
        assert!(markers.iter().any(|m| m == "MERGE_VDB=ok"), "{markers:?}");
        // The masked file merged nowhere: not on the root, not in CONTENTS.
        assert!(
            !root.join("usr/share/packagepkg/hello.txt").exists(),
            "masked file must not reach the merged root"
        );
        let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
        let contents = std::fs::read_to_string(vdb.join("CONTENTS")).unwrap();
        assert!(
            !contents.contains("hello.txt"),
            "masked file must not reach CONTENTS:\n{contents}"
        );
        // The merge-time aux files land in the vdb entry verbatim.
        let bytes = std::fs::read(&binpkg).unwrap();
        assert_eq!(
            std::fs::read_to_string(vdb.join("BINPKGMD5")).unwrap(),
            format!("{:x}\n", md5::Md5::digest(&bytes)),
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("INSTALL_MASK")).unwrap(),
            "/usr/share/packagepkg\n",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #169, site (b): the receive driver must preserve
    /// setuid/setgid/sticky bits through the bundle tarball: an
    /// unprivileged `tar -xf` applies the umask and strips all three
    /// bits at extraction, `tar -xpf` (`--preserve-permissions`) keeps
    /// them, so the merge stage never sees them otherwise (the bed's
    /// non-root client capture, `4711/2755/1750 -> 711/755/750`). End to end through both shell
    /// fragments: stage a unit with modes 4711/2755/1750 plus a 2750
    /// directory, tar it the way `build_bundle` does, run the real
    /// `unpack_script`, then the real merge driver, and assert the
    /// modes on the merged root. Fails while unpack uses `tar -xf`;
    /// as non-root the merge's chown step is skipped, so this
    /// isolates the extraction site.
    #[test]
    fn unpack_then_merge_preserves_special_mode_bits() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let tmp = TempDir::new("portuale-remote-setuid").keep();
        let pf = "probe-1.0";
        // Stage the unit the way `build_bundle` lays it out (`<pf>/`
        // with image/, build-info/, remote-manifest, filemeta).
        let staging = tmp.join("staging").join(pf);
        let image = staging.join("image");
        for (rel, mode) in [
            ("usr/bin/pt-setuid", 0o4711u32),
            ("usr/bin/pt-setgid", 0o2755u32),
            ("usr/bin/pt-sticky", 0o1750u32),
        ] {
            let path = image.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"probe payload\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let sgiddir = image.join("usr/lib/pt-sgiddir");
        std::fs::create_dir_all(&sgiddir).unwrap();
        std::fs::set_permissions(&sgiddir, std::fs::Permissions::from_mode(0o2750)).unwrap();
        let build_info = staging.join("build-info");
        std::fs::create_dir_all(&build_info).unwrap();
        for (name, content) in [
            ("PF", "probe-1.0"),
            ("CATEGORY", "dev-libs"),
            ("SLOT", "0"),
            ("DEFINED_PHASES", "-"),
        ] {
            std::fs::write(build_info.join(name), content).unwrap();
        }
        // The unpack driver only gates `FORMAT=` in the manifest.
        std::fs::write(staging.join("remote-manifest"), "FORMAT=1\n").unwrap();
        let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
        let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
        std::fs::write(staging.join("filemeta"), &filemeta).unwrap();
        // Tar `<pf>/` exactly like `build_bundle` (`tar -cf`, `-C` the
        // staging dir), then deliver it as the streamed bundle.
        let tarball = tmp.join("bundle.tar");
        let status = std::process::Command::new("tar")
            .args(["-cf"])
            .arg(&tarball)
            .args(["-C"])
            .arg(tmp.join("staging"))
            .arg(pf)
            .status()
            .expect("tar -cf stages the bundle");
        assert!(status.success());
        let byte_count = std::fs::metadata(&tarball).unwrap().len();
        let work = tmp.join("work");
        let unit = work.join(pf);
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::copy(&tarball, unit.join("bundle.tar")).unwrap();

        let script = unpack_script(work.to_str().unwrap(), pf, byte_count);
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs the driver");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success() && stdout.contains("UNPACK=ok"),
            "unpack driver failed:\n{stdout}"
        );

        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let staged = crate::remote_bundle::StagedBundle {
            tarball: tmp.join("bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/probe-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: false,
            },
            eapi: "8".to_string(),
            category: "dev-libs".to_string(),
            pn: "probe".to_string(),
            pv: "1.0".to_string(),
            pr: "r0".to_string(),
            pvr: "1.0".to_string(),
            p: "probe-1.0".to_string(),
            pf: "probe-1.0".to_string(),
            phases: Vec::new(),
            postinst_defined: false,
        };
        let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
        let markers = run_merge_stage(&ctx, None, unit.to_str().unwrap(), &staged, None)
            .expect("merge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");

        for (rel, mode) in [
            ("usr/bin/pt-setuid", 0o4711u32),
            ("usr/bin/pt-setgid", 0o2755u32),
            ("usr/bin/pt-sticky", 0o1750u32),
            ("usr/lib/pt-sgiddir", 0o2750u32),
        ] {
            let got = std::fs::metadata(root.join(rel)).unwrap().mode() & 0o7777;
            assert_eq!(
                got, mode,
                "{rel}: special mode bits must survive unpack + merge"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #169, site (a): `chown(2)` on a regular file clears
    /// `S_ISUID`/`S_ISGID` even when owner and group are unchanged, so
    /// the merge copy driver must apply ownership *before* restoring
    /// the mode -- real `lib/portage/util/movefile.py::_apply_stat`
    /// does `os.chown` then `os.chmod` for exactly this reason. That
    /// needs root to observe behaviourally (no sudo on this host), so
    /// this pins the order structurally in `MERGE_FLOW` instead: the
    /// `chown --reference` line must precede the `chmod --reference`
    /// line in both the `obj` (`$tmp`) and `dir` (`$dest`) branches.
    /// Fails while chmod runs first.
    #[test]
    fn merge_flow_applies_chown_before_chmod() {
        for (what, chown, chmod) in [
            (
                "obj",
                "chown --reference \"$src\" \"$tmp\"",
                "chmod --reference \"$src\" \"$tmp\"",
            ),
            (
                "dir",
                "chown --reference \"$src\" \"$dest\"",
                "chmod --reference \"$src\" \"$dest\"",
            ),
        ] {
            for pat in [chown, chmod] {
                assert_eq!(
                    MERGE_FLOW.matches(pat).count(),
                    1,
                    "{what} branch: `{pat}` must occur exactly once for the order check"
                );
            }
            let chown_pos = MERGE_FLOW
                .find(chown)
                .unwrap_or_else(|| panic!("{what} branch lost its chown --reference line"));
            let chmod_pos = MERGE_FLOW
                .find(chmod)
                .unwrap_or_else(|| panic!("{what} branch lost its chmod --reference line"));
            assert!(
                chown_pos < chmod_pos,
                "{what} branch must chown before chmod (chown(2) clears setuid/setgid)"
            );
        }
    }

    /// Synthetic merge through the real driver: protect rename and
    /// fail-closed collision, no binpkg needed (unit staged by hand,
    /// `filemeta` via the real collector).
    fn synthetic_unit(
        tmp: &std::path::Path,
        files: &[(&str, &str)],
    ) -> (String, crate::remote_bundle::StagedBundle) {
        let bytes: Vec<(&str, &[u8])> = files.iter().map(|(r, c)| (*r, c.as_bytes())).collect();
        synthetic_unit_bytes(tmp, &bytes)
    }

    /// [`synthetic_unit`] with binary payloads (a real executable in the
    /// image, for the atomic-replacement pins).
    fn synthetic_unit_bytes(
        tmp: &std::path::Path,
        files: &[(&str, &[u8])],
    ) -> (String, crate::remote_bundle::StagedBundle) {
        let unit = tmp.join("work/probe-1.0");
        let image = unit.join("image");
        let build_info = unit.join("build-info");
        std::fs::create_dir_all(&build_info).unwrap();
        for (rel, content) in files {
            let path = image.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
        }
        for (name, content) in [
            ("PF", "probe-1.0"),
            ("CATEGORY", "dev-libs"),
            ("SLOT", "0"),
            ("DEFINED_PHASES", "-"),
        ] {
            std::fs::write(build_info.join(name), content).unwrap();
        }
        let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
        let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
        std::fs::write(unit.join("filemeta"), &filemeta).unwrap();
        let staged = crate::remote_bundle::StagedBundle {
            tarball: tmp.join("bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/probe-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: false,
            },
            eapi: "8".to_string(),
            category: "dev-libs".to_string(),
            pn: "probe".to_string(),
            pv: "1.0".to_string(),
            pr: "r0".to_string(),
            pvr: "1.0".to_string(),
            p: "probe-1.0".to_string(),
            pf: "probe-1.0".to_string(),
            phases: Vec::new(),
            postinst_defined: false,
        };
        (unit.to_str().unwrap().to_string(), staged)
    }

    /// Serialises the tests that pin `PORTAGE_CONFIGROOT`: it is
    /// process-global, and `place_config_root` reads it (the server
    /// `make.globals` lookup) while `PlacedConfig` overwrites it, so
    /// two such tests racing would seed from each other's server root.
    static PLACED_CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn local_ctx(root: &str, workdir: &str) -> RemoteContext {
        // Unit tests run stages directly (no preflight): the scripts
        // still need a resolved absolute path, so the fixture context
        // carries the built `portuale` binary next to `deps/` -- a real
        // binary, never the libtest harness (#326 D8: the phase shims
        // exec `$PORTUALE_BIN`, and a harness would answer its test
        // banner). `cargo build -p portuale` before `cargo test`.
        let abs = {
            let mut exe = std::env::current_exe().expect("current test exe");
            exe.pop();
            if exe.ends_with("deps") {
                exe.pop();
            }
            exe.push("portuale");
            exe.display().to_string()
        };
        RemoteContext {
            hostname: "localtest".to_string(),
            user: None,
            port: 22,
            key_file: None,
            timeout_secs: 10,
            ssh_args: None,
            strict_host_key_checking: StrictHostKeyChecking::AcceptNew,
            max_clock_skew_secs: 0,
            root: root.to_string(),
            workdir: workdir.to_string(),
            transport: RemoteTransport::Local,
            binpkg: None,
            config_protect: "/etc".to_string(),
            config_protect_explicit: false,
            config_protect_mask: "/etc/env.d".to_string(),
            config_protect_mask_explicit: false,
            etc_portage: ConfigPlacement::Client("/etc/portage".to_string()),
            vdb: ConfigPlacement::Client(format!("{root}/var/db/pkg")),
            edb: ConfigPlacement::Server(format!("{root}/var/cache/edb")),
            ledger_dir: None,
            require_ledger_match: false,
            portuale_binary: None,
            portuale_dir: None,
            // Unit tests run stages directly (no preflight): the scripts
            // still need a resolved absolute path, so the fixture context
            // carries a fake-but-absolute one.
            bin_plan: ClientBinPlan::Local(abs),
        }
    }

    #[test]
    fn merge_driver_protects_a_modified_config() {
        let tmp = TempDir::new("portuale-remote-protect").keep();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        // Live config differs from the incoming image version.
        std::fs::write(
            root.join("etc/probe.conf"),
            "live
",
        )
        .unwrap();
        let (unit, staged) = synthetic_unit(
            &tmp,
            &[(
                "etc/probe.conf",
                "incoming
",
            )],
        );
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
        // Original untouched, update diverted to a `._cfg` sibling...
        assert_eq!(
            std::fs::read_to_string(root.join("etc/probe.conf")).unwrap(),
            "live
"
        );
        let cfg = root.join("etc/._cfg0000_probe.conf");
        assert_eq!(
            std::fs::read_to_string(&cfg).unwrap(),
            "incoming
"
        );
        // ...while CONTENTS records the logical path (like the local merge).
        let contents =
            std::fs::read_to_string(root.join("var/db/pkg/dev-libs/probe-1.0/CONTENTS")).unwrap();
        assert!(
            contents.contains("obj /etc/probe.conf "),
            "unexpected CONTENTS:\n{contents}"
        );
        // Identical content merges in place (no `._cfg` spam): the
        // live file now matches the image, so reinstalling overwrites.
        std::fs::write(root.join("etc/probe.conf"), "incoming\n").unwrap();
        let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("remerge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
        assert!(!root.join("etc/._cfg0001_probe.conf").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("etc/probe.conf")).unwrap(),
            "incoming\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #97's core invariant for the remote driver: replacing a
    /// file must never write the existing inode. An open fd must keep
    /// reading the old bytes after the merge while the path carries the
    /// new content on a fresh inode. The old `cp -p "$src" "$dest"`
    /// truncates that inode -- fatal for an mmap'd `libc.so.6` /
    /// `libreadline.so.8` in a running client (backlog #96).
    #[test]
    fn merge_driver_never_writes_the_existing_inode() {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt as _;

        let tmp = TempDir::new("portuale-remote-atomic-fd").keep();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        std::fs::write(root.join("lib/libx.so.1"), b"old library bytes").unwrap();
        let old_inode = std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino();
        let mut old_fd = std::fs::File::open(root.join("lib/libx.so.1")).unwrap();

        let (unit, staged) =
            synthetic_unit_bytes(&tmp, &[("lib/libx.so.1", b"new library bytes, longer")]);
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");

        let mut still_open = Vec::new();
        old_fd.read_to_end(&mut still_open).unwrap();
        assert_eq!(
            still_open, b"old library bytes",
            "the existing inode must never be written to"
        );
        assert_eq!(
            std::fs::read(root.join("lib/libx.so.1")).unwrap(),
            b"new library bytes, longer"
        );
        assert_ne!(
            std::fs::metadata(root.join("lib/libx.so.1")).unwrap().ino(),
            old_inode,
            "the destination must be a fresh inode"
        );
        let leftovers: Vec<String> = std::fs::read_dir(root.join("lib"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_portage_merge_"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no merge temporary may remain: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The live shape of backlog #97: a client re-merging a binary that
    /// is currently executing (its own `/bin/bash`, or glibc's ld.so).
    /// The old `cp -p` hit `ETXTBSY` and failed the copy step; the
    /// temp+rename replacement succeeds and the running process keeps
    /// its old inode. `/bin/sleep` stands in for the shell: the same
    /// execve-protected kernel path.
    #[test]
    fn merge_driver_replaces_a_running_executable_atomically() {
        use std::os::unix::fs::MetadataExt as _;

        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(std::path::Path::new)
            .find(|p| p.is_file())
            .expect("a system sleep binary");
        let true_bin = ["/bin/true", "/usr/bin/true"]
            .iter()
            .map(std::path::Path::new)
            .find(|p| p.is_file())
            .expect("a system true binary");

        let tmp = TempDir::new("portuale-remote-atomic-exec").keep();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        std::fs::copy(sleep, root.join("bin/prog")).unwrap();
        let old_inode = std::fs::metadata(root.join("bin/prog")).unwrap().ino();
        let new_bytes = std::fs::read(true_bin).unwrap();
        let (unit, staged) = synthetic_unit_bytes(&tmp, &[("bin/prog", new_bytes.as_slice())]);

        let spawn = || {
            std::process::Command::new(root.join("bin/prog"))
                .arg("60")
                .spawn()
        };
        let mut child = match spawn() {
            Ok(child) => child,
            Err(first) => {
                // A loaded test machine can transiently fail a fork/exec
                // with EAGAIN; retry once before giving up.
                std::thread::sleep(std::time::Duration::from_millis(500));
                spawn().unwrap_or_else(|e| panic!("spawn the old binary ({first}, {e})"))
            }
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "the old binary must still be executing before the merge"
        );

        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let outcome = run_merge_stage(&ctx, None, &unit, &staged, None);

        let alive = child.try_wait().expect("try_wait").is_none();
        let _ = child.kill();
        let _ = child.wait();
        let markers = outcome.expect(
            "replacing a running executable must not write its inode (ETXTBSY would fail the copy)",
        );
        assert!(markers.iter().any(|m| m == "MERGE_COPY=ok"), "{markers:?}");
        assert!(alive, "the running process must survive the merge");
        assert_eq!(
            std::fs::read(root.join("bin/prog")).unwrap(),
            new_bytes,
            "the path must carry the new binary"
        );
        assert_ne!(
            std::fs::metadata(root.join("bin/prog")).unwrap().ino(),
            old_inode,
            "the destination must be a fresh inode"
        );
        let leftovers: Vec<String> = std::fs::read_dir(root.join("bin"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("_portage_merge_"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no merge temporary may remain: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_driver_aborts_on_unowned_collision() {
        let tmp = TempDir::new("portuale-remote-collide").keep();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        // A file another installed package owns: fail-closed (no
        // protect-owned nuance in v1). An unowned orphan would simply
        // be overwritten -- only foreign ownership aborts.
        std::fs::write(
            root.join("usr/share/foreign.txt"),
            "mine
",
        )
        .unwrap();
        let (unit, staged) = synthetic_unit(
            &tmp,
            &[(
                "usr/share/foreign.txt",
                "theirs
",
            )],
        );
        let owner = root.join("var/db/pkg/dev-libs/owner-1.0");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(owner.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            owner.join("CONTENTS"),
            "obj /usr/share/foreign.txt deadbeef 100\n",
        )
        .unwrap();
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let err = run_merge_stage(&ctx, None, &unit, &staged, None).unwrap_err();
        assert!(err.contains("collision"), "{err}");
        // Nothing was written: no vdb for the new package (the other
        // owner's entry stays), foreign file untouched.
        assert!(!root.join("var/db/pkg/dev-libs/probe-1.0").exists());
        assert!(root.join("var/db/pkg/dev-libs/owner-1.0").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("usr/share/foreign.txt")).unwrap(),
            "mine
"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #172: `phase_exports` carries `MERGE_TYPE=binary` exactly
    /// when real's `eapi_exports_merge_type` holds.
    #[test]
    fn phase_exports_gates_merge_type_on_eapi() {
        // Real `_emerge/Binpkg.py:92` sets `MERGE_TYPE=binary` for a binpkg
        // merge, and real `config.environ()` (`config.py:3335-3336`) exports
        // it only when `eapi_exports_merge_type` holds (EAPI >= 4,
        // `eapi.py:300`). Bed `l31-20260926T061556Z` shows real appending
        // `merge_type=binary` x4 (322 B) where mrg wrote `?` x4 (302 B).
        // `mrg` only merges binpkgs, so `binary` is the only value.
        fn exports_for(eapi: &str) -> String {
            let staged = crate::remote_bundle::StagedBundle {
                tarball: std::path::PathBuf::from("/tmp/bundle.tar"),
                byte_count: 0,
                manifest: crate::remote_bundle::BundleManifest {
                    format: 1,
                    cpv: "dev-libs/probe-1.0".to_string(),
                    slot: "0".to_string(),
                    repo: "test".to_string(),
                    has_environment: false,
                },
                eapi: eapi.to_string(),
                category: "dev-libs".to_string(),
                pn: "probe".to_string(),
                pv: "1.0".to_string(),
                pr: "r0".to_string(),
                pvr: "1.0".to_string(),
                p: "probe-1.0".to_string(),
                pf: "probe-1.0".to_string(),
                phases: Vec::new(),
                postinst_defined: false,
            };
            phase_exports(
                "/tmp/work/build-info/probe-1.0.ebuild",
                "/tmp/work",
                "/",
                "/tmp/work/image",
                "/tmp/work/temp",
                "/tmp/work/work",
                "/tmp/work/homedir",
                "/tmp/work/files",
                "/tmp/work/bin",
                "/tmp",
                "",
                &staged,
                "setup",
                "/opt/bin/portuale-0123456789abcdef",
            )
        }
        assert!(eapi_exports_merge_type("4"));
        assert!(eapi_exports_merge_type("8"));
        assert!(!eapi_exports_merge_type("0"));
        assert!(!eapi_exports_merge_type("3"));
        assert!(!eapi_exports_merge_type("2"));
        assert!(eapi_exports_merge_type("5"));
        assert!(eapi_exports_merge_type("9"));
        assert!(eapi_exports_merge_type("3-foo"));
        assert!(eapi_exports_merge_type("bogus"));
        let exporting = exports_for("8");
        assert!(
            exporting.contains("export EBUILD_PHASE=setup EMERGE_FROM=binary MERGE_TYPE=binary\n"),
            "EAPI 8 must export MERGE_TYPE=binary:\n{exporting}"
        );
        let gated = exports_for("3");
        assert!(
            !gated.contains("MERGE_TYPE"),
            "EAPI 3 must not export MERGE_TYPE:\n{gated}"
        );
        assert!(
            gated.contains("export EBUILD_PHASE=setup EMERGE_FROM=binary\n"),
            "EAPI 3 keeps the bare EMERGE_FROM line:\n{gated}"
        );
    }

    /// Positive pretend dispatch through the real stack: a synthetic unit
    /// (hand-written ebuild + environment carrying `pkg_pretend`, the real
    /// shipped `bin/`) runs the generated phase script under local bash.
    /// Proves template + `ebuild.sh` + DEFINED_PHASES-agnostic dispatch;
    /// the ebuild/env being synthetic is the only unreal part.
    #[test]
    fn phase_script_runs_pkg_pretend_from_saved_env() {
        let tmp = TempDir::new("portuale-remote-pretend").keep();
        let unit = tmp.join("work/probe-1.0");
        let build_info = unit.join("build-info");
        std::fs::create_dir_all(&build_info).unwrap();
        std::fs::write(
            build_info.join("probe-1.0.ebuild"),
            "EAPI=8\nDESCRIPTION=\"synthetic pretend probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        )
        .unwrap();
        std::fs::write(
            unit.join("environment"),
            "EAPI=8\npkg_pretend() {\n\techo pretend-ok >> \"${EROOT}/var/lib/probe.log\"\n}\n",
        )
        .unwrap();
        // The real runtime, as shipped in bundles.
        let status = std::process::Command::new("cp")
            .args(["-a"])
            .arg(crate::ebuild_phases::bin_dir().unwrap())
            .arg(unit.join("bin"))
            .status()
            .expect("cp -a bin");
        assert!(status.success());
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/lib")).unwrap();

        let staged = crate::remote_bundle::StagedBundle {
            tarball: tmp.join("bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/probe-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: true,
            },
            eapi: "8".to_string(),
            category: "dev-libs".to_string(),
            pn: "probe".to_string(),
            pv: "1.0".to_string(),
            pr: "r0".to_string(),
            pvr: "1.0".to_string(),
            p: "probe-1.0".to_string(),
            pf: "probe-1.0".to_string(),
            phases: vec!["pretend".to_string()],
            postinst_defined: false,
        };
        let script = phase_script(
            unit.to_str().unwrap(),
            &staged,
            "pretend",
            root.to_str().unwrap(),
            tmp.to_str().unwrap(),
            "",
            // The phase may invoke the shipped `portuale-python` shim,
            // which execs `$PORTUALE_BIN`: a real binary, never the
            // libtest harness (#326 D8).
            s8_portuale_bin().to_str().unwrap(),
        );
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs the phase script");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            stdout.contains("PHASE_pretend=0"),
            "phase marker missing:\n{stdout}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/probe.log")).unwrap(),
            "pretend-ok\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Build-time hook env, mimicking `__save_ebuild_env` output with a
    /// stray build-host local (`declare -- x=""`, as the bed's
    /// `setuid-1.0-1.gpkg.tar` build-time env carries) and build-time
    /// `FEATURES` (with `buildpkg`). Real binpkg envs also carry the
    /// hook functions themselves (the phase runs from the saved env,
    /// never re-sourcing the ebuild) -- `with_postinst` adds the test's
    /// `pkg_postinst`, without which the regen run is a no-op like
    /// real's undefined-function case.
    fn build_time_environment(with_postinst: bool) -> String {
        let mut lines = vec![
            "declare -x EAPI=\"8\"",
            "declare -x CATEGORY=\"dev-libs\"",
            "declare -x PN=\"regen\"",
            "declare -x PV=\"1.0\"",
            "declare -x PR=\"r0\"",
            "declare -x PVR=\"1.0\"",
            "declare -x P=\"regen-1.0\"",
            "declare -x PF=\"regen-1.0\"",
            "declare -- x=\"\"",
            "declare -x FEATURES=\"buildpkg sandbox\"",
            "declare -x PORTAGE_FEATURES=\"buildpkg sandbox\"",
            "declare -x USE=\"amd64\"",
        ];
        if with_postinst {
            lines.push("pkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}");
        }
        lines.join("\n") + "\n"
    }

    /// A full client unit for the regen tests: image + build-info (with
    /// the given ebuild text and `DEFINED_PHASES`) + build-time
    /// `$UNIT/environment` + the real runtime `bin/` + `filemeta`, so
    /// both `run_merge_stage` and the regen phase run for real under
    /// local bash.
    fn regen_unit(
        tmp: &std::path::Path,
        ebuild: &str,
        defined_phases: &str,
    ) -> (String, crate::remote_bundle::StagedBundle) {
        let unit = tmp.join("work/regen-1.0");
        let image = unit.join("image");
        let build_info = unit.join("build-info");
        std::fs::create_dir_all(&build_info).unwrap();
        let path = image.join("usr/share/regen/payload.txt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "payload\n").unwrap();
        for (name, content) in [
            ("PF", "regen-1.0"),
            ("CATEGORY", "dev-libs"),
            ("SLOT", "0"),
            ("DEFINED_PHASES", defined_phases),
        ] {
            std::fs::write(build_info.join(name), content).unwrap();
        }
        std::fs::write(build_info.join("regen-1.0.ebuild"), ebuild).unwrap();
        // The saved env carries the hooks the phase runs (real
        // `__save_ebuild_env` keeps `pkg_postinst`; an ebuild without
        // one yields an env without one).
        let with_postinst = ebuild.contains("pkg_postinst()");
        std::fs::write(
            unit.join("environment"),
            build_time_environment(with_postinst),
        )
        .unwrap();
        // Like a real binpkg, `build-info/` carries the build-time
        // `environment.bz2` (the MERGE_FLOW vdb fallback the regen
        // later overwrites).
        {
            let compressed = bzip2_compress(build_time_environment(with_postinst).as_bytes())
                .expect("server bzip2 compresses");
            std::fs::write(build_info.join("environment.bz2"), &compressed).unwrap();
        }
        let status = std::process::Command::new("cp")
            .args(["-a"])
            .arg(crate::ebuild_phases::bin_dir().unwrap())
            .arg(unit.join("bin"))
            .status()
            .expect("cp -a bin");
        assert!(status.success());
        // Mirror `build_bundle`: the regen run's
        // `${PORTAGE_BZIP2_COMMAND}` stand-in rides `bin/`.
        {
            use std::os::unix::fs::PermissionsExt as _;
            let passthrough = unit.join("bin").join("bzip2-passthrough");
            std::fs::write(&passthrough, crate::remote_bundle::BZIP2_PASSTHROUGH).unwrap();
            std::fs::set_permissions(&passthrough, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let entries = crate::remote_bundle::collect_filemeta(&image).unwrap();
        let filemeta = crate::remote_bundle::render_filemeta(&entries).unwrap();
        std::fs::write(unit.join("filemeta"), &filemeta).unwrap();
        let staged = crate::remote_bundle::StagedBundle {
            tarball: tmp.join("bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/regen-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: true,
            },
            eapi: "8".to_string(),
            category: "dev-libs".to_string(),
            pn: "regen".to_string(),
            pv: "1.0".to_string(),
            pr: "r0".to_string(),
            pvr: "1.0".to_string(),
            p: "regen-1.0".to_string(),
            pf: "regen-1.0".to_string(),
            phases: Vec::new(),
            postinst_defined: defined_phases.split_whitespace().any(|w| w == "postinst"),
        };
        (unit.to_str().unwrap().to_string(), staged)
    }

    fn regen_tmp(tag: &str) -> std::path::PathBuf {
        TempDir::new(&format!("portuale-remote-regen-{tag}")).keep()
    }

    /// Decompress a vdb `environment.bz2` for content assertions.
    fn read_vdb_env(vdb_env_bz2: &std::path::Path) -> String {
        let bytes = std::fs::read(vdb_env_bz2).unwrap();
        let output = std::process::Command::new("bzip2")
            .args(["-d", "-c", "--"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(&bytes)?;
                child.wait_with_output()
            })
            .expect("server bzip2 decompresses");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    }

    /// Backlog #171: a merge through the real driver + the regen
    /// postinst run leaves `var/db/pkg/<cat>/<pf>/environment.bz2`
    /// whose decompressed content is the **postinst-time** env -- a
    /// variable the test sets only in `pkg_postinst` is present, the
    /// stray build-time `x=""` is gone (real `ebuild.sh`'s own
    /// `unset path seen i x` after sourcing), `FEATURES` /
    /// `PORTAGE_FEATURES` are the resolved merge-time value (not the
    /// build-time `buildpkg` one) -- and **no** plain `environment`
    /// file (real's vdb has `environment.bz2` only).
    #[test]
    fn regen_postinst_rewrites_vdb_env_from_merge_time_env() {
        let tmp = regen_tmp("merge-time");
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let (unit, staged) = regen_unit(
            &tmp,
            "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
            "postinst",
        );
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let markers = run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
        assert!(markers.iter().any(|m| m == "MERGE_VDB=ok"), "{markers:?}");
        let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
        // No plain `environment` in the vdb (real's shape).
        assert!(
            !vdb.join("environment").exists(),
            "MERGE_FLOW must not write a plain environment file"
        );
        // The vdb still holds the build-time env before the regen.
        let before = read_vdb_env(&vdb.join("environment.bz2"));
        assert!(
            before.contains("declare -- x=\"\""),
            "pre-regen env:\n{before}"
        );
        assert!(before.contains("buildpkg"), "pre-regen env:\n{before}");

        let report =
            run_postinst_regen_stage(&ctx, None, &unit, &staged, Some("sandbox merge-time"), &[])
                .expect("regen phase runs");
        assert_eq!(report.phase_rc, 0);
        assert!(report.regen_present);
        assert!(!report.skipped_no_hooks);
        install_regenerated_env(
            &ctx,
            None,
            &unit,
            vdb.join("environment.bz2").to_str().unwrap(),
            Some("bzip2"),
        )
        .expect("install succeeds");

        let after = read_vdb_env(&vdb.join("environment.bz2"));
        assert!(
            after.contains("declare -x PT_MERGE_MARKER=\"merge-time\""),
            "postinst-time var missing:\n{after}"
        );
        assert!(
            !after.contains("x=\"\""),
            "stray build-time x survived:\n{after}"
        );
        assert!(
            !after.contains("buildpkg"),
            "build-time FEATURES survived:\n{after}"
        );
        assert!(
            after.contains("declare -x FEATURES=\"sandbox merge-time\""),
            "resolved FEATURES missing:\n{after}"
        );
        assert!(
            after.contains("declare -x PORTAGE_FEATURES=\"sandbox merge-time\""),
            "resolved PORTAGE_FEATURES missing:\n{after}"
        );
        assert!(
            after.contains("declare -x PORTAGE_BZIP2_COMMAND=\"bzip2\""),
            "passthrough path leaked into the vdb env:\n{after}"
        );
        assert!(
            !vdb.join("environment").exists(),
            "install must not write a plain environment file"
        );
        // Atomic install leaves no temp behind.
        assert!(!vdb.join("environment.bz2.portuale-regen-tmp").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171: a package **without** `pkg_postinst` still gets a
    /// regenerated env -- real's postinst `EbuildPhase` always starts
    /// (`phase-functions.sh:1061-1083` runs the hooks, no-op when
    /// undefined, then the `PORTAGE_UPDATE_ENV` block), so the phase rc
    /// is 0 and the env carries the merge-time `FEATURES`.
    #[test]
    fn regen_runs_without_pkg_postinst() {
        let tmp = regen_tmp("no-postinst");
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let (unit, staged) = regen_unit(
            &tmp,
            "EAPI=8\nDESCRIPTION=\"synthetic regen probe, no postinst\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
            "-",
        );
        assert!(!staged.postinst_defined);
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
        let report = run_postinst_regen_stage(&ctx, None, &unit, &staged, Some("sandbox"), &[])
            .expect("regen phase runs");
        assert_eq!(report.phase_rc, 0, "undefined pkg_postinst is a no-op");
        assert!(report.regen_present);
        let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
        install_regenerated_env(
            &ctx,
            None,
            &unit,
            vdb.join("environment.bz2").to_str().unwrap(),
            Some("bzip2"),
        )
        .expect("install succeeds");
        let after = read_vdb_env(&vdb.join("environment.bz2"));
        assert!(
            !after.contains("x=\"\""),
            "stray build-time x survived:\n{after}"
        );
        assert!(
            after.contains("declare -x FEATURES=\"sandbox\""),
            "resolved FEATURES missing:\n{after}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171b task 3: the server's set locale variables reach
    /// the client phase env, and only those -- a fake getter pins the
    /// selection (`LANG` set + empty-but-set `LANGUAGE` forwarded in
    /// `REGEN_LOCALE_VARS` order; unset `LC_ALL` skipped; a
    /// non-locale variable never consulted).
    #[test]
    fn collect_server_locale_forwards_only_set_vars() {
        use std::collections::HashMap;
        let env: HashMap<&str, &str> = [("LANG", "C.UTF-8"), ("LANGUAGE", ""), ("TERM", "xterm")]
            .into_iter()
            .collect();
        let got = collect_server_locale(|name| env.get(name).map(|v| v.to_string()));
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["LANG", "LANGUAGE"]);
        assert_eq!(got[0].1, "C.UTF-8");
        assert_eq!(got[1].1, "");
    }

    /// A minimal staged bundle for the render-level regen script test
    /// (the script only needs the identity fields).
    fn render_test_staged() -> crate::remote_bundle::StagedBundle {
        crate::remote_bundle::StagedBundle {
            tarball: std::path::PathBuf::from("/tmp/bundle.tar"),
            byte_count: 0,
            manifest: crate::remote_bundle::BundleManifest {
                format: 1,
                cpv: "dev-libs/regen-1.0".to_string(),
                slot: "0".to_string(),
                repo: "test".to_string(),
                has_environment: true,
            },
            eapi: "8".to_string(),
            category: "dev-libs".to_string(),
            pn: "regen".to_string(),
            pv: "1.0".to_string(),
            pr: "r0".to_string(),
            pvr: "1.0".to_string(),
            p: "regen-1.0".to_string(),
            pf: "regen-1.0".to_string(),
            phases: Vec::new(),
            postinst_defined: true,
        }
    }

    /// Backlog #171b tasks 2-3, render level: the regen script unsets
    /// `O`, unsets `SHELL`, unsets `LC_ALL` (EAPI 8 is posixish), and
    /// re-exports exactly the given locale pairs (quoting values like
    /// any other export).
    #[test]
    fn postinst_regen_script_matches_real_shell_shape() {
        let staged = render_test_staged();
        let locale = vec![
            ("LANG".to_string(), "C.UTF-8".to_string()),
            ("LC_NUMERIC".to_string(), "a b".to_string()),
        ];
        let script = postinst_regen_script(
            "/work/regen-1.0",
            &staged,
            "/",
            "/work",
            "never",
            Some("sandbox merge-time"),
            &locale,
            "/opt/bin/portuale-0123456789abcdef",
        );
        assert!(
            script.contains("unset O\n"),
            "O must go (real environ_filter):\n{script}"
        );
        assert!(
            script.contains("unset SHELL\n"),
            "SHELL must be unset so bash self-inits it (real declare --):\n{script}"
        );
        assert!(
            script.contains("unset LC_ALL\n"),
            "LC_ALL must go on posixish EAPIs (real split_LC_ALL):\n{script}"
        );
        assert!(
            script.contains("export LANG='C.UTF-8'\n"),
            "server LANG missing:\n{script}"
        );
        assert!(
            script.contains("export LC_NUMERIC='a b'\n"),
            "server LC_* missing or misquoted:\n{script}"
        );
        assert!(
            !script.contains("LANGUAGE"),
            "uninjected locale must not appear:\n{script}"
        );
    }

    /// Backlog #171b tasks 2-3, stage level: a real regen run under
    /// local bash leaves no `O` line in `environment.regen` (real
    /// `doebuild.py:475` sets `mysettings["O"]` but
    /// `config.environ()` drops it via `special_env_vars.py:
    /// environ_filter`), records `SHELL` as `declare --` (bash's own,
    /// unexported -- real has no exported `SHELL` to inherit), and
    /// carries the injected locale values as `declare -x`.
    #[test]
    fn regen_matches_real_o_shell_and_locale() {
        let tmp = regen_tmp("o-shell-locale");
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let (unit, staged) = regen_unit(
            &tmp,
            "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
            "postinst",
        );
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let locale = vec![
            ("LANG".to_string(), "C.UTF-8".to_string()),
            ("LC_MESSAGES".to_string(), "C.UTF-8".to_string()),
        ];
        let report = run_postinst_regen_stage(&ctx, None, &unit, &staged, Some("sandbox"), &locale)
            .expect("regen phase runs");
        assert_eq!(report.phase_rc, 0);
        assert!(report.regen_present);
        let regen =
            std::fs::read_to_string(format!("{unit}/environment.regen")).expect("regen file");
        for line in regen.lines() {
            let Some(rest) = line.strip_prefix("declare ") else {
                continue;
            };
            let rest = rest
                .strip_prefix("-x ")
                .or_else(|| rest.strip_prefix("-- "))
                .unwrap_or(rest);
            let name = rest.split(['=', ' ']).next().unwrap_or("");
            assert_ne!(
                name, "O",
                "unit-local O leaked into the regen'd env:\n{regen}"
            );
        }
        assert!(
            regen.contains("declare -- SHELL="),
            "SHELL must be unexported like real's:\n{regen}"
        );
        assert!(
            !regen.contains("declare -x SHELL="),
            "exported SHELL leaked into the regen'd env:\n{regen}"
        );
        assert!(
            regen.contains("declare -x LANG=\"C.UTF-8\""),
            "server LANG missing:\n{regen}"
        );
        assert!(
            regen.contains("declare -x LC_MESSAGES=\"C.UTF-8\""),
            "server LC_* missing:\n{regen}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171c task 1, unit level: the `split_LC_ALL` port fans a
    /// set `LC_ALL` out over all twelve real `locale_categories` and
    /// drops it (real `portage/util/locale.py:22-36,160` +
    /// `config.py:3374-3385`), fills an unset `LANG`, never clobbers
    /// an explicit one, drops an empty `LC_ALL` without fanning out,
    /// leaves `LANGUAGE` alone, and passes everything through
    /// untouched on non-posixish EAPIs.
    #[test]
    fn split_server_locale_fans_lc_all_out_like_real() {
        // The twelve `locale_categories` real fans out over
        // (`portage/util/locale.py:22-36`) -- hardcoded here (not via
        // `REAL_LOCALE_CATEGORIES`) so the test pins the list's
        // content, not just its own reference to it.
        const TWELVE: &[&str] = &[
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MONETARY",
            "LC_MESSAGES",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ];
        assert_eq!(REAL_LOCALE_CATEGORIES, TWELVE);
        assert!(eapi_is_posixish("6"));
        assert!(eapi_is_posixish("8"));
        assert!(!eapi_is_posixish("5"));
        let get = |pairs: &[(String, String)], name: &str| {
            pairs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        // The brief's case: server `LC_ALL=C.UTF-8` alone → `LANG` +
        // all twelve categories carry it, no `LC_ALL`.
        let out = split_server_locale(&[("LC_ALL".to_string(), "C.UTF-8".to_string())], "8");
        assert!(get(&out, "LC_ALL").is_none(), "LC_ALL survived:\n{out:?}");
        assert_eq!(get(&out, "LANG").as_deref(), Some("C.UTF-8"));
        for name in TWELVE {
            assert_eq!(
                get(&out, name).as_deref(),
                Some("C.UTF-8"),
                "{name} not split"
            );
        }
        // Real overwrites unconditionally; an explicit `LANG` still wins.
        let out = split_server_locale(
            &[
                ("LC_ALL".to_string(), "C.UTF-8".to_string()),
                ("LC_MESSAGES".to_string(), "en_US.UTF-8".to_string()),
                ("LANG".to_string(), "POSIX".to_string()),
                ("LANGUAGE".to_string(), "de".to_string()),
            ],
            "8",
        );
        assert_eq!(get(&out, "LC_MESSAGES").as_deref(), Some("C.UTF-8"));
        assert_eq!(get(&out, "LANG").as_deref(), Some("POSIX"));
        assert_eq!(get(&out, "LANGUAGE").as_deref(), Some("de"));
        assert!(get(&out, "LC_ALL").is_none());
        // Set-but-empty `LC_ALL` is dropped, never fanned out.
        let out = split_server_locale(
            &[
                ("LC_ALL".to_string(), String::new()),
                ("LANG".to_string(), "C".to_string()),
            ],
            "8",
        );
        assert!(get(&out, "LC_ALL").is_none());
        assert_eq!(get(&out, "LANG").as_deref(), Some("C"));
        assert!(get(&out, "LC_CTYPE").is_none());
        // Non-posixish EAPIs keep `LC_ALL` as-is (real never splits).
        let input = vec![("LC_ALL".to_string(), "C.UTF-8".to_string())];
        assert_eq!(split_server_locale(&input, "5"), input);
    }

    /// Backlog #171c task 1, stage level: a real regen run with only
    /// `LC_ALL=C.UTF-8` forwarded records `LANG` + all twelve real
    /// locale categories as `C.UTF-8` and no `LC_ALL` line (bed run
    /// `l31-20260927T052232Z`: real has `LANG` + split categories where
    /// `mrg` echoed `LC_ALL` back).
    #[test]
    fn regen_splits_lc_all_like_real() {
        let tmp = regen_tmp("lc-all-split");
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let (unit, staged) = regen_unit(
            &tmp,
            "EAPI=8\nDESCRIPTION=\"synthetic regen probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\npkg_postinst() {\n\texport PT_MERGE_MARKER=\"merge-time\"\n}\n",
            "postinst",
        );
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let locale = vec![("LC_ALL".to_string(), "C.UTF-8".to_string())];
        let report = run_postinst_regen_stage(&ctx, None, &unit, &staged, Some("sandbox"), &locale)
            .expect("regen phase runs");
        assert_eq!(report.phase_rc, 0);
        assert!(report.regen_present);
        let regen =
            std::fs::read_to_string(format!("{unit}/environment.regen")).expect("regen file");
        assert!(
            regen.contains("declare -x LANG=\"C.UTF-8\""),
            "split LANG missing:\n{regen}"
        );
        for name in [
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MESSAGES",
            "LC_MONETARY",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ] {
            assert!(
                regen.contains(&format!("declare -x {name}=\"C.UTF-8\"")),
                "split {name} missing:\n{regen}"
            );
        }
        assert!(
            !regen.lines().any(|line| line.contains("LC_ALL")),
            "LC_ALL leaked into the regen'd env:\n{regen}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171c task 4: the `--remote-binpkg` trial path places
    /// `/etc/portage` through the shared `place_config_root` helper --
    /// `Server` used directly, `Client` pulled over local transport and
    /// re-rooted -- with the `PORTAGE_CONFIGROOT` override held for the
    /// resolve, so the same `load_repos_and_config` +
    /// `config_install_mask` / `config_features_string` /
    /// `config_bzip2_command` getters the plan path uses see the placed
    /// tree. A synthetic client tree pins both placements (`server:`
    /// takes a config root in real-root layout, `client:` the
    /// `/etc/portage` dir itself -- the contract suite's
    /// `test_mrg_remote_resolve_merges_a_binhost_binary` uses
    /// `server:{root}` the same way); the pulled temp dir is gone after
    /// the `Client` resolve (task 3).
    #[test]
    fn placed_config_root_covers_both_placements() {
        // `PORTAGE_CONFIGROOT` is process-global (see
        // `PLACED_CONFIG_ENV_LOCK`); pin it to an empty dir so the
        // host's own `/usr/share/portage/config/make.globals` -- when
        // one is installed -- cannot leak into this hermetic resolve.
        let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
        let tmp = regen_tmp("placed-config");
        let pinned_server = tmp.join("pinned-server");
        std::fs::create_dir_all(&pinned_server).unwrap();
        let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
        // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in this
        // binary pins `PORTAGE_CONFIGROOT` without it.
        unsafe {
            std::env::set_var("PORTAGE_CONFIGROOT", &pinned_server);
        }
        let config_root = tmp.join("clientroot");
        let client_etc = config_root.join("etc/portage");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            client_etc.join("repos.conf/testrepo.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                repo.display()
            ),
        )
        .unwrap();
        std::fs::write(
            client_etc.join("make.conf"),
            "INSTALL_MASK=\"/usr/share/porttest/im/drop.txt *.la\"\nFEATURES=\"sandbox merge-time\"\nPORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
        )
        .unwrap();
        // `config_install_mask` prefers the process `INSTALL_MASK` env
        // var; clear it so the test pins the placed file.
        let saved_mask = std::env::var_os("INSTALL_MASK");
        // SAFETY: no other test in this binary sets `INSTALL_MASK`.
        unsafe {
            std::env::remove_var("INSTALL_MASK");
        }
        let eroot = tmp.join("eroot");
        let mut client_tmp: Option<std::path::PathBuf> = None;
        for placement in [
            ConfigPlacement::Client(client_etc.to_string_lossy().into_owned()),
            ConfigPlacement::Server(config_root.to_string_lossy().into_owned()),
        ] {
            let mut ctx = local_ctx(
                tmp.join("root").to_str().unwrap(),
                tmp.join("work").to_str().unwrap(),
            );
            ctx.etc_portage = placement.clone();
            let placed = place_config_root(&ctx, None).expect("placement resolves");
            // Both shapes resolve to a real-root layout: the `Client`
            // pull is re-rooted as `<tmp>/etc/portage`, the `Server`
            // root is used directly.
            assert!(
                placed.dir.join("etc/portage/make.conf").is_file(),
                "placed root lacks the client make.conf: {}",
                placed.dir.display()
            );
            assert_eq!(
                std::env::var_os("PORTAGE_CONFIGROOT"),
                Some(placed.dir.as_os_str().to_os_string()),
                "config-root override not held for the resolve"
            );
            // The same shared resolver + getters `run_bundle_stage`
            // feeds into `run_binpkg_flow`.
            let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
                .expect("placed client config resolves");
            assert!(repos.iter().any(|r| r.is_main));
            let (mask, _) = crate::pretend::config_install_mask(&config);
            assert_eq!(mask, "/usr/share/porttest/im/drop.txt *.la");
            assert_eq!(
                crate::pretend::config_features_string(&config),
                "merge-time sandbox"
            );
            assert_eq!(crate::pretend::config_bzip2_command(&config), "lbzip2");
            if matches!(placement, ConfigPlacement::Client(_)) {
                client_tmp = Some(placed.dir.to_path_buf());
            }
            drop(placed);
        }
        // SAFETY: same as above.
        unsafe {
            match saved_mask {
                Some(value) => std::env::set_var("INSTALL_MASK", value),
                None => std::env::remove_var("INSTALL_MASK"),
            }
            match saved_config_root {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
        }
        // Task 3: the pulled temp copy is removed best-effort with the
        // resolve; the server source tree is untouched.
        assert!(
            client_tmp.as_ref().is_some_and(|dir| !dir.exists()),
            "pulled client config temp dir leaked: {client_tmp:?}"
        );
        assert!(
            client_etc.join("make.conf").is_file(),
            "server placement must not remove the source tree"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 3 (owner Q9 = b): the pulled client
    /// config stacks the **server's** `make.globals` as its bottom
    /// layer. A synthetic server root carries
    /// `usr/share/portage/config/make.globals` with
    /// `FEATURES="news sandbox"`; the pulled client `make.conf` says
    /// `FEATURES="-news sign"` -- real's incremental fold
    /// (`const.INCREMENTALS`, `config.py:446-499`) resolves that to
    /// `sandbox sign`, and the regen'd vdb env carries exactly that
    /// list (not the build-time `buildpkg` one). `PORTAGE_CONFIGROOT`
    /// is pinned to the synthetic server root (process-global; see
    /// `PLACED_CONFIG_ENV_LOCK`) and ambient `FEATURES`/`INSTALL_MASK`
    /// cleared so the host cannot leak into the hermetic resolve.
    #[test]
    fn remote_client_config_stacks_server_make_globals() {
        let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
        let tmp = regen_tmp("server-globals");
        let server_root = tmp.join("serverroot");
        std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
        std::fs::write(
            server_root.join("usr/share/portage/config/make.globals"),
            "FEATURES=\"news sandbox\"\n",
        )
        .unwrap();
        let client_etc = tmp.join("clientroot/etc/portage");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            client_etc.join("repos.conf/testrepo.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                repo.display()
            ),
        )
        .unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news sign\"\n").unwrap();
        let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
        let saved_features = std::env::var_os("FEATURES");
        let saved_mask = std::env::var_os("INSTALL_MASK");
        // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in this
        // binary pins these without it.
        unsafe {
            std::env::set_var("PORTAGE_CONFIGROOT", &server_root);
            std::env::remove_var("FEATURES");
            std::env::remove_var("INSTALL_MASK");
        }
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        assert!(
            placed
                .dir
                .join("usr/share/portage/config/make.globals")
                .is_file(),
            "the server make.globals must be seeded under the pulled root"
        );
        let eroot = tmp.join("eroot");
        let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
            .expect("placed client config resolves");
        assert!(repos.iter().any(|r| r.is_main));
        let resolved = crate::pretend::config_features_string(&config);
        assert_eq!(
            resolved, "sandbox sign",
            "incremental fold over the seeded defaults"
        );
        drop(placed);
        // SAFETY: same as above.
        unsafe {
            match saved_config_root {
                Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                None => std::env::remove_var("PORTAGE_CONFIGROOT"),
            }
            match saved_features {
                Some(value) => std::env::set_var("FEATURES", value),
                None => std::env::remove_var("FEATURES"),
            }
            match saved_mask {
                Some(value) => std::env::set_var("INSTALL_MASK", value),
                None => std::env::remove_var("INSTALL_MASK"),
            }
        }
        // The regenerated vdb env carries the resolved list: merge the
        // synthetic unit, run the regen postinst with the resolved
        // `FEATURES`, and read the installed `environment.bz2`.
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let (unit, staged) = regen_unit(
            &tmp,
            "EAPI=8\nDESCRIPTION=\"synthetic globals probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
            "-",
        );
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        run_merge_stage(&ctx, None, &unit, &staged, None).expect("merge succeeds");
        let report = run_postinst_regen_stage(&ctx, None, &unit, &staged, Some(&resolved), &[])
            .expect("regen phase runs");
        assert_eq!(report.phase_rc, 0);
        assert!(report.regen_present);
        let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
        install_regenerated_env(
            &ctx,
            None,
            &unit,
            vdb.join("environment.bz2").to_str().unwrap(),
            Some("bzip2"),
        )
        .expect("install succeeds");
        let after = read_vdb_env(&vdb.join("environment.bz2"));
        assert!(
            after.contains("declare -x FEATURES=\"sandbox sign\""),
            "resolved FEATURES missing:\n{after}"
        );
        assert!(
            after.contains("declare -x PORTAGE_FEATURES=\"sandbox sign\""),
            "resolved PORTAGE_FEATURES missing:\n{after}"
        );
        assert!(
            !after.contains("buildpkg"),
            "build-time FEATURES survived:\n{after}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 4 (owner Q10 = b) scaffolding: a
    /// synthetic server root (`etc/portage/repos.conf` + the server's
    /// own `make.globals`, the two server-side inputs `place_config_root`
    /// reads before repointing `PORTAGE_CONFIGROOT`). Returns the root;
    /// repos are added per-test with `l171e_server_repo`.
    fn l171e_server_root(tmp: &std::path::Path, repos_conf: &str) -> std::path::PathBuf {
        let server_root = tmp.join("serverroot");
        std::fs::create_dir_all(server_root.join("etc/portage/repos.conf")).unwrap();
        std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
        std::fs::write(
            server_root.join("etc/portage/repos.conf/l171e.conf"),
            repos_conf,
        )
        .unwrap();
        std::fs::write(
            server_root.join("usr/share/portage/config/make.globals"),
            "FEATURES=\"sandbox news sign\"\n",
        )
        .unwrap();
        server_root
    }

    /// Add a server repo with one profile `<location>/profiles/<rel>`
    /// carrying `make.defaults` `FEATURES="<features>"` (plus a
    /// `repo_name` file so `find_repos` keeps the section name).
    fn l171e_server_repo(
        server_root: &std::path::Path,
        name: &str,
        rel: &str,
        features: &str,
    ) -> std::path::PathBuf {
        let location = server_root.join("repos").join(name);
        let profile = location.join("profiles").join(rel);
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("make.defaults"),
            format!("FEATURES=\"{features}\"\n"),
        )
        .unwrap();
        std::fs::write(location.join("profiles/repo_name"), format!("{name}\n")).unwrap();
        location
    }

    /// Pin `PORTAGE_CONFIGROOT` at a synthetic server root with ambient
    /// `FEATURES`/`INSTALL_MASK` cleared (all process-global; shares
    /// `PLACED_CONFIG_ENV_LOCK`), restoring everything on drop while the
    /// lock is still held.
    struct ServerEnvPin {
        saved_config_root: Option<std::ffi::OsString>,
        saved_features: Option<std::ffi::OsString>,
        saved_mask: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl ServerEnvPin {
        fn pin(server_root: &std::path::Path) -> Self {
            let guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
            let saved_config_root = std::env::var_os("PORTAGE_CONFIGROOT");
            let saved_features = std::env::var_os("FEATURES");
            let saved_mask = std::env::var_os("INSTALL_MASK");
            // SAFETY: held `PLACED_CONFIG_ENV_LOCK`; no other test in
            // this binary pins these without it.
            unsafe {
                std::env::set_var("PORTAGE_CONFIGROOT", server_root);
                std::env::remove_var("FEATURES");
                std::env::remove_var("INSTALL_MASK");
            }
            Self {
                saved_config_root,
                saved_features,
                saved_mask,
                _guard: guard,
            }
        }
    }

    impl Drop for ServerEnvPin {
        fn drop(&mut self) {
            // SAFETY: same as above.
            unsafe {
                match self.saved_config_root.take() {
                    Some(value) => std::env::set_var("PORTAGE_CONFIGROOT", value),
                    None => std::env::remove_var("PORTAGE_CONFIGROOT"),
                }
                match self.saved_features.take() {
                    Some(value) => std::env::set_var("FEATURES", value),
                    None => std::env::remove_var("FEATURES"),
                }
                match self.saved_mask.take() {
                    Some(value) => std::env::set_var("INSTALL_MASK", value),
                    None => std::env::remove_var("INSTALL_MASK"),
                }
            }
        }
    }

    /// Resolve a placed client tree's config against explicit server
    /// repos (kept for the l171e remap tests, which pin the rewritten
    /// link against hand-picked server repos; the production path now
    /// resolves through the shared `load_repos_and_config` -- see
    /// `remote_client_without_repos_conf_resolves_on_the_production_path`).
    fn l171e_resolve_with_server_repos(
        placed_dir: &std::path::Path,
        server_root: &std::path::Path,
        eroot: &std::path::Path,
    ) -> portage_profile::Config {
        let server_repos = portage_repo::find_repos(server_root).expect("server repos resolve");
        let main = server_repos
            .iter()
            .find(|repo| repo.is_main)
            .expect("a main repo");
        let overlays: Vec<(String, std::path::PathBuf)> = server_repos
            .iter()
            .filter(|repo| !repo.is_main)
            .map(|repo| (repo.name.clone(), repo.location.clone()))
            .collect();
        let aliases: Vec<(String, std::path::PathBuf)> = server_repos
            .iter()
            .flat_map(|repo| {
                repo.aliases
                    .iter()
                    .map(|alias| (alias.clone(), repo.location.clone()))
            })
            .collect();
        let masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = server_repos
            .iter()
            .map(|repo| (repo.name.clone(), repo.masters.clone()))
            .collect();
        portage_profile::resolve_config(
            placed_dir,
            &main.location,
            &overlays,
            &aliases,
            &main.name,
            &masters,
            eroot,
        )
        .expect("placed client config resolves")
    }

    const L171E_REL: &str = "default/linux/amd64/23.0";

    /// Backlog #171 follow-up 4, step 2: a dangling absolute
    /// `make.profile` into the client's repo path plus a client
    /// `repos.conf` naming that repo resolves to the **same-named**
    /// server repo's profile -- even when the main repo provides the
    /// same relative profile with different content (a pure
    /// main-first scan would pick the wrong one). Real's stacking
    /// order holds: server `make.globals` + profile + client
    /// `make.conf` (`config.py:446-499`, `const.INCREMENTALS`).
    #[test]
    fn remote_client_make_profile_resolves_against_same_named_server_repo() {
        let tmp = regen_tmp("make-profile-name");
        let server_root = l171e_server_root(
            &tmp,
            &format!(
                "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n\n[custom]\nlocation = {}\n",
                tmp.join("serverroot/repos/gentoo").display(),
                tmp.join("serverroot/repos/custom").display(),
            ),
        );
        l171e_server_repo(&server_root, "gentoo", L171E_REL, "otherfeat -sign");
        let custom_profile = l171e_server_repo(&server_root, "custom", L171E_REL, "filecaps -sign");
        // The client's repo path: text only, never created (dangling,
        // like a client holding its tree elsewhere).
        let prefix = "/var/db/repos-client-l171e/custom";
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
        std::fs::write(
            client_etc.join("repos.conf/l171e.conf"),
            format!("[DEFAULT]\nmain-repo = custom\n\n[custom]\nlocation = {prefix}\n"),
        )
        .unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
        std::os::unix::fs::symlink(
            format!("{prefix}/profiles/{L171E_REL}"),
            client_etc.join("make.profile"),
        )
        .unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        // Remapped onto the same-named server repo, not the main one.
        assert_eq!(
            std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
            custom_profile.join("profiles").join(L171E_REL),
            "the pulled link must point at the same-named server repo's profile"
        );
        let eroot = tmp.join("eroot");
        let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
            .expect("placed client config resolves");
        assert!(repos.iter().any(|repo| repo.is_main));
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "custom filecaps sandbox",
            "server make.globals + same-named profile + client make.conf"
        );
        drop(placed);
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 4, step 3: with **no** client `repos.conf`
    /// (the owner Q10 client holds no repo at all) the dangling link
    /// resolves by `profiles/<rel>` against the server repos.
    #[test]
    fn remote_client_make_profile_resolves_without_client_repos_conf() {
        let tmp = regen_tmp("make-profile-norepos");
        let server_root = l171e_server_root(
            &tmp,
            &format!(
                "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
                tmp.join("serverroot/repos/gentoo").display(),
            ),
        );
        let server_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "filecaps -sign");
        let target = format!("/var/db/repos-client-l171e/gentoo/profiles/{L171E_REL}");
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(&client_etc).unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
        std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        assert_eq!(
            std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
            server_profile.join("profiles").join(L171E_REL),
        );
        let eroot = tmp.join("eroot");
        let config = l171e_resolve_with_server_repos(&placed.dir, &server_root, &eroot);
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "custom filecaps sandbox",
            "server make.globals + profile + client make.conf"
        );
        drop(placed);
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 5 (l171f; review of l171e): the owner
    /// Q10 client holds no `repos.conf` at all -- the pulled config
    /// must resolve on the PRODUCTION path (`place_config_root` +
    /// `load_repos_and_config`, the shared chain `run_remote_resolve`
    /// / `pretend::run` and `run_bundle_stage` both resolve through),
    /// not just through the test-only
    /// `l171e_resolve_with_server_repos` helper. The server root
    /// carries its repos *only* in the global
    /// `usr/share/portage/config/repos.conf` slot (real's
    /// `repository/config.py:1488-1508` reads the global file before
    /// the user's); the seed copies it into the placed root because
    /// the pulled tree has none, the dangling `make.profile` remaps
    /// onto the server repo, and the resolved `FEATURES` stack server
    /// globals + profile + client `make.conf` in real's order
    /// (`config.py:446-499`, `const.INCREMENTALS`).
    #[test]
    fn remote_client_without_repos_conf_resolves_on_the_production_path() {
        let tmp = regen_tmp("norepos-production");
        // A server root with repos in the global slot (the exact file
        // `place_config_root` seeds from) *and* a user slot -- like a
        // real server, which always has `/etc/portage/repos.conf`, so
        // `find_repos` on the server root itself keeps working. The
        // user slot carries an extra overlay the global slot lacks, so
        // the placed tree pins the seed's source: only the global
        // content may arrive.
        let server_root = tmp.join("serverroot");
        std::fs::create_dir_all(server_root.join("usr/share/portage/config")).unwrap();
        std::fs::create_dir_all(server_root.join("etc/portage/repos.conf")).unwrap();
        let global_conf = format!(
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
            tmp.join("serverroot/repos/gentoo").display(),
        );
        std::fs::write(
            server_root.join("usr/share/portage/config/repos.conf"),
            &global_conf,
        )
        .unwrap();
        let ovl = tmp.join("serverroot/repos/ovl");
        std::fs::create_dir_all(&ovl).unwrap();
        std::fs::write(
            server_root.join("etc/portage/repos.conf/l171f.conf"),
            format!(
                "[DEFAULT]\nmain-repo = gentoo\n\n[ovl]\nlocation = {}\n",
                ovl.display(),
            ),
        )
        .unwrap();
        std::fs::write(
            server_root.join("usr/share/portage/config/make.globals"),
            "FEATURES=\"sandbox news sign\"\n",
        )
        .unwrap();
        let server_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "filecaps -sign");
        let target = format!("/var/db/repos-client-l171f/gentoo/profiles/{L171E_REL}");
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(&client_etc).unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
        std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        assert_eq!(
            std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
            server_profile.join("profiles").join(L171E_REL),
            "the dangling client link must remap onto the server repo's profile"
        );
        let eroot = tmp.join("eroot");
        // The production resolver both mrg paths share -- previously
        // `Err(NoReposConf)` here (review of l171e).
        let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
            .expect("placed client config resolves without a client repos.conf");
        // Only the seeded global content arrived: the server user
        // slot's overlay must not leak into the placed resolve.
        assert!(
            repos.iter().all(|repo| repo.name == "gentoo"),
            "placed repos must come from the seeded global file: {:?}",
            repos.iter().map(|repo| &repo.name).collect::<Vec<_>>()
        );
        assert!(repos.iter().any(|repo| repo.is_main));
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "custom filecaps sandbox",
            "server make.globals + server profile + client make.conf"
        );
        drop(placed);
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 4, step 5: an unresolvable target fails
    /// loudly -- a `!!!` message naming the target -- never a silent
    /// empty chain.
    #[test]
    fn remote_client_make_profile_unresolvable_fails_loudly() {
        let tmp = regen_tmp("make-profile-loud");
        let server_root = l171e_server_root(
            &tmp,
            &format!(
                "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n",
                tmp.join("serverroot/repos/gentoo").display(),
            ),
        );
        l171e_server_repo(
            &server_root,
            "gentoo",
            "default/linux/amd64/22.0",
            "filecaps -sign",
        );
        let target = "/var/db/repos-client-l171e/gentoo/profiles/default/linux/amd64/99.9";
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
        std::fs::write(
            client_etc.join("repos.conf/l171e.conf"),
            "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = /var/db/repos-client-l171e/gentoo\n",
        )
        .unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"custom\"\n").unwrap();
        std::os::unix::fs::symlink(target, client_etc.join("make.profile")).unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let error = match place_config_root(&ctx, None) {
            Ok(_) => panic!("unresolvable target must fail"),
            Err(message) => message,
        };
        assert!(error.contains("!!!"), "failure must be loud, got: {error}");
        assert!(
            error.contains(target),
            "failure must name the target, got: {error}"
        );
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 4, step 4: a directory `make.profile`
    /// keeps working through the normal machinery -- its `parent`
    /// `gentoo:base` entry resolves against the configured repos.
    #[test]
    fn remote_client_make_profile_dir_with_cross_repo_parent_resolves() {
        let tmp = regen_tmp("make-profile-dir");
        let server_root = l171e_server_root(
            &tmp,
            &format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                tmp.join("serverroot/repos/testrepo").display(),
            ),
        );
        // The client repo lives client-side (a directory profile's
        // parents resolve through the normal machinery, no remap).
        let client_repo = tmp.join("crepo");
        let base = client_repo.join("profiles/base");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("make.defaults"), "FEATURES=\"filecaps -sign\"\n").unwrap();
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(client_etc.join("repos.conf")).unwrap();
        std::fs::create_dir_all(client_etc.join("make.profile")).unwrap();
        std::fs::write(
            client_etc.join("repos.conf/l171e.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                client_repo.display(),
            ),
        )
        .unwrap();
        std::fs::write(client_etc.join("make.profile/parent"), "testrepo:base\n").unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        assert!(
            placed.dir.join("etc/portage/make.profile").is_dir()
                && placed
                    .dir
                    .join("etc/portage/make.profile")
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_dir(),
            "a directory make.profile must pass through untouched"
        );
        let eroot = tmp.join("eroot");
        let (repos, config) = crate::pretend::load_repos_and_config(&placed.dir, &eroot)
            .expect("placed client config resolves");
        assert!(repos.iter().any(|repo| repo.is_main));
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "custom filecaps sandbox",
            "server make.globals + parent profile + client make.conf"
        );
        drop(placed);
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 follow-up 4, step 3 order: with no client
    /// `repos.conf` and two server repos providing `profiles/<rel>`,
    /// the main repo wins; the link itself is client-relative (its
    /// text climbs out of the client's `/etc/portage`, never followed
    /// there).
    #[test]
    fn remote_client_make_profile_prefers_main_repo_and_relative_targets() {
        let tmp = regen_tmp("make-profile-order");
        let server_root = l171e_server_root(
            &tmp,
            &format!(
                "[DEFAULT]\nmain-repo = gentoo\n\n[gentoo]\nlocation = {}\n\n[ovl]\nlocation = {}\n",
                tmp.join("serverroot/repos/gentoo").display(),
                tmp.join("serverroot/repos/ovl").display(),
            ),
        );
        let main_profile = l171e_server_repo(&server_root, "gentoo", L171E_REL, "mainfeat -sign");
        l171e_server_repo(&server_root, "ovl", L171E_REL, "otherfeat -sign");
        let client_etc = tmp.join("clientroot/etc/portage");
        std::fs::create_dir_all(&client_etc).unwrap();
        std::fs::write(client_etc.join("make.conf"), "FEATURES=\"-news custom\"\n").unwrap();
        // A client-relative target: climb from the client's
        // `/etc/portage` to `/`, then the dangling repo path.
        let depth = client_etc
            .components()
            .filter(|component| matches!(component, std::path::Component::Normal(_)))
            .count();
        let target = format!(
            "{}var/db/repos-client-l171e/gentoo/profiles/{L171E_REL}",
            "../".repeat(depth)
        );
        std::os::unix::fs::symlink(&target, client_etc.join("make.profile")).unwrap();
        let _env = ServerEnvPin::pin(&server_root);
        let mut ctx = local_ctx(
            tmp.join("root").to_str().unwrap(),
            tmp.join("work").to_str().unwrap(),
        );
        ctx.etc_portage = ConfigPlacement::Client(client_etc.to_string_lossy().into_owned());
        let placed = place_config_root(&ctx, None).expect("client placement resolves");
        assert_eq!(
            std::fs::read_link(placed.dir.join("etc/portage/make.profile")).unwrap(),
            main_profile.join("profiles").join(L171E_REL),
            "the main repo must win the server scan"
        );
        let eroot = tmp.join("eroot");
        let config = l171e_resolve_with_server_repos(&placed.dir, &server_root, &eroot);
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "custom mainfeat sandbox",
            "server make.globals + main profile + client make.conf"
        );
        drop(placed);
        drop(_env);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171b task 1: `--remote-binpkg` loads the placed client
    /// config through the SAME shared helper `pretend::run` uses
    /// (`load_repos_and_config`: one resolver call, no duplicate
    /// logic), so `build_bundle` / the env regen see the same
    /// `INSTALL_MASK` (+ the `no{man,info,doc}` fold), merge-time
    /// `FEATURES` and `PORTAGE_BZIP2_COMMAND` as the plan path. A
    /// synthetic client config root pins the wiring end to end.
    #[test]
    fn remote_binpkg_path_resolves_the_placed_client_config() {
        // `INSTALL_MASK` is process-global (see `PLACED_CONFIG_ENV_LOCK`);
        // hold the lock across the remove/restore below so a parallel
        // test pinning a different value cannot interleave.
        let _env_guard = PLACED_CONFIG_ENV_LOCK.lock().unwrap();
        let tmp = regen_tmp("binpkg-config");
        let config_root = tmp.join("configroot");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(config_root.join("etc/portage/repos.conf")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            config_root.join("etc/portage/repos.conf/testrepo.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                repo.display()
            ),
        )
        .unwrap();
        std::fs::write(
            config_root.join("etc/portage/make.conf"),
            "INSTALL_MASK=\"/usr/share/porttest/im/drop.txt *.la\"\nFEATURES=\"sandbox merge-time\"\nPORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
        )
        .unwrap();
        // `config_install_mask` prefers the process `INSTALL_MASK` env
        // var (the same rule the plan path runs under); clear it so
        // the test pins the placed file, restoring before asserting.
        let saved_mask = std::env::var_os("INSTALL_MASK");
        // SAFETY: no other test in this binary sets `INSTALL_MASK`;
        // readers elsewhere only consume it.
        unsafe {
            std::env::remove_var("INSTALL_MASK");
        }
        let eroot = tmp.join("eroot");
        let resolved = crate::pretend::load_repos_and_config(&config_root, &eroot);
        // SAFETY: same as above.
        unsafe {
            match saved_mask {
                Some(value) => std::env::set_var("INSTALL_MASK", value),
                None => std::env::remove_var("INSTALL_MASK"),
            }
        }
        let (repos, config) = resolved.expect("synthetic client config resolves");
        assert!(repos.iter().any(|r| r.is_main));
        let (mask, prunes_usr_share) = crate::pretend::config_install_mask(&config);
        assert_eq!(mask, "/usr/share/porttest/im/drop.txt *.la");
        assert!(!prunes_usr_share);
        assert_eq!(
            crate::pretend::config_features_string(&config),
            "merge-time sandbox"
        );
        assert_eq!(crate::pretend::config_bzip2_command(&config), "lbzip2");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171, failure path: a forced pull failure (no
    /// `environment.regen` in the unit) keeps the build-time
    /// `environment.bz2` byte-identical and reports the step.
    #[test]
    fn install_keeps_build_time_env_when_pull_fails() {
        let tmp = regen_tmp("failure");
        let root = tmp.join("root");
        let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        // A build-time env already in the vdb (the MERGE_FLOW fallback).
        let build_time = bzip2_compress(build_time_environment(false).as_bytes())
            .expect("server bzip2 compresses");
        std::fs::write(vdb.join("environment.bz2"), &build_time).unwrap();
        let unit = tmp.join("work/regen-1.0");
        std::fs::create_dir_all(&unit).unwrap();
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let err = install_regenerated_env(
            &ctx,
            None,
            unit.to_str().unwrap(),
            vdb.join("environment.bz2").to_str().unwrap(),
            Some("bzip2"),
        )
        .expect_err("pull must fail without environment.regen");
        assert_eq!(err.0, "pull", "unexpected failing step: {err:?}");
        assert_eq!(
            std::fs::read(vdb.join("environment.bz2")).unwrap(),
            build_time,
            "the build-time env must survive byte-identical"
        );
        assert_eq!(
            regen_warn_message("dev-libs/regen-1.0", "pull"),
            "!!! Remote dev-libs/regen-1.0: merge-time environment regen failed at pull, keeping build-time environment.bz2",
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 review: a failed atomic install leaves no client
    /// tmp litter -- the remote temp name is removed best-effort.
    /// (Local transport; the ssh branch's `rm -f` needs a live client
    /// and is not covered here.)
    #[test]
    fn install_file_atomic_removes_tmp_when_mv_fails() {
        let tmp = regen_tmp("install-tmp");
        let root = tmp.join("root");
        let vdb = root.join("var/db/pkg/dev-libs/regen-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        // A directory at the dest path makes the `rename` fail.
        let dest = vdb.join("environment.bz2");
        std::fs::create_dir_all(&dest).unwrap();
        let ctx = local_ctx(root.to_str().unwrap(), tmp.join("work").to_str().unwrap());
        let err = install_file_atomic(&ctx, None, b"regen", dest.to_str().unwrap())
            .expect_err("install into a directory must fail");
        assert!(err.contains("installing"), "unexpected error text: {err}");
        assert!(
            !vdb.join("environment.bz2.portuale-regen-tmp").exists(),
            "the remote temp name must not litter the client"
        );
        assert!(dest.is_dir(), "the blocking directory is untouched");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #262: the ssh leg of the atomic install must stage the
    /// bytes into a *file* before shipping them. `TempDir::new` creates
    /// a directory (backlog #260), so staging straight into its path
    /// fails with `Is a directory (os error 21)` on every ssh install
    /// -- the regen'd env never reaches the vdb and the build-time env
    /// (with its stray `declare -- x=""`) survives. Against an
    /// unreachable host the install still fails, but past staging, at
    /// transport -- the assertion pins exactly that boundary.
    #[test]
    fn install_file_atomic_ssh_stages_bytes_before_transport() {
        let ctx = ctx_with_args("unreachable.invalid", None);
        let err = install_file_atomic(
            &ctx,
            None,
            b"regen",
            "/var/db/pkg/dev-libs/regen-1.0/environment.bz2",
        )
        .expect_err("no ssh server: the install must fail");
        assert!(
            !err.contains("staging the regen install failed"),
            "ssh staging must succeed (transport may fail): {err}"
        );
    }

    /// Pure unit: the `PORTAGE_BZIP2_COMMAND` scrub rewrites only the
    /// unit-local passthrough stand-in -- to the resolved client value
    /// when one is configured, to the `make.globals` default otherwise
    /// -- and copies every other value verbatim.
    #[test]
    fn scrub_bzip2_command_rewrites_only_the_passthrough() {
        let regen = b"declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"'/tmp/w/bin/bzip2-passthrough'\"\ndeclare -x USE=\"amd64\"\n";
        // Configured client value (e.g. `lbzip2` in `make.conf`) is written.
        let scrubbed = scrub_bzip2_command(regen, "lbzip2");
        assert_eq!(
            String::from_utf8(scrubbed).unwrap(),
            "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"lbzip2\"\ndeclare -x USE=\"amd64\"\n",
        );
        // Unset (the `make.globals` default) restores `bzip2`.
        let scrubbed = scrub_bzip2_command(regen, "bzip2");
        assert_eq!(
            String::from_utf8(scrubbed).unwrap(),
            "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"bzip2\"\ndeclare -x USE=\"amd64\"\n",
        );
        // An unrelated value -- even a foreign compressor path -- is
        // copied verbatim, never forced to the configured one ...
        let foreign = b"declare -x PORTAGE_BZIP2_COMMAND=\"/opt/bin/pbzip2\"\n";
        assert_eq!(scrub_bzip2_command(foreign, "lbzip2"), foreign);
        // ... real's own spelling passes through byte-identical ...
        let real = b"declare -x PORTAGE_BZIP2_COMMAND=\"bzip2\"\n";
        assert_eq!(scrub_bzip2_command(real, "lbzip2"), real);
        // ... and a missing line stays missing (never invent it).
        let bare = b"declare -x FEATURES=\"sandbox\"\n";
        assert_eq!(scrub_bzip2_command(bare, "lbzip2"), bare);
        // Trailing line without a newline is preserved.
        let noeol =
            b"declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"'/tmp/w/bin/bzip2-passthrough'\"";
        assert_eq!(
            String::from_utf8(scrub_bzip2_command(noeol, "lbzip2")).unwrap(),
            "declare -x FEATURES=\"sandbox\"\ndeclare -x PORTAGE_BZIP2_COMMAND=\"lbzip2\"\n",
        );
    }

    /// Pure unit: the old-version probe parser takes only `OLDPF=`
    /// lines apart into `(pf, slot)`.
    #[test]
    fn parse_oldpf_probe_parses_only_probe_lines() {
        assert_eq!(
            parse_oldpf_probe("OLDPF=old-1.0 SLOT=0\nnoise\nOLDPF=old-0.9 SLOT=unknown\n"),
            vec![
                ("old-1.0".to_string(), "0".to_string()),
                ("old-0.9".to_string(), "unknown".to_string()),
            ]
        );
        assert!(parse_oldpf_probe("UNPACK=ok\n").is_empty());
        assert!(parse_oldpf_probe("OLDPF= SLOT=0\n").is_empty());
    }

    /// Backlog #171 review: when both the `environment.bz2` pull and
    /// the plain fallback pull fail, the staging warning carries the
    /// original bz2 error (no more silent staging failures).
    #[test]
    fn ship_old_hook_envs_warns_when_both_pulls_fail() {
        let tmp = regen_tmp("old-noenv");
        let root = tmp.join("root");
        let work = tmp.join("work");
        let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
        std::fs::create_dir_all(&oldvdb).unwrap();
        std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
        // Neither `environment.bz2` nor `environment` on the client.
        assert!(!oldvdb.join("environment.bz2").exists());
        assert!(!oldvdb.join("environment").exists());

        let unit = work.join("oldhook-2.0");
        std::fs::create_dir_all(&unit).unwrap();
        let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
        let shipment = ship_old_hook_envs(
            &ctx,
            None,
            unit.to_str().unwrap(),
            root.join("var/db/pkg").to_str().unwrap(),
            "dev-libs",
            "oldhook",
            "oldhook-2.0",
            "0",
        );
        assert!(shipment.shipped.is_empty(), "{:?}", shipment.shipped);
        assert_eq!(shipment.warnings.len(), 1, "{:?}", shipment.warnings);
        assert!(
            shipment.warnings[0].contains("oldhook-1.0"),
            "warning must name the pf: {:?}",
            shipment.warnings
        );
        assert!(
            shipment.warnings[0].contains("environment.bz2"),
            "warning must carry the original bz2 error: {:?}",
            shipment.warnings
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Saved env for an old instance: `declare -x` lines (the
    /// `env_val` form `run_old_hook` greps) plus hook functions writing
    /// to a client log.
    fn old_hook_environment() -> String {
        [
            "declare -x EAPI=\"8\"",
            "declare -x CATEGORY=\"dev-libs\"",
            "declare -x PN=\"oldhook\"",
            "declare -x PV=\"1.0\"",
            "declare -x PR=\"r0\"",
            "declare -x PVR=\"1.0\"",
            "declare -x P=\"oldhook-1.0\"",
            "declare -x PF=\"oldhook-1.0\"",
            "pkg_prerm() {",
            "\techo prerm-ok >> \"${EROOT}/var/lib/oldhook.log\"",
            "}",
            "pkg_postrm() {",
            "\techo postrm-ok >> \"${EROOT}/var/lib/oldhook.log\"",
            "}",
        ]
        .join("\n")
            + "\n"
    }

    /// PATH farm: symlinks to every `/usr/bin` + `/bin` entry EXCEPT
    /// `bzip2`, so `command -v bzip2` fails exactly like a bzip2-less
    /// client (plan §6 tool floor).
    fn farm_path_without_bzip2(dir: &std::path::Path) -> String {
        let farm = dir.join("farm");
        std::fs::create_dir_all(&farm).unwrap();
        for bindir in ["/usr/bin", "/bin"] {
            let Ok(entries) = std::fs::read_dir(bindir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == "bzip2" {
                    continue;
                }
                let link = farm.join(&name);
                if !link.exists() {
                    let _ = std::os::unix::fs::symlink(entry.path(), &link);
                }
            }
        }
        assert!(!farm.join("bzip2").exists());
        assert!(farm.join("bash").exists(), "the farm must resolve bash");
        farm.to_str().unwrap().to_string()
    }

    /// Drive `run_old_hook` straight through local bash (header exports
    /// plus the real `MERGE_HELPERS` plus both old phases) under a PATH
    /// with no `bzip2`. Returns stdout.
    fn run_old_hook_snippet(
        unit: &str,
        root: &str,
        work: &str,
        farm_path: &str,
        oldvdb: &str,
    ) -> String {
        // #326 S8.3: the old hooks resolve `PORTAGE_PYTHON` through
        // `$UNITBIN/portuale-python`, whose shim execs `$PORTUALE_BIN` --
        // the header of a real merge driver exports the resolved client
        // binary, so this snippet exports the built binary directly
        // (never the libtest harness, #326 D8).
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        let script = format!(
            concat!(
                "export PATH={farm}\n",
                "export PORTUALE_BIN={bin}\n",
                "command -v bzip2 >/dev/null 2>&1 && echo BZIP2-PRESENT || echo BZIP2-ABSENT\n",
                "UNIT={unit}\n",
                "ROOT={root}\n",
                "WORKDIR={work}\n",
                "UNITBIN=\"$UNIT/bin\"\n",
                "COLORMAP=''\n",
                "{helpers}\n",
                "run_old_hook {oldvdb} prerm\n",
                "echo \"RC=$?\"\n",
                "run_old_hook {oldvdb} postrm\n",
                "echo \"RC=$?\"\n",
            ),
            farm = sh_quote(farm_path),
            bin = sh_quote(exe.to_str().unwrap()),
            unit = sh_quote(unit),
            root = sh_quote(root),
            work = sh_quote(work),
            helpers = MERGE_HELPERS,
            oldvdb = sh_quote(oldvdb),
        );
        let output = std::process::Command::new("bash")
            .args(["-s"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child.stdin.take().unwrap().write_all(script.as_bytes())?;
                child.wait_with_output()
            })
            .expect("local bash runs run_old_hook");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            stdout.contains("BZIP2-ABSENT"),
            "bzip2 must be shadowed away:\n{stdout}\n{stderr}"
        );
        stdout
    }

    /// Backlog #171: with only `environment.bz2` in the old vdb entry
    /// and no client `bzip2` on `PATH`, `run_old_hook` still gets a
    /// sourceable env via the server-side decompress staged at
    /// `$UNIT/old-env/<pf>`.
    #[test]
    fn old_hook_uses_server_decompressed_env_without_client_bzip2() {
        let tmp = regen_tmp("old-shipped");
        let root = tmp.join("root");
        let work = tmp.join("work");
        std::fs::create_dir_all(root.join("var/lib")).unwrap();
        let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
        std::fs::create_dir_all(&oldvdb).unwrap();
        std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            oldvdb.join("oldhook-1.0.ebuild"),
            "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        )
        .unwrap();
        // Only `environment.bz2` -- no plain file (real's shape).
        let compressed =
            bzip2_compress(old_hook_environment().as_bytes()).expect("server bzip2 compresses");
        std::fs::write(oldvdb.join("environment.bz2"), &compressed).unwrap();

        // The new unit (hook runtime for the old phases).
        let unit = work.join("oldhook-2.0");
        std::fs::create_dir_all(&unit).unwrap();
        let status = std::process::Command::new("cp")
            .args(["-a"])
            .arg(crate::ebuild_phases::bin_dir().unwrap())
            .arg(unit.join("bin"))
            .status()
            .expect("cp -a bin");
        assert!(status.success());

        // Server stages the decompressed env (local transport = same
        // paths the client sees).
        let ctx = local_ctx(root.to_str().unwrap(), work.to_str().unwrap());
        let shipment = ship_old_hook_envs(
            &ctx,
            None,
            unit.to_str().unwrap(),
            root.join("var/db/pkg").to_str().unwrap(),
            "dev-libs",
            "oldhook",
            "oldhook-2.0",
            "0",
        );
        assert!(shipment.warnings.is_empty(), "{:?}", shipment.warnings);
        assert_eq!(shipment.shipped, vec!["oldhook-1.0".to_string()]);
        assert_eq!(
            std::fs::read_to_string(unit.join("old-env/oldhook-1.0")).unwrap(),
            old_hook_environment(),
        );

        let farm = farm_path_without_bzip2(&tmp);
        let stdout = run_old_hook_snippet(
            unit.to_str().unwrap(),
            root.to_str().unwrap(),
            work.to_str().unwrap(),
            &farm,
            oldvdb.to_str().unwrap(),
        );
        assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
        assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
            "prerm-ok\npostrm-ok\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171, legacy fallback: a vdb entry written by an older
    /// `mrg` (plain `environment`, no `environment.bz2`) still sources
    /// with no client `bzip2` and nothing staged.
    #[test]
    fn old_hook_falls_back_to_legacy_plain_environment() {
        let tmp = regen_tmp("old-legacy");
        let root = tmp.join("root");
        let work = tmp.join("work");
        std::fs::create_dir_all(root.join("var/lib")).unwrap();
        let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
        std::fs::create_dir_all(&oldvdb).unwrap();
        std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            oldvdb.join("oldhook-1.0.ebuild"),
            "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        )
        .unwrap();
        std::fs::write(oldvdb.join("environment"), old_hook_environment()).unwrap();
        assert!(!oldvdb.join("environment.bz2").exists());

        let unit = work.join("oldhook-2.0");
        std::fs::create_dir_all(&unit).unwrap();
        let status = std::process::Command::new("cp")
            .args(["-a"])
            .arg(crate::ebuild_phases::bin_dir().unwrap())
            .arg(unit.join("bin"))
            .status()
            .expect("cp -a bin");
        assert!(status.success());

        let farm = farm_path_without_bzip2(&tmp);
        let stdout = run_old_hook_snippet(
            unit.to_str().unwrap(),
            root.to_str().unwrap(),
            work.to_str().unwrap(),
            &farm,
            oldvdb.to_str().unwrap(),
        );
        assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
        assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
            "prerm-ok\npostrm-ok\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Backlog #171 review: an older-`mrg` entry with **both** files
    /// whose staging failed still runs its hooks on a bzip2-less
    /// client, from the plain `environment` final fallback.
    #[test]
    fn old_hook_falls_back_to_plain_when_bz2_present_without_client_bzip2() {
        let tmp = regen_tmp("old-both");
        let root = tmp.join("root");
        let work = tmp.join("work");
        std::fs::create_dir_all(root.join("var/lib")).unwrap();
        let oldvdb = root.join("var/db/pkg/dev-libs/oldhook-1.0");
        std::fs::create_dir_all(&oldvdb).unwrap();
        std::fs::write(oldvdb.join("SLOT"), "0\n").unwrap();
        std::fs::write(
            oldvdb.join("oldhook-1.0.ebuild"),
            "EAPI=8\nDESCRIPTION=\"synthetic old-hook probe\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        )
        .unwrap();
        // Both files (older `mrg`), nothing staged: the bz2 is
        // unreadable without a client bzip2, so the plain file wins.
        let compressed =
            bzip2_compress(old_hook_environment().as_bytes()).expect("server bzip2 compresses");
        std::fs::write(oldvdb.join("environment.bz2"), &compressed).unwrap();
        std::fs::write(oldvdb.join("environment"), old_hook_environment()).unwrap();

        let unit = work.join("oldhook-2.0");
        std::fs::create_dir_all(&unit).unwrap();
        let status = std::process::Command::new("cp")
            .args(["-a"])
            .arg(crate::ebuild_phases::bin_dir().unwrap())
            .arg(unit.join("bin"))
            .status()
            .expect("cp -a bin");
        assert!(status.success());

        let farm = farm_path_without_bzip2(&tmp);
        let stdout = run_old_hook_snippet(
            unit.to_str().unwrap(),
            root.to_str().unwrap(),
            work.to_str().unwrap(),
            &farm,
            oldvdb.to_str().unwrap(),
        );
        assert!(stdout.contains("OLDHOOK_prerm=0"), "stdout:\n{stdout}");
        assert!(stdout.contains("OLDHOOK_postrm=0"), "stdout:\n{stdout}");
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/oldhook.log")).unwrap(),
            "prerm-ok\npostrm-ok\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // --- #326 S8 e2e: install and reuse over the local transport --------
    //
    // Local transport installs nothing by design (D5) -- except through
    // `--remote-portuale-dir`, which names local paths the test owns, so
    // the ssh-shaped install path (mktemp, stream, chmod, digest, mv,
    // ping) runs without sshd. The shipped file is the built `portuale`
    // binary itself (via `--remote-portuale-binary`), so the install's
    // `__helper ping` answers the D8 token for real.

    /// The `portuale` binary next to the current test executable (the
    /// same layout `pretend.rs`'s remote test relies on; `cargo
    /// build --release -p portuale` before `cargo test --release`).
    fn s8_portuale_bin() -> std::path::PathBuf {
        let mut exe = std::env::current_exe().expect("current test exe");
        exe.pop();
        if exe.ends_with("deps") {
            exe.pop();
        }
        exe.push("portuale");
        exe
    }

    fn s8_fixtures() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    /// Full SHA-256 hex of a file.
    fn s8_sha256(path: &std::path::Path) -> String {
        use sha2::Digest as _;
        format!(
            "{:x}",
            sha2::Sha256::digest(std::fs::read(path).expect("test file reads"))
        )
    }

    /// One S8 e2e tree: a client root + workdir, a `--remote-portuale-dir`
    /// bin dir, and a minimal `server:` config (a main repo over an empty
    /// dir, like `placed_config_root_covers_both_placements`).
    struct S8Tree {
        tmp: std::path::PathBuf,
        root: std::path::PathBuf,
        work: std::path::PathBuf,
        bindir: std::path::PathBuf,
        clientetc: std::path::PathBuf,
    }

    impl S8Tree {
        fn fresh(tag: &str) -> Self {
            let tmp = TempDir::new(&format!("portuale-remote-s8-{tag}")).keep();
            let root = tmp.join("root");
            let work = tmp.join("work");
            let bindir = tmp.join("bin");
            std::fs::create_dir_all(root.join("var/db/pkg")).unwrap();
            std::fs::create_dir_all(&work).unwrap();
            std::fs::create_dir_all(&bindir).unwrap();
            let clientetc = tmp.join("clientetc");
            let repo = tmp.join("repo");
            std::fs::create_dir_all(clientetc.join("etc/portage/repos.conf")).unwrap();
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::write(
                clientetc.join("etc/portage/repos.conf/testrepo.conf"),
                format!(
                    "[DEFAULT]\nmain-repo = testrepo\n\n[testrepo]\nlocation = {}\n",
                    repo.display()
                ),
            )
            .unwrap();
            std::fs::write(
                clientetc.join("etc/portage/make.conf"),
                "FEATURES=\"sandbox\"\n",
            )
            .unwrap();
            Self {
                tmp,
                root,
                work,
                bindir,
                clientetc,
            }
        }
    }

    impl Drop for S8Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.tmp);
        }
    }

    /// Run the child `portuale mrg` trial flow against the tree: local
    /// transport, the built binary as the shipped file, one fixture
    /// binpkg. `extra` appends argv (e.g. `--pretend`).
    fn s8_run(tree: &S8Tree, extra: &[&str]) -> std::process::Output {
        let binpkg = s8_fixtures().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2");
        assert!(
            binpkg.is_file(),
            "fixture binpkg missing: {}",
            binpkg.display()
        );
        let shipped = s8_portuale_bin();
        let etc = format!("server:{}", tree.clientetc.display());
        let mut args = vec![
            "mrg",
            "--getbinpkgonly",
            "--remote-hostname",
            "localtest",
            "--remote-transport",
            "local",
            "--remote-root",
            tree.root.to_str().unwrap(),
            "--remote-workdir",
            tree.work.to_str().unwrap(),
            "--remote-etc-portage",
            etc.as_str(),
            "--remote-portuale-dir",
            tree.bindir.to_str().unwrap(),
            "--remote-portuale-binary",
            shipped.to_str().unwrap(),
            "--remote-binpkg",
            binpkg.to_str().unwrap(),
        ];
        args.extend(extra.iter().copied());
        std::process::Command::new(s8_portuale_bin())
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("PORTAGE_CONFIGROOT", &tree.clientetc)
            .env("ROOT", &tree.root)
            .env("DISTDIR", s8_fixtures().join("distfiles"))
            .env("PORTAGE_TMPDIR", tree.tmp.join("pt"))
            .env("FEATURES", "sandbox")
            .output()
            .expect("portuale mrg spawns")
    }

    fn s8_stdout(output: &std::process::Output) -> String {
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// #326 S8 e2e: the first run installs `portuale-<hash>` (with the
    /// `install-bin 0` line) and merges the unit; the second run reuses
    /// it with no `install-bin` line.
    #[test]
    fn s8_install_then_reuse_over_local_transport() {
        let tree = S8Tree::fresh("install-reuse");
        let bin = s8_portuale_bin();
        assert!(bin.is_file(), "run cargo build --release -p portuale first");
        let digest = s8_sha256(&bin);
        let short: String = digest.chars().take(16).collect();
        let installed = tree.bindir.join(format!("portuale-{short}"));

        let first = s8_run(&tree, &[]);
        let stdout = s8_stdout(&first);
        assert_eq!(
            first.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert!(
            stdout.contains("portuale-remote: install-bin 0"),
            "first run must report the install:\n{stdout}"
        );
        assert!(
            stdout.contains(">>> Remote merged dev-libs/binpkgrmpkg-1.0"),
            "the unit merges after the install:\n{stdout}"
        );
        assert_eq!(s8_sha256(&installed), digest);

        let second = s8_run(&tree, &[]);
        let stdout = s8_stdout(&second);
        assert_eq!(
            second.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&second.stderr)
        );
        assert!(
            !stdout.contains("install-bin"),
            "a reuse must print no install-bin line:\n{stdout}"
        );
        assert_eq!(s8_sha256(&installed), digest);
    }

    /// #326 S8 e2e (Q5/D5): a matching plain `portuale` placed first is
    /// reused -- nothing is installed next to it.
    #[test]
    fn s8_matching_plain_portuale_is_reused() {
        let tree = S8Tree::fresh("plain-match");
        let bin = s8_portuale_bin();
        let digest = s8_sha256(&bin);
        let short: String = digest.chars().take(16).collect();
        std::fs::copy(&bin, tree.bindir.join("portuale")).unwrap();

        let run = s8_run(&tree, &[]);
        let stdout = s8_stdout(&run);
        assert_eq!(
            run.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(
            !stdout.contains("install-bin"),
            "a matching plain binary installs nothing:\n{stdout}"
        );
        assert!(
            !tree.bindir.join(format!("portuale-{short}")).exists(),
            "no portuale-<hash> may appear next to a matching plain binary"
        );
    }

    /// #326 S8 e2e (Q5/D5): a non-matching plain `portuale` is left
    /// byte-identical, and `portuale-<hash>` is installed next to it.
    #[test]
    fn s8_mismatched_plain_portuale_is_left_alone() {
        let tree = S8Tree::fresh("plain-mismatch");
        let bin = s8_portuale_bin();
        let digest = s8_sha256(&bin);
        let short: String = digest.chars().take(16).collect();
        let plain = tree.bindir.join("portuale");
        std::fs::write(&plain, b"an operator's own install").unwrap();

        let run = s8_run(&tree, &[]);
        let stdout = s8_stdout(&run);
        assert_eq!(
            run.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(
            stdout.contains("portuale-remote: install-bin 0"),
            "the hashed binary still installs:\n{stdout}"
        );
        assert_eq!(
            std::fs::read(&plain).unwrap(),
            b"an operator's own install",
            "the mismatched plain binary must be byte-identical"
        );
        assert_eq!(
            s8_sha256(&tree.bindir.join(format!("portuale-{short}"))),
            digest
        );
    }

    /// #326 S8 e2e (D5): a corrupted `portuale-<hash>` (wrong digest) is
    /// replaced.
    #[test]
    fn s8_corrupted_hashed_binary_is_replaced() {
        let tree = S8Tree::fresh("corrupt-replace");
        let bin = s8_portuale_bin();
        let digest = s8_sha256(&bin);
        let short: String = digest.chars().take(16).collect();
        let hashed = tree.bindir.join(format!("portuale-{short}"));
        std::fs::write(&hashed, b"corrupted bytes").unwrap();

        let run = s8_run(&tree, &[]);
        let stdout = s8_stdout(&run);
        assert_eq!(
            run.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(
            stdout.contains("portuale-remote: install-bin 0"),
            "the corrupted binary is reinstalled:\n{stdout}"
        );
        assert_eq!(s8_sha256(&hashed), digest);
    }

    /// #326 S8 e2e (S8.2): `--pretend` installs nothing. Pretend stays
    /// local (`mrg` routes it to `pretend::run` with the remote options
    /// dropped), so the client bin dir is untouched even for a run that
    /// resolves. Setup mirrors `pretend.rs`'s local-transport remote
    /// test: a `file://` binhost plus the fixture client config.
    #[test]
    fn s8_pretend_installs_nothing() {
        fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let dst_path = dst.join(entry.file_name());
                let ft = entry.file_type().unwrap();
                if ft.is_symlink() {
                    let target = std::fs::read_link(entry.path()).unwrap();
                    std::os::unix::fs::symlink(target, dst_path).unwrap();
                } else if ft.is_dir() {
                    copy_tree(&entry.path(), &dst_path);
                } else if ft.is_file() {
                    std::fs::copy(entry.path(), dst_path).unwrap();
                }
            }
        }

        let fixtures = s8_fixtures();
        let base = TempDir::new("portuale-remote-s8-pretend").keep();
        let root = base.join("root");
        copy_tree(&fixtures.join("var"), &root.join("var"));
        let bindir = base.join("bin");
        std::fs::create_dir_all(&bindir).unwrap();
        let binhost = base.join("binhost");
        std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures.join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"),
        )
        .unwrap();
        let size = std::fs::metadata(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"))
            .unwrap()
            .len();
        std::fs::write(
            binhost.join("Packages"),
            format!(
                "TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\nDEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\nREPO: testrepo\nSIZE: {size}\nSLOT: 0\nUSE:\n"
            ),
        )
        .unwrap();
        let clientetc = base.join("clientetc");
        copy_tree(
            &fixtures.join("etc/portage"),
            &clientetc.join("etc/portage"),
        );
        let profile_link = clientetc.join("etc/portage/make.profile");
        let _ = std::fs::remove_file(&profile_link);
        std::os::unix::fs::symlink(fixtures.join("repo/profiles/default"), &profile_link).unwrap();
        std::fs::write(
            clientetc.join("etc/portage/binrepos.conf"),
            format!(
                "[tmpbinhost]\nsync-uri = file://{}\npriority = 1\n",
                binhost.display()
            ),
        )
        .unwrap();
        for entry in std::fs::read_dir(clientetc.join("etc/portage/repos.conf")).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_file() {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let mut lines = Vec::new();
            for line in text.lines() {
                let mut line = line.to_string();
                if line.trim_start().starts_with("location") && line.contains('=') {
                    let value = line
                        .split('=')
                        .nth(1)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if !value.is_empty() && !value.starts_with('/') {
                        line = line.replace(
                            value.as_str(),
                            fixtures.join(&value).display().to_string().as_str(),
                        );
                    }
                }
                lines.push(line);
            }
            lines.push(String::new());
            std::fs::write(entry.path(), lines.join("\n")).unwrap();
        }
        let output = std::process::Command::new(s8_portuale_bin())
            .args([
                "mrg",
                "--pretend",
                "--getbinpkgonly",
                "--remote-hostname",
                "localtest",
                "--remote-transport",
                "local",
                "--remote-root",
                root.to_str().unwrap(),
                "--remote-workdir",
                base.join("work").to_str().unwrap(),
                "--remote-etc-portage",
                &format!("server:{}", clientetc.display()),
                "--remote-portuale-dir",
                bindir.to_str().unwrap(),
                "dev-libs/binpkgrmpkg",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("PORTAGE_CONFIGROOT", &clientetc)
            .env("ROOT", &root)
            .env("PORTAGE_RUNNING_ROOT", &fixtures)
            .env("DISTDIR", fixtures.join("distfiles"))
            .env("PORTAGE_TMPDIR", base.join("pt"))
            .env("FEATURES", "sandbox")
            .output()
            .expect("portuale mrg spawns");
        let stdout = s8_stdout(&output);
        assert_eq!(
            output.status.code(),
            Some(0),
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let entries: Vec<String> = std::fs::read_dir(&bindir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            entries.is_empty(),
            "--pretend must install nothing: {entries:?}\n{stdout}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
