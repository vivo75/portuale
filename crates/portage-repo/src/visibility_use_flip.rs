//! USE-flip suggestions for autounmask.

use crate::*;

/// A package's declared `IUSE` (markers stripped) folded together with
/// the arch list, `use.mask ∪ use.force`, the literal
/// `build`/`bootstrap` flags and `iuse_effective`: the full domain for
/// contextual flag validity (wider than the `use_deps_satisfied` subset).
/// Portage: config.py, _get_implicit_iuse
pub(crate) fn implicit_iuse_set(iuse: &str, config: &portage_profile::Config) -> HashSet<String> {
    let mut iuse_set: HashSet<String> = iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    iuse_set.extend(config.archlist.iter().cloned());
    iuse_set.extend(config.use_mask.iter().cloned());
    iuse_set.extend(config.use_force.iter().cloned());
    iuse_set.insert("build".to_string());
    iuse_set.insert("bootstrap".to_string());
    // EAPI 5+ `check_required_use` also accepts `USE_EXPAND_IMPLICIT`
    // flags (`Config::iuse_effective`) the same way.
    iuse_set.extend(config.iuse_effective.iter().cloned());
    iuse_set
}

/// The parent-side `--autounmask-use` mechanism: unlike
/// `suggested_use_candidate` (which flips the candidate's flag), this
/// flips the requesting parent's flag for atoms whose use-dep was
/// originally conditional on the parent's USE (already evaluated away by
/// `enqueue_flat_deps`): would the re-evaluated atom resolve under the
/// flip?
///
/// Only parent-active violated conditionals count ([`violated_parent_flags`]).
/// Gated on: valid parent IUSE, no mask/force pin, satisfiable under the
/// flip, and no new REQUIRED_USE violation (an old one doesn't disqualify).
/// First viable candidate in descending version order wins.
/// Portage: lib/_emerge/depgraph.py, _show_unsatisfied_dep
///
/// Attached to the dependency's own `GraphEntry` (`parent_use_suggestion`)
/// rather than the parent's entry (no per-parent reasons list exists).
/// `(parent_category, parent_package, parent_version, [(flag, desired)])`.
pub type ParentUseSuggestion = (String, String, String, Vec<(String, bool)>);
