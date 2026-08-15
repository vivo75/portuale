//! Installed packages read from the VDB.

use crate::*;

/// One installed package in the vdb, `category/package-version` plus its recorded
/// main `SLOT`.
// Portage: vartree.dbapi.cpv_all, _pkg_str
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub category: String,
    pub package: String,
    pub version: String,
    pub slot: String,
}
