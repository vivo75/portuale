//! The server/client merge ledger.

use crate::remote::*;

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
    pub(crate) fn line(&self) -> String {
        format!("{} {} {} {}", self.ts, self.repo, self.commit, self.cpv)
    }
}

/// Append one line, keeping the last 10 (rotation both sides share).
pub(crate) fn ledger_append(path: &std::path::Path, line: &str) -> Result<(), String> {
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
pub(crate) fn sanitize_hostname(hostname: &str) -> String {
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
pub(crate) fn client_ledger_file(ctx: &RemoteContext) -> String {
    format!("{}/var/db/remote-repos", ctx.root.trim_end_matches('/'))
}

/// The newest (last non-empty) line of a last-10 ledger file: the plan's
/// append-ordered `<unix-ts> <repo> <commit> <cpv>` line (`docs/
/// remote-merge.md` §8). `None` for an absent/empty ledger -- a client
/// (or server) that has never recorded provenance.
pub(crate) fn newest_ledger_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(String::from)
}

/// The server-side ledger record for one hostname:
/// `<base>/<hostname>` (last 10 kept). `None` when absent/empty.
pub(crate) fn server_latest_ledger(base: &std::path::Path, hostname: &str) -> Option<String> {
    std::fs::read_to_string(base.join(sanitize_hostname(hostname)))
        .ok()
        .and_then(|text| newest_ledger_line(&text))
}

/// Strict-mode provenance comparison (`--remote-require-ledger-match`,
/// plan §8): the client's and server's *newest* ledger line must agree --
/// both absent (fresh client + no server record) or byte-identical. Any
/// asymmetry is drift: a client that lost its ledger, was reimaged or
/// hand-edited, or was last merged by a different server.
pub(crate) fn ledger_lines_match(client: Option<&str>, server: Option<&str>) -> bool {
    match (client, server) {
        (None, None) => true,
        (Some(c), Some(s)) => c == s,
        _ => false,
    }
}

/// Read the client ledger's newest line over the transport (plan §8's
/// "provenance story"). Missing/empty reads as `None`; only a transport
/// or command failure is an `Err`.
pub(crate) fn read_client_latest_ledger(
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
pub(crate) fn check_ledger_match(
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
