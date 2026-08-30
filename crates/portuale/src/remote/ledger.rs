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

/// The newest (last non-empty) line of a last-10 ledger file.
/// `None` for an absent/empty ledger.
pub(crate) fn newest_ledger_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(String::from)
}
