//! Eliminating unnecessary slot-operator rebuilds.

use crate::*;

/// `<version>` with a trailing `-r<digits>` revision suffix removed.
/// Returns the input unchanged when there is no such suffix.
pub(crate) fn strip_revision(version: &str) -> &str {
    match version.rsplit_once("-r") {
        Some((base, rev)) if !rev.is_empty() && rev.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => version,
    }
}
