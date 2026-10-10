//! Merging a binary package.

use crate::ebuild_merge::*;

/// Merge an already-downloaded binary package (`.tbz2` xpak or
/// `.gpkg.tar`) into `root`'s vdb -- real portage's `_emerge/Binpkg`
/// task, narrowed. Extracts the binpkg image, copies it into `${ROOT}`
/// exactly as `merge_tree` does for a source build (CONFIG_PROTECT
/// included), writes the vdb entry from the binpkg's own metadata plus
/// the freshly-generated `CONTENTS`, and runs `env_update()`/`ldconfig`.
///
/// All four install/remove `pkg_*` phase hooks run, each from a saved
/// bash environment (`ebuild_phases::run_phase_from_saved_env`) and only
/// when the relevant `DEFINED_PHASES` names it (real `_defined_phases`),
/// so a binpkg that defines none -- the common case -- spawns no shell:
///   - `pkg_setup` -> `pkg_preinst` from the *new* binpkg's own
///     `environment.bz2` + `<pf>.ebuild`, before a single file is copied
///     (real `_emerge/Binpkg`: `setup` is an `EbuildPhase` right after
///     metadata extraction; `dblink.treewalk()` runs `preinst` before
///     `mergeme()`).
///   - for every same-slot version this merge replaces, real
///     `dblink.unmerge()` inside `treewalk()`'s replace loop:
///     `pkg_prerm`, then remove that version's files, then `pkg_postrm`,
///     then drop its vdb entry -- each phase from *that* version's own
///     vdb-stored `environment.bz2` + `<pf>.ebuild`. A phase failure
///     here is logged, not fatal (real "TODO: Check status and abort if
///     necessary" -- it doesn't).
///   - `pkg_postinst` from the new binpkg, after the vdb entry is live
///     and every replaced version is gone, before `env_update()`.
///
/// `FEATURES=collision-protect` / `protect-owned`, real blocker
/// exclusion (`mypkglist = others_in_slot + blockers` -- the blocker
/// term reads the binpkg's already-USE-reduced `*DEPEND` build-info
/// files, `blockers_from_flat_deps`), and preserve-libs collision
/// exclusion + `unregister_preserved_libs` all run now, identical to the
/// source `merge_after_install`.
///
/// **v1 cuts, all deliberate** (same "narrow the first slice, document
/// it" pattern as every other real-execution feature here):
///   - a binpkg (or a replaced version) carrying no `environment.bz2` /
///     `<pf>.ebuild` -- older, or built before portuale kept them --
///     gets no hooks: a documented degrade, not a fallback to
///     re-sourcing the ebuild.
///   - a *different*-slot installed version is left untouched (real slot
///     semantics); the replace also skips the preserve-libs /
///     reverse-dependency check `dblink.unmerge()` would otherwise do.
pub fn merge_binpkg(
    binpkg_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &MergeOptions,
) -> Result<i32, String> {
    // Real `config.environ()`'s `filter_calling_env` (`config.py:3275-
    // 3310`): once `${T}/environment` exists -- every binary-merge hook
    // has it, seeded from the archive -- the calling environment is
    // narrowed to `environ_whitelist` (bug #189417: a variable the
    // ebuild unsets must not leak back in), so a harness/portuale-only
    // var (`PORTAGE_RUNNING_ROOT`, `L1_SKIP_PORTAGE_UPGRADE`,
    // synthesized `GNUMAKEFLAGS`) never reaches the regenerated vdb
    // env. Programmatic per-phase keys (`EMERGE_FROM`, `MERGE_TYPE`,
    // `PORTAGE_UPDATE_ENV`, the `*_EXCLUDE` reads) are added separately
    // by `run_phase_from_saved_env` and are unaffected. L3 finding: the
    // L2 real-set control leg carries exactly those three vars without
    // this filter.
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

    // Peek the embedded metadata first -- real portage knows the cpv
    // (and so `${PORTAGE_BUILDDIR}`) before it extracts anything. This
    // lets the image land straight in `${PORTAGE_BUILDDIR}/image`, the
    // exact `${D}` a real `pkg_preinst`/`pkg_postinst` expects.
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
    // `PF` -> `PN`: real `catpkgsplit` (`portage_dep::parse_candidate`'s
    // own regex, the canonical PMS `<pkg>-<version>[-r<rev>]` split).
    // The earlier hand-rolled `rsplit_once('-')` only recognised a
    // version-final `PF` (`foo-1.2`) or `foo-1.2-r3` where the split
    // happened to land a digit last -- it fell through to `package = pf`
    // for `libgcrypt-1.12.3-r1` (last token `r1`), which then made the
    // extracted-ebuild dir `ebuild-src/<cat>/libgcrypt-1.12.3-r1/` and
    // `split_package` reject `libgcrypt-1.12.3-r1.ebuild` ("filename
    // doesn't start with the parent directory's own name").
    let package = portage_dep::parse_candidate(&format!("{category}/{pf}"))
        .map(|c| c.package)
        .unwrap_or_else(|| pf.clone());
    // Real vdb `SLOT` keeps the full `slot/sub_slot`; `merge_tree` only
    // wants the main slot for the installed-instance lookup.
    let full_slot = meta_get("SLOT").unwrap_or_else(|| "0".to_string());
    let main_slot = full_slot.split('/').next().unwrap_or("0").to_string();
    let repository = meta_get("repository")
        .or_else(|| meta_get("REPO"))
        .unwrap_or_else(|| "__unknown__".to_string());

    // Real `${PORTAGE_BUILDDIR}` = `${PORTAGE_TMPDIR}/portage/<cat>/<pf>`
    // -- `ebuild_phases::compute_environment` derives the same path from
    // the (extracted) ebuild, so this is exactly where a `pkg_preinst`/
    // `pkg_postinst` run will look for `${D}` / `${T}`.
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

    // Real `_emerge/Binpkg._start_task`: "Store the md5sum in the vdb."
    // It prefers the `MD5` field from the package index, else
    // `perform_md5(pkg_path)`. Neither xpak nor gpkg carries `BINPKGMD5`
    // inside itself -- it's the digest of the *whole* binpkg file, which
    // real records so a later `emerge -k` / index rebuild can tell a
    // still-current instance from a rebuilt one. Written into build-info
    // so `populate_vdb_tmp` copies it into the vdb like every
    // other build-info file.
    let binpkg_md5 = md5_hex(binpkg_path)?;
    std::fs::write(build_info.join("BINPKGMD5"), format!("{binpkg_md5}\n"))
        .map_err(|e| format!("{}: {e}", build_info.join("BINPKGMD5").display()))?;

    // Real `_emerge/Binpkg`: `pkg_setup`/`pkg_preinst`/`pkg_postinst`
    // run from the extracted `<pf>.ebuild` + the `bunzip2`'d
    // `environment.bz2` (see `ebuild_phases::run_phase_from_saved_env`).
    // Gated on `DEFINED_PHASES` (real `_defined_phases`) so a binpkg
    // that defines neither -- the common case -- spawns no shell at all.
    // Both files must be present (an older binpkg, or one built before
    // portuale kept them, gets no hooks -- a documented degrade).
    let defined_phases = meta_get("DEFINED_PHASES").unwrap_or_default();
    let phase_defined = |p: &str| defined_phases.split_whitespace().any(|d| d == p);
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
    // `always`: run the phase even when the ebuild does not define it --
    // real portage's `postinst` `EbuildPhase` always starts (`pkg_postinst`
    // defined or not) so its post-hook `PORTAGE_UPDATE_ENV` block can run.
    //
    // Real `_emerge/BinpkgEnvExtractor` extracts `${T}/environment` once
    // per package; every hook then evolves that one file. Seed on the
    // first hook that actually runs, `false` afterwards -- a per-hook
    // re-seed would wipe `pkg_setup`'s own variable mutations before
    // `pkg_preinst`/`pkg_postinst` (and the vdb env regeneration) see
    // them (#30 finding `l3-binpkg-hook-env-reseed`).
    let seeded = std::cell::Cell::new(false);
    // Real `self._installed_instance` (`vartree.py:4409-4418`) is computed
    // before `treewalk()`'s `pkg_preinst`, and `REPLACING_VERSIONS` is set
    // from it right before that hook (`:4768-4771`). Computed here so the
    // hook closure below can thread it into `pkg_preinst`/`pkg_postinst`;
    // `merge_tree` reuses the same value.
    let installed_instance = installed_instance_pf(root, &category, &package, &main_slot);
    let replacing_versions = replacing_versions(&package, installed_instance.as_deref());
    let run_hook_ex =
        |phase: &str, always: bool, update_env: Option<&Path>| -> Result<i32, String> {
            match &extracted_ebuild {
                Some(ebuild) if always || phase_defined(phase) => {
                    let seed = !seeded.replace(true);
                    crate::ebuild_phases::run_phase_from_saved_env(
                        crate::ebuild_phases::RunPhaseFromSavedEnvArgs {
                            ebuild_path: ebuild,
                            saved_env_bz2: &saved_env,
                            seed,
                            phase,
                            root,
                            portage_tmpdir,
                            debug: options.debug,
                            config_root: &options.config_root,
                            shell: options.shell,
                            log_file: options.log_file.as_deref(),
                            update_env,
                            refresh_features: update_env.map(|_| options.features.as_str()),
                            replacing_versions: replacing_versions.as_deref(),
                        },
                    )
                }
                _ => Ok(0),
            }
        };
    let run_hook = |phase: &str| run_hook_ex(phase, false, None);

    // Real `Scheduler._run_pkg_pretend` runs `pkg_pretend` for every
    // package in the merge list -- a binary package included: only the
    // `SRC_URI` fetch inside it is guarded by `if not x.built`, and the
    // status line even switches colour to `PKG_BINARY_MERGE` for the
    // built case. It skips EAPI 0-3 (where `pkg_pretend` does not
    // exist, so `DEFINED_PHASES` cannot list it anyway -- the
    // `phase_defined` gate in `run_hook` already covers that) and
    // anything not defining the phase. Real runs this as an up-front
    // pass over the whole list; portuale folds it into the per-package
    // flow here, exactly as the source path runs `pretend` inline
    // before each build rather than as a separate scheduler stage.
    let pretend_status = run_hook("pretend")?;
    if pretend_status != 0 {
        return Ok(pretend_status);
    }

    // Real `_emerge/Binpkg` order: `pkg_setup` (an `EbuildPhase`) runs
    // right after the metadata is extracted, before `unpack_contents` /
    // the merge.
    let setup_status = run_hook("setup")?;
    if setup_status != 0 {
        return Ok(setup_status);
    }

    // Real `EbuildMerge` -> `dblink.treewalk()`: the `instprep` phase
    // runs first, on the extracted image, from the binpkg's saved env
    // (see `ebuild_phases::run_instprep`). A binpkg without a saved env
    // gets no phase at all -- the same documented degrade as the hooks.
    if let Some(ebuild) = &extracted_ebuild {
        // `seeded` is false when no hook before this ran (a binpkg that
        // defines neither `pkg_pretend` nor `pkg_setup`): seed here so
        // `__dyn_instprep` still runs from the archive's env; otherwise
        // reuse the env `pkg_setup` evolved (see `run_instprep`'s own
        // `seed` doc comment).
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
            log_file: options.log_file.as_deref(),
        })?;
        if instprep_status != 0 {
            eprintln!("!!! instprep failed");
            return Ok(1);
        }
    }

    // Real `dblink.treewalk()`: `preinst_mask` + `install_mask_dir` run
    // before `_collision_protect` and before any file is copied -- so a
    // masked path (e.g. `/usr/share/info` under `FEATURES=noinfo`) never
    // reaches `${ROOT}` and never shows up in the vdb `CONTENTS`. The
    // resolved `INSTALL_MASK` also lands in `build-info`, so
    // `populate_vdb_tmp` copies it into the vdb entry.
    apply_install_mask(&image, &build_info, options)?;

    // Real `dblink.merge()`'s own `_collision_protect` check, run before
    // `pkg_preinst` and the file copy (real `treewalk()` ordering) --
    // now shared with the source `merge_after_install`. A binary package
    // carries no ebuild/repo, so `mypkglist`'s blocker term
    // (`blockers_from_flat_deps`) reads the already-USE-reduced
    // `*DEPEND` build-info files directly. The preserve-libs registry is
    // consulted unconditionally (real `_plib_registry` is never `None`).
    let plib_registry = read_plib_registry(root);
    let plib_inodes = plib_inode_map(root, &plib_registry.preserved_libs());
    let mut flat_deps: Vec<String> = Vec::new();
    for dep_key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
        if let Ok(d) = std::fs::read_to_string(build_info.join(dep_key)) {
            flat_deps.extend(d.split_whitespace().map(String::from));
        }
    }
    let blocked = blockers_from_flat_deps(root, &flat_deps);
    let (collisions, symlink_collisions, plib_collisions) = find_collisions(FindCollisionsArgs {
        d: &image,
        root,
        category: &category,
        package: &package,
        slot: &main_slot,
        config_protect: &options.config_protect,
        config_protect_mask: &options.config_protect_mask,
        plib_inodes: &plib_inodes,
        blocked: &blocked,
    })?;
    // Real `dblink.merge()`'s own abort condition (Python operator
    // precedence: `collision_protect or (protect_owned and owners)`);
    // identical to `merge_after_install`.
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

    // Real `dblink.treewalk()` order: the `-MERGING-<pf>` temporary vdb
    // entry is created before `pkg_preinst` runs, populated after it, and
    // only renamed into place after every replaced same-slot version is
    // unmerged -- identical to `merge_after_install`.
    create_vdb_tmp(root, &category, &pf)?;
    let preinst_status = run_hook("preinst")?;
    if preinst_status != 0 {
        return Ok(preinst_status);
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
    })?;
    // Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
    // block (identical to `merge_after_install`): the replaced same-slot instance's
    // still-needed libraries are carried into this package's own
    // `CONTENTS`; the record itself lands after the replace loop below.
    let new_image_paths: BTreeSet<String> = contents
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    let mut preserve_paths = BTreeSet::new();
    if let Some((paths, old_contents_text)) =
        find_preserve_paths_for_merge(FindPreservePathsForMergeArgs {
            root,
            category: &category,
            package: &package,
            new_pf: &pf,
            main_slot: &main_slot,
            new_image_paths: &new_image_paths,
        })
    {
        let (injected, surviving) =
            inject_preserved_libs_into_contents(&old_contents_text, &paths, &new_image_paths);
        contents.push_str(&injected);
        preserve_paths = surviving;
    }
    write_cfgfiledict(root, &cfgfiledict)?;
    write_vdb_tmp_contents(root, &category, &pf, &contents)?;

    // Real `treewalk()`: a preserved lib this new version now provides
    // itself is taken over from the `preserved_libs_registry` and
    // stripped from the previous owner's `CONTENTS` -- identical to
    // `merge_after_install`.
    if !plib_collisions.is_empty() {
        let cpv = format!("{category}/{pf}");
        unregister_preserved_libs(root, &cpv, plib_registry, &plib_collisions)?;
    }

    // Real merge-then-unmerge: drop every same-slot version the new one
    // replaced while its own vdb entry still sits in the `-MERGING-<pf>`
    // temporary (see `unmerge_replaced_same_slot`).
    // The merge-side just-preserved set travels with the replace loop
    // (real `preserve_paths` into `dblink.unmerge`) -- identical to
    // `merge_after_install`.
    let replacement_preserved: BTreeMap<String, Vec<String>> = if preserve_paths.is_empty() {
        BTreeMap::new()
    } else {
        BTreeMap::from([(
            format!("{category}/{pf}"),
            preserve_paths.iter().cloned().collect(),
        )])
    };
    let replaced_same_slot = unmerge_replaced_same_slot(
        root,
        &category,
        &package,
        &pf,
        &main_slot,
        &builddir.join("unmerge-src"),
        portage_tmpdir,
        options,
        &replacement_preserved,
    )?;
    // Publish, then real `dblink.treewalk()`'s own post-replace-loop
    // registration (`vartree.py:5266-5272`) -- identical to
    // `merge_after_install` (one commit on a database backend, see
    // `publish_merged_entry`).
    publish_merged_entry(PublishMergedEntryArgs {
        root,
        category: &category,
        pn: &package,
        pf: &pf,
        main_slot: &main_slot,
        preserve_paths: &preserve_paths,
        replaced: &replaced_same_slot,
    })?;

    // Real `treewalk()` order: `pkg_postinst` runs after the vdb entry
    // is live *and* every replaced same-slot version is gone, but before
    // `env_update()`. Its own non-zero exit is logged, never fatal (real
    // `_postinst_failure` -- "It's stupid to bail out here").
    //
    // `PORTAGE_UPDATE_ENV` -> the vdb entry's own `environment.bz2` (just
    // written, verbatim from the binpkg): `phase-functions.sh`
    // regenerates it from the live, merge-time environment (real
    // `vartree.py:5334`), so the vdb env carries the resolved merge-time
    // `FEATURES` and drops stale build-host locals -- see
    // `ebuild_phases::run_phase_from_saved_env`. `always` so it runs even
    // with no `pkg_postinst`. No-op when the binpkg carries no saved env.
    //
    // The live entry's own path (`InstalledDb::entry_path`, N9), or a
    // scratch copy on a database backend (see `merge_after_install`).
    let update_env = update_env_target(root, &category, &pf, &builddir.join("vdb-update-env"))?;
    let vdb_env_bz2 = &update_env.path;
    let postinst_status = run_hook_ex(
        "postinst",
        true,
        vdb_env_bz2.is_file().then_some(vdb_env_bz2.as_path()),
    )?;
    absorb_update_env(root, &category, &pf, &update_env)?;
    if postinst_status != 0 {
        eprintln!(
            "{category}/{pf}: FAILED postinst ({postinst_status}) -- merge kept (real _postinst_failure)"
        );
    }

    if !contents.is_empty() || !replaced_same_slot.is_empty() {
        env_update::run_env_update(root)?;
        // Real `treewalk()`: prune preserved libs orphaned by this merge
        // (identical to `merge_after_install`). No include feed: the new
        // entry is already renamed into place, so its own lines are
        // enumerated (backlog #224).
        prune_unused_preserved_libs(root, false, &|_| false, &[], &BTreeMap::new(), &[])?;
    }

    // Real `dblink.merge()`'s `_elog_process()`, before the builddir
    // removal below -- the binary merge's `pkg_preinst`/`pkg_postinst`
    // output lives in this builddir's `${T}/logging`, and the
    // unconditional `remove_dir_all` right after would take it with it
    // (the same #42 ordering as `merge_after_install`). Empty (and
    // silent) for a binpkg that carried no saved env.
    crate::elog::process_batch(crate::elog::ProcessBatchArgs {
        logdir: &crate::elog::logdir(root),
        root_display: &root.display().to_string(),
        items: &[(format!("{category}/{pf}"), builddir.join("temp"))],
        phases: None,
        color: &crate::color::Colorizer::new(crate::color::resolve_havecolor(None)),
        split_elog: feature_enabled(options, "split-elog"),
    });

    let _ = std::fs::remove_dir_all(&builddir);
    Ok(postinst_status)
}
