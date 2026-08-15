//! depclean, prune and clean selection.

use crate::*;

/// One cp's `emerge --prune --nodeps` split: `best_version` is
/// `protected` (kept), `other_versions` (ascending) are all `selected`
/// for removal.
#[derive(Debug, Clone)]
pub struct PruneNodepsCp {
    pub category: String,
    pub package: String,
    pub best_version: String,
    pub other_versions: Vec<String>,
}
