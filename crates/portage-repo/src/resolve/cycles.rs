//! Circular dependencies: hard-cycle detection, restart triggers, virtual
//! cycles and USE-fix suggestions.

use crate::*;

/// Splits `category/package-version` into its three parts (the last `-`
/// boundary whose right half verifies as a version).
pub fn split_cpv(cpv: &str) -> Option<(String, String, String)> {
    let (cat, rest) = cpv.split_once('/')?;
    let parts: Vec<&str> = rest.split('-').collect();
    for i in 1..parts.len() {
        let version = parts[i..].join("-");
        if portage_versions::ververify(&version) {
            return Some((cat.to_string(), parts[..i].join("-"), version));
        }
    }
    None
}
