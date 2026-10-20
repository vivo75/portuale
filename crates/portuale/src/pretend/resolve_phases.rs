//! Running the resolver: the first pass, the locked merges, the complete-graph pass, and the running-root repaint.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::GraphResult;
use portage_repo::RepoConfig;

/// The values [`run_resolve_phases`] reads from its caller.
pub(crate) struct RunResolvePhasesArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) config: &'a Config,
    pub(crate) config_root: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) expanded_atoms: &'a [String],
    pub(crate) set_args: &'a HashSet<String>,
    pub(crate) argument_sets: &'a HashMap<String, String>,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) color: &'a Colorizer,
}

/// The values [`resolve_once`] reads from its caller.
pub(crate) struct ResolveOnceArgs<'a> {
    pub(crate) plan: &'a ResolvePlan,
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) config: &'a Config,
    pub(crate) config_root: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) expanded_atoms: &'a [String],
    pub(crate) set_args: &'a HashSet<String>,
    pub(crate) argument_sets: &'a HashMap<String, String>,
    pub(crate) complete: bool,
    pub(crate) locked: &'a [String],
    pub(crate) with_seeds: bool,
    pub(crate) reuse: &'a HashMap<(String, String), portage_repo::PretendOutcome>,
}

/// One resolver run: the request built from the plan and the options.
pub(crate) fn resolve_once(args: ResolveOnceArgs<'_>) -> Result<GraphResult, portage_repo::Error> {
    let ResolveOnceArgs {
        plan,
        opts,
        config,
        config_root,
        root,
        expanded_atoms,
        set_args,
        argument_sets,
        complete,
        locked,
        with_seeds,
        reuse,
    } = args;
    let &ResolvePlan {
        ref complete_seed_atoms,
        selective,
        autounmask_suggest_keywords,
        autounmask_suggest_use,
        autounmask_suggest_license,
        autounmask_suggest_masks,
        usepkg,
        usepkgonly,
        binpkg_respect_use,
        rebuilt_binaries,
        ref root_deps_running_root,
        ref distdir,
        getbinpkg,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        dynamic_deps,
        ..
    } = plan;
    let &EmergeOpts {
        newuse,
        changed_use,
        nodeps,
        onlydeps,
        onlydeps_with_rdeps,
        onlydeps_with_ideps,
        update,
        deep,
        ref excluded,
        with_bdeps,
        changed_deps,
        changed_slot,
        with_test_deps,
        changed_deps_report,
        ref usepkg_exclude,
        ref usepkg_include,
        rebuilt_binaries_timestamp,
        newrepo,
        buildpkgonly,
        emptytree,
        ignore_built_slot_operator_deps,
        backtrack_max,
        ref reinstall_atoms,
        rebuild_if_new_slot,
        ref rebuild_exclude,
        ref rebuild_ignore,
        implicit_system_deps,
        solver,
        ..
    } = opts;
    let cfg: std::borrow::Cow<portage_profile::Config> =
        if !complete_seed_atoms.is_empty() || !locked.is_empty() {
            let mut c = config.clone();
            // Blocker classification follows the complete-mode retry even
            // on a non-complete pass, while `complete_seed_atoms` stays
            // gated on `with_seeds`: filling that field here would switch
            // the slot-operator-rebuild scan on for every plain resolve.
            // Portage: depgraph.py, _complete_graph
            c.blocker_retry_seed_atoms = complete_seed_atoms.to_vec();
            if with_seeds {
                c.complete_seed_atoms = complete_seed_atoms.to_vec();
                c.complete_locked_merges = locked.to_vec();
            }
            std::borrow::Cow::Owned(c)
        } else {
            std::borrow::Cow::Borrowed(config)
        };
    // `repos.conf`'s per-repo `usepkg-exclude`/`usepkg-include` join the CLI lists.
    // Portage: _emerge/depgraph.py:384-392
    let (repo_exclude, repo_include) = portage_repo::repo_usepkg_atoms(config_root);
    // New code builds a `ResolveRequest` and calls the runtime-selected
    // `Resolver` directly (see `resolve_pretend_graph`'s doc comment) --
    // this is the one production call site that carries `--solver=`.
    let req = ResolveRequest {
        config_root: config_root.to_path_buf(),
        root: root.to_path_buf(),
        atoms: expanded_atoms.to_vec(),
        config: cfg.into_owned(),
        newuse,
        changed_use,
        nodeps,
        onlydeps,
        onlydeps_with_rdeps,
        onlydeps_with_ideps,
        update,
        deep,
        excluded: excluded.to_vec(),
        with_bdeps,
        changed_deps,
        changed_slot,
        with_test_deps,
        changed_deps_report,
        selective,
        autounmask_suggest_keywords,
        autounmask_suggest_use,
        autounmask_suggest_license,
        autounmask_suggest_masks,
        usepkg,
        usepkgonly,
        binpkg_respect_use,
        usepkg_exclude: usepkg_exclude.iter().cloned().chain(repo_exclude).collect(),
        usepkg_include: usepkg_include.iter().cloned().chain(repo_include).collect(),
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        newrepo,
        buildpkgonly,
        root_deps_running_root: root_deps_running_root.clone(),
        distdir: distdir.to_path_buf(),
        empty: emptytree,
        getbinpkg,
        ignore_built_slot_operator_deps,
        backtrack_max,
        reinstall_atoms: reinstall_atoms.to_vec(),
        rebuild_if_new_slot,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        rebuild_exclude: rebuild_exclude.to_vec(),
        rebuild_ignore: rebuild_ignore.to_vec(),
        dynamic_deps,
        implicit_system_deps,
        complete,
        complete_reuse: reuse.clone(),
        set_args: set_args.clone(),
        argument_sets: argument_sets.clone(),
        solver,
    };
    active_resolver_for(solver).resolve(&req)
}

/// Running the resolver: the first pass, the locked merges, the complete-graph pass, and the running-root repaint.
pub(crate) fn run_resolve_phases(args: RunResolvePhasesArgs<'_>) -> Result<GraphResult, ExitCode> {
    let RunResolvePhasesArgs {
        plan,
        opts,
        config,
        config_root,
        root,
        expanded_atoms,
        set_args,
        argument_sets,
        repos,
        color,
    } = args;
    let &ResolvePlan {
        ref root_deps_running_root,
        complete_if_new_use,
        complete_if_new_ver,
        complete_graph,
        ref running_config,
        ..
    } = plan;
    let &EmergeOpts {
        nodeps,
        deep,
        rebuild_if_new_slot,
        misspell_suggestions,
        pretend,
        ..
    } = opts;
    let run_resolve =
        |complete: bool,
         locked: &[String],
         with_seeds: bool,
         reuse: &HashMap<(String, String), portage_repo::PretendOutcome>| {
            resolve_once(ResolveOnceArgs {
                plan,
                opts,
                config,
                config_root,
                root,
                expanded_atoms,
                set_args,
                argument_sets,
                complete,
                locked,
                with_seeds,
                reuse,
            })
        };
    let handle = |r: Result<portage_repo::GraphResult, crate::error::Error>| match r {
        Ok(result) => Ok(result),
        Err(e) => {
            let msg = e.to_string();
            // A run that aborts on a top-level all-masked
            // target prints the resolution notice first (stdout), exactly
            // what `show_resolution_header` prints on the other abort arms.
            // Portage: _emerge/depgraph.py, _start_resolution_display
            if top_level_abort_notice_shown(&msg) {
                show_resolution_header_with_counts(opts, 0, opts.backtrack_max, pretend);
            }
            // Portage never prefixes a `!!!`-headed report with `emerge: `;
            // it is emitted verbatim.
            // Portage: _emerge/depgraph.py, _show_unsatisfied_dep
            if msg.trim_start().starts_with("!!!") {
                eprint!("{msg}");
            } else {
                eprint!("emerge: {msg}");
            }
            let pkgdir = std::path::PathBuf::from(&config.pkgdir);
            let dbs = SimilarNameDbs {
                ebuild: !(plan.usepkgonly || portage_repo::ebuild_db_dropped()),
                // `--usepkgonly` and `--getbinpkg` each imply `--usepkg`.
                // Portage: actions.py, run_action
                pkgdir: (plan.usepkg || plan.usepkgonly || plan.getbinpkg)
                    .then_some(pkgdir.as_path()),
                binrepos: if plan.getbinpkg {
                    &config.binrepos
                } else {
                    &[]
                },
            };
            if let Some(extra) =
                misspell_suggestion_block(&msg, repos, root, misspell_suggestions, dbs)
            {
                eprint!("{extra}");
            }
            eprintln!();
            Err(ExitCode::from(1))
        }
    };

    // Phase 1 is always non-complete: the authoritative merge list.
    // Portage's `_complete_graph` runs after the normal graph is built
    // and only extends it, never adding a new merge.
    // Portage: depgraph.py, _complete_graph
    let empty_reuse: HashMap<(String, String), portage_repo::PretendOutcome> = HashMap::new();
    let result = match handle(
        run_resolve(false, &[], false, &empty_reuse).map_err(crate::error::Error::from),
    ) {
        Ok(result) => result,
        Err(code) => {
            pretend_end_news_notice(pretend, repos, root, config, color);
            return Err(code);
        }
    };
    // The `cp`s phase 1 would actually merge. The complete pass may not
    // merge anything outside this set.
    let locked_merges: Vec<String> = result
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.outcome,
                portage_repo::PretendOutcome::New { .. }
                    | portage_repo::PretendOutcome::Upgrade { .. }
                    | portage_repo::PretendOutcome::Downgrade { .. }
                    | portage_repo::PretendOutcome::Reinstall { .. }
            )
        })
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    // Phase 1's `New`/`Upgrade`/`Downgrade` picks for the complete walk:
    // complete mode never wants an update, so without this a non-top-level
    // merge demotes to kept; Portage keeps the graph node
    // (`_select_pkg_from_graph`). `Reinstall`s stay out, all of them: the
    // complete walk re-derives a reinstall through its own machinery --
    // the slot-operator scan + eliminate, the `--rebuild-if-*` restart
    // step, the `--reinstall-atoms`/USE/argument flips -- which is also
    // what records (or withholds) its red `r` and what latches a refused
    // display pair. Reusing a reinstall record would freeze a flag the
    // re-derivation drops -- e.g. a top-level consumer Portage rebuilds
    // as a plain argument `R`, never `rR` (oracle `soc` cell, host Portage
    // 3.0.82.2: no pair, no block).
    // Portage: depgraph.py, _complete_graph
    let complete_reuse: HashMap<(String, String), portage_repo::PretendOutcome> = result
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.outcome,
                portage_repo::PretendOutcome::New { .. }
                    | portage_repo::PretendOutcome::Upgrade { .. }
                    | portage_repo::PretendOutcome::Downgrade { .. }
            )
        })
        .map(|e| ((e.category.clone(), e.package.clone()), e.outcome.clone()))
        .collect();
    // A cp with picks in more than one slot is keyed once but settles per
    // slot: reusing one slot's pick would drop the other (firefox's
    // rust-bin:1.96.1 next to :1.94.1), so those re-resolve as before.
    let multi_slot: std::collections::HashSet<(String, String)> = {
        let mut seen = std::collections::HashSet::new();
        result
            .entries
            .iter()
            .map(|e| (e.category.clone(), e.package.clone()))
            .filter(|k| !seen.insert(k.clone()))
            .collect()
    };
    let mut complete_reuse = complete_reuse;
    complete_reuse.retain(|k, _| !multi_slot.contains(k));
    // A `--rebuild-if-*` restart consumes a backtrack try, so phase 1's
    // count carries into the second walk -- but only then: an unseeded
    // first walk can misfire a probe the seeded re-walk avoids (r25).
    // Portage: depgraph.py, _complete_graph
    let phase1_restarts = result.backtrack_restarts;
    let phase1_rebuild_if = !result.rebuild_if_reinstalls.is_empty();
    // Phase 2 (`_complete_graph`): run when `--complete-graph` is explicit
    // or auto-enabled (first graph changes an installed package while a
    // `--complete-graph-if-new-*` / `--rebuild-if-new-slot` switch is on).
    // Skipped entirely under `--nodeps`. It never changes the visible
    // merge list (the locked set gates it); it only widens `--json` /
    // `--changed-deps-report` / slot-op accounting.
    // Portage: depgraph.py, _complete_graph
    let auto_enable = !nodeps
        && portage_repo::complete_graph_auto_enable(portage_repo::CompleteGraphAutoEnableArgs {
            entries: &result.entries,
            if_new_use: complete_if_new_use,
            if_new_ver: complete_if_new_ver,
            if_new_slot: rebuild_if_new_slot,
            root,
            repos,
            config,
        });
    let want_complete = complete_graph || (deep == portage_repo::Deep::NotRequested && auto_enable);
    // A first walk that already aborted (a masked or flip-only dependency) keeps its
    // report: Portage reaches `_complete_graph` only after `_create_graph()` succeeded.
    // Portage: depgraph.py, _resolve (select_files)
    let phase1_aborted = matches!(result.outcome, portage_repo::ResolveOutcome::Aborted { .. });
    let result = if phase1_aborted {
        result
    } else if want_complete {
        match handle(
            run_resolve(true, &locked_merges, true, &complete_reuse)
                .map_err(crate::error::Error::from),
        ) {
            Ok(mut result) => {
                if phase1_rebuild_if {
                    result.backtrack_restarts = result.backtrack_restarts.max(phase1_restarts);
                }
                result
            }
            Err(code) => {
                pretend_end_news_notice(pretend, repos, root, config, color);
                return Err(code);
            }
        }
    } else if auto_enable {
        // `--deep` in force (the only way to reach here with `auto_enable`
        // true): re-run the phase-1 walk with the seeds fed in. A
        // deterministic input means an identical walk; the only delta is
        // the slot-operator-rebuild scan now seeing `slot_op_reachable`.
        // Portage: depgraph.py, _complete_graph
        match handle(run_resolve(false, &[], true, &empty_reuse).map_err(crate::error::Error::from))
        {
            Ok(mut result) => {
                if phase1_rebuild_if {
                    result.backtrack_restarts = result.backtrack_restarts.max(phase1_restarts);
                }
                result
            }
            Err(code) => {
                pretend_end_news_notice(pretend, repos, root, config, color);
                return Err(code);
            }
        }
    } else {
        result
    };
    // Repaint running-rooted rows from the running root's own profile,
    // merge entries and abort-partial rows alike. Runs only on a genuine
    // cross-root run with a loaded running profile; single-root runs keep
    // the resolve-time target paint.
    let mut result = result;
    if let (Some(running), Some(running_cfg)) =
        (root_deps_running_root.as_deref(), running_config.as_ref())
    {
        portage_repo::repaint_running_root_entry_displays(
            &mut result.entries,
            repos,
            running,
            running_cfg,
        );
        if let portage_repo::ResolveOutcome::Aborted { partial, .. } = &mut result.outcome {
            portage_repo::repaint_running_root_entry_displays(partial, repos, running, running_cfg);
        }
    }
    Ok(result)
}
