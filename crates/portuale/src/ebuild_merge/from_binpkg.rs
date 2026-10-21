//! Merging a binary package.

use crate::ebuild_merge::*;

/// Merge an already-downloaded binary package (`.tbz2` xpak or
/// `.gpkg.tar`) into `root`'s vdb: extract the image, copy it into
/// `${ROOT}` exactly as `merge_tree` does (CONFIG_PROTECT included),
/// write the vdb entry from the binpkg's metadata plus fresh `CONTENTS`,
/// and run `env_update()`/`ldconfig`.
///
/// All four install/remove `pkg_*` hooks run, each from a saved bash
/// environment and only when `DEFINED_PHASES` names it, so a binpkg that
/// defines none spawns no shell:
///   - `pkg_setup` -> `pkg_preinst` from the new binpkg's own
///     `environment.bz2` + `<pf>.ebuild`, before a single file is copied.
///   - per replaced same-slot version: `pkg_prerm`, remove files,
///     `pkg_postrm`, drop the vdb entry; a phase failure is logged, not
///     fatal.
///   - `pkg_postinst` from the new binpkg, after the vdb entry is live
///     and every replaced version is gone, before `env_update()`.
///
/// `collision-protect` / `protect-owned`, blocker exclusion (reading the
/// binpkg's already-USE-reduced `*DEPEND` files) and preserve-libs
/// handling all run, identical to `merge_after_install`.
///
/// Cuts, all deliberate: a binpkg (or replaced version) with no
/// `environment.bz2` / `<pf>.ebuild` gets no hooks; a different-slot
/// version is left untouched, skipping the preserve-libs /
/// reverse-dependency check.
/// What the binpkg hook closures read from `merge_binpkg`.
pub(crate) struct HookCtx<'a> {
    pub(crate) extracted_ebuild: &'a Option<PathBuf>,
    pub(crate) defined_phases: &'a str,
    pub(crate) saved_env: &'a Path,
    pub(crate) seeded: &'a std::cell::Cell<bool>,
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) replacing_versions: &'a String,
    /// The binary merge's repo for the hook env (`PORTAGE_REPO_NAME`),
    /// `None` when the binpkg carries no usable repo record.
    /// Portage: doebuild.py, `doebuild_environment`.
    pub(crate) repo_name: Option<&'a str>,
}

impl HookCtx<'_> {
    fn phase_defined(&self, p: &str) -> bool {
        let HookCtx { defined_phases, .. } = self;
        defined_phases.split_whitespace().any(|d| d == p)
    }

    /// `always`: run even when the ebuild does not define the phase.
    pub(crate) fn run_hook_ex(
        &self,
        phase: &str,
        always: bool,
        update_env: Option<&Path>,
    ) -> Result<i32, String> {
        let &HookCtx {
            extracted_ebuild,
            saved_env,
            seeded,
            root,
            portage_tmpdir,
            options,
            replacing_versions,
            repo_name,
            ..
        } = self;
        match &extracted_ebuild {
            Some(ebuild) if always || self.phase_defined(phase) => {
                let seed = !seeded.replace(true);
                crate::ebuild_phases::run_phase_from_saved_env(
                    crate::ebuild_phases::RunPhaseFromSavedEnvArgs {
                        ebuild_path: ebuild,
                        saved_env_bz2: saved_env,
                        seed,
                        phase,
                        root,
                        portage_tmpdir,
                        debug: options.debug,
                        config_root: &options.config_root,
                        shell: options.shell,
                        log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()),
                        update_env,
                        refresh_features: update_env.map(|_| options.features.as_str()),
                        replacing_versions: Some(replacing_versions.as_str()),
                        merge_type: Some("binary"),
                        replaced_by_version: None,
                        repo_name,
                    },
                )
            }
            _ => Ok(0),
        }
    }

    pub(crate) fn run_hook(&self, phase: &str) -> Result<i32, String> {
        self.run_hook_ex(phase, false, None)
    }
}

/// Portage: _emerge/Binpkg, vartree.py, dblink.treewalk
pub fn merge_binpkg(
    binpkg_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<MergeOutcome, String> {
    // Once `${T}/environment` exists, the calling environment is narrowed
    // to the whitelist, so harness-only vars never reach the regenerated
    // vdb env. Per-phase keys are added separately by
    // `run_phase_from_saved_env` and are unaffected.
    // Portage: config.py, filter_calling_env
    let filtered_options = {
        let build_env: Vec<(String, String)> = options
            .build_env
            .iter()
            .filter(|(k, _)| ebuild_phases::environ_whitelisted(k))
            .cloned()
            .collect();
        if build_env.len() == options.build_env.len() {
            None
        } else {
            let mut filtered = options.clone();
            filtered.build_env = build_env;
            Some(filtered)
        }
    };
    let options = filtered_options.as_ref().unwrap_or(options);

    // Peek the embedded metadata first (Portage knows the cpv before
    // extracting): the image lands straight in
    // `${PORTAGE_BUILDDIR}/image`, the `${D}` the hooks expect.
    let name = binpkg_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let meta = if name.ends_with(".gpkg.tar") {
        crate::binpkg::read_gpkg_metadata(binpkg_path)?
    } else {
        crate::binpkg::read_xpak_metadata(binpkg_path)?
    };
    let meta_get = |key: &str| -> Option<String> {
        meta.get(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let category = meta_get("CATEGORY")
        .ok_or_else(|| format!("{}: binpkg has no CATEGORY", binpkg_path.display()))?;
    let pf =
        meta_get("PF").ok_or_else(|| format!("{}: binpkg has no PF", binpkg_path.display()))?;
    // `PF` -> `PN` via the canonical PMS split. The earlier
    // hand-rolled `rsplit_once('-')` mis-split revisions like `r1`,
    // which broke the extracted-ebuild directory layout.
    let package = portage_dep::parse_candidate(&format!("{category}/{pf}"))
        .map(|c| c.package)
        .unwrap_or_else(|| pf.clone());
    // The vdb `SLOT` keeps the full `slot/sub_slot`; the merge below
    // only wants the main slot.
    let full_slot = meta_get("SLOT").unwrap_or_else(|| "0".to_string());
    let main_slot = full_slot.split('/').next().unwrap_or("0").to_string();
    let repo_name = meta_get("repository").or_else(|| meta_get("REPO"));
    let repository = repo_name
        .clone()
        .unwrap_or_else(|| "__unknown__".to_string());

    // `${PORTAGE_BUILDDIR}` = `${PORTAGE_TMPDIR}/portage/<cat>/<pf>`,
    // derived here the same way `compute_environment` derives it from
    // the extracted ebuild.
    let builddir = portage_tmpdir.join("portage").join(&category).join(&pf);
    if builddir.exists() {
        std::fs::remove_dir_all(&builddir).map_err(|e| format!("{}: {e}", builddir.display()))?;
    }
    let image = builddir.join("image");
    let build_info = builddir.join("build-info");
    crate::binpkg::extract_binpkg(
        binpkg_path,
        &image,
        &build_info,
        &options.gpg_verify,
        &XattrPolicy::from_options(options),
    )?;

    // "Store the md5sum in the vdb": the whole-file digest, so a later
    // `emerge -k` / index rebuild can tell a current instance from a
    // rebuilt one. Written into build-info so `populate_vdb_tmp` copies
    // it like every other file.
    // Portage: _emerge/Binpkg._start_task
    let binpkg_md5 = md5_hex(binpkg_path)?;
    std::fs::write(build_info.join("BINPKGMD5"), format!("{binpkg_md5}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BINPKGMD5").display()))?;

    // Hooks run from the extracted `<pf>.ebuild` + `environment.bz2`,
    // gated on `DEFINED_PHASES` so a binpkg defining neither spawns no
    // shell. Both files must be present (older binpkgs get no hooks).
    let defined_phases = meta_get("DEFINED_PHASES").unwrap_or_default();
    let saved_env = build_info.join("environment.bz2");
    let extracted_ebuild = {
        let src = build_info.join(format!("{pf}.ebuild"));
        if src.is_file() && saved_env.is_file() {
            let pkgdir = builddir.join("ebuild-src").join(&category).join(&package);
            std::fs::create_dir_all(&pkgdir).map_err(|e| format!("{}: {e}", pkgdir.display()))?;
            let dst = pkgdir.join(format!("{pf}.ebuild"));
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
            Some(dst)
        } else {
            None
        }
    };
    // `always`: run even when the ebuild does not define the phase.
    //
    // The `${T}/environment` is extracted once per package; every hook
    // then evolves that one file. Seed on the first hook that runs,
    // `false` afterwards -- re-seeding would wipe the earlier hooks'
    // variable mutations.
    let seeded = std::cell::Cell::new(false);
    // `REPLACING_VERSIONS` is computed before the hooks so the closure
    // can thread it into `pkg_preinst`/`pkg_postinst`; `merge_tree`
    // reuses the same value. Always threaded (empty on a fresh install),
    // like `_emerge/Binpkg` sets it unconditionally: the replace-var
    // phase gate lives centrally in `apply_eapi_phase_env`.
    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let replacing_versions =
        replacing_versions(&package, installed_instance.as_deref()).unwrap_or_default();
    let hooks = HookCtx {
        extracted_ebuild: &extracted_ebuild,
        defined_phases: &defined_phases,
        saved_env: &saved_env,
        seeded: &seeded,
        root,
        portage_tmpdir,
        options,
        replacing_versions: &replacing_versions,
        // The saved env alone never carries the repo (real re-derives it
        // from the binary record); without a usable record the hooks keep
        // today's behavior. Portage: doebuild.py, doebuild_environment
        repo_name: repo_name.as_deref(),
    };

    // `pkg_pretend` runs for every package in the merge list, binary
    // included; folded into the per-package flow here rather than as a
    // separate scheduler stage, like the source path runs `pretend`
    // inline before each build.
    // Portage: Scheduler._run_pkg_pretend
    let pretend_status = hooks.run_hook("pretend")?;
    if pretend_status != 0 {
        return Ok(MergeOutcome {
            status: pretend_status,
            postinst_failed: false,
        });
    }

    // `pkg_setup` runs right after extraction, before the merge.
    let setup_status = hooks.run_hook("setup")?;
    if setup_status != 0 {
        return Ok(MergeOutcome {
            status: setup_status,
            postinst_failed: false,
        });
    }

    // The `instprep` phase runs first on the extracted image, from the
    // binpkg's saved env. A binpkg without one gets no phase. Seed only
    // when no earlier hook seeded, to reuse the evolved env.
    // (see `ebuild_phases::run_instprep`).
    if let Some(ebuild) = &extracted_ebuild {
        // Seed here only when no earlier hook seeded.
        let seed_instprep = !seeded.replace(true);
        let instprep_status = ebuild_phases::run_instprep(ebuild_phases::RunInstprepArgs {
            ebuild_path: ebuild,
            saved_env_bz2: Some(&saved_env),
            seed: seed_instprep,
            root,
            portage_tmpdir,
            build_env: &options.build_env,
            debug: options.debug,
            config_root: &options.config_root,
            shell: options.shell,
            log_file: ebuild_phases::PhaseLog::capture(options.log_file.as_deref()),
            repo_name: hooks.repo_name,
        })?;
        if instprep_status != 0 {
            eprintln!("!!! instprep failed");
            return Ok(MergeOutcome {
                status: 1,
                postinst_failed: false,
            });
        }
    }

    // `preinst_mask` + `install_mask_dir` run before the collision check
    // and before any copy; the resolved mask also lands in `build-info`.
    apply_install_mask(&image, &build_info, options)?;

    // Same refusal as `merge_after_install`: the scan repeats for binary
    // packages.
    // Portage: vartree.py, dblink.treewalk (`paths_with_newlines`)
    let line_ending_paths = image_paths_with_line_endings(&image)?;
    if !line_ending_paths.is_empty() {
        let lines = refusal_lines(&format!("{category}/{pf}"), &line_ending_paths);
        if options.direct {
            MergeListing::new(true).eerror(&lines);
            return Ok(MergeOutcome {
                status: 1,
                postinst_failed: false,
            });
        }
        return Err(lines.join("\n"));
    }

    // The collision check runs before `pkg_preinst` and the file copy,
    // shared with `merge_after_install`. A binpkg carries no ebuild/repo,
    // so the blocker term reads the already-USE-reduced `*DEPEND`
    // build-info files directly; the registry is consulted
    // unconditionally.
    let MergeBinpkgFindCollisionsOut {
        plib_registry,
        collisions,
        symlink_collisions,
        plib_collisions,
        image_files,
        image_symlinks,
    } = merge_binpkg_find_collisions(MergeBinpkgFindCollisionsArgs {
        root,
        build_info: &build_info,
        image: &image,
        category: &category,
        package: &package,
        main_slot: &main_slot,
        options,
    })?;
    // Same abort condition as `merge_after_install`. The listing opens
    // before the abort, like `merge_after_install`.
    // Portage: vartree.py, dblink._collision_protect, dblink.treewalk
    let mut listing = MergeListing::new(options.direct);
    listing.collision_line(image_files + image_symlinks);
    let protect_owned_abort = options.protect_owned
        && !collisions.is_empty()
        && !find_owners(root, &collisions).is_empty();
    if !symlink_collisions.is_empty()
        || (options.collision_protect && !collisions.is_empty())
        || protect_owned_abort
    {
        let cpv = format!("{category}/{pf}");
        return Err(collision_message(
            root,
            &cpv,
            &collisions,
            &symlink_collisions,
        ));
    }

    // `>>> Merging` runs after the collision phase and before `preinst`.
    // Portage: vartree.py, dblink.treewalk
    listing.merging(&format!("{category}/{pf}"), root);

    // The `-MERGING-<pf>` temporary is created before `pkg_preinst` and
    // renamed into place after every replaced version is unmerged,
    // identical to `merge_after_install`.
    create_vdb_tmp(root, &category, &pf)?;
    let preinst_status = hooks.run_hook("preinst")?;
    if preinst_status != 0 {
        return Ok(MergeOutcome {
            status: preinst_status,
            postinst_failed: false,
        });
    }
    populate_vdb_tmp(PopulateVdbTmpArgs {
        root,
        category: &category,
        pf: &pf,
        build_info_dir: &build_info,
        slot: &full_slot,
        repository: &repository,
    })?;

    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let mut cfgfiledict = read_cfgfiledict(root);
    let mut contents = merge_tree(MergeTreeArgs {
        d: &image,
        root,
        category: &category,
        installed_instance_pf: installed_instance.as_deref(),
        protect_if_modified: options.protect_if_modified,
        config_protect: &options.config_protect,
        config_protect_mask: &options.config_protect_mask,
        noconfmem: options.noconfmem,
        cfgfiledict: &mut cfgfiledict,
        xattrs: &XattrPolicy::from_options(options),
        listing: &mut listing,
    })?;
    // Pre-replace-loop preserve-libs, identical to `merge_after_install`.
    let new_image_paths: BTreeSet<String> = portage_vdb::paths(&contents)
        .map(|(_, path)| path.to_string())
        .collect();
    merge_binpkg_finish(MergeBinpkgFinishArgs {
        root,
        category: &category,
        package: &package,
        pf: &pf,
        main_slot: &main_slot,
        new_image_paths: &new_image_paths,
        contents: &mut contents,
        cfgfiledict: &cfgfiledict,
        plib_collisions: &plib_collisions,
        plib_registry,
        builddir: &builddir,
        portage_tmpdir,
        options,
        hooks: &hooks,
        listing: &mut listing,
    })
}

/// What [`merge_binpkg_find_collisions`] hands back to its caller.
pub(crate) struct MergeBinpkgFindCollisionsOut {
    pub(crate) plib_registry: PlibRegistry,
    pub(crate) collisions: Vec<String>,
    pub(crate) symlink_collisions: Vec<String>,
    pub(crate) plib_collisions: BTreeMap<String, BTreeSet<String>>,
    pub(crate) image_files: usize,
    pub(crate) image_symlinks: usize,
}

/// The values [`merge_binpkg_find_collisions`] reads from its caller.
pub(crate) struct MergeBinpkgFindCollisionsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) build_info: &'a Path,
    pub(crate) image: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) options: &'a MergeOptions,
}

/// Look up the preserved-library registry and the collisions of the new image.
pub(crate) fn merge_binpkg_find_collisions(
    args: MergeBinpkgFindCollisionsArgs<'_>,
) -> Result<MergeBinpkgFindCollisionsOut, String> {
    let MergeBinpkgFindCollisionsArgs {
        root,
        build_info,
        image,
        category,
        package,
        main_slot,
        options,
    } = args;
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    let mut flat_deps: Vec<String> = Vec::new();
    for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
        if let Ok(d) = std::fs::read_to_string(build_info.join(dep_key)) {
            flat_deps.extend(d.split_whitespace().map(String::from));
        }
    }
    let blocked = blockers_from_flat_deps(root, &flat_deps);
    let (collisions, symlink_collisions, plib_collisions, image_files, image_symlinks) =
        find_collisions(FindCollisionsArgs {
            d: image,
            root,
            category,
            package,
            slot: main_slot,
            config_protect: &options.config_protect,
            config_protect_mask: &options.config_protect_mask,
            plib_inodes: &plib_inodes,
            blocked: &blocked,
        })?;
    Ok(MergeBinpkgFindCollisionsOut {
        plib_registry,
        collisions,
        symlink_collisions,
        plib_collisions,
        image_files,
        image_symlinks,
    })
}

/// The values [`merge_binpkg_finish`] reads from its caller.
pub(crate) struct MergeBinpkgFinishArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) new_image_paths: &'a BTreeSet<String>,
    pub(crate) contents: &'a mut String,
    pub(crate) cfgfiledict: &'a BTreeMap<String, String>,
    pub(crate) plib_collisions: &'a BTreeMap<String, BTreeSet<String>>,
    pub(crate) plib_registry: PlibRegistry,
    pub(crate) builddir: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) options: &'a MergeOptions,
    pub(crate) hooks: &'a HookCtx<'a>,
    pub(crate) listing: &'a mut MergeListing,
}

/// Preserve, replace, publish, postinst and elog of a binary-package merge.
pub(crate) fn merge_binpkg_finish(args: MergeBinpkgFinishArgs<'_>) -> Result<MergeOutcome, String> {
    let MergeBinpkgFinishArgs {
        root,
        category,
        package,
        pf,
        main_slot,
        new_image_paths,
        contents,
        cfgfiledict,
        plib_collisions,
        plib_registry,
        builddir,
        portage_tmpdir,
        options,
        hooks,
        listing,
    } = args;
    let mut preserve_paths = BTreeSet::new();
    if let Some((paths, old_contents_text)) =
        find_preserve_paths_for_merge(FindPreservePathsForMergeArgs {
            root,
            category,
            package,
            new_pf: pf,
            main_slot,
            new_image_paths,
            preserve_libs: preserve_libs_for_merge(
                options.preserve_libs,
                root,
                category,
                package,
                main_slot,
            ),
        })
    {
        let (injected, surviving) =
            inject_preserved_libs_into_contents(&old_contents_text, &paths, new_image_paths);
        contents.push_str(&injected);
        preserve_paths = surviving;
    }
    write_cfgfiledict(root, cfgfiledict)?;
    write_vdb_tmp_contents(root, category, pf, contents)?;

    // A preserved lib this version now provides is taken over from the
    // registry and stripped from the previous owner's `CONTENTS`.
    if !plib_collisions.is_empty() {
        let cpv = format!("{category}/{pf}");
        unregister_preserved_libs(root, &cpv, plib_registry, plib_collisions)?;
    }

    // Merge-then-unmerge while the new entry still sits in its temporary;
    // the just-preserved set travels with the replace loop.
    let replacement_preserved: BTreeMap<String, Vec<String>> = if preserve_paths.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(
            format!("{category}/{pf}"),
            preserve_paths.iter().cloned().collect(),
        )])
    };
    let replaced_same_slot = unmerge_replaced_same_slot(UnmergeReplacedSameSlotArgs {
        root,
        category,
        package,
        new_pf: pf,
        main_slot,
        scratch_dir: &builddir.join("unmerge-src"),
        portage_tmpdir,
        options,
        replacement_preserved: &replacement_preserved,
    })?;
    // Publish, then the post-replace-loop registration, identical to
    // `merge_after_install` (one commit on a database backend).
    publish_merged_entry(PublishMergedEntryArgs {
        root,
        category,
        pn: package,
        pf,
        main_slot,
        preserve_paths: &preserve_paths,
        replaced: &replaced_same_slot,
    })?;

    // `pkg_postinst` runs after the vdb entry is live and every replaced
    // version is gone, before `env_update()`. A non-zero exit is logged,
    // never fatal.
    //
    // `PORTAGE_UPDATE_ENV` regenerates the vdb env from the live,
    // merge-time environment (resolved `FEATURES`, no stale build-host
    // locals); `always` so it runs even with no `pkg_postinst`. No-op
    // when the binpkg carries no saved env.
    //
    // The live entry's own path, or a scratch copy on a database backend.
    let update_env = update_env_target(root, category, pf, &builddir.join("vdb-update-env"))?;
    let vdb_env_bz2 = &update_env.path;
    let postinst_status = hooks.run_hook_ex(
        "postinst",
        true,
        vdb_env_bz2.is_file().then_some(vdb_env_bz2.as_path()),
    )?;
    // `>>> <cpv> merged.`, right after a successful `postinst` — a
    // failed `postinst` prints nothing.
    // Portage: vartree.py, dblink.treewalk
    if postinst_status == 0 {
        listing.merged(&format!("{category}/{pf}"));
    }
    // A failed `pkg_postinst` still merges (the `_postinst_failure`
    // rule): elog it so the saved elog, the `Messages` summary and the
    // build log all carry the `FAILED postinst` line Portage prints
    // after the die block. Shared with the source merge. The log path is
    // the merge's own when it captures, else the build log Portage's
    // `Binpkg` always writes (a foreground run's hooks inherit the
    // terminal, so nothing has created it yet).
    // Portage: vartree.py, dblink.treewalk
    if postinst_status != 0 {
        let foreground_log;
        let log_file = if let Some(path) = options.log_file.as_deref() {
            Some(path)
        } else {
            let version = pf.strip_prefix(&format!("{package}-")).unwrap_or(pf);
            foreground_log = crate::emerge_build::build_log_path(
                portage_tmpdir,
                category,
                package,
                version,
                &crate::emerge_build::resolved_features(options),
            );
            Some(foreground_log.as_path())
        };
        record_postinst_failure(&builddir.join("temp"), log_file, postinst_status)?;
    }
    absorb_update_env(root, category, pf, &update_env)?;

    if !contents.is_empty() || !replaced_same_slot.is_empty() {
        env_update::run_env_update(root)?;
        // Prune preserved libs orphaned by this merge, identical to
        // `merge_after_install` (no include feed: the new entry is
        // already renamed into place).
        prune_unused_preserved_libs(PruneUnusedPreservedLibsArgs {
            root,
            unmerge_no_replacement: false,
            being_unmerged: &|_| false,
            exclude_cpvs: &[],
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
        })?;
    }

    // `_elog_process` before the builddir removal below: the hooks'
    // output lives in this builddir's `${T}/logging`, which the removal
    // would take with it. Empty for a binpkg with no saved env. Skipped
    // after a postinst failure: like the source merge, the end-of-run
    // `Messages` flush prints it once, after the `Failed to install`
    // block and the die-log replay (a mid-merge flush would print it
    // twice, and before the failure block).
    // Portage: vartree.py, dblink.treewalk (`_elog_process`)
    if postinst_status == 0 {
        crate::elog::process_batch(crate::elog::ProcessBatchArgs {
            logdir: &crate::elog::logdir(root),
            root_display: &root.display().to_string(),
            items: &[(format!("{category}/{pf}"), builddir.join("temp"))],
            phases: None,
            color: &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)),
            split_elog: feature_enabled(options, "split-elog"),
        });
    }

    // A postinst failure keeps the builddir (bug #704866), like the
    // source merge; the post-merge clean never runs for it either.
    if postinst_status == 0 {
        let _ = std::fs::remove_dir_all(builddir);
    }
    Ok(MergeOutcome {
        status: postinst_status,
        postinst_failed: postinst_status != 0,
    })
}
