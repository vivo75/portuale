//! Preflight gates checked before a remote merge.

use crate::remote::*;

// --- Preflight -------------------------------------------------------------

/// Read-only gates, generated bash (no arrays, no `set -e` -- every gate
/// reports instead of aborting). `KEY=VALUE` on stdout, human log on
/// stderr. `@ROOT@`/`@WORKDIR@` are substituted server-side, pre-quoted.
/// `bin` is `(dirs, candidates)` from the client-binary search: per
/// candidate the full digest or `missing`, plus `uname -m` and each
/// directory's existence, writability and octal mode. `None` skips the
/// binary section (local transport without the override installs
/// nothing, so there is nothing to report).
pub(crate) fn preflight_script(
    root: &str,
    workdir: &str,
    bin: Option<(&[String], &[String])>,
) -> String {
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
pub(crate) fn parse_kv(stdout: &str) -> HashMap<String, String> {
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
/// the run; a missing vdb only warns (the placement matrix owns the
/// degrade -- see docs/remote-merge.md §7).
pub(crate) fn preflight_gates(values: &HashMap<String, String>) -> (Vec<String>, Vec<String>) {
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
pub(crate) fn clock_gate(
    client_time: Option<&str>,
    server_time: u64,
    max_skew: u64,
) -> Option<String> {
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

/// Connect and run the preflight script: parsed `KEY=VALUE` gates on
/// success, `mrg: …`-prefixed message on transport or command failure.
/// Human log lines (client stderr) print straight through on success.
/// The binary facts (`bin_preflight`) are computed before the first
/// connection, so an unreadable `--remote-portuale-binary` fails fast.
/// Returns the parsed values plus the binary facts the client-binary
/// gates decide on (`None` when the run skips those gates by design).
pub(crate) fn run_preflight_inner(
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
pub(crate) fn evaluate_preflight(
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
pub(crate) fn print_preflight_report(
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

// --- Bundle streaming --------------------------------------------------------

/// Unpack driver: byte-count gate, `tar -xpf`, member + manifest sanity.
/// Values baked in server-side, pre-quoted. The tarball itself arrived
/// earlier via `send_file` (`$WORKDIR/<pf>/bundle.tar`). `UNPACK=...` on
/// stdout, log on stderr; any gate prints its reason and exits 1.
pub(crate) fn unpack_script(workdir: &str, pf: &str, expected_bytes: u64) -> String {
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
