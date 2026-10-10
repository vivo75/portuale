//! The post-install merge driver and same-slot unmerge.

use crate::ebuild_merge::*;

pub(crate) fn merge_after_install(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    env: &ebuild_phases::Environment,
    options: &MergeOptions,
) -> Result<i32, String> {
    // Real `dblink.treewalk()` starts with the `instprep` phase, before
    // anything below -- see `ebuild_phases::run_instprep`.
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
        log_file: options.log_file.as_deref(),
    })?;
    if instprep_status != 0 {
        eprintln!("!!! instprep failed");
        return Ok(1);
    }

    // Real `dblink.treewalk()`'s own `self._installed_instance` slot
    // read (`vartree.py:4455-4498`): the **evaluated** slot comes from
    // `build-info/SLOT` (the install phase's `__dyn_install` wrote the
    // live `${SLOT}`, so a computed `SLOT="$(ver_cut 1)"` is already
    // evaluated there -- real `_eqawarn`s a divergence only), not from
    // parsing the raw ebuild text (which would hand back the literal
    // `$(ver_cut 1)` -- the #149 symptom). The full real chain:
    //
    // 1. read `build-info/SLOT`'s first line (empty/whitespace ~= read
    //    error; real tolerates a missing file as empty);
    // 2. empty -> fall back to the resolved `settings["SLOT"]` here
    //    (`options.build_env`'s `SLOT`, the emerge path's resolved
    //    metadata value) and `write_atomic` it back into the inforoot;
    // 3. still empty -> `!!! SLOT is undefined`, abort the merge;
    // 4. source builds only (`is_binpkg` is always false here -- the
    //    binary path reads `meta_get("SLOT")` instead): a *known*
    //    settings value that differs from the inforoot value gets a
    //    `_eqawarn` QA Notice. Guarded on the settings value being
    //    non-empty: real always carries a resolved `settings["SLOT"]`
    //    on a merge (`depend` has run), while portuale's fixture path
    //    can hand `merge_after_install` an empty `build_env` -- against
    //    that state an `Expected SLOT='', got '1'` notice would be noise.
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

    // Real `self._installed_instance` (`vartree.py:4409-4418`), computed
    // early -- before the vdb write below ever touches this exact
    // category/pf's own real `CONTENTS` -- see `installed_instance_pf`'s
    // own doc comment.
    // `installed_instance_pf`/`find_collisions`/`blocked_installed_
    // packages` all compare against `read_installed_slot`, which returns
    // the installed vdb `SLOT` file's *main* slot (`"0"` for a real
    // `"0/6"`): the merge must pass the main slot to them, while the vdb
    // write records the ebuild's full `slot/sub_slot`. Passing the full
    // slot here made every sub-slotted package (ncurses, glibc, ...)
    // look like it owned none of its own files, so `-e`/reinstall merges
    // aborted under `FEATURES=protect-owned` (L3 smoke finding).
    let main_slot = full_slot.split('/').next().unwrap_or("0");
    let installed_instance_pf =
        installed_instance_pf(root, &env.category, &env.split.pn, main_slot);
    // Real `dblink.treewalk()` sets `REPLACING_VERSIONS` right before
    // `pkg_preinst` (`vartree.py:4768-4771`) -- and real's general
    // `doebuild_environment` setter runs it for `postinst` too
    // (`doebuild.py:1317-1322`). See `replacing_versions`'s own doc.
    let replacing_versions = replacing_versions(&env.split.pn, installed_instance_pf.as_deref());
    let mut preinst_env = options.build_env.clone();
    if let Some(version) = &replacing_versions {
        preinst_env.push(("REPLACING_VERSIONS".to_string(), version.clone()));
    }

    // Real `dblink.treewalk()`'s `preinst_mask` + `install_mask_dir`
    // step, run before `_collision_protect` and before any file is
    // copied -- a masked path never reaches `${ROOT}` nor the vdb
    // `CONTENTS`. See `apply_install_mask`.
    apply_install_mask(&env.d(), &env.build_info(), options)?;

    // Real `merge()`'s own ordering: the collision-protect abort check
    // (`_collision_protect`) happens before `pkg_preinst` ever runs, not
    // after -- confirmed by reading it, the real `EbuildPhase(phase=
    // "preinst")` block sits strictly after the real `if abort: return
    // 1` check. The preserve-libs registry is consulted unconditionally
    // here too (real `_plib_registry` is never `None` in practice --
    // see this module's own doc comment), regardless of
    // `FEATURES=collision-protect`.
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    // Real `mypkglist = others_in_slot + blockers` -- see
    // `blocked_installed_packages`'s own doc comment for the full real
    // grounding (this is genuinely new machinery: `ebuild <file> merge`
    // has never resolved real config/USE at all before this).
    let blocked =
        blocked_installed_packages(root, &options.config_root, env, main_slot, &repository);
    let (collisions, symlink_collisions, plib_collisions) = find_collisions(FindCollisionsArgs {
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
    // Real `dblink.merge()`'s own abort condition (`vartree.py:4830-
    // 4838`, Python operator precedence: `collision_protect or
    // (protect_owned and owners)`): a checked symlink-over-directory
    // violation (file-target/dangling symlink over a real directory;
    // directory-target symlinks never reach `symlink_collisions` --
    // see `find_collisions`)
    // always aborts; otherwise `collision_protect` alone aborts on any
    // collision, but `protect_owned` alone only aborts when an actual
    // owning package was identified for at least one collision (real
    // "None of the installed packages claim the file(s)" case does
    // *not* abort under `protect_owned` alone). `find_owners` is only
    // computed here (a second time, alongside `collision_message`'s own
    // call) when `protect_owned` might actually need it -- matching real
    // `get_owners()` itself only running when `collision_protect or
    // protect_owned or symlink_collisions`.
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

    // Real `dblink.treewalk()`'s own order: the `-MERGING-<pf>` temporary
    // vdb entry is created before `pkg_preinst` runs (wiping a stale one
    // a killed previous merge left), populated after it, and only renamed
    // into place after every replaced same-slot version is unmerged --
    // `pkg_postinst` runs last and sees the live entry. See
    // `create_vdb_tmp`/`populate_vdb_tmp`/`publish_vdb_tmp`.
    // `run_single_phase` (not `run_commands`) since neither hook is part
    // of `install`'s own `actionmap_deps` chain (real `treewalk()` invokes
    // them directly, not through `doebuild()`).
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
        log_file: options.log_file.as_deref(),
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
    })?;
    // Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
    // block: the replaced same-slot instance's
    // still-needed libraries are selected now -- the new vdb entry sits
    // in its `-MERGING-<pf>` temporary (written by `populate_vdb_tmp`
    // above), so enumeration skips it and it reaches the linkage map
    // only through the explicit include feed inside
    // `find_preserve_paths_for_merge` (real `include_file=needed`) --
    // and their entries are carried
    // into the new package's own `CONTENTS`
    // (`_add_preserve_libs_to_contents`). The
    // record itself lands after the replace loop below
    // (`register_merge_preserved_libs`).
    let new_image_paths: BTreeSet<String> = contents
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
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

    // Real `dblink.treewalk()`'s replace loop: unmerge every same-slot
    // version this merge replaced while the new version's vdb entry still
    // sits in its `-MERGING-<pf>` temporary -- see
    // `unmerge_replaced_same_slot`. Real `treewalk()` order: *after* the
    // file copy, *before* the rename into place and `pkg_postinst` /
    // `env_update`. A same-cpv `Reinstall` finds nothing to unmerge (its
    // own live entry is untouched until `publish_vdb_tmp` below),
    // matching the pre-replace-loop behaviour.
    // The merge-side just-preserved set travels with the replace loop
    // (real `preserve_paths` into `dblink.unmerge`) so the post-unmerge
    // prune can see files no `NEEDED.ELF.2` indexes yet -- see
    // `linkage_owner_entries`.
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

    // Real `dblink.treewalk()`'s own post-replace-loop registration
    // (`vartree.py:5266-5272`) -- backlog #178: the one `cp:slot`
    // record is replaced by the merging package's own `(cpv, counter,
    // paths)`, so a second consecutive soname bump cannot keep a stale
    // path list.
    // After the rename into place, like real (backlog #183: the counter
    // is read back from the published entry). On a database backend the
    // publish, the deletion of the replaced entries and this registration
    // are one commit (see `publish_merged_entry`).
    publish_merged_entry(PublishMergedEntryArgs {
        root,
        category: &env.category,
        pn: &env.split.pn,
        pf: &env.split.pf,
        main_slot,
        preserve_paths: &preserve_paths,
        replaced: &replaced,
    })?;

    // Real `merge()`'s own ordering: `postinst` runs, but its own exit
    // status never gates anything after it ("It's stupid to bail out
    // here, so keep going regardless of phase return code") -- real
    // `env_update()` always runs next, as long as anything was actually
    // installed (real `if contents:`) or a replaced version was removed.
    //
    // Real `dblink.treewalk()` (`vartree.py:5334-5337`) sets
    // `PORTAGE_UPDATE_ENV=<dbpkgdir>/environment.bz2` before the
    // postinst phase of *every* merge, source included, so
    // `bin/phase-functions.sh:1072-1082` rewrites the vdb environment
    // from the hook's live environment through the filtered save
    // (`__save_ebuild_env --exclude-init-phases | __filter_readonly_variables
    // --filter-path --filter-sandbox --allow-extra-vars`). That rewrite is
    // what drops the install-time `build-info/environment` save's stray
    // globals (`f`, `x`) real never records (#45 P2a finding). The
    // filtered `${T}/environment` drops `A`, so it is re-supplied here
    // exactly as real `doebuild_environment` exports it
    // (`doebuild.py:585-594`: the use-reduced, unique distfile names;
    // real's `config.environ()` gives every phase that value).
    //
    // The path is the live entry's own file (`InstalledDb::entry_path`,
    // feat#157 N9; on `files` `<root>/var/db/pkg/<cat>/<pf>`, so bash
    // rewrites it in place exactly as before). A database backend has
    // no such path: a scratch copy in the build dir, stored back after
    // the phase (`update_env_target` / `absorb_update_env`, S4.1).
    let update_env = update_env_target(
        root,
        &env.category,
        &env.split.pf,
        &env.portage_builddir().join("vdb-update-env"),
    )?;
    let mut postinst_env = options.build_env.clone();
    if let Some(version) = &replacing_versions {
        postinst_env.push(("REPLACING_VERSIONS".to_string(), version.clone()));
    }
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
        log_file: options.log_file.as_deref(),
    })?;
    absorb_update_env(root, &env.category, &env.split.pf, &update_env)?;

    if !contents.is_empty() || !replaced.is_empty() {
        env_update::run_env_update(root)?;
        // Real `treewalk()`: "For gcc upgrades, preserved libs have to be
        // removed after the library path has been updated" -- a preserved
        // lib whose last consumer this merge just rebuilt is now orphaned
        // and gets deleted + unregistered (real `_prune_plib_registry()`).
        // No include feed: the new entry is already renamed into place,
        // so its own lines are enumerated (backlog #224).
        prune_unused_preserved_libs(PruneUnusedPreservedLibsArgs {
            root,
            unmerge_no_replacement: false,
            being_unmerged: &|_| false,
            exclude_cpvs: &[],
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
        })?;
    }

    // Real `dblink.merge()`: `self._elog_process()` runs here, after the
    // merge body and before the clean (which every caller runs once this
    // returns). Backlog #42: the post-merge clean removes `${T}`, so
    // this must not be deferred to a batch after the whole `emerge`
    // returns any more -- and real never deferred it either.
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

/// Real `dblink.treewalk()`'s replace loop (`vartree.py:5187-5219`):
/// while the *new* version's vdb entry still sits in its `-MERGING-<pf>`
/// temporary (it is renamed into place only after this returns), every
/// already-installed **same-slot** version of the same cp is removed --
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop its vdb entry). Each `pkg_prerm`/
/// `pkg_postrm` runs from *that* version's own vdb-stored
/// `environment.bz2` + `<pf>.ebuild` (`ebuild_phases::
/// run_phase_from_saved_env`, gated on its recorded `DEFINED_PHASES`) --
/// a version merged before portuale kept those files, or via a bare
/// `ebuild <file> merge` of an older build, has neither and its rm
/// hooks are skipped (documented degrade). Only the files the new
/// version does **not** itself own are deleted (`also_keep = [new_pf]`,
/// folded into real `others_in_slot`). A replace-loop phase failure is
/// logged, never fatal -- real `treewalk()` there is a literal
/// `# TODO: Check status and abort if necessary` that doesn't.
///
/// A *different*-slot version is left untouched (real slot semantics).
/// `scratch_dir` is where the extracted-from-vdb ebuild is laid out for
/// its phase run (`<scratch_dir>/<cat>/<pn>/<old_pf>.ebuild`, so
/// `compute_environment`'s own `<cat>/<pn>/<pf>.ebuild` path parse
/// works). Returns every replaced `PF` (empty when there was nothing to
/// replace) so the caller can fold it into its own `env_update` gate.
/// Shared by `merge_after_install` (source `emerge <atom>` / `ebuild
/// <file> merge`) and `merge_binpkg`.
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
    // Real merge-then-unmerge: `<package>-<digit...>` vdb-dir names, the
    // same shape `blocked_installed_packages` and `installed_instance_pf`
    // match; exclude the just-merged new entry. A `-MERGING-<pf>`
    // temporary can never match the `<package>-<digit...>` shape (real
    // package names never start with `-`), but skip it explicitly anyway:
    // readers never see in-progress entries, not even here.
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
    // Real `dblink.unmerge(needed=...)` (backlog #224): the replacing
    // package's own `NEEDED.ELF.2` lines travel with the replace loop
    // into `_prune_plib_registry`'s `_linkmap_rebuild(include_file=...)`,
    // because the new entry still sits in its `-MERGING-<pf>` temporary
    // and enumeration skips it. Read once here (the temporary is live
    // for the whole loop) and thread through every replaced instance's
    // own post-unmerge prune below.
    let replacement_needed = replacement_needed_entries(root, category, new_pf);
    // feat#157 S4.1: on a database backend the replaced entries are
    // deleted by the publishing commit (`publish_merged_entry`), not here,
    // so the old instance stays installed until the new one is. A later
    // iteration must still treat the instances already unmerged as gone
    // (`others_in_slot`), as it does on `files` where their directories
    // are removed in this loop.
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

    // Real `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm",
    // "postrm"))`: the superseded version's `pkg_prerm`/`pkg_postrm`
    // `elog`/`ewarn`/`eerror` output reaches the `echo`/`save`/
    // `save_summary` modules, exactly as `emerge -C` already does through
    // `execute_unmerge`. Each old PF's `${T}` is `unmerge_one_installed`'s
    // own `run_phase_from_saved_env` builddir
    // (`<PORTAGE_TMPDIR>/portage/<cat>/<pf>/temp`), which here only ever
    // holds `prerm`/`postrm` logs. Colour is resolved the same way every
    // non-graph path does (`NO_COLOR` / not-a-tty aware).
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
        crate::elog::ProcessBatchArgs { logdir: &crate::elog::logdir(root), root_display: &root.display().to_string(), items: &items, phases: Some(&["prerm", "postrm"]), color: &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)), split_elog: // The merge path's resolved list when one was set (#37 S3); the
        // standalone `ebuild <file> merge` fallback is the raw env.
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

/// Remove ONE already-installed version from the vdb -- real
/// `dblink.unmerge()` (`pkg_prerm` -> delete its files -> `pkg_postrm`)
/// then `dblink.delete()` (drop the vdb dir) -- for a single
/// `<category>/<pf>`. Both phase hooks run from *that version's own*
/// vdb-stored `environment.bz2` + `<pf>.ebuild`
/// (`ebuild_phases::run_phase_from_saved_env`, gated on its recorded
/// `DEFINED_PHASES`); a version installed before portuale kept those
/// files has neither and its rm hooks are skipped (documented degrade).
/// A phase failure is logged, never fatal -- real `treewalk()`'s replace
/// loop is a literal `# TODO: Check status and abort if necessary` that
/// doesn't, and real `emerge -C`'s own loop only aborts on the
/// file-removal core failing, not on a phase.
///
/// `also_keep` is folded into real `others_in_slot` so a path a
/// replacing version now owns is left in place -- empty for a standalone
/// `emerge -C`, `[new_pf]` for `treewalk()`'s replace loop.
/// `is_replacement` is real `unmerge_with_replacement` (see
/// `preserve_libs_on_unmerge`): set by `unmerge_replaced_same_slot`,
/// cleared by `pretend.rs`'s real `emerge -C`.
/// `scratch_dir` holds the ebuild extracted from the vdb for its phase
/// run (`<scratch_dir>/<cat>/<pn>/<pf>.ebuild`, so
/// `compute_environment`'s path parse works). Shared by
/// `unmerge_replaced_same_slot` and `pretend.rs`'s real `emerge -C`.
///
/// `backup` is `Some` only for the standalone removal paths with
/// `FEATURES=unmerge-backup` (real `dblink._pre_unmerge_backup`, run at
/// the very top of `dblink.unmerge()` -- before `pkg_prerm`, before a
/// single file is touched): a `quickpkg` of the still-installed package
/// into `$PKGDIR` (`ebuild_package::quickpkg_from_vdb`). A quickpkg
/// failure aborts this package's unmerge, real `unmerge()`'s own
/// `if retval != os.EX_OK: ... return retval`. `treewalk()`'s replace
/// loop passes `None` -- its own `_pre_merge_backup`/`downgrade-backup`
/// path is a documented cut.
///
/// `replacement_preserved` is real `preserve_paths` (owner -> paths
/// just preserved merge-side): threaded from the merge caller through
/// `unmerge_replaced_same_slot` into the post-unmerge prune (see
/// `ebuild_unmerge::unmerge_pkgfiles`); empty on the standalone path.
/// `replacement_needed` is real `include_file` (the replacing
/// package's own `NEEDED.ELF.2` lines -- see
/// `unmerge_replaced_same_slot`): same threading, same empty-on-
/// standalone rule.
///
/// `deferred_delete` (feat#157 S4.1) is `None` everywhere except the
/// replace loop on a database backend: then the entry is **not** deleted
/// here (the merge's publishing commit deletes it, see
/// `publish_merged_entry`), and the slice names the instances this loop
/// already unmerged, which are still installed in the database but must
/// not count as `others_in_slot` (on `files` their directories are gone
/// by now).
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
    // feat#157 S4.2: a standalone unmerge on a database backend collects
    // its D4 writes and commits them with the row's deletion (below).
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
/// `scratch_dir` as `<cat>/<pn>/<pf>.ebuild` so `compute_environment`'s
/// `<cat>/<pn>/<pf>.ebuild` path parse works (the vdb layout is
/// `<cat>/<pf>/<pf>.ebuild`). Errors if the vdb entry carries no saved
/// environment or ebuild (a package installed before portuale started
/// keeping them). Unlike `unmerge_one_installed`'s own internal hook
/// runner this does **not** gate on `DEFINED_PHASES` -- the caller
/// decides (real `emerge --config` runs `pkg_config` unconditionally,
/// real `doebuild(ebuildpath, "config", ...)`). Shared by
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
    // Both files go to bash / a copier by path (N9): the entry's own
    // directory on `files`; on a database backend a scratch copy of the
    // two files under `scratch_dir` (feat#157 S4.1; nothing is stored
    // back, the phase gets no `PORTAGE_UPDATE_ENV`).
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
        true, phase, root, portage_tmpdir, debug: options.debug, config_root: &options.config_root, shell: options.shell, log_file: options.log_file.as_deref(), update_env: // An unmerge's `prerm`/`postrm` never rewrites a vdb env (the
        // entry is on its way out).
        None, refresh_features: None, replacing_versions: // Real does not give an unmerge hook `REPLACING_VERSIONS`.
        None },
    )
}
