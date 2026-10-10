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

/// The values [`plan_resolve`] reads from its caller.
pub(crate) struct PlanResolveArgs<'a, 'o> {
    pub(crate) opts: &'a mut EmergeOpts<'o>,
    pub(crate) config: &'a mut Config,
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) config_root: &'a Path,
    pub(crate) color: &'a Colorizer,
}

/// The resolver parameters derived from the options: bdeps, selective, autounmask, binary packages, rebuild and complete-graph switches, seed atoms.
pub(crate) fn plan_resolve(args: PlanResolveArgs<'_, '_>) -> Result<ResolvePlan, ExitCode> {
    let PlanResolveArgs {
        opts,
        config,
        root,
        repos,
        config_root,
        color,
    } = args;
    let &mut EmergeOpts {
        with_bdeps_given,
        with_bdeps: ref mut with_bdeps_out,
        with_bdeps_auto,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
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
        quickpkg_direct,
        ref quickpkg_direct_root,
        binpkg_respect_use,
        rebuilt_binaries,
        deep,
        root_deps,
        pretend,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        dynamic_deps,
        nodeps,
        complete_graph,
        complete_if_new_use,
        complete_if_new_ver,
        ignore_world,
        ask,
        read_news,
        ..
    } = opts;
    let mut with_bdeps = *with_bdeps_out;
    // Real create_depgraph_params.py:97-103's own precedence: an explicit
    // --with-bdeps always wins; absent it, `bdeps` is set to "auto" only
    // when `--with-bdeps-auto != n` AND `--usepkg` is NOT in myopts --
    // and `--getbinpkg`/`--usepkgonly`/`--getbinpkgonly` all imply
    // `--usepkg` (actions.py:3716-3723). When `bdeps` is left unset (the
    // --usepkg case), depgraph.py:4196 treats it as "not in (y, auto)",
    // so `_add_pkg_dep_string` drops DEPEND/BDEPEND for every *built*
    // (binary or installed) package. `with_bdeps` carries exactly that
    // "keep build-time deps of a built package" bit through the resolver.
    if !with_bdeps_given {
        with_bdeps = with_bdeps_auto && !(usepkg || usepkgonly || getbinpkg || getbinpkgonly);
    }

    // Real create_depgraph_params.py's own `selective` condition,
    // computed from whichever of its real trigger flags portuale
    // implements -- see `resolve_pretend`'s own doc comment
    // (portage-repo) for the full grounding, including why
    // `--changed-use` alone covers portuale's whole share of real
    // `--reinstall`'s own contribution. `--newrepo` is one of real
    // create_depgraph_params.py's own listed triggers too (confirmed by
    // reading it, line ~147: `"--newrepo" in myopts`). An explicit
    // `--selective=n` unconditionally cancels it regardless of what the
    // other flags computed, matching real `create_depgraph_params.py`'s
    // own unconditional `if myopts.get("--selective") == "n": pop`,
    // checked last, after every other trigger.
    let selective = selective_flag.unwrap_or(
        update || newuse || changed_use || changed_deps || changed_slot || noreplace || newrepo,
    );

    // --autounmask/--autounmask-keep-keywords/--autounmask-use/-license:
    // real create_depgraph_params.py's own default-resolution logic,
    // simplified for portuale's own v1 scope (--autounmask-keep-masks
    // still isn't read, matching every real fixture/user who never
    // touches it getting the exact same outcome this simplification
    // produces). Real logic: `autounmask` itself defaults
    // to enabled (only `--autounmask=n` turns the whole feature off --
    // with `--autounmask-use`/`-license` now read, the "is
    // autounmask_use/license itself what makes
    // autounmask default true" branch in real create_depgraph_params.py
    // takes the "yes" arm whenever `--autounmask-use` isn't explicitly
    // "n", which portuale's own `autounmask_enabled` below still
    // simplifies to the same "only `--autounmask=n` turns it off"
    // shortcut -- a real, narrow gap only when `--autounmask-use=n` is
    // given *without* `--autounmask` itself, which real portage would
    // still leave `autounmask` enabled for (since keywords/masks aren't
    // read either) but is close enough in practice to be the same
    // pre-existing simplification, not a new one). `autounmask_keep_
    // keywords` (real: "suppress keyword suggestions") is subtler: it
    // defaults to suppressed (true) when `--autounmask` itself was NOT
    // explicitly given at all, but defaults to *not* suppressed (false,
    // i.e. keyword suggestions ARE generated) once `--autounmask` itself
    // WAS explicitly given (any value) -- real portage's own "explicitly
    // asking for autounmask implies wanting its keyword suggestions
    // too, but the ambient always-on default doesn't" asymmetry, ported
    // exactly. `autounmask_use` (real: "allow autounmask to change
    // package.use") has no such asymmetry at all -- real
    // `myparams["autounmask_keep_use"] = True if autounmask_use == "n"
    // else False`, unconditionally on (not suppressed) whenever
    // `--autounmask-use` isn't explicitly "n", regardless of whether
    // `--autounmask` itself was ever explicitly given. Either way, an
    // explicit `--autounmask-keep-keywords=y`/`=n` or
    // `--autounmask-use=y`/`=n` always wins outright.
    //
    // KNOWN GAP: real `autounmask_use` is also forced to `"n"` whenever
    // `myparams["binpkg_respect_use"] == "y"` (an explicit, literal
    // `--binpkg-respect-use=y`, not the "auto" default) -- portuale's
    // own `binpkg_respect_use` below is already a resolved bool by the
    // time it's available, with no way to distinguish "explicitly y"
    // from "auto-resolved to true", so that interaction isn't
    // reproduced. A real, narrow corner case: giving both
    // `--binpkg-respect-use=y` and relying on `--autounmask-use`'s own
    // default (rather than an explicit `=n`) at once.
    let autounmask_enabled = autounmask != Some(false);
    let autounmask_suggest_keywords = autounmask_enabled
        && match autounmask_keep_keywords {
            Some(keep) => !keep,
            None => autounmask.is_some(),
        };
    let autounmask_suggest_use = autounmask_enabled && autounmask_use != Some(false);
    // Real `create_depgraph_params.py`: `autounmask_license` defaults to
    // `"y"` only when `--autounmask` itself is explicitly True, else
    // `"n"`; `myparams["autounmask_keep_license"] = autounmask_license !=
    // "y"`. So the license kind is OFF by default (unlike USE) and only
    // an explicit `--autounmask` or `--autounmask-license=y` enables it.
    let autounmask_suggest_license =
        autounmask_enabled && autounmask_license.unwrap_or(autounmask == Some(true));
    // Real `create_depgraph_params.py`: `myparams["autounmask_keep_masks"]
    // = autounmask_keep_masks not in (None, "n")` -- masks stay masked
    // unless `--autounmask-keep-masks=n` is given explicitly.
    let autounmask_suggest_masks = autounmask_enabled && autounmask_keep_masks == Some(false);
    // Real `_backtrack_depgraph` (`depgraph.py:11736`): backtracking keeps
    // going after autounmask changes only with `--autounmask-backtrack=y`
    // -- or `--autounmask-continue` (which implies it), unless
    // `--autounmask-backtrack=n` is explicit. Off by default. Carried into
    // the resolver on `Config` (see `Config::autounmask_backtrack`).
    config.autounmask_backtrack = autounmask_enabled
        && match autounmask_backtrack {
            Some(v) => v,
            None => autounmask_continue == Some(true),
        };

    // Fold the --getbinpkg family into the --usepkg family (see their
    // parsing above): `--getbinpkgonly` implies binary-only; either
    // getbinpkg flag makes binary candidates eligible; `getbinpkg`
    // additionally turns on *remote* binrepo candidate loading.
    let usepkgonly = usepkgonly || getbinpkgonly;
    let usepkg = usepkg || getbinpkg || getbinpkgonly;
    let getbinpkg = getbinpkg || getbinpkgonly;

    // Real `bintree._populate_local`: when `--usepkg`/`--usepkgonly`
    // makes local binary candidates eligible, walk `$PKGDIR` for binpkg
    // files and build the pool from each file's own embedded metadata,
    // trusting any already-parsed `<pkgdir>/Packages` entry that
    // vouches for the file under real's own default (`FEATURES=
    // pkgdir-index-trusted` is in real `make.globals`' own default
    // `FEATURES`, so `_populate_local` runs with `reindex=False` and
    // leaves size/digest verification to merge-time `BinpkgVerifier`;
    // with `FEATURES=-pkgdir-index-trusted` only the `_mtime_`/`SIZE`
    // fast path is trusted and stale stanzas are dropped -- backlog
    // #199, `pkgdir_index_trusted` below threads the resolved
    // `FEATURES` in).
    // `binpkg::populate_local_pkgdir`'s own doc comment has the full
    // real grounding, backlog #174). Runs unconditionally now,
    // `Packages` present or not -- real portage always walks
    // `$PKGDIR` (a file no stanza vouches for is parsed from its own
    // bytes, rejected at scan when unreadable). Unlike real
    // portage this is NOT written back to `Packages` (see
    // `Config::scanned_binpkgs`) -- portuale recomputes each run, so
    // `--pretend` still writes nothing.
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

    // Real `actions.py:134-149`: `--quickpkg-direct` is active only when
    // `--usepkg` is on, `--quickpkg-direct=y` was given, AND the target
    // `ROOT` differs from the source root (`--quickpkg-direct-root`, else
    // the running root). When active, every package installed in the
    // source root joins the binary-candidate pool for the target build
    // (`portage_repo::set_quickpkg_direct_root` ->
    // `build_local_binpkg_index`). Path comparison is `canonicalize`-based --
    // real portage's full `os.path.abspath` (cwd-relative, non-existent
    // paths) is a minor documented cut; a real source root always exists.
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

    // --binpkg-respect-use: real default is "auto" (effectively on)
    // whenever --usepkgonly is NOT given, left off (unset/falsy) when it
    // IS -- create_depgraph_params.py:47-55, confirmed by reading it. An
    // explicit --binpkg-respect-use=y/=n always wins outright either way.
    let binpkg_respect_use = binpkg_respect_use.unwrap_or(!usepkgonly);

    // --rebuilt-binaries's own real default-resolution
    // (create_depgraph_params.py:185-193, confirmed by reading it):
    // `rebuilt_binaries is True or (rebuilt_binaries != "n" and
    // usepkgonly is True and deep is True and "--update" in myopts)` --
    // an explicit "=n" always wins outright (turns the auto-on
    // condition off too, not just the bare flag); an explicit bare/"=y"
    // always wins on; otherwise (never mentioned at all) it still
    // auto-enables once --usepkgonly, bare --deep (no explicit number --
    // `Deep::Unlimited`, real `myopts.get("--deep") is True`), and
    // --update are ALL given together. `--rebuilt-binaries-timestamp`
    // needs no separate default: `rebuilt_binary_changed` (portage-repo)
    // already treats "no timestamp given" as its own distinct branch.
    let rebuilt_binaries =
        rebuilt_binaries.unwrap_or(usepkgonly && deep == portage_repo::Deep::Unlimited && update);

    // See `resolve_root_deps_running_root`'s own doc comment: `BDEPEND`/
    // `IDEPEND` route to the running root whenever it differs from the
    // target `ROOT` (a cross-root/stage build), plus always under
    // `--root-deps`.
    let root_deps_running_root =
        resolve_root_deps_running_root(root_deps, root, portage_repo::running_root_from_env());

    // Backlog #242 Slice D: the running root's own profile for per-node
    // USE paint (see `load_running_config`). Loaded once per run, from
    // the effective running root — never the target config.
    let running_config = load_running_config(root, &portage_repo::running_root_from_env(), repos);

    // Real `make.globals`'s own `DISTDIR="/var/cache/distfiles"` --
    // env-var-sourced at this CLI boundary, the same "env var / hardcoded
    // default" shortcut `fetch.rs`'s own `FetchOptions::distdir` already
    // uses. Consulted only for the `f`/`F` fetch-restrict bracket column
    // (see `GraphEntry::fetch_restrict`, portage-repo).
    let distdir = std::env::var_os("DISTDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/distfiles"));

    // Real `bintree._populate_remote`: a `--getbinpkg` run refreshes
    // each binhost's `Packages` index into the local edb cache *before*
    // resolution, so the resolver picks up the live pool -- in
    // `--pretend` exactly like in a real merge (real `actions.py:3752`
    // passes `getbinpkg_refresh=True` unconditionally; `pretend` only
    // selects the stale-fallback message, never skips the fetch,
    // `bintree.py:917-920`, backlog #192). A failed refresh is non-fatal
    // (backlog #175): real warns (`!!! [<repo>] Error fetching ...`)
    // and resolves against the local pool, so this returns nothing --
    // it never aborts the run.
    if getbinpkg {
        emerge_getbinpkg::refresh_binhost_indexes(&config.binrepos, root, pretend);
    }

    // Real `main.py:958-975` precedence for the `--rebuild-if-*` trio:
    // `--rebuild-if-unbuilt` (=y) clears `new-rev` + `new-ver`;
    // `--rebuild-if-new-rev` (=y) clears `new-ver`. An explicit `=n`
    // leaves that one off; anything unset stays off.
    let rebuild_if_unbuilt = rebuild_if_unbuilt == Some(true);
    let rebuild_if_new_rev = !rebuild_if_unbuilt && rebuild_if_new_rev == Some(true);
    let rebuild_if_new_ver =
        !rebuild_if_unbuilt && !rebuild_if_new_rev && rebuild_if_new_ver == Some(true);

    // Real `create_depgraph_params.py:122`: `--nodeps` forces
    // `--dynamic-deps` off (there is no dep walk to apply it to).
    let dynamic_deps = dynamic_deps && !nodeps;

    // Real `create_depgraph_params.py:169-175`: `--complete-graph` *and*
    // any of `--rebuild-if-{unbuilt,new-rev,new-ver}` set
    // `myparams["complete"] = True`; `main.py:181-184` (`--nodeps`) pops
    // it back off. In portuale `complete` collapses to "force the deep
    // walk" -- see `resolve_pretend_graph`'s `complete` param.
    let complete_graph =
        (complete_graph || rebuild_if_unbuilt || rebuild_if_new_rev || rebuild_if_new_ver)
            && !nodeps;
    // Real `_complete_graph` 8581-8648: both `--complete-graph-if-new-*`
    // auto-enable triggers default ON (`myparams.get(..., "y")`).
    let complete_if_new_use = complete_if_new_use != Some(false);
    let complete_if_new_ver = complete_if_new_ver != Some(false);

    // Real `_complete_graph`'s required sets (`@world ∪ @selected ∪
    // @system`, and real `@world` itself is `@profile ∪ @selected ∪
    // @system` -- `_sets/__init__.py:97-98`): the resolver walks their
    // installed closure in complete mode to gate the
    // slot-operator-rebuild scan (real only slot-op-rebuilds a consumer
    // reachable from these). Computed once; `run_resolve` injects it into
    // a cloned `Config` only for the `complete = true` pass.
    // `--ignore-world` (real `depgraph.py:357-360`) empties
    // `_required_set_names`, so the seeds are empty here too -- the
    // complete walk then covers only the args' own deep dependencies
    // (real `man/emerge.1`). Explicit `@world`/`@selected` args still
    // expand above (real always processes `_initial_arg_list`); only the
    // seed sets drop out.
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
    // Backlog #196: real `_emerge/actions.py:4264-4270` (`run_action`):
    // the GLEP 42 news-count notice prints BEFORE resolution on a
    // non-`--pretend` run (after argument validation, before
    // `action_build`), so it fires even when resolution itself later
    // fails -- and, like real, it is NOT gated on `--quiet`, `--ask`,
    // or `--nodeps` (only `news` in FEATURES plus a nonzero unread
    // count, inside `display_news_notice_if_any`). The m185 probe shows
    // it standing first in real's pre-`>>>` output (news, blank,
    // header, `Calculating...`), which this placement reproduces: the
    // resolution-phase display block below runs after.
    if !pretend {
        let notice_printed = display_news_notice_if_any(repos, root, config, color);
        // Backlog #231 (c): real `actions.py:4266-4281` offers the
        // `eselect news read` spawn here, once the notice printed. An
        // interrupted news prompt (backlog #234) exits `128 + SIGINT`
        // before `action_build`, like real `UserQuery.query`'s own
        // `sys.exit` (`_emerge/UserQuery.py:74-76`).
        if offer_news_reading(ask, read_news, notice_printed, color) {
            return Err(ExitCode::from(130));
        }
    }
    *with_bdeps_out = with_bdeps;
    Ok(ResolvePlan {
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
    })
}
