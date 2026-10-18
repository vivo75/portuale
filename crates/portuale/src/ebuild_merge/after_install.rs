//! The post-install merge driver and same-slot unmerge.

use crate::ebuild_merge::*;

pub(crate) fn merge_after_install(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    env: &ebuild_phases::Environment,
    options: &MergeOptions,
) -> Result<i32, String> {
    // `treewalk()` starts with the `instprep` phase.
    // Portage: vartree.py, dblink.treewalk
    let instprep_status = ebuild_phases::run_instprep(ebuild_phases::RunInstprepArgs {
        ebuild_path,
        saved_env_bz2: None,
        seed: false,
        root,
        portage_tmpdir,
        build_env: &options.build_env,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()),
    })?;
    if instprep_status != 0 {
        eprintln!("!!! instprep failed");
        return Ok(1);
    }

    // The evaluated slot comes from `build-info/SLOT` (written by
    // `__dyn_install`, so a computed `SLOT` is already evaluated),
    // falling back to the resolved settings value and back into the
    // inforoot; still empty aborts. A known settings value differing
    // from the inforoot value gets a QA Notice. Guarded on a non-empty
    // settings value so the fixture path (empty `build_env`) stays quiet.
    // Portage: vartree.py, treewalk
    let info_root_slot = std::fs::read_to_string(env.build_info().join("SLOT"))
        .ok()
        .and_then(|text| text.lines().next().map(str::to_string))
        .map(|line| line.trim().to_string())
        .unwrap_or_default();
    let settings_slot = options
        .build_env
        .iter()
        .find(|(name, _)| name == "SLOT")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let full_slot = if info_root_slot.is_empty() {
        if settings_slot.is_empty() {
            eprintln!("!!! SLOT is undefined");
            return Ok(1);
        }
        let _ = std::fs::write(env.build_info().join("SLOT"), format!("{settings_slot}\n"));
        settings_slot
    } else {
        if !settings_slot.is_empty() && info_root_slot != settings_slot {
            eprintln!("QA Notice: Expected SLOT='{settings_slot}', got '{info_root_slot}'");
        }
        info_root_slot
    };
    let repository = repository_name_for(&env.pkg_dir).unwrap_or_else(|| "__unknown__".to_string());

    // The installed instance is computed before the vdb write touches
    // this category/pf's `CONTENTS`.
    // The slot comparisons below use the main slot (`"0"` for `"0/6"`);
    // the vdb write records the full `slot/sub_slot`. Passing the full
    // slot here orphaned every sub-slotted package's files.
    let main_slot = full_slot.split('/').next().unwrap_or("0");
    let installed_instance_pf =
        installed_instance_pf(root, &env.category, &env.split.pn, main_slot);
    // `REPLACING_VERSIONS` is set for `pkg_preinst` (and `postinst`),
    // always threaded (empty on a fresh install), like `vartree.py`
    // `treewalk` sets it unconditionally: the replace-var phase gate
    // lives centrally in `apply_eapi_phase_env`.
    // Portage: vartree.py, treewalk; doebuild.py, doebuild_environment
    let replacing_versions =
        replacing_versions(&env.split.pn, installed_instance_pf.as_deref()).unwrap_or_default();
    let mut preinst_env = options.build_env.clone();
    preinst_env.push(("REPLACING_VERSIONS".to_string(), replacing_versions.clone()));
    // `A` is re-supplied (the filtered `${T}/environment` drops it),
    // like `postinst` below.
    // Portage: vartree.py, treewalk; doebuild.py, doebuild_environment
    if let Some(a) = source_distfiles(env, &preinst_env) {
        preinst_env.push(("A".to_string(), a));
    }

    // The install mask runs before the collision check and before any
    // file is copied; masked paths never reach `${ROOT}` nor `CONTENTS`.
    apply_install_mask(&env.d(), &env.build_info(), options)?;

    // The collision abort runs before `pkg_preinst`, and the
    // preserve-libs registry is consulted unconditionally.
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    // `mypkglist = others_in_slot + blockers`; standalone `ebuild
    // <file> merge` has no depgraph, so blockers resolve self-contained.
    let blocked =
        blocked_installed_packages(root, &options.config_root, env, main_slot, &repository);
    let (collisions, symlink_collisions, plib_collisions, image_files, image_symlinks) =
        find_collisions(FindCollisionsArgs {
            d: &env.d(),
            root,
            category: &env.category,
            package: &env.split.pn,
            slot: main_slot,
            config_protect: &options.config_protect,
            config_protect_mask: &options.config_protect_mask,
            plib_inodes: &plib_inodes,
            blocked: &blocked,
        })?;
    // Abort condition: a symlink-over-directory violation always aborts;
    // otherwise `collision_protect` aborts on any collision while
    // `protect_owned` aborts only when an owner was identified.
    // `find_owners` runs only when `protect_owned` might need it.
    // Portage: vartree.py, dblink.merge
    //
    // The listing opens here, before the abort: Portage's
    // `_collision_protect` prints the line first thing, even when the
    // merge then aborts. A disabled listing is a no-op (the emerge path).
    // Portage: vartree.py, dblink._collision_protect
    let mut listing = MergeListing::new(options.direct);
    listing.collision_line(image_files + image_symlinks);
    let protect_owned_abort = options.protect_owned
        && !collisions.is_empty()
        && !find_owners(root, &collisions).is_empty();
    if !symlink_collisions.is_empty()
        || (options.collision_protect && !collisions.is_empty())
        || protect_owned_abort
    {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        return Err(collision_message(
            root,
            &cpv,
            &collisions,
            &symlink_collisions,
        ));
    }

    // `>>> Merging` runs after the collision phase and before `preinst`.
    // Portage: vartree.py, dblink.treewalk
    listing.merging(&format!("{}/{}", env.category, env.split.pf), root);

    // The `-MERGING-<pf>` temporary is created before `pkg_preinst`,
    // populated after it, and renamed into place only after every
    // replaced same-slot version is unmerged; `pkg_postinst` runs last.
    // `run_single_phase` since neither hook is in the `install` chain.
    create_vdb_tmp(root, &env.category, &env.split.pf)?;
    let preinst_status = ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
        ebuild_path,
        phase: "preinst",
        root,
        portage_tmpdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &preinst_env,
        log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()),
    })?;
    if preinst_status != 0 {
        return Ok(preinst_status);
    }
    populate_vdb_tmp(PopulateVdbTmpArgs {
        root,
        category: &env.category,
        pf: &env.split.pf,
        build_info_dir: &env.build_info(),
        slot: &full_slot,
        repository: &repository,
    })?;

    let mut cfgfiledict = read_cfgfiledict(root);
    let mut contents = merge_tree(MergeTreeArgs {
        d: &env.d(),
        root,
        category: &env.category,
        installed_instance_pf: installed_instance_pf.as_deref(),
        protect_if_modified: options.protect_if_modified,
        config_protect: &options.config_protect,
        config_protect_mask: &options.config_protect_mask,
        noconfmem: options.noconfmem,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::from_options(options),
        listing: &mut listing,
    })?;
    // Pre-replace-loop preserve-libs: the replaced instance's
    // still-needed libraries are selected now (the new entry still sits
    // in its temporary) and carried into the new `CONTENTS`; the record
    // lands after the replace loop.
    let new_image_paths: BTreeSet<String> = portage_vdb::paths(&contents)
        .map(|(_, path)| path.to_string())
        .collect();
    let mut preserve_paths = BTreeSet::new();
    if let Some((paths, old_contents_text)) =
        find_preserve_paths_for_merge(FindPreservePathsForMergeArgs {
            root,
            category: &env.category,
            package: &env.split.pn,
            new_pf: &env.split.pf,
            main_slot,
            new_image_paths: &new_image_paths,
        })
    {
        let (injected, surviving) =
            inject_preserved_libs_into_contents(&old_contents_text, &paths, &new_image_paths);
        contents.push_str(&injected);
        preserve_paths = surviving;
    }
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_tmp_contents(root, &env.category, &env.split.pf, &contents)?;

    if !plib_collisions.is_empty() {
        let cpv = format!("{}/{}", env.category, env.split.pf);
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Replace loop: unmerge every same-slot version while the new entry
    // still sits in its temporary. The just-preserved set travels with
    // the loop into the post-unmerge prune.
    let replacement_preserved: BTreeMap<String, Vec<String>> = if preserve_paths.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(
            format!("{}/{}", env.category, env.split.pf),
            preserve_paths.iter().cloned().collect(),
        )])
    };
    let replaced = unmerge_replaced_same_slot(UnmergeReplacedSameSlotArgs {
        root,
        category: &env.category,
        package: &env.split.pn,
        new_pf: &env.split.pf,
        main_slot,
        scratch_dir: &env.portage_builddir().join("unmerge-src"),
        portage_tmpdir,
        options,
        replacement_preserved: &replacement_preserved,
    })?;

    // Post-replace-loop registration: the one `cp:slot` record is
    // replaced by the merging package's own entry. Publish, deletion of
    // replaced entries and registration commit together on a database
    // backend.
    publish_merged_entry(PublishMergedEntryArgs {
        root,
        category: &env.category,
        pn: &env.split.pn,
        pf: &env.split.pf,
        main_slot,
        preserve_paths: &preserve_paths,
        replaced: &replaced,
    })?;

    // `postinst` runs, but its exit never gates what follows. `A` is
    // re-supplied (the filtered `${T}/environment` drops it); the target
    // is the live entry's own file, or a scratch copy on a database
    // backend stored back after the phase.
    // Portage: vartree.py, treewalk; doebuild.py, doebuild_environment
    let finish_status = merge_after_install_finish(MergeAfterInstallFinishArgs {
        root,
        options,
        ebuild_path,
        portage_tmpdir,
        contents: &contents,
        replaced: &replaced,
        env,
        replacing_versions,
        listing: &mut listing,
    })?;
    Ok(finish_status)
}

/// The values [`merge_after_install_finish`] reads from its caller.
pub(crate) struct MergeAfterInstallFinishArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) ebuild_path: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) contents: &'a str,
    pub(crate) replaced: &'a [String],
    pub(crate) env: &'a ebuild_phases::Environment,
    pub(crate) replacing_versions: String,
    pub(crate) listing: &'a mut MergeListing,
}

pub(crate) fn merge_after_install_finish(
    args: MergeAfterInstallFinishArgs<'_>,
) -> Result<i32, String> {
    let MergeAfterInstallFinishArgs {
        root,
        options,
        ebuild_path,
        portage_tmpdir,
        contents,
        replaced,
        env,
        replacing_versions,
        listing,
    } = args;
    let update_env = update_env_target(
        root,
        &env.category,
        &env.split.pf,
        &env.portage_builddir().join("vdb-update-env"),
    )?;
    let mut postinst_env = options.build_env.clone();
    postinst_env.push(("REPLACING_VERSIONS".to_string(), replacing_versions));
    postinst_env.push((
        "PORTAGE_UPDATE_ENV".to_string(),
        update_env.path.display().to_string(),
    ));
    if let Some(a) = source_distfiles(env, &postinst_env) {
        postinst_env.push(("A".to_string(), a));
    }
    let postinst_status = ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
        ebuild_path,
        phase: "postinst",
        root,
        portage_tmpdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &postinst_env,
        log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()),
    })?;
    // `>>> <cpv> merged.`, right after a successful `postinst` and
    // before `env_update` — a failed `postinst` prints nothing.
    // Portage: vartree.py, dblink.treewalk
    if postinst_status == 0 {
        listing.merged(&format!("{}/{}", env.category, env.split.pf));
    }
    absorb_update_env(root, &env.category, &env.split.pf, &update_env)?;

    if !contents.is_empty() || !replaced.is_empty() {
        env_update::run_env_update(root)?;
        // Prune preserved libs orphaned by this merge (no include feed:
        // the new entry is already renamed into place).
        // Portage: vartree.py, _prune_plib_registry
        prune_unused_preserved_libs(PruneUnusedPreservedLibsArgs {
            root,
            unmerge_no_replacement: false,
            being_unmerged: &|_| false,
            exclude_cpvs: &[],
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
        })?;
    }

    // `elog_process` runs here, after the merge body and before the
    // clean removes `${T}`; it must not be deferred to a batch after
    // the whole `emerge` returns.
    process_merge_elog(env, root, options);

    Ok(postinst_status)
}

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
/// `pkg_postrm`, then drop the vdb entry) — including a same-`pf`
/// instance on a reinstall. Each hook runs from that
/// version's own vdb-stored `environment.bz2` + `<pf>.ebuild`, gated on
/// its recorded `DEFINED_PHASES`; a version without them skips its
/// hooks. Only files the new version does not itself own are deleted; the
/// new version's ownership is read from its temporary entry, also on a
/// same-`pf` reinstall where the live entry is the copy being replaced.
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
    // Merge-then-unmerge: `<package>-<digit...>` vdb-dir names. A
    // `-MERGING-<pf>` temporary can never match, but would be skipped
    // explicitly anyway. The live entry carrying the new `pf` itself is
    // NOT skipped: on a same-version reinstall it is the replaced
    // instance, and Portage unmerges it too (`others_in_slot` holds a
    // dblink for the same cpv; only the merging `self` is skipped, by
    // identity). Skipping it dropped the old `pkg_prerm`/`pkg_postrm`.
    // Portage: vartree.py, dblink.treewalk
    let mut replaced: Vec<String> = Vec::new();
    if let Ok(names) = portage_vdb::for_root(root).category_entries(category) {
        for name in names {
            let is_this_cp = name.starts_with(&format!("{package}-"))
                && name[package.len() + 1..].starts_with(|c: char| c.is_ascii_digit());
            if !is_this_cp {
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
    // The version doing the replacing: every replaced same-slot hook
    // sees it as `REPLACED_BY_VERSION` (Portage: `dblink.treewalk`).
    let new_version = new_pf
        .strip_prefix(&format!("{package}-"))
        .unwrap_or(new_pf);
    // The replace loop's frame lines, once per replaced same-slot
    // instance: `>>> Safely unmerging …` before its unmerge (which runs
    // `pkg_prerm`, removes the files and runs `pkg_postrm`), then
    // `>>> Original …` after its vdb entry is deleted. A disabled
    // listing is a no-op (the emerge path).
    // Portage: vartree.py, dblink.treewalk
    let mut replace_listing = MergeListing::new(options.direct);
    for (i, old_pf) in replaced.iter().enumerate() {
        replace_listing.safely_unmerging();
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
            replaced_by_version: Some(new_version),
            replacement_preserved,
            replacement_needed: &replacement_needed,
            deferred_delete: defer_delete.then(|| &replaced[..i]),
        })?;
        replace_listing.original_unmerged();
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
    /// The replacing version for a replace-loop unmerge (`Some`): its
    /// hooks see `REPLACED_BY_VERSION`. `None` for a standalone unmerge.
    /// Portage: vartree.py, `dblink.treewalk` (sets it) vs `dblink.unmerge`.
    pub(crate) replaced_by_version: Option<&'a str>,
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
        replaced_by_version,
        replacement_preserved,
        replacement_needed,
        deferred_delete,
    } = args;
    let unmerge_options = crate::ebuild_unmerge::UnmergeOptions {
        debug: options.debug,
        shell: options.shell,
        config_protect: options.config_protect.clone(),
        config_protect_mask: options.config_protect_mask.clone(),
        unmerge_orphans: options.unmerge_orphans,
        uninstall_ignore: options.uninstall_ignore.clone(),
        config_root: options.config_root.clone(),
        already_unmerged: deferred_delete.map(<[String]>::to_vec).unwrap_or_default(),
        direct: options.direct,
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
            replaced_by_version,
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
    let mut unmerge_listing = MergeListing::new(unmerge_options.direct);
    let elog_lines = crate::ebuild_unmerge::unmerge_pkgfiles_into(
        crate::ebuild_unmerge::UnmergePkgfilesIntoArgs {
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
            listing: &mut unmerge_listing,
        },
    )?;
    crate::ebuild_unmerge::emit_unmerge_elog(root, category, pf, &elog_lines);
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
    /// The replacing version for a replace-loop unmerge (`Some`): the
    /// hook sees `REPLACED_BY_VERSION`. `None` for a standalone unmerge
    /// (and for `pkg_config`, which is not a replacement either).
    pub(crate) replaced_by_version: Option<&'a str>,
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
        replaced_by_version,
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
        true, phase, root, portage_tmpdir, debug: options.debug, config_root: &options.config_root, shell: options.shell, log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()), update_env: // An unmerge's `prerm`/`postrm` never rewrites a vdb env.
        None, refresh_features: None, replacing_versions: // No `REPLACING_VERSIONS` on an unmerge hook.
        None,
        // No `MERGE_TYPE`/`EMERGE_FROM` on an unmerge hook either: both
        // stay unset and `ebuild.sh` re-creates them readonly-empty.
        merge_type: None, replaced_by_version },
    )
}
