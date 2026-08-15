//! Per-package index over `package.*` config atoms.

use crate::*;

/// Whether `entry` (a `package.mask`/`.unmask`/`.accept_keywords` line)
/// matches this candidate: `match_from_list` first, the bounded wildcard-atom
/// matcher (`*/*`, `category/*`, `*/package`) when it fails to parse.
pub(crate) fn matches_config_entry(
    entry: &str,
    candidate_str: &str,
    category: &str,
    package: &str,
) -> bool {
    if let Some(matches) = portage_dep::match_from_list(entry, &[candidate_str]) {
        return !matches.is_empty();
    }
    match portage_dep::parse_wildcard_atom(entry) {
        Some(w) => portage_dep::wildcard_atom_matches(&w, category, package),
        None => false,
    }
}
