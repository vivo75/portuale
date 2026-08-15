//! Blocker collection and resolution.

use crate::*;

/// A blocker atom from one package's dependency strings, not yet matched --
/// collected during the BFS and resolved in one post-pass once the whole
/// graph's New/Upgrade set is known, so a match never depends on discovery
/// order.
pub(crate) struct PendingBlocker {
    pub(crate) atom_str: String,
    pub(crate) strong: bool,
    /// Whether the atom came from a build-time key (`DEPEND`/`BDEPEND`).
    /// Under `--buildpkgonly` Portage clears every blocker's uninstall edge
    /// unless it is hard (`!!`) *and* buildtime.
    pub(crate) buildtime: bool,
    pub(crate) target_category: String,
    pub(crate) target_package: String,
    pub(crate) owner_key: (String, String),
    pub(crate) owner_version: String,
    /// Whether the producing owner was walked as a merge. Refined from the
    /// owner's final outcome when the owner is in the graph.
    pub(crate) owner_merging: bool,
    /// Whether this blocker came from the all-installed-packages scan: that
    /// owner is always `nomerge`, so its state must never be overridden from
    /// a same-cp merge entry.
    pub(crate) owner_installed: bool,
}

/// A resolved blocker with the owner version its renderer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiledBlocker {
    pub owner: (String, String),
    pub owner_version: String,
    pub conflict: BlockerConflict,
}

/// Arguments of [`set_seed_holds_instance`].
pub(crate) struct SetSeedHoldsInstanceArgs<'a> {
    pub(crate) set_seed_atoms: &'a [String],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) sub_slot: &'a str,
}

/// The owner-side half of the "a digraph node with parents" test: whether the
/// blocker owner is in the required-set closure. The set is cp-keyed, which
/// is exact for this single-instance arm.
// Portage: depgraph.py, _validate_blockers (step 4)
pub(crate) fn owner_set_parent(
    owner_key: &(String, String),
    blocker_retry_closure: &HashSet<(String, String)>,
) -> bool {
    blocker_retry_closure.contains(owner_key)
}
