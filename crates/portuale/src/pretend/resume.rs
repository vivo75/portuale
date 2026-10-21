//! `--resume`.

use crate::ebuild_merge::MergeOptions;
use crate::ebuild_phases::ShellBackend;
use crate::emerge_build::StatusMode;
use crate::mtimedb::ResumeOpts;
use crate::pretend::*;

/// The `(kind, category, package, version)` of every entry in `entries`
/// (source or binary) that was supposed to merge but has no `CONTENTS`
/// under `root` yet: the resume list for `mtimedb::write_resume_list` on
/// the failure path.
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

/// The resume list a failed run re-saves: the still-unmerged entries (no
/// vdb `CONTENTS`), plus any postinst-failed entry, whose merge landed.
/// Portage never removes a failed entry from the saved list except through
/// a `--keep-going` resume pass, which already re-saved without it (see
/// `MergePassReport::resumed`).
// Portage: Scheduler.py, _do_merge_exit / merge (`_save_resume_list`)
pub(crate) fn unmerged_for_resume_save(
    root: &Path,
    entries: &[portage_repo::GraphEntry],
    report: &emerge_build::MergePassReport,
) -> Vec<crate::mtimedb::ResumeCpv> {
    let unmerged = entries_not_merged(root, entries);
    if report.resumed {
        return unmerged;
    }
    let mut out = Vec::new();
    for e in entries {
        let cp = (e.category.clone(), e.package.clone());
        let Some(cpv) = emerge_build::resume_cpv(e) else {
            continue;
        };
        if unmerged.contains(&cpv) || report.postinst_failed.contains(&cp) {
            out.push(cpv);
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
    /// Suppresses the post-merge info-regen einfo lines like every other
    /// action's.
    pub(crate) quiet: bool,
    /// Whether to offer the `--read-news` prompt when the pre-resolution
    /// notice printed.
    pub(crate) ask: bool,
    pub(crate) read_news: bool,
    /// The run's options, for `--fetchonly`/`--fetch-all-uri` on a resume.
    pub(crate) opts: &'a EmergeOpts<'a>,
}

/// `emerge --resume [--skipfirst] [--pretend]`: replay the saved resume
/// mergelist. With `--pretend`, print the saved list and stop; otherwise
/// build + merge each entry in order (`--skipfirst` drops the first). On
/// success clear the resume list and record the saved `favorites` in the
/// world file (honouring the recorded `--oneshot` / `--onlydeps`); on
/// failure re-save the still-unmerged tail with the same `myopts`.
/// Portage: _emerge/actions.py
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
        opts: emerge_opts,
    } = args;
    let color = Colorizer::new(color::resolve_havecolor(color_opt));
    let repos = match portage_repo::find_repos(config_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    // The GLEP 42 pre-resolution notice prints here too (resume handling
    // lives inside `action_build`), even when the resume list turns out
    // missing or empty below. Gated on non-`--pretend`.
    // Portage: _emerge/actions.py, run_action
    let resume_pre_printed = if !pretend {
        display_news_notice_if_any(&repos, root, config, &color)
    } else {
        false
    };
    // An interrupted news prompt exits before `action_build`.
    // Portage: _emerge/UserQuery.py, query
    if offer_news_reading(OfferNewsReadingArgs {
        ask,
        read_news,
        notice_printed: resume_pre_printed,
        color: &color,
    }) {
        return ExitCode::from(130);
    }
    let Some((favorites, mut mergelist, opts)) = crate::mtimedb::read_resume_list(root) else {
        // Portage reaches `post_emerge` even on this path: with nothing
        // merged only a `--pretend` run prints there.
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
    // The local binary pool for resumed `Binary` entries, so a resumed
    // binary names the same `::repo` a fresh merge would. Built once even
    // when the list turns out all-source.
    // Portage: depgraph.py, _loadResumeCommand
    let resume_bin_index = portage_repo::build_local_binpkg_index(config);
    let entries: Vec<portage_repo::GraphEntry> = mergelist
        .iter()
        .map(|(kind, c, p, v)| {
            let source = match kind {
                crate::mtimedb::ResumeEntryKind::Ebuild => portage_repo::CandidateSource::Ebuild,
                crate::mtimedb::ResumeEntryKind::Binary => portage_repo::CandidateSource::Binary,
            };
            // A resumed binary entry re-resolves its `::repo` from the
            // binary pool, never from the ebuild repos.
            // Portage: depgraph.py, _loadResumeCommand
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
            // resolver's `use_flags_display` population is skipped here.
            // Recompute it for a source build: an empty one makes a
            // USE-conditional ebuild `die` in `src_compile`. A binary entry
            // carries its own baked USE, so it needs nothing here.
            if source == portage_repo::CandidateSource::Ebuild {
                entry.use_flags_display =
                    portage_repo::candidate_use_flags_display(&repos, config, c, p, v);
                entry.interactive = portage_repo::candidate_interactive(&repos, config, c, p, v);
            }
            entry
        })
        .collect();

    // `emerge --resume --fetchonly`: fetch the saved list's sources, merge
    // nothing; `-p` lists their URIs. The saved list stays as it is.
    // Portage: _emerge/actions.py (`action_build`), Scheduler.py (fetchonly)
    if emerge_opts.fetchonly {
        if pretend {
            println!(
                "\n{}\n",
                color.c(
                    "darkgreen",
                    "These are the packages that would be fetched, in order:"
                )
            );
        }
        if super::gates::resolution_notice_shown(quiet, emerge_opts.nodeps) {
            println!("Calculating dependencies ... done!");
            print!("{}", super::timing_line(&color, None));
        }
        println!("*** Resuming merge...");
        return ExitCode::from(super::run_fetchonly_action(super::FetchonlyActionArgs {
            opts: emerge_opts,
            entries: &entries,
            config,
            repos: &repos,
            root,
            config_root,
        }));
    }

    // `emerge --resume --pretend`: show the saved list and stop, merging
    // nothing and touching neither the resume list nor `world`. Every line
    // prints as `[ebuild N ...]`, since the saved list records no markers.
    // Portage: _emerge/actions.py
    if let Some(code) = resume_pretend(ResumePretendArgs {
        pretend,
        entries: &entries,
        root,
        opts,
        color,
        repos: &repos,
        config,
    }) {
        return code;
    }

    // A resumed run re-saves before merging too: this drops a
    // `--skipfirst`-skipped head and leaves exactly the tail behind if
    // this run is itself killed. A preview never touches the list.
    // Portage: Scheduler.merge, _save_resume_list
    let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
    let _ = crate::mtimedb::write_resume_list(root, &fav_refs, &mergelist, &opts);

    // Portage re-resolves the saved list (`resume_depgraph`) and then
    // `Scheduler.merge` prints the banner, ahead of the interactive-task
    // notice below.
    // Portage: _emerge/actions.py, Scheduler.py (merge, `*** Resuming merge...`)
    if super::gates::resolution_notice_shown(quiet, emerge_opts.nodeps) {
        println!("Calculating dependencies ... done!");
        print!("{}", super::timing_line(&color, None));
    }
    println!("{}", color.c("GOOD", "*** Resuming merge..."));
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
    // `CONFIG_PROTECT[_MASK]` from the resolved config, as the non-resume
    // `emerge` path does.
    merge_options.apply_config_protect(config);
    // The run-wide phase env and `package.env` vars the non-resume
    // `emerge` path resolves (see `run_wide_phase_env` there):
    // a resumed source build is a genuine build and needs the same phase
    // env -- without these a resumed `src_compile` sees `CFLAGS=""`.
    // (The per-entry resolved `USE` already flows via
    // `candidate_use_flags_display` above.)
    merge_options.build_env = crate::emerge_build::run_wide_phase_env(config);
    merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
    merge_options.package_env_vars = config.package_env_vars.clone();
    // An all-source list goes through `run_source_merge`; a list with a
    // resumed binary entry goes through `run_merge_plan` instead (the
    // resume mergelist can mix both). The saved `myopts` carries no
    // `--quiet`/`--verbose`, so the display is serial unless `-j` > 1.
    let resume_mode = emerge_build::scheduler_status_mode(jobs, false, None, entries.len(), false);
    let (resume_mode, jobs) =
        emerge_build::apply_interactive_tasks(resume_mode, jobs, &entries, root, &color);
    if let Some(code) = resume_merge(ResumeMergeArgs {
        entries: &entries,
        repos: &repos,
        root,
        portage_tmpdir,
        merge_options: &merge_options,
        jobs,
        load_average,
        resume_mode,
        shell,
        debug,
        config,
        favorites: &favorites,
        opts,
        color,
    }) {
        return code;
    }

    crate::mtimedb::clear_resume_list(root);
    let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
    // The saved `favorites` are only world-recorded when the original run
    // would have recorded them (recorded `--oneshot`/`--onlydeps`).
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

    // The info-dir regen fires after a `--resume` too (the vdb changed),
    // before the preserved-libs advisory.
    // Portage: post_emerge.py, post_emerge
    let noinfo = config_features_list(config).iter().any(|t| t == "noinfo");
    if let Err(e) = crate::info_files::post_merge_info_update(root, &color, quiet, noinfo) {
        eprintln!("emerge: {e}");
        return ExitCode::from(1);
    }
    crate::preserved_libs::show_preserved_libs_notice(root, &color, false, false);
    // The GLEP 42 notice prints a second time once the resumed merge
    // changed the vdb.
    // Portage: post_emerge.py, post_emerge
    display_news_notice_if_any(&repos, root, config, &color);

    ExitCode::SUCCESS
}

/// The values [`resume_pretend`] reads from its caller.
pub(crate) struct ResumePretendArgs<'a> {
    pub(crate) pretend: bool,
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) opts: ResumeOpts,
    pub(crate) color: Colorizer,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config: &'a Config,
}

/// The `--resume --pretend` display.
pub(crate) fn resume_pretend(args: ResumePretendArgs<'_>) -> Option<ExitCode> {
    let ResumePretendArgs {
        pretend,
        entries,
        root,
        opts,
        color,
        repos,
        config,
    } = args;
    if pretend {
        let mut blocker_lines: Vec<String> = Vec::new();
        for (i, _entry) in entries.iter().enumerate() {
            print_entry_line(PrintEntryLineArgs {
                entries,
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
        // Portage reaches `post_emerge` after the `--resume --pretend`
        // display too.
        pretend_end_news_notice(pretend, repos, root, config, &color);
        return Some(ExitCode::SUCCESS);
    }
    None
}

/// The values [`resume_merge`] reads from its caller.
pub(crate) struct ResumeMergeArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) portage_tmpdir: &'a Path,
    pub(crate) merge_options: &'a MergeOptions,
    pub(crate) jobs: usize,
    pub(crate) load_average: Option<f64>,
    pub(crate) resume_mode: StatusMode,
    pub(crate) shell: ShellBackend,
    pub(crate) debug: bool,
    pub(crate) config: &'a Config,
    pub(crate) favorites: &'a [String],
    pub(crate) opts: ResumeOpts,
    pub(crate) color: Colorizer,
}

/// Run the resumed merge and report its failure.
pub(crate) fn resume_merge(args: ResumeMergeArgs<'_>) -> Option<ExitCode> {
    let ResumeMergeArgs {
        entries,
        repos,
        root,
        portage_tmpdir,
        merge_options,
        jobs,
        load_average,
        resume_mode,
        shell,
        debug,
        config,
        favorites,
        opts,
        color,
    } = args;
    let merge_result = if entries
        .iter()
        .all(|e| e.source == portage_repo::CandidateSource::Ebuild)
    {
        emerge_build::run_source_merge(emerge_build::RunSourceMergeArgs {
            entries,
            repos,
            root,
            portage_tmpdir,
            options: merge_options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            jobs,
            load_average,
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
            entries,
            config,
            repos,
            root,
            pkgdir: &package_options.pkgdir,
            portage_tmpdir,
            merge_options,
            keep_going: false,
            buildpkg: None,
            buildpkg_exclude: &[],
            mode: resume_mode,
        })
    };
    if let Err(e) = merge_result {
        let still = unmerged_for_resume_save(root, entries, &e.report);
        let fav_refs: Vec<&str> = favorites.iter().map(String::as_str).collect();
        // A failure with nothing unmerged leaves the per-merge shrink's
        // own tail alone.
        if !still.is_empty() {
            let _ = crate::mtimedb::write_resume_list(root, &fav_refs, &still, &opts);
        }
        // A merge-time binpkg digest/missing-file failure already printed
        // its own tail: no `emerge:` line. The silent resume re-save above
        // stays; Portage keeps the list resumable too.
        if !crate::emerge_getbinpkg::is_binpkg_digest_failure(&e.message)
            && !crate::emerge_getbinpkg::is_binpkg_missing_failure(&e.message)
        {
            eprintln!("emerge: {}", e.message);
        }
        // The notice prints when the vdb changed (at least one resumed
        // entry landed); see `merge_failure_news_notice`.
        merge_failure_news_notice(still.len() < entries.len(), repos, root, config, &color);
        return Some(ExitCode::from(1));
    }
    None
}
