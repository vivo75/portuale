//! The post-install merge driver and same-slot unmerge.

use crate::ebuild_merge::*;

/// Arguments of [`unmerge_replaced_same_slot`].
pub(crate) struct UnmergeReplacedSameSlotArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
}

/// Arguments of [`unmerge_one_installed`].
pub(crate) struct UnmergeOneInstalledArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) also_keep: &'a [String],
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) backup: Option<&'a crate::ebuild_package::PackageOptions>,
    pub(crate) is_replacement: bool,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
    pub(crate) deferred_delete: Option<&'a [String]>,
}

/// Arguments of [`run_vdb_saved_env_phase`].
pub(crate) struct RunVdbSavedEnvPhaseArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) phase: &'a str,
    pub(crate) scratch_dir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
}

/// Run one phase for an already-installed `<category>/<pf>` straight from
/// its own vdb-stored `environment.bz2` + `<pf>.ebuild`, copied into
/// `scratch_dir` as `<cat>/<pn>/<pf>.ebuild` for the path parse. Errors
/// if the entry carries no saved environment or ebuild. Unlike
/// `unmerge_one_installed`'s hook runner this does not gate on
/// `DEFINED_PHASES` -- the caller decides. Shared by
/// `unmerge_one_installed`'s `pkg_prerm`/`pkg_postrm` and
/// `pretend.rs::run_config_action`'s `pkg_config`.
pub(crate) fn run_vdb_saved_env_phase(args: RunVdbSavedEnvPhaseArgs<'_>) -> Result<i32, String> {
    let RunVdbSavedEnvPhaseArgs {
        root,
        category,
        package,
        pf,
        phase,
        scratch_dir,
        portage_tmpdir,
        options,
    } = args;
    // The entry's own directory on `files`; on a database backend a
    // scratch copy under `scratch_dir` (nothing stored back).
    let vdb_dir = crate::ebuild_unmerge::entry_path_for_message(root, category, pf);
    let ebuild_name = format!("{pf}.ebuild");
    if !entry_file_is_regular(root, category, pf, "environment.bz2")
        || !entry_file_is_regular(root, category, pf, &ebuild_name)
    {
        return Err(format!(
            "{}: no saved build environment (installed before portuale kept one?)",
            vdb_dir.display()
        ));
    }
    let db = portage_vdb::for_root(root);
    let key = portage_vdb::EntryKey::new(category, pf);
    let vdb_dir = if db.entry_path(&key).is_some() {
        vdb_dir
    } else {
        let dir = scratch_dir.join("vdb-entry").join(category).join(pf);
        portage_vdb::materialize_files(
            db.as_ref(),
            &key,
            &["environment.bz2", ebuild_name.as_str()],
            &dir,
        )
        .map_err(|e| e.to_string())?;
        dir
    };
    let env = vdb_dir.join("environment.bz2");
    let ebuild = vdb_dir.join(&ebuild_name);
    let src_dir = scratch_dir.join(category).join(package);
    std::fs::create_dir_all(&src_dir).map_err(|e| format!("{}: {e}", src_dir.display()))?;
    let dst = src_dir.join(format!("{pf}.ebuild"));
    std::fs::copy(&ebuild, &dst).map_err(|e| format!("{}: {e}", ebuild.display()))?;
    crate::ebuild_phases::run_phase_from_saved_env(
        crate::ebuild_phases::RunPhaseFromSavedEnvArgs { ebuild_path: &dst, saved_env_bz2: &env, seed: // Each unmerge seeds its own (old version's) builddir once.
        true, phase, root, portage_tmpdir, debug: options.debug, config_root: &options.config_root, shell: options.shell, log_file: options.log_file.as_deref(), update_env: // An unmerge's `prerm`/`postrm` never rewrites a vdb env.
        None, refresh_features: None, replacing_versions: // No `REPLACING_VERSIONS` on an unmerge hook.
        None },
    )
}
