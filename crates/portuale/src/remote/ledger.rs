//! The server/client merge ledger.

use crate::remote::*;

/// The newest (last non-empty) line of a last-10 ledger file.
/// `None` for an absent/empty ledger.
pub(crate) fn newest_ledger_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(String::from)
}
