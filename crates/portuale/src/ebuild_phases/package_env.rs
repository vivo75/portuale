//! `package.env` matching and its incremental variables.

use crate::ebuild_phases::*;

/// One key's incremental fold in layer order `[base-lower, pkg, calling-env]`:
/// `-*` clears, `-tok` removes; sorted and space-joined. The calling env folds
/// last, so its `-tok` prunes package.env tokens but not vice versa; re-applying
/// is idempotent, so `base_lower` may be the already-folded value.
/// Portage: config.py, regenerate
pub(crate) fn fold_package_env_incremental(
    base_lower: &str,
    pkg: &str,
    calling_env: &str,
) -> String {
    let mut set: std::collections::BTreeSet<String> =
        base_lower.split_whitespace().map(String::from).collect();
    for tok in pkg.split_whitespace().chain(calling_env.split_whitespace()) {
        if tok == "-*" {
            set.clear();
        } else if let Some(rest) = tok.strip_prefix('-') {
            set.remove(rest);
        } else {
            set.insert(tok.to_string());
        }
    }
    set.into_iter().collect::<Vec<_>>().join(" ")
}
