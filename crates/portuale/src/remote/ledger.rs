//! The server/client merge ledger.

use crate::remote::*;

/// One repo-ledger line, shared by both sides:
/// `<unix-ts> <repo> <commit> <cpv>`.
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
/// `<pkgdir>/remote-ledger/<hostname>`, last 10 kept. Takes the ledger
/// *directory*; the hostname file hangs off it.
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

/// The newest (last non-empty) line of a last-10 ledger file.
/// `None` for an absent/empty ledger.
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

/// Strict-mode provenance comparison: the client's and server's
/// *newest* ledger line must agree (both absent counts as agreeing).
pub(crate) fn ledger_lines_match(client: Option<&str>, server: Option<&str>) -> bool {
    match (client, server) {
        (None, None) => true,
        (Some(c), Some(s)) => c == s,
        _ => false,
    }
}
