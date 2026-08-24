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

/// The replace loop: while the new version's vdb entry still sits in
/// its `-MERGING-<pf>` temporary, every already-installed same-slot
/// version of the same cp is removed (`pkg_prerm` -> delete files ->
/// `pkg_postrm`, then drop the vdb entry). Each hook runs from that
/// version's own vdb-stored `environment.bz2` + `<pf>.ebuild`, gated on
/// its recorded `DEFINED_PHASES`; a version without them skips its
/// hooks. Only files the new version does not itself own are deleted.
/// A phase failure is logged, never fatal.
///
/// A different-slot version is left untouched. `scratch_dir` holds the
/// ebuild extracted from the vdb for its phase run. Returns every
/// replaced `PF` for the caller's `env_update` gate. Shared by
/// `merge_after_install` and `merge_binpkg`.
/// Portage: vartree.py, dblink.treewalk, dblink.unmerge
pub(crate) fn unmerge_replaced_same_slot(
    args: UnmergeReplacedSameSlotArgs<'_>,
) -> Result<Vec<String>, String> {
    let UnmergeReplacedSameSlotArgs {
        root,
        category,
        package,
        new_pf,
        main_slot,
        scratch_dir,
        portage_tmpdir,
        options,
        replacement_preserved,
    } = args;
    // Merge-then-unmerge: `<package>-<digit...>` vdb-dir names; exclude
    // the just-merged entry. A `-MERGING-<pf>` temporary can never
    // match, but skip it explicitly anyway.
    let mut replaced: Vec<String> = Vec::new();
    if let Ok(names) = portage_vdb::for_root(root).category_entries(category) {
        for name in names {
            let is_this_cp = name.starts_with(&format!("{package}-"))
                && name[package.len() + 1..].starts_with(|c: char| c.is_ascii_digit());
            if !is_this_cp || name == new_pf {
                continue;
            }
            let version = &name[package.len() + 1..];
            if read_installed_slot(root, category, package, version).as_deref() == Some(main_slot) {
                replaced.push(name);
            }
        }
    }
    if replaced.is_empty() {
        return Ok(replaced);
    }

    let keep = [new_pf.to_string()];
    // The replacing package's `NEEDED.ELF.2` lines travel with the loop
    // into the post-unmerge prune (the new entry still sits in its
    // temporary and enumeration skips it).
    let replacement_needed = replacement_needed_entries(root, category, new_pf);
    // On a database backend the replaced entries are deleted by the
    // publishing commit, not here; later iterations still treat already
    // unmerged instances as gone.
    let defer_delete = portage_vdb::for_root(root).replace_in_publish();
    for (i, old_pf) in replaced.iter().enumerate() {
        unmerge_one_installed(UnmergeOneInstalledArgs {
            root,
            category,
            package,
            pf: old_pf,
            also_keep: &keep,
            scratch_dir,
            portage_tmpdir,
            options,
            backup: None,
            is_replacement: true,
            replacement_preserved,
            replacement_needed: &replacement_needed,
            deferred_delete: defer_delete.then(|| &replaced[..i]),
        })?;
    }

    // The superseded versions' `prerm`/`postrm` elog output reaches the
    // modules, like `emerge -C` does.
    let items: Vec<(String, PathBuf)> = replaced
        .iter()
        .map(|old_pf| {
            (
                format!("{category}/{old_pf}"),
                portage_tmpdir
                    .join("portage")
                    .join(category)
                    .join(old_pf)
                    .join("temp"),
            )
        })
        .collect();
    crate::elog::process_batch(
        crate::elog::ProcessBatchArgs { logdir: &crate::elog::logdir(root), root_display: &root.display().to_string(), items: &items, phases: Some(&["prerm", "postrm"]), color: &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)), split_elog: // The merge path's resolved list when set; else the raw env.
        options
            .features
            .split_whitespace()
            .any(|t| t == "split-elog") },
    );

    Ok(replaced)
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

/// Remove ONE already-installed version: `pkg_prerm`, delete its files,
/// `pkg_postrm`, then drop the vdb entry. Both hooks run from that
/// version's own vdb-stored `environment.bz2` + `<pf>.ebuild`, gated on
/// its recorded `DEFINED_PHASES`; a version without them skips its
/// hooks. A phase failure is logged, never fatal.
///
/// `also_keep` folds into `others_in_slot` (`[new_pf]` for the replace
/// loop, empty for standalone `emerge -C`). `is_replacement` is
/// `unmerge_with_replacement`. `scratch_dir` holds the ebuild extracted
/// from the vdb for its phase run. Shared by `unmerge_replaced_same_slot`
/// and `pretend.rs`'s `emerge -C`.
///
/// `backup` is `Some` only for standalone removals with
/// `FEATURES=unmerge-backup`: a `quickpkg` of the still-installed package
/// into `$PKGDIR` before anything is touched; a quickpkg failure aborts
/// this unmerge. The replace loop passes `None`.
///
/// `replacement_preserved` is `preserve_paths` (owner -> just-preserved
/// paths); `replacement_needed` is `include_file` (the replacing
/// package's `NEEDED.ELF.2` lines). Both empty on the standalone path.
///
/// `deferred_delete` is `None` except in the replace loop on a database
/// backend: the entry is not deleted here (the publishing commit deletes
/// it), and the slice names already-unmerged instances that must not
/// count as `others_in_slot`.
/// Portage: vartree.py, dblink.unmerge, dblink.treewalk
pub(crate) fn unmerge_one_installed(args: UnmergeOneInstalledArgs<'_>) -> Result<(), String> {
    let UnmergeOneInstalledArgs {
        root,
        category,
        package,
        pf,
        also_keep,
        scratch_dir,
        portage_tmpdir,
        options,
        backup,
        is_replacement,
        replacement_preserved,
        replacement_needed,
        deferred_delete,
    } = args;
    let unmerge_options = crate::ebuild_unmerge::UnmergeOptions {
        debug: options.debug,
        shell: options.shell,
        config_protect: options.config_protect.clone(),
        config_protect_mask: options.config_protect_mask.clone(),
        config_root: options.config_root.clone(),
        already_unmerged: deferred_delete.map(<[String]>::to_vec).unwrap_or_default(),
        ..Default::default()
    };

    if let Some(pkg_options) = backup {
        match crate::ebuild_package::quickpkg_from_vdb(crate::ebuild_package::QuickpkgFromVdbArgs {
            root,
            category,
            package,
            pf,
            scratch_ebuild_dir: scratch_dir,
            portage_tmpdir,
            options: pkg_options,
            config_protect: &options.config_protect,
            config_protect_mask: &options.config_protect_mask,
        }) {
            Ok(Some(path)) => {
                println!(">>> Building backup package for {category}/{pf}");
                println!(">>> Wrote {}", path.display());
            }
            Ok(None) => {}
            Err(e) => return Err(format!("!!! FAILED prerm: quickpkg: {e}")),
        }
    }

    let run_hook = |phase: &str| -> Result<i32, String> {
        let defined = read_entry_text(root, category, pf, "DEFINED_PHASES").unwrap_or_default();
        if !entry_file_is_regular(root, category, pf, "environment.bz2")
            || !entry_file_is_regular(root, category, pf, &format!("{pf}.ebuild"))
            || !defined.split_whitespace().any(|d| d == phase)
        {
            return Ok(0);
        }
        run_vdb_saved_env_phase(RunVdbSavedEnvPhaseArgs {
            root,
            category,
            package,
            pf,
            phase,
            scratch_dir,
            portage_tmpdir,
            options,
        })
    };

    let prerm_status = run_hook("prerm")?;
    if prerm_status != 0 {
        eprintln!("{category}/{pf}: FAILED prerm ({prerm_status}) -- unmerge continues");
    }
    // A standalone unmerge on a database backend batches its writes
    // with the row's deletion.
    let mut retire = (deferred_delete.is_none() && !is_replacement)
        .then(|| RetireWrites::for_root(root))
        .flatten();
    crate::ebuild_unmerge::unmerge_pkgfiles_into(crate::ebuild_unmerge::UnmergePkgfilesIntoArgs {
        root,
        category,
        pn: package,
        pf,
        also_keep,
        options: &unmerge_options,
        is_replacement,
        replacement_preserved,
        replacement_needed,
        retire: retire.as_mut(),
    })?;
    let postrm_status = run_hook("postrm")?;
    if postrm_status != 0 {
        eprintln!("{category}/{pf}: FAILED postrm ({postrm_status}) -- unmerge continues");
    }
    if deferred_delete.is_none() {
        crate::ebuild_unmerge::retire_entry(root, category, pf, retire)?;
    }
    Ok(())
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
