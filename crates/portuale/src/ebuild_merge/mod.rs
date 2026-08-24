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

/// `merge` always runs after the `install` phase chain has completed.
/// Run here directly rather than requiring the caller to have run it
/// first; resume markers make re-running a done `install` cheap.
// Portage: vartree.py, dblink.merge; doebuild.py, actionmap_deps
pub fn run_merge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
    // `Some` for a source `emerge <atom>` under `FEATURES=buildpkg` /
    // `--buildpkg`: a binpkg of the fresh `${D}` is written into
    // `$PKGDIR` before the vdb merge. `ebuild <file> merge` passes
    // `None`: `buildpkg` is an `emerge`-flow concept with no
    // `bin/ebuild` equivalent.
    buildpkg: Option<&crate::ebuild_package::PackageOptions>,
) -> Result<i32, String> {
    run_merge_with_hook(RunMergeWithHookArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        before_merge: None,
    })
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

/// [`run_merge`], with a hook fired between the two halves: after the
/// build phases (and packaging) succeed, before the vdb merge begins.
/// A failed build never reaches the hook. `None` runs the fused
/// build+merge exactly like [`run_merge`].
// Portage: Scheduler.py, PackageMerge
pub fn run_merge_with_hook(args: RunMergeWithHookArgs<'_>) -> Result<i32, String> {
    let RunMergeWithHookArgs {
        ebuild_path,
        root,
        portage_tmpdir,
        options,
        buildpkg,
        before_merge,
    } = args;
    let status = ebuild_phases::run_commands(ebuild_phases::RunCommandsArgs {
        ebuild_path,
        commands: &["install"],
        root,
        portage_tmpdir,
        distdir: &options.distdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &options.build_env,
    })?;
    if status != 0 {
        return Ok(status);
    }
    if let Some(package_options) = buildpkg {
        // The binpkg's own `Packages` index `USE` field: the resolved
        // flag set the `install` phase above just ran with.
        let use_flags = options
            .build_env
            .iter()
            .find(|(k, _)| k == "USE")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        // `options.features` is already this call's fully resolved
        // value, so re-derive the binpkg-affecting fields from it to
        // keep them in step with what `install` just saw.
        let mut per_entry_package_options = package_options.clone();
        per_entry_package_options.set_resolved_features(&options.features);
        // Per-package `BUILD_ID` gate: the export follows the resolved
        // features while the layout stays run-wide.
        let per_entry_binpkg_multi_instance = options
            .features
            .split_whitespace()
            .any(|t| t == "binpkg-multi-instance");
        let status = crate::ebuild_package::package_after_install(
            crate::ebuild_package::PackageAfterInstallArgs {
                ebuild_path,
                root,
                portage_tmpdir,
                options: &per_entry_package_options,
                use_flags,
                per_entry_binpkg_multi_instance,
            },
        )?;
        if status != 0 {
            return Ok(status);
        }
    }
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    // The hook point: the build halves succeeded, the vdb merge has
    // not started.
    if let Some(hook) = before_merge {
        hook();
    }
    let merge_status = merge_after_install(ebuild_path, root, portage_tmpdir, &env, options)?;
    // After the success hooks and `env_update`, the `clean` phase
    // removes the builddir unless `FEATURES=noclean` (and never after
    // a postinst failure). The phase itself honors `keeptemp`/`keepwork`.
    // Portage: vartree.py, dblink.merge
    if merge_status == 0 && !feature_enabled(options, "noclean") {
        ebuild_phases::run_clean(ebuild_phases::RunCleanArgs {
            ebuild_path,
            root,
            portage_tmpdir,
            build_env: &options.build_env,
            debug: options.debug,
            config_root: &options.config_root,
            shell: options.shell,
            log_file: options.log_file.as_deref(),
        })?;
    }
    Ok(merge_status)
}

/// `qmerge`: skips the `install` phase entirely, assuming a prior
/// `install` (or `merge`) already populated `${D}` -- gated on the
/// `${PORTAGE_BUILDDIR}/.installed` marker. A missing marker is an
/// `Err` (exit 1 through `ebuild.rs` either way), for consistency with
/// this module's other "not in the expected state" checks.
///
/// **No post-merge `clean` here**: `qmerge` implies `noclean`. The
/// `emerge` scheduler path post-cleans separately in
/// `emerge_build::merge_one_built_entry`.
/// Portage: doebuild.py, mydo=qmerge
pub fn run_qmerge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    if !env.installed_marker().exists() {
        return Err("mydo=qmerge, but the install phase has not been run".to_string());
    }
    merge_after_install(ebuild_path, root, portage_tmpdir, &env, options)
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
