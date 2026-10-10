//! `--resume`.

use crate::pretend::*;

/// The `(kind, category, package, version)` of every entry in `entries`
/// (source *or* binary -- real portage's own resume mergelist records
/// both) that was supposed to merge but has no `CONTENTS` under `root`
/// yet -- the resume list for `mtimedb::write_resume_list` on the failure
/// path. The per-entry mapping itself is `emerge_build::resume_cpv` (the
/// same mapping the up-front save uses), narrowed here to entries whose
/// merge never landed.
pub(crate) fn entries_not_merged(
    root: &Path,
    entries: &[portage_repo::GraphEntry],
) -> Vec<crate::mtimedb::ResumeCpv> {
    let mut out = Vec::new();
    for e in entries {
        let Some((kind, category, package, version)) = emerge_build::resume_cpv(e) else {
            continue;
        };
        // One `stat` of the entry's `CONTENTS` through the root's backend.
        if !ebuild_merge::entry_file_is_regular(
            root,
            &category,
            &format!("{package}-{version}"),
            "CONTENTS",
        ) {
            out.push((kind, category, package, version));
        }
    }
    out
}

/// Arguments of [`run_resume`].
pub(crate) struct RunResumeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) skipfirst: bool,
    pub(crate) pretend: bool,
    pub(crate) color_opt: Option<bool>,
    pub(crate) jobs: usize,
    pub(crate) load_average: Option<f64>,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    /// Real `--quiet` (`noiselimit < 0`, `actions.py:3907-3908`): suppresses
    /// the post-merge info-regen einfo lines like every other action's.
    pub(crate) quiet: bool,
    /// Backlog #231 (a/c): real `run_action` prints the pre-resolution
    /// news notice (and offers the `--read-news` prompt) before
    /// `action_build` -- which is where the resume handling lives -- so
    /// resumed builds print both notices too.
    pub(crate) ask: bool,
    pub(crate) read_news: bool,
}

/// `emerge --resume [--skipfirst] [--pretend]`: replay
/// `mtimedb["resume"]["mergelist"]` (real `_emerge/actions.py`'s resume
/// handling). With `--pretend`, print the saved list and stop (merging
/// nothing, touching neither the resume list nor `world`). Otherwise
/// each saved `cat/pkg-ver` is built + merged through
/// `emerge_getbinpkg::run_merge_plan` in the recorded order and per its
/// own recorded `ResumeEntryKind` (source or binary -- real portage's
/// own resume mergelist records both); `--skipfirst` drops the first
/// (the one that failed). On success the resume list is cleared and the
/// saved `favorites` are recorded in the world file -- unless the
/// recorded `myopts` carried `--oneshot` / `--onlydeps`, in which case
/// the same suppression the original run would have applied is
/// honoured. On another failure the still-unmerged tail is re-saved
/// with the same `myopts`.
pub(crate) fn run_resume(args: RunResumeArgs<'_>) -> ExitCode {
    let RunResumeArgs {
        root,
        config_root,
        config,
        portage_tmpdir,
        skipfirst,
        pretend,
        color_opt,
        jobs,
        load_average,
        shell,
        debug,
        quiet,
        ask,
        read_news,
    } = args;
    let color = Colorizer::new(color::resolve_havecolor(color_opt));
    let repos = match portage_repo::find_repos(config_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    // Backlog #231 (a): real `_emerge/actions.py::run_action`
    // (`actions.py:4266-4270`) prints the GLEP 42 pre-resolution notice
    // before `action_build` -- and the resume handling (`resume =
    // True`, `resume_depgraph`, `actions.py:220-...`) lives *inside*
    // `action_build` -- so a resumed build prints it too (even when the
    // resume list turns out missing or empty below: real's notice
    // precedes all of that). Gated on non-`--pretend` like real; the
    // `--ask` + `--read-news` prompt (c) rides the same pre point.
    let resume_pre_printed = if !pretend {
        display_news_notice_if_any(&repos, root, config, &color)
    } else {
        false
    };
    // An interrupted news prompt (backlog #234) exits `128 + SIGINT`
    // before `action_build`, like real `UserQuery.query`'s own
    // `sys.exit` (`_emerge/UserQuery.py:74-76`).
    if offer_news_reading(OfferNewsReadingArgs {
        ask,
        read_news,
        notice_printed: resume_pre_printed,
        color: &color,
    }) {
        return ExitCode::from(130);
    }
    let Some((favorites, mut mergelist, opts)) = crate::mtimedb::read_resume_list(root) else {
        // Real reaches `post_emerge` even on this path (unconditional
        // in `run_action`, `actions.py:4289-4297`): with nothing merged
        // that prints the notice only under `--pretend`
        // (`post_emerge.py:112-117`) -- a no-op otherwise.
        pretend_end_news_notice(pretend, &repos, root, config, &color);
        eprintln!("emerge: could not find a valid resume list");
        return ExitCode::from(1);
    };
    if skipfirst && !mergelist.is_empty() {
        mergelist.remove(0);
    }
    if mergelist.is_empty() {
        crate::mtimedb::clear_resume_list(root);
        // Same `post_emerge` arm as above: only a `--pretend` run
        // prints here.
        pretend_end_news_notice(pretend, &repos, root, config, &color);
        println!("emerge: the resume list is empty; nothing to do.");
        return ExitCode::SUCCESS;
    }
    // The local binary pool for resumed `Binary` entries (real's bintree
    // re-resolution, backlog #186): the resolver's own pool constructor,
    // so a resumed binary names the same `::repo` a fresh merge would.
    // Built once even when the list turns out all-source (cheap: the
    // constructor reads `<pkgdir>/Packages`, and only scans `$PKGDIR`
    // files when a `--usepkg`-family run already scanned them into
    // `config.scanned_binpkgs`).
    let resume_bin_index = portage_repo::build_local_binpkg_index(config);
    let entries: Vec<portage_repo::GraphEntry> = mergelist
        .iter()
        .map(|(kind, c, p, v)| {
            let source = match kind {
                crate::mtimedb::ResumeEntryKind::Ebuild => portage_repo::CandidateSource::Ebuild,
                crate::mtimedb::ResumeEntryKind::Binary => portage_repo::CandidateSource::Binary,
            };
            // A resumed binary entry re-resolves its `::repo` from the
            // binary pool, never from the ebuild repos (real
            // `depgraph.py::_loadResumeCommand`, backlog #186).
            let binary_repo = if source == portage_repo::CandidateSource::Binary {
                emerge_build::resume_binary_repo(emerge_build::ResumeBinaryRepoArgs {
                    local: &resume_bin_index,
                    binrepos: &config.binrepos,
                    root,
                    category: c,
                    package: p,
                    version: v,
                })
            } else {
                None
            };
            let mut entry = emerge_build::resume_entry(emerge_build::ResumeEntryArgs {
                category: c,
                package: p,
                version: v,
                source,
                repos: &repos,
                binary_repo: binary_repo.as_deref(),
            });
            // The `mtimedb` resume list records only `cat/pkg-ver`, so the
            // resolver's own `use_flags_display` population is skipped.
            // Recompute it for a source build -- `emerge_build::
            // build_use_env` turns it into the phase `USE=`, and an empty
            // one makes a `python-r1`/`distutils-r1` (or any
            // USE-conditional) ebuild `die` in `src_compile`. A binary
            // entry carries its own baked USE (`merge_binpkg`), so it
            // needs nothing here.
            if source == portage_repo::CandidateSource::Ebuild {
                entry.use_flags_display =
                    portage_repo::candidate_use_flags_display(&repos, config, c, p, v);
            }
            entry
        })
        .collect();

    // `emerge --resume --pretend` (real `_emerge/actions.py`: re-resolve
    // the resume list, `mydepgraph.display(...)`, `return os.EX_OK`):
    // show the saved list and stop, merging nothing and touching neither
    // the resume list nor `world`. Portuale's resume list only records
    // `cat/pkg-ver` (no re-resolution), so every line prints as
    // `[ebuild N ...]` -- a documented divergence from real portage's
    // re-derived `N`/`U`/`R` markers, same limitation the resume merge
    // itself already carries.
    if pretend {
        let mut blocker_lines: Vec<String> = Vec::new();
        for (i, _entry) in entries.iter().enumerate() {
            print_entry_line(PrintEntryLineArgs {
                entries: &entries,
                root,
                index: i,
                indent: "",
                ordered: true,
                top_level_pkgs: &HashSet::new(),
                onlydeps: false,
                oneshot: opts.oneshot,
                verbose: false,
                quiet: false,
                alphabetical: false,
                columns: false,
                columnwidth: 0,
                running_root: None,
                color: &color,
                system_atoms: &[],
                world_atoms: &[], // `--resume --pretend` never re-resolves, so it can't
                // know about slot-operator rebuilds -- see the marker
                // caveat above.
                force_reinstall_cps: &HashSet::new(),
                blocker_lines: &mut blocker_lines, // No resolve ran, so no masked-dependency,
                // USE-unsatisfied or plain-miss disclosure either (any
                // NVC entry here renders the bare line).
                masked_deps: &[],
                use_unsat_deps: &[],
                plain_miss_deps: &[],
                force_sizes: false,
                circular: &HashMap::new(),
            });
        }
        // Backlog #231 (a): real reaches `post_emerge` after the
        // `--resume --pretend` display too (`actions.py:4289-4297`),
        // printing the notice when the vdb did not change
        // (`post_emerge.py:112-117`).
        pretend_end_news_notice(pretend, &repos, root, config, &color);
        return ExitCode::SUCCESS;
    }

    // Real `Scheduler.merge() -> _save_resume_list()` (#168): a resumed
    // run re-saves before merging too -- this drops a `--skipfirst`-
    // skipped head from the on-disk list and, together with the per-merge
    // shrink inside the merge loops, leaves exactly the tail behind if
    // this run is itself SIGKILLed. `--resume --pretend` returns above, so
    // a preview never touches the list.
    let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
    let _ = crate::mtimedb::write_resume_list(root, &fav_refs, &mergelist, &opts);

    println!(">>> Resuming merge of {} package(s)...", entries.len());
    let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
    // Real `preinst_mask()`: config-resolved `INSTALL_MASK` + the
    // `no{man,info,doc}` `FEATURES` fold (the env read `from_env` did is
    // the `ebuild <file> merge` fallback -- override it here like the
    // non-resume `emerge` path does).
    (
        merge_options.install_mask,
        merge_options.install_mask_prunes_usr_share,
    ) = config_install_mask(config);
    merge_options.set_resolved_features(&config_features_string(config));
    // #333: `CONFIG_PROTECT[_MASK]` from the resolved config, as the
    // non-resume `emerge` path does.
    merge_options.apply_config_protect(config);
    // The run-wide phase env and `package.env` vars the non-resume
    // `emerge` path resolves (see `run_wide_phase_env` there):
    // a resumed source build is a real build and needs the same phase
    // env -- without these a resumed `src_compile` sees `CFLAGS=""`.
    // (The per-entry resolved `USE` already flows via
    // `candidate_use_flags_display` above.)
    merge_options.build_env = crate::emerge_build::run_wide_phase_env(config);
    merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
    merge_options.package_env_vars = config.package_env_vars.clone();
    // An all-source resume list keeps going through `run_source_merge`
    // (full `--jobs`/`--load-average` scheduler support); a mergelist
    // with at least one resumed binary entry (real portage's own resume
    // mergelist can mix both -- `ResumeEntryKind`) goes through
    // `run_merge_plan` instead, the same per-entry binary-vs-source
    // dispatch `--getbinpkg` itself already uses. `run_merge_plan` has
    // no scheduler of its own yet (a pre-existing, separate gap, not
    // introduced here) -- unlike a pure-source resume, a binary-
    // involving one never had `--jobs` parallelism to begin with, so
    // this isn't a regression for it.
    // Backlog #197: `--resume` carries no `--quiet`/`--verbose` in
    // `mtimedb["resume"]` (a documented `myopts` cut), so the display
    // mode is serial-streaming unless `-j` >1.
    let resume_mode = emerge_build::scheduler_status_mode(jobs, false, None, entries.len(), false);
    let merge_result = if entries
        .iter()
        .all(|e| e.source == portage_repo::CandidateSource::Ebuild)
    {
        emerge_build::run_source_merge(emerge_build::RunSourceMergeArgs {
            entries: &entries,
            repos: &repos,
            root,
            portage_tmpdir,
            options: &merge_options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs,
            load_average, // `--resume` carries no `--quiet-build` in `mtimedb["resume"]`
            // (a documented `myopts` cut) -- stream, unless `-j` >1.
            capture_log: false,
            mode: resume_mode,
        })
    } else {
        let package_options = package_options_from_env(
            shell,
            debug,
            Some(&config_features_string(config)),
            Some(config),
        );
        emerge_getbinpkg::run_merge_plan(emerge_getbinpkg::RunMergePlanArgs {
            entries: &entries,
            config,
            repos: &repos,
            root,
            pkgdir: &package_options.pkgdir,
            portage_tmpdir,
            merge_options: &merge_options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            mode: resume_mode,
        })
    };
    if let Err(e) = merge_result {
        let still = entries_not_merged(root, &entries);
        let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
        // The empty-`still` guard is the failure path's own
        // (`write_resume_list` itself is unconditional now, #179): a
        // failure with nothing unmerged leaves the per-merge shrink's
        // own tail alone.
        if !still.is_empty() {
            let _ = crate::mtimedb::write_resume_list(root, &fav_refs, &still, &opts);
        }
        // Backlog #174: a merge-time binpkg digest failure already
        // printed real's full tail -- no `emerge:` line either (real
        // exits a merge failure via `FAILURE`, not the action error
        // path). Backlog #187 extends the same silence to the
        // missing-file sentinel (real `BinpkgVerifier`'s ENOENT arm).
        // The silent resume re-save above stays: real keeps the
        // list resumable too.
        if !crate::emerge_getbinpkg::is_binpkg_digest_failure(&e)
            && !crate::emerge_getbinpkg::is_binpkg_missing_failure(&e)
        {
            eprintln!("emerge: {e}");
        }
        // Backlog #231 (a): real `post_emerge.py:155` prints the notice
        // regardless of retval -- gated on the vdb having changed
        // (`:112-117`), which here is `still.len() < entries.len()` (at
        // least one resumed entry landed; the same structural
        // approximation as the non-resume failure arms'
        // `merge_failure_news_notice`).
        merge_failure_news_notice(still.len() < entries.len(), &repos, root, config, &color);
        return ExitCode::from(1);
    }

    crate::mtimedb::clear_resume_list(root);
    let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
    // Real `--resume` honours the recorded `--oneshot`/`--onlydeps`: the
    // saved `favorites` are only world-recorded when the original run
    // would have recorded them.
    if let Err(e) = update_world_file(UpdateWorldFileArgs {
        root,
        target_atoms: &fav_refs,
        entries: &entries,
        system_atoms: &[],
        repos: &repos,
        oneshot: opts.oneshot,
        onlydeps: opts.onlydeps,
    }) {
        eprintln!("emerge: {e}");
        return ExitCode::from(1);
    }

    // Real `post_emerge()`: the info-dir regen fires after a
    // `--resume` too (the vdb changed -- the failure branch above
    // already returned, and an empty mergelist returned even earlier),
    // before the preserved-libs advisory.
    let noinfo = config_features_list(config).iter().any(|t| t == "noinfo");
    if let Err(e) = crate::info_files::post_merge_info_update(root, &color, quiet, noinfo) {
        eprintln!("emerge: {e}");
        return ExitCode::from(1);
    }
    crate::preserved_libs::show_preserved_libs_notice(root, &color, false, false);
    // Backlog #231 (a): real `post_emerge()` (`post_emerge.py:155`)
    // prints the GLEP 42 notice a second time once the resumed merge
    // changed the vdb -- same FEATURES-plus-nonzero-count gates as the
    // non-resume success path above.
    display_news_notice_if_any(&repos, root, config, &color);

    ExitCode::SUCCESS
}
