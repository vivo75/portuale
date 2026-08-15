//! Graph entries and their edges.

use crate::*;

/// One installed package named in an `[old-ver]` column. `repo` is `"__unknown__"`
/// when absent. Portage: output.py, convert_myoldbest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRef {
    pub version: String,
    pub slot: String,
    pub sub_slot: String,
    pub repo: String,
}
