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

mod after_install;
mod collisions;
mod installed_instance;
mod options;
mod plib_merge;
mod plib_registry;
mod plib_unmerge;
mod protect;
mod tree;
mod vdb_entry;

#[cfg(test)]
mod tests;

pub(crate) use after_install::*;
pub(crate) use collisions::*;
pub(crate) use installed_instance::*;
pub use options::*;
pub(crate) use plib_merge::*;
pub(crate) use plib_registry::*;
pub(crate) use plib_unmerge::*;
pub(crate) use protect::*;
pub(crate) use tree::*;
pub(crate) use vdb_entry::*;

use crate::env_update;

use md5::{Digest, Md5};

use mrg_director::PackagesDb as _;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use std::os::unix::fs::{FileTypeExt, MetadataExt};

use std::path::{Path, PathBuf};

/// Whether `command` is the merge command this module implements.
pub fn is_merge_command(command: &str) -> bool {
    command == "merge"
}

/// Whether `command` is `qmerge` (routed to `run_qmerge`, which skips
/// the `install` phase).
pub fn is_qmerge_command(command: &str) -> bool {
    command == "qmerge"
}

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

/// Hand this package's `${T}/logging/<phase>` files to every configured
/// elog module. Runs per-package, before the post-merge clean removes
/// `${T}`.
///
/// `run_merge`/`run_qmerge` (through `merge_after_install`) and
/// `merge_binpkg` each call it once on their success path.
pub(crate) fn process_merge_elog(
    env: &ebuild_phases::Environment,
    root: &Path,
    options: &MergeOptions,
) {
    let cpv = format!("{}/{}", env.category, env.split.pf);
    crate::elog::process_batch(
        crate::elog::ProcessBatchArgs { logdir: &crate::elog::logdir(root), root_display: &root.display().to_string(), items: &[(cpv, env.t())], phases: None, color: &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)), split_elog: // The merge's resolved list when set; else the raw env.
        feature_enabled(options, "split-elog") },
    );
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

/// The `merge()` body shared by `merge` (after a fresh `install`) and
/// `qmerge` (assuming `install` already ran).
/// `configdict["pkg"]["A"]`: the use-reduced, unique distfile basenames
/// of `SRC_URI`, re-supplied here because the filtered `${T}/environment`
/// drops `A`. `None` outside a repo checkout, with no `SRC_URI`, or on
/// an unparsable one.
fn source_distfiles(
    env: &ebuild_phases::Environment,
    hook_env: &[(String, String)],
) -> Option<String> {
    let repo_root = crate::ebuild_phases::repo_root_for(&env.pkg_dir)?;
    let metadata =
        portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()?;
    // Portage sets `A` unconditionally (`""` for an SRC_URI-less
    // package), so empty yields `Some("")`, not `None`.
    let src_uri = metadata.get("SRC_URI").map(String::as_str).unwrap_or("");
    if src_uri.trim().is_empty() {
        return Some(String::new());
    }
    let use_value = hook_env
        .iter()
        .rev()
        .find(|(k, _)| k == "USE")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let use_flags: std::collections::HashSet<String> =
        use_value.split_whitespace().map(String::from).collect();
    let mut a: Vec<String> = Vec::new();
    for entry in
        portage_fetch::flatten_src_uri(src_uri, |negated, flag| use_flags.contains(flag) != negated)
            .ok()?
    {
        if !a.contains(&entry.filename) {
            a.push(entry.filename);
        }
    }
    Some(a.join(" "))
}
