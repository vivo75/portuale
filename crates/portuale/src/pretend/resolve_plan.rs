//! The resolver parameters derived from the options: bdeps, selective, autounmask, binary packages, rebuild and complete-graph switches, seed atoms.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;
use std::path::PathBuf;

/// What [`plan_resolve`] hands back to its caller.
pub(crate) struct ResolvePlan {
    pub(crate) selective: bool,
    pub(crate) autounmask_suggest_keywords: bool,
    pub(crate) autounmask_suggest_use: bool,
    pub(crate) autounmask_suggest_license: bool,
    pub(crate) autounmask_suggest_masks: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) usepkg: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) binpkg_respect_use: bool,
    pub(crate) rebuilt_binaries: bool,
    pub(crate) root_deps_running_root: Option<PathBuf>,
    pub(crate) running_config: Option<Config>,
    pub(crate) distdir: PathBuf,
    pub(crate) rebuild_if_unbuilt: bool,
    pub(crate) rebuild_if_new_rev: bool,
    pub(crate) rebuild_if_new_ver: bool,
    pub(crate) dynamic_deps: bool,
    pub(crate) complete_graph: bool,
    pub(crate) complete_if_new_use: bool,
    pub(crate) complete_if_new_ver: bool,
    pub(crate) complete_seed_atoms: Vec<String>,
}

/// Whether build-time deps of built packages are traversed: Portage's
/// `bdeps=auto` as a key-presence test on the post-implication options.
/// An explicit `--with-bdeps` always wins; otherwise `bdeps` is "auto"
/// only when `--with-bdeps-auto` is not `n` and the `--usepkg` key is
/// absent — absent meaning neither given on the command line (any value,
/// `=n` included, `EMERGE_DEFAULT_OPTS` included) nor implied `True` by
/// `--getbinpkg`/`--getbinpkgonly`/`--usepkgonly` or the
/// `FEATURES=getbinpkg` fold (which an explicit `--getbinpkg=n` vetoes).
/// When `bdeps` is left unset, Portage drops DEPEND/BDEPEND for every
/// built package, so `false` here means "do not traverse".
/// Portage: create_depgraph_params.py:97-103, _emerge/main.py:1109-1121,
/// _emerge/actions.py:3709-3723
pub(crate) fn plan_with_bdeps(opts: &EmergeOpts<'_>, features: &[String]) -> bool {
    if opts.with_bdeps_given {
        return opts.with_bdeps;
    }
    let getbinpkg = opts.getbinpkg
        || (features.iter().any(|t| t == "getbinpkg")
            && !(opts.getbinpkg_given && !opts.getbinpkg));
    opts.with_bdeps_auto
        && !(opts.usepkg_given || opts.usepkgonly || getbinpkg || opts.getbinpkgonly)
}

/// The values [`plan_resolve_live_prefix`] reads from its caller.
pub(crate) struct PlanResolveLiveArgs<'a, 'o> {
    pub(crate) opts: &'a EmergeOpts<'o>,
    pub(crate) config: &'a mut Config,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
}

/// Everything the old [`plan_resolve`] did with output or global effects, in
/// original order: the local `$PKGDIR` scan, the quickpkg global, the
/// running-root config, `DISTDIR`, the `--getbinpkg` refresh and the overlap
/// warnings, and the non-`--pretend` news notice. The `--pretend` plan cache
/// runs exactly this prefix live on every run (a hit replays only the
/// resolve+display bytes); the pure parameter derivation in
/// [`plan_resolve_pure`] never runs on a hit.
pub(crate) struct LivePrefix {
    pub(crate) usepkgonly: bool,
    pub(crate) usepkg: bool,
    pub(crate) getbinpkg: bool,
    pub(crate) root_deps_running_root: Option<PathBuf>,
    pub(crate) running_config: Option<Config>,
    pub(crate) distdir: PathBuf,
}

pub(crate) fn plan_resolve_live_prefix(
    args: PlanResolveLiveArgs<'_, '_>,
) -> Result<LivePrefix, ExitCode> {
    let PlanResolveLiveArgs {
        opts,
        config,
        root,
        repos,
    } = args;
    let &EmergeOpts {
        usepkg,
        usepkgonly,
        getbinpkg,
        ref getbinpkg_exclude,
        ref getbinpkg_include,
        getbinpkgonly,
        quickpkg_direct,
        ref quickpkg_direct_root,
        pretend,
        root_deps,
        ask,
        read_news,
        ..
    } = opts;

    // Fold the --getbinpkg family into the --usepkg family (see their
    // parsing above): `--getbinpkgonly` implies binary-only; either
    // getbinpkg flag makes binary candidates eligible; `getbinpkg`
    // additionally turns on *remote* binrepo candidate loading.
    let usepkgonly = usepkgonly || getbinpkgonly;
    let usepkg = usepkg || getbinpkg || getbinpkgonly;
    let getbinpkg = getbinpkg || getbinpkgonly;

    // When `--usepkg`/`--usepkgonly` makes local binary candidates
    // eligible, walk `$PKGDIR` for binpkg files (see
    // `binpkg::populate_local_pkgdir`). Runs unconditionally now;
    // unlike Portage this is never written back to `Packages`.
    // Portage: bintree.py, _populate_local
    if usepkg || usepkgonly {
        let pkgdir_path = Path::new(&config.pkgdir);
        match crate::binpkg::populate_local_pkgdir(pkgdir_path, pkgdir_index_trusted(config)) {
            Ok(entries) if !entries.is_empty() => config.scanned_binpkgs = Some(entries),
            Ok(_) => {}
            Err(e) => {
                eprintln!("emerge: scanning {}: {e}", config.pkgdir);
                return Err(ExitCode::from(1));
            }
        }
    }

    // `--quickpkg-direct` is active only with `--usepkg`, an explicit
    // `--quickpkg-direct=y`, and a target `ROOT` differing from the source
    // root. Path comparison is `canonicalize`-based.
    // Portage: _emerge/actions.py
    let quickpkg_source = if usepkg && quickpkg_direct == Some(true) {
        let norm = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let src = quickpkg_direct_root
            .as_deref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(portage_repo::running_root_from_env);
        Some(norm(&src)).filter(|s| *s != norm(root))
    } else {
        None
    };
    portage_repo::set_quickpkg_direct_root(quickpkg_source);

    // The running root's own profile for per-node USE paint. Loaded once
    // per run, from the effective running root, never the target config.
    let running_config = load_running_config(root, &portage_repo::running_root_from_env(), repos);

    // `DISTDIR` is config-rooted (calling env highest) for the `f`/`F`
    // fetch-restrict bracket column. Portage: config.py; cnf/make.globals:30
    let distdir = portage_repo::distdir_from_env_or_config(config);

    // A `--getbinpkg` run refreshes each binhost's `Packages` index before
    // resolution, in `--pretend` exactly like in a merge. A failed
    // refresh is non-fatal: warn and resolve against the local pool.
    // Portage: _emerge/actions.py, bintree.py
    if getbinpkg {
        let _refresh_span = portage_util::profile::span("refresh_binhost_indexes");
        emerge_getbinpkg::refresh_binhost_indexes(
            &config.binrepos,
            root,
            pretend,
            getbinpkg_exclude,
            getbinpkg_include,
        );
    }
    // Atoms in both CLI lists warn (after the refresh: Portage populates
    // before adjusting configs, so the per-repo warnings print first).
    // Portage: _emerge/actions.py, binpkg_selection_config
    emerge_getbinpkg::warn_getbinpkg_cli_overlap(getbinpkg_exclude, getbinpkg_include);

    // See `resolve_root_deps_running_root`: `BDEPEND`/`IDEPEND` route to
    // the running root on a cross-root build, plus always under `--root-deps`.
    let root_deps_running_root =
        resolve_root_deps_running_root(root_deps, root, portage_repo::running_root_from_env());

    // The GLEP 42 news-count notice prints before resolution on a
    // non-`--pretend` run, so it fires even when resolution later fails.
    // Not gated on `--quiet`, `--ask`, or `--nodeps`.
    // Portage: _emerge/actions.py, run_action
    if !pretend {
        let color = Colorizer::new(color::resolve_havecolor(opts.color_opt));
        let notice_printed = display_news_notice_if_any(repos, root, config, &color);
        // Offer the `eselect news read` spawn once the notice printed; an
        // interrupted prompt exits before `action_build`.
        // Portage: _emerge/actions.py, run_action
        if offer_news_reading(OfferNewsReadingArgs {
            ask,
            read_news,
            notice_printed,
            color: &color,
        }) {
            return Err(ExitCode::from(130));
        }
    }
    Ok(LivePrefix {
        usepkgonly,
        usepkg,
        getbinpkg,
        root_deps_running_root,
        running_config,
        distdir,
    })
}

/// The resolver parameters derived from the options: bdeps, selective,
/// autounmask, binary packages, rebuild and complete-graph switches, seed
/// atoms. Pure computation, no output: safe to skip on a plan-cache hit.
pub(crate) fn plan_resolve_pure(
    opts: &mut EmergeOpts<'_>,
    config: &mut Config,
    root: &Path,
    config_root: &Path,
    live: LivePrefix,
) -> ResolvePlan {
    // `--with-bdeps` resolves before the destructure below borrows `opts`.
    let with_bdeps = plan_with_bdeps(opts, &config_features_list(config));
    let &mut EmergeOpts {
        with_bdeps: ref mut with_bdeps_out,
        selective_flag,
        update,
        newuse,
        changed_use,
        changed_deps,
        changed_slot,
        noreplace,
        newrepo,
        autounmask,
        autounmask_keep_keywords,
        autounmask_use,
        autounmask_license,
        autounmask_keep_masks,
        autounmask_backtrack,
        autounmask_continue,
        binpkg_respect_use,
        rebuilt_binaries,
        deep,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        dynamic_deps,
        nodeps,
        complete_graph,
        complete_if_new_use,
        complete_if_new_ver,
        ignore_world,
        ..
    } = opts;
    // `--with-bdeps` already resolved above (`plan_with_bdeps`).
    // `selective` follows Portage's own trigger flags (see
    // `resolve_pretend_graph` for the full grounding). An explicit
    // `--selective=n` unconditionally cancels it, checked last.
    // Portage: create_depgraph_params.py
    let selective = selective_flag.unwrap_or(
        update || newuse || changed_use || changed_deps || changed_slot || noreplace || newrepo,
    );

    // --autounmask defaults follow Portage's key-presence implications
    // (`create_depgraph_params.py`): the `keep` flags default to kept,
    // but an explicit `--autounmask` (any value but `n`) clears the
    // keep-keywords/keep-masks defaults, so masks are proposed without
    // an explicit `--autounmask-keep-masks=n`. An explicit
    // `--autounmask-keep-keywords=y`/`=n`, `--autounmask-use=y`/`=n`, or
    // `--autounmask-keep-masks=y`/`=n` always wins outright. `autounmask`
    // itself defaults on; only `--autounmask=n` turns the whole feature
    // off. `autounmask_license`
    // is off by default (unlike USE) and only an explicit `--autounmask`
    // or `--autounmask-license=y` enables it.
    // Portage: create_depgraph_params.py
    //
    // Known gap: Portage also forces `autounmask_use` to `"n"` on an
    // explicit `--binpkg-respect-use=y`, which this can't distinguish from
    // the auto-resolved default; see the notes.
    let autounmask_enabled = autounmask != Some(false);
    let autounmask_suggest_keywords = autounmask_enabled
        && match autounmask_keep_keywords {
            Some(keep) => !keep,
            None => autounmask.is_some(),
        };
    let autounmask_suggest_use = autounmask_enabled && autounmask_use != Some(false);
    // `autounmask_license` is off by default (unlike USE) and only an
    // explicit `--autounmask` or `--autounmask-license=y` enables it.
    // Portage: create_depgraph_params.py
    let autounmask_suggest_license =
        autounmask_enabled && autounmask_license.unwrap_or(autounmask == Some(true));
    // Like keywords above: kept by default, cleared by an explicit
    // `--autounmask`, always overridden by an explicit `=y`/`=n`.
    // Portage: create_depgraph_params.py
    let autounmask_suggest_masks = autounmask_enabled
        && match autounmask_keep_masks {
            Some(keep) => !keep,
            None => autounmask.is_some(),
        };
    // Backtracking keeps going after autounmask changes only with
    // `--autounmask-backtrack=y`, or `--autounmask-continue` (which
    // implies it) unless `--autounmask-backtrack=n` is explicit. Off by
    // default; carried into the resolver on `Config`.
    // Portage: depgraph.py, _backtrack_depgraph
    config.autounmask_backtrack = autounmask_enabled
        && match autounmask_backtrack {
            Some(v) => v,
            None => autounmask_continue == Some(true),
        };

    // The folded `--usepkg` family comes from the live prefix (it ran the
    // scan and the refresh off the same values).
    let usepkgonly = live.usepkgonly;
    let usepkg = live.usepkg;
    let getbinpkg = live.getbinpkg;

    // --binpkg-respect-use defaults to "auto" (effectively on) whenever
    // `--usepkgonly` is not given, off when it is. An explicit
    // `--binpkg-respect-use=y`/`=n` always wins outright.
    // Portage: create_depgraph_params.py
    let binpkg_respect_use = binpkg_respect_use.unwrap_or(!usepkgonly);

    // --rebuilt-binaries auto-enables with `--usepkgonly` + bare `--deep`
    // + `--update` together. An explicit `=n`/`=y` always wins outright.
    // Portage: create_depgraph_params.py
    let rebuilt_binaries =
        rebuilt_binaries.unwrap_or(usepkgonly && deep == portage_repo::Deep::Unlimited && update);

    // The running root's own profile, `DISTDIR` and the root-deps routing
    // all come from the live prefix (it loaded them before the refresh).
    let LivePrefix {
        root_deps_running_root,
        running_config,
        distdir,
        ..
    } = live;

    // `--rebuild-if-unbuilt` (=y) clears `new-rev` + `new-ver`;
    // `--rebuild-if-new-rev` (=y) clears `new-ver`. Unset stays off.
    // Portage: _emerge/main.py
    let rebuild_if_unbuilt = rebuild_if_unbuilt == Some(true);
    let rebuild_if_new_rev = !rebuild_if_unbuilt && rebuild_if_new_rev == Some(true);
    let rebuild_if_new_ver =
        !rebuild_if_unbuilt && !rebuild_if_new_rev && rebuild_if_new_ver == Some(true);

    // `--nodeps` forces `--dynamic-deps` off (no dep walk to apply it to).
    // Portage: create_depgraph_params.py
    let dynamic_deps = dynamic_deps && !nodeps;

    // `--complete-graph` plus any `--rebuild-if-*` sets `complete` (which
    // collapses to "force the deep walk"); `--nodeps` pops it back off.
    // Portage: create_depgraph_params.py, _emerge/main.py
    let complete_graph =
        (complete_graph || rebuild_if_unbuilt || rebuild_if_new_rev || rebuild_if_new_ver)
            && !nodeps;
    // Both `--complete-graph-if-new-*` auto-enable triggers default on.
    // Portage: depgraph.py, _complete_graph
    let complete_if_new_use = complete_if_new_use != Some(false);
    let complete_if_new_ver = complete_if_new_ver != Some(false);

    // The complete walk's required seed sets (`@world` ∪ `@selected` ∪
    // `@system`): the resolver walks their installed closure in complete
    // mode to gate the slot-operator-rebuild scan. `--ignore-world`
    // empties the seeds; explicit `@world`/`@selected` args still expand.
    // Portage: depgraph.py, _complete_graph
    let complete_seed_atoms: Vec<String> = {
        if ignore_world {
            Vec::new()
        } else {
            let mut v = expand_selected(root, config_root).unwrap_or_default();
            v.extend(config.profile_packages.iter().cloned());
            v.extend(config.system_packages.iter().cloned());
            v.sort();
            v.dedup();
            v
        }
    };
    // The GLEP 42 notice for non-`--pretend` runs lives in the live
    // prefix (it prints before resolution even when resolution fails).
    *with_bdeps_out = with_bdeps;
    ResolvePlan {
        selective,
        autounmask_suggest_keywords,
        autounmask_suggest_use,
        autounmask_suggest_license,
        autounmask_suggest_masks,
        usepkgonly,
        usepkg,
        getbinpkg,
        binpkg_respect_use,
        rebuilt_binaries,
        root_deps_running_root,
        running_config,
        distdir,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        dynamic_deps,
        complete_graph,
        complete_if_new_use,
        complete_if_new_ver,
        complete_seed_atoms,
    }
}
