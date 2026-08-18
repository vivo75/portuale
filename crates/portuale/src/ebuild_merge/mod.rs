//! Real merge/filesystem mutation: after the `install` phase chain,
//! run `pkg_preinst`, copy `${D}`'s files/dirs/symlinks into `${ROOT}`,
//! write a vdb entry, then run `pkg_postinst`.
//! Portage: lib/portage/dbapi/vartree.py, dblink.merge, treewalk, mergeme
//!
//! CONFIG_PROTECT (for `obj` and `sym`), `collision-protect`,
//! `protect-owned`, preserve-libs handling, blocker exclusion and
//! `config-protect-if-modified` are implemented; see the submodules.
//! `pkg_preinst`/`pkg_postinst` run via `run_single_phase`, not the
//! `actionmap_deps` chain, like `treewalk()` itself.
//!
//! Known gaps: orphaned-preserved-lib `scanelf`, privilege-dropping,
//! and parts of the `SLOT`/`repository` fallback chains. Merge order is
//! sorted by filename for determinism rather than `os.listdir()` order;
//! `CONTENTS` line order has no semantic meaning.

use crate::ebuild_phases;
mod collisions;
mod installed_instance;
mod options;
mod plib_registry;
mod plib_unmerge;
mod protect;
mod tree;
mod vdb_entry;

#[cfg(test)]
mod tests;
pub(crate) use collisions::*;
pub(crate) use installed_instance::*;
pub use options::*;
pub(crate) use plib_registry::*;
pub(crate) use plib_unmerge::*;
pub(crate) use protect::*;
pub(crate) use tree::*;
pub(crate) use vdb_entry::*;

use md5::{Digest, Md5};

use mrg_director::PackagesDb as _;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use std::os::unix::fs::{FileTypeExt, MetadataExt};

use std::path::{Path, PathBuf};

/// `token` in the merge's own `FEATURES`: the resolved incremental list
/// when set, else the raw `$FEATURES` process env. Kept private to the
/// merge path so its `noclean` gate and the phase env can never disagree.
pub(crate) fn feature_enabled(options: &MergeOptions, token: &str) -> bool {
    let env_features;
    let features: &str = if options.features.is_empty() {
        env_features = std::env::var("FEATURES").unwrap_or_default();
        &env_features
    } else {
        &options.features
    };
    features.split_whitespace().any(|t| t == token)
}

/// Arguments of [`run_merge_with_hook`].
pub struct RunMergeWithHookArgs<'a> {
    pub ebuild_path: &'a Path,
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub options: &'a MergeOptions,
    pub buildpkg: Option<&'a crate::ebuild_package::PackageOptions>,
    pub before_merge: Option<&'a dyn Fn()>,
}
