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
    Ok(Some(RemoteContext {
        hostname,
        user: get(matches, "remote_user").filter(|u| !u.is_empty()),
        port,
        key_file: get(matches, "remote_key_file").filter(|k| !k.is_empty()),
        timeout_secs,
        ssh_args: get(matches, "remote_ssh_args").filter(|a| !a.is_empty()),
        strict_host_key_checking,
        max_clock_skew_secs,
        root: get(matches, "remote_root").unwrap_or_else(|| "/".to_string()),
        workdir: get(matches, "remote_workdir")
            .unwrap_or_else(|| "/var/tmp/portage-remote".to_string()),
        transport,
        binpkg: get(matches, "remote_binpkg").filter(|b| !b.is_empty()),
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
for t in tar mkdir rm cat chmod ln; do
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
    for tool in ["tar", "mkdir", "rm", "cat", "chmod", "ln"] {
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
fn run_bundle_stage(
    ctx: &RemoteContext,
    control: Option<&std::path::Path>,
    binpkg_path: &Path,
) -> ExitCode {
    if !binpkg_path.is_file() {
        eprintln!("mrg: --remote-binpkg {}: not found", binpkg_path.display());
        return ExitCode::from(1);
    }
    let staging = std::env::temp_dir().join(format!(
        "portuale-remote-bundle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    if let Err(message) = std::fs::create_dir_all(&staging) {
        eprintln!("mrg: staging dir {}: {message}", staging.display());
        return ExitCode::from(1);
    }
    let staged = match crate::remote_bundle::build_bundle(binpkg_path, &staging) {
        Ok(staged) => staged,
        Err(message) => {
            eprintln!("mrg: bundle build failed: {message}");
            let _ = std::fs::remove_dir_all(&staging);
            return ExitCode::from(1);
        }
    };
    let pf = staged
        .manifest
        .cpv
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
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
                eprintln!(
                    "mrg: creating unit dir failed (exit {}):\n{stderr}",
                    output.status.code().unwrap_or(-1)
                );
                let _ = std::fs::remove_dir_all(&staging);
                return ExitCode::from(1);
            }
            Err(message) => {
                eprintln!("{message}");
                let _ = std::fs::remove_dir_all(&staging);
                return ExitCode::from(1);
            }
        }
    }
    if let Err(message) = send_file(ctx, control, &staged.tarball, &dest_tar) {
        eprintln!("{message}");
        let _ = std::fs::remove_dir_all(&staging);
        return ExitCode::from(1);
    }
    let script = unpack_script(&ctx.workdir, &pf, staged.byte_count);
    let output = match run_script_stdin(ctx, control, &script) {
        Ok(output) => output,
        Err(message) => {
            eprintln!("{message}");
            let _ = std::fs::remove_dir_all(&staging);
            return ExitCode::from(1);
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
    if output.status.success() && unpack == "ok" {
        println!(
            ">>> Remote bundle {}: unpacked ({} bytes, slot {}, repo {})",
            staged.manifest.cpv, staged.byte_count, staged.manifest.slot, staged.manifest.repo,
        );
        ExitCode::from(0)
    } else {
        eprintln!(
            "mrg: bundle unpack failed (exit {}, UNPACK={unpack}):",
            code.unwrap_or(-1)
        );
        for line in stderr.lines().take(5) {
            eprintln!("mrg:   {line}");
        }
        ExitCode::from(1)
    }
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
        missing.insert("ROOT_WRITABLE".to_string(), "no".to_string());
        missing.insert("WORKDIR".to_string(), "uncreatable".to_string());
        let (failures, _) = preflight_gates(&missing);
        assert_eq!(failures.len(), 3, "{failures:?}");

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
        }
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
}
