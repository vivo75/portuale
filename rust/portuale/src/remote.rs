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

use clap::ArgMatches;
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
];

fn get(matches: &ArgMatches, id: &str) -> Option<String> {
    matches.get_one::<String>(id).cloned()
}

/// Validate the `--remote-*` surface: `Ok(None)` = local mode,
/// `Ok(Some(ctx))` = remote mode, `Err(message)` = usage error (exit 2).
pub fn check_remote(matches: &ArgMatches) -> Result<Option<RemoteContext>, String> {
    let hostname = get(matches, "remote_hostname");
    let Some(hostname) = hostname else {
        if let Some(offender) = REMOTE_OPTION_IDS
            .iter()
            .find(|id| get(matches, id).is_some())
        {
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
        config_protect,
        config_protect_explicit,
        config_protect_mask,
        config_protect_mask_explicit,
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
        RemoteTransport::Local => std::process::Command::new("bash")
            .args(["-s"])
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

/// Run an arbitrary remote command (`tar`, …), not just `bash -s`.
/// stdout bytes come back to the caller (used by the config pull).
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
            argv.extend(remote_argv.iter().cloned());
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
    match ctx.transport {
        RemoteTransport::Local => {
            if let Some(parent) = std::path::Path::new(dest).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(local_path, dest)
                .map_err(|e| format!("{}: {e}", local_path.display()))?;
            Ok(())
        }
        RemoteTransport::Ssh => {
            use std::io::Write as _;
            let bytes =
                std::fs::read(local_path).map_err(|e| format!("{}: {e}", local_path.display()))?;
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
                .write_all(&bytes)
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
fn preflight_script(root: &str, workdir: &str) -> String {
    format!(
        r#"echo "PREFLIGHT=1"
echo "BASH_MAJOR=${{BASH_VERSINFO[0]}}"
echo "BASH_MINOR=${{BASH_VERSINFO[1]}}"
for t in tar mkdir rm cat chmod ln find grep sed cmp stat readlink id tail; do
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
    )
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
        "tar", "mkdir", "rm", "cat", "chmod", "ln", "find", "grep", "sed", "cmp", "stat",
        "readlink", "id", "tail",
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
pub fn run_remote_resolve(matches: &ArgMatches, ctx: RemoteContext, argv: Vec<String>) -> ExitCode {
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
    let values = match run_preflight_inner(&ctx, control) {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (failures, warnings) = evaluate_preflight(&ctx, &values);
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(&ctx);
    if !failures.is_empty() {
        return print_preflight_report(&ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(&ctx, &failures, &warnings, first_contact);
    // etc-portage placement → config root for the resolve.
    let pull_tmp;
    let config_dir: std::path::PathBuf = match &ctx.etc_portage {
        ConfigPlacement::Server(path) => std::path::PathBuf::from(path),
        ConfigPlacement::Client(path) => {
            pull_tmp = std::env::temp_dir().join(format!(
                "portuale-remote-etc-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            // The pulled tree is the *contents* of the client's
            // `/etc/portage`; re-root it as `<tmp>/etc/portage` so the
            // dir is a valid `PORTAGE_CONFIGROOT` (real-root layout).
            let pulled_portage = pull_tmp.join("etc/portage");
            if let Err(message) = pull_dir(&ctx, control, path, &pulled_portage) {
                eprintln!("{message}");
                return ExitCode::from(1);
            }
            pull_tmp.clone()
        }
    };
    let _config_guard = ConfigRootOverride::set(&config_dir);
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
    for entry in entries {
        let version = match &entry.outcome {
            PretendOutcome::AlreadyInstalled { .. } => continue,
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
) -> Result<(), String> {
    let binpkg_path = if entry.remote_binary {
        let (sync_uri, record) = portage_repo::find_remote_binpkg(
            &config.binrepos,
            root,
            &entry.category,
            &entry.package,
            version,
        )
        .ok_or_else(|| {
            format!(
                "{}/{}-{version}: not found in any binhost `Packages` index",
                entry.category, entry.package
            )
        })?;
        crate::emerge_getbinpkg::download_and_verify(
            &sync_uri,
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
    )?;
    record_server_ledger(server_ledger_base, &ctx.hostname, &ledger_entry)?;
    Ok(())
}

/// Slice-1 entry point: connect, preflight, report. Exit 0 = every hard
/// gate passed; 1 = a gate failed or the client is unreachable.
/// Slice 2+ entry point with `--remote-binpkg`: preflight, then bundle,
/// stream, unpack and verify one explicit binpkg file.
pub fn run_remote(ctx: &RemoteContext) -> ExitCode {
    let control_dir = control_dir();
    let control = control_dir.as_deref();
    // Before the first connection: afterwards the key is present even on
    // first contact, so this decision cannot be made later.
    let first_contact =
        ctx.strict_host_key_checking == StrictHostKeyChecking::AcceptNew && is_first_contact(ctx);
    let values = match run_preflight_inner(ctx, control) {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
    };
    let (failures, warnings) = evaluate_preflight(ctx, &values);
    if !failures.is_empty() {
        return print_preflight_report(ctx, &failures, &warnings, first_contact);
    }
    print_preflight_report(ctx, &failures, &warnings, first_contact);
    match &ctx.binpkg {
        None => ExitCode::from(0),
        Some(path) => run_bundle_stage(ctx, control, Path::new(path)),
    }
}

/// Connect and run the preflight script: parsed `KEY=VALUE` gates on
/// success, `mrg: …`-prefixed message on transport or command failure.
/// Human log lines (client stderr) print straight through on success.
fn run_preflight_inner(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
) -> Result<HashMap<String, String>, String> {
    let script = preflight_script(&ctx.root, &ctx.workdir);
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
    Ok(parse_kv(&String::from_utf8_lossy(&output.stdout)))
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

/// Unpack driver: byte-count gate, `tar -xf`, member + manifest sanity.
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
if ! tar -xf "$BUNDLE" -C {workdir}; then echo "UNPACK=tar-failed"; exit 1; fi
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
        let mut owners = std::collections::HashMap::new();
        let Ok(cats) = std::fs::read_dir(dir) else {
            return Self { owners };
        };
        for cat in cats.filter_map(|e| e.ok()) {
            let category = cat.file_name().to_string_lossy().into_owned();
            let Ok(pfs) = std::fs::read_dir(cat.path()) else {
                continue;
            };
            for pf in pfs.filter_map(|e| e.ok()) {
                let pfname = pf.file_name().to_string_lossy().into_owned();
                let contents = pf.path().join("CONTENTS");
                let Ok(text) = std::fs::read_to_string(&contents) else {
                    continue;
                };
                for path in contents_owned_paths(&text) {
                    owners
                        .entry(path)
                        .or_insert_with(|| (category.clone(), pfname.clone()));
                }
            }
        }
        Self { owners }
    }

    /// Owner `(category, pf)` of an absolute image path, if recorded.
    fn owner(&self, abspath: &str) -> Option<&(String, String)> {
        self.owners.get(abspath)
    }

    /// True when `pf` belongs to package `pn` (real `_pkgsplit`
    /// longest-trailing-version rule via `split_pf`).
    fn same_package(pf: &str, pn: &str) -> bool {
        crate::remote_bundle::split_pf(pf).is_some_and(|(name, _)| name == pn)
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
            let tmp = std::env::temp_dir().join(format!(
                "portuale-remote-vdb-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
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
/// pre-check before anything ships). Prints the stage report lines;
/// `Ok(cpv)` is the merged `category/package-version`.
fn run_binpkg_flow(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
    ledger: Option<&LedgerSpec>,
    repo_override: Option<&str>,
    shadow: Option<&VdbShadow>,
) -> Result<String, String> {
    let staging = std::env::temp_dir().join(format!(
        "portuale-remote-bundle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(message) = std::fs::create_dir_all(&staging) {
        return Err(format!("mrg: staging dir {}: {message}", staging.display()));
    }
    let staged = match crate::remote_bundle::build_bundle(
        binpkg_path,
        &staging,
        repo_override,
        &crate::binpkg::GpgVerify::from_env(),
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
    // Slice-4 merge (copy+vdb+replace) then new-postinst, non-fatal like
    // the local merge's own `_postinst_failure` rule.
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
    if staged.postinst_defined {
        let colormap = crate::color::phase_colormap_export();
        let workdir_parent = std::path::Path::new(&ctx.workdir)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/var/tmp".to_string());
        let script = phase_script(
            &unit_dir,
            &staged,
            "postinst",
            &ctx.root,
            &workdir_parent,
            &colormap,
        );
        match run_script_stdin(ctx, control, &script) {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                for line in stderr.lines().chain(stdout.lines()) {
                    println!("{line}");
                }
                let rc: i32 = parse_kv(&stdout)
                    .get("PHASE_postinst")
                    .and_then(|rc| rc.parse().ok())
                    .unwrap_or(-1);
                if output.status.success() && rc == 0 {
                    println!(">>> Remote postinst {}: ok", staged.manifest.cpv);
                } else {
                    println!(
                        ">>> Remote postinst {}: FAILED (exit {}) -- merge kept (real _postinst_failure)",
                        staged.manifest.cpv,
                        output.status.code().unwrap_or(-1)
                    );
                }
            }
            Err(message) => {
                println!(
                    ">>> Remote postinst {}: transport failed, merge kept: {message}",
                    staged.manifest.cpv
                );
            }
        }
    } else {
        println!(
            ">>> Remote postinst {}: none defined, skipped",
            staged.manifest.cpv
        );
    }
    println!(">>> Remote merged {}", staged.manifest.cpv);
    Ok(staged.manifest.cpv.clone())
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
    match run_binpkg_flow(ctx, control, binpkg_path, None, None, None) {
        Ok(_) => ExitCode::from(0),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

// --- Client merge (slice 4) ----------------------------------------------------

/// Shared `export` block for any hook run (new phases in `phase_script`,
/// old prerm/postrm inside the merge driver): path overrides with
/// caller-side values, everything else from the sourced saved env.
/// `unit_bin` is the shipped runtime dir (`$UNIT/bin`,
/// `$OLD_TMP/bin` symlink or copy -- callers decide).
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
            "export PORTAGE_PYTHON=/usr/bin/python\n",
            "export PORTAGE_COLORMAP={colormap}\n",
            "export PORTAGE_TMPDIR={tmpdir}\n",
            "export SANDBOX_LOG={temp}/sandbox.log\n",
            "export EBUILD_PHASE={phase} EMERGE_FROM=binary\n",
        ),
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
) -> String {
    // D keeps local's trailing slash.
    format!(
        concat!(
            "UNIT={unit}\n",
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
        ),
        phase = phase,
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
  if [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment.bz2" ] && command -v bzip2 >/dev/null 2>&1; then
    bzip2 -dc -- "$vdbdir/environment.bz2" > "$OTMP/temp/environment" || return 1
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
  export PORTAGE_PYTHON=/usr/bin/python
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
        chmod --reference "$src" "$dest" || mfail copy "chmod $apath"
        [ "$ROOTUID" = 0 ] && chown --reference "$src" "$dest" || true
      fi
      echo "dir $apath" >> "$NEWCONTENTS"
      ;;
    obj)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        if cmp -s "$src" "$dest"; then :;
        else dest=$(alloc_cfg "$dest"); fi
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # A symlink at dest would make `cp` follow it and clobber the
      # target: drop the link first (local removes before writing too).
      [ -L "$dest" ] && rm -f "$dest"
      cp -p "$src" "$dest" || mfail copy "$apath"
      chmod --reference "$src" "$dest" || mfail copy "chmod $apath"
      [ "$ROOTUID" = 0 ] && chown --reference "$src" "$dest" || true
      echo "obj $apath $md5 $mtime" >> "$NEWCONTENTS"
      ;;
    sym)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        cur=""; [ -L "$dest" ] && cur=$(readlink "$dest")
        [ "$cur" = "$target" ] || dest=$(alloc_cfg "$dest")
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      rm -f "$dest"
      ln -s "$target" "$dest" || mfail copy "symlink $apath"
      touch -h -r "$src" "$dest" 2>/dev/null || true
      if [ "$ROOTUID" = 0 ]; then chown -h --reference "$src" "$dest" 2>/dev/null || true; fi
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
# The plain hook environment ships only when the binpkg carried an
# `environment.bz2` (else there is nothing future hooks could source;
# the verbatim `build-info/*` copies above already kept the original).
if [ -f "$UNIT/environment" ]; then
  cp "$UNIT/environment" "$TMPVDB/environment" || mfail vdb "copy environment"
fi
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
    ))
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
    // Server-side vdb placement is the stateless degrade (plan §7): the
    // driver merges files only -- no vdb entry, no old hooks, and
    // fail-closed collisions.
    let (vdb, stateless) = match &ctx.vdb {
        ConfigPlacement::Client(path) => (path.clone(), false),
        ConfigPlacement::Server(_) => (
            format!("{}/var/db/pkg", ctx.root.trim_end_matches('/')),
            true,
        ),
    };
    let script = merge_script(
        unit_dir,
        staged,
        &ctx.root,
        &vdb,
        stateless,
        &ctx.config_protect,
        &ctx.config_protect_mask,
        ledger,
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

/// Run the staged phases in order, stopping at the first non-zero
/// (pretend/setup/preinst are all fatal -- local rule kept). Returns the
/// per-phase `ok` markers for the report line.
fn run_phases_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
) -> Result<Vec<String>, String> {
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
        missing.insert("ROOT_WRITABLE".to_string(), "no".to_string());
        missing.insert("WORKDIR".to_string(), "uncreatable".to_string());
        let (failures, _) = preflight_gates(&missing);
        assert_eq!(failures.len(), 4, "{failures:?}");

        // A missing vdb warns (slice-5 placement matrix owns the degrade),
        // never fails.
        let mut novdb = ok.clone();
        novdb.insert("VDB_DIR".to_string(), "no".to_string());
        let (failures, warnings) = preflight_gates(&novdb);
        assert!(failures.is_empty());
        assert_eq!(warnings.len(), 1);
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
        let tmp = std::env::temp_dir().join(format!("portuale-cp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
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
        let dir = std::env::temp_dir().join(format!(
            "portuale-remote-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
        let dir = std::env::temp_dir().join(format!(
            "portuale-remote-shadow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
        let tmp = std::env::temp_dir().join(format!(
            "portuale-remote-trunc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let staging = tmp.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let staged = crate::remote_bundle::build_bundle(
            &fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &staging,
            None,
            &crate::binpkg::GpgVerify::default(),
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

    /// Synthetic merge through the real driver: protect rename and
    /// fail-closed collision, no binpkg needed (unit staged by hand,
    /// `filemeta` via the real collector).
    fn synthetic_unit(
        tmp: &std::path::Path,
        files: &[(&str, &str)],
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

    fn local_ctx(root: &str, workdir: &str) -> RemoteContext {
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
        }
    }

    #[test]
    fn merge_driver_protects_a_modified_config() {
        let tmp = std::env::temp_dir().join(format!(
            "portuale-remote-protect-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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

    #[test]
    fn merge_driver_aborts_on_unowned_collision() {
        let tmp = std::env::temp_dir().join(format!(
            "portuale-remote-collide-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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

    /// Positive pretend dispatch through the real stack: a synthetic unit
    /// (hand-written ebuild + environment carrying `pkg_pretend`, the real
    /// shipped `bin/`) runs the generated phase script under local bash.
    /// Proves template + `ebuild.sh` + DEFINED_PHASES-agnostic dispatch;
    /// the ebuild/env being synthetic is the only unreal part.
    #[test]
    fn phase_script_runs_pkg_pretend_from_saved_env() {
        let tmp = std::env::temp_dir().join(format!(
            "portuale-remote-pretend-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
            .arg(crate::ebuild_phases::bin_dir())
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
}
