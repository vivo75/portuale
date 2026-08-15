//! When an installed package needs a reinstall (changed deps, slot, repo, USE).

use crate::*;

/// Splits a raw `SLOT` string into `(slot, sub_slot)`: `SLOT="main/sub"`,
/// `sub_slot` defaulting to the slot itself when no `/` is present. An
/// empty string (missing `SLOT` file/key) defaults to `("0", "0")`, the
/// same `"0"` fallback `list_candidates`/`installed_candidates` use.
/// Portage: portage.versions slot parsing.
pub(crate) fn split_slot(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    match raw.split_once('/') {
        Some((slot, sub_slot)) => (slot.to_string(), sub_slot.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}
