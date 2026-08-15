//! Blocker collection and resolution.

use crate::*;

/// A resolved blocker with the owner version its renderer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FiledBlocker {
    pub owner: (String, String),
    pub owner_version: String,
    pub conflict: BlockerConflict,
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
