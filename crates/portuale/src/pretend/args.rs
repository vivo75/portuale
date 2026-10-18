//! Parsing `emerge` arguments into [`EmergeOpts`].

use crate::pretend::*;

/// The options `run` reads, parsed from argv (`EMERGE_DEFAULT_OPTS`
/// first, so argv wins for last-wins options).
pub(crate) struct EmergeOpts<'a> {
    pub(crate) atom_args: Vec<&'a str>,
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) read_news: bool,
    pub(crate) resume: bool,
    pub(crate) skipfirst: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) nodeps: bool,
    pub(crate) onlydeps: bool,
    pub(crate) onlydeps_with_rdeps: bool,
    pub(crate) onlydeps_with_ideps: bool,
    pub(crate) oneshot: bool,
    pub(crate) tree: bool,
    pub(crate) unordered_display: bool,
    pub(crate) columns: bool,
    pub(crate) alphabetical: bool,
    pub(crate) color_opt: Option<bool>,
    pub(crate) lib_check: bool,
    pub(crate) update: bool,
    pub(crate) deep: portage_repo::Deep,
    pub(crate) excluded: Vec<String>,
    pub(crate) reinstall_atoms: Vec<String>,
    pub(crate) useoldpkg_atoms: Vec<String>,
    pub(crate) rebuild_if_new_slot: bool,
    pub(crate) rebuild_if_unbuilt: Option<bool>,
    pub(crate) rebuild_if_new_rev: Option<bool>,
    pub(crate) rebuild_if_new_ver: Option<bool>,
    pub(crate) binpkg_changed_deps_override: Option<bool>,
    pub(crate) use_ebuild_visibility: bool,
    pub(crate) rebuild_exclude: Vec<String>,
    pub(crate) rebuild_ignore: Vec<String>,
    pub(crate) complete_graph: bool,
    pub(crate) ignore_world: bool,
    pub(crate) complete_if_new_use: Option<bool>,
    pub(crate) complete_if_new_ver: Option<bool>,
    pub(crate) dynamic_deps: bool,
    pub(crate) misspell_suggestions: bool,
    pub(crate) package_moves: bool,
    pub(crate) implicit_system_deps: bool,
    pub(crate) usepkg_exclude: Vec<String>,
    pub(crate) usepkg_include: Vec<String>,
    pub(crate) getbinpkg_exclude: Vec<String>,
    pub(crate) getbinpkg_include: Vec<String>,
    pub(crate) json: bool,
    pub(crate) deselect: bool,
    pub(crate) deselect_n: bool,
    pub(crate) unmerge: bool,
    pub(crate) depclean: bool,
    pub(crate) prune: bool,
    pub(crate) config_action: bool,
    pub(crate) list_sets: bool,
    pub(crate) search_action: bool,
    pub(crate) searchdesc: bool,
    pub(crate) fuzzy_search: bool,
    pub(crate) regex_search_auto: bool,
    pub(crate) search_similarity: f64,
    pub(crate) check_news: bool,
    pub(crate) regen_action: bool,
    pub(crate) metadata_action: bool,
    pub(crate) sync_action: bool,
    pub(crate) clean_action: bool,
    pub(crate) rage_clean: bool,
    pub(crate) info_action: bool,
    pub(crate) with_bdeps: bool,
    pub(crate) with_bdeps_given: bool,
    pub(crate) with_bdeps_auto: bool,
    pub(crate) changed_deps: bool,
    pub(crate) changed_slot: bool,
    pub(crate) newrepo: bool,
    pub(crate) emptytree: bool,
    pub(crate) buildpkgonly: bool,
    pub(crate) buildpkg_opt: Option<bool>,
    pub(crate) quiet_build: Option<bool>,
    pub(crate) buildpkg_exclude: Vec<String>,
    pub(crate) keep_going: bool,
    pub(crate) debug: bool,
    pub(crate) jobs: usize,
    pub(crate) load_average: Option<f64>,
    pub(crate) root_deps: bool,
    pub(crate) with_test_deps: bool,
    pub(crate) changed_deps_report: bool,
    pub(crate) verbose_slot_rebuilds: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) backtrack_max: u32,
    pub(crate) solver: portage_repo::SolverKind,
    pub(crate) verbose_conflicts: bool,
    pub(crate) autounmask: Option<bool>,
    pub(crate) autounmask_keep_keywords: Option<bool>,
    pub(crate) autounmask_use: Option<bool>,
    pub(crate) autounmask_license: Option<bool>,
    pub(crate) autounmask_keep_masks: Option<bool>,
    pub(crate) autounmask_only: bool,
    pub(crate) autounmask_continue: Option<bool>,
    pub(crate) autounmask_backtrack: Option<bool>,
    pub(crate) usepkg: bool,
    // Whether `--usepkg`/`-k` was given (any value, incl. `=n` and via
    // `EMERGE_DEFAULT_OPTS`, which parses through the same function).
    // Portage tests key presence (`"--usepkg" not in myopts`), so an
    // explicit `=n` still turns bdeps-auto off; the value alone cannot.
    pub(crate) usepkg_given: bool,
    pub(crate) usepkgonly: bool,
    pub(crate) getbinpkg: bool,
    // Whether `--getbinpkg`/`-g` was given (any value). Only read to tell
    // an explicit `=n` (which vetoes the `FEATURES=getbinpkg` fold) apart
    // from the untouched default.
    pub(crate) getbinpkg_given: bool,
    pub(crate) getbinpkgonly: bool,
    pub(crate) binpkg_respect_use: Option<bool>,
    pub(crate) quickpkg_direct: Option<bool>,
    pub(crate) quickpkg_direct_root: Option<String>,
    pub(crate) rebuilt_binaries: Option<bool>,
    pub(crate) rebuilt_binaries_timestamp: Option<u64>,
    pub(crate) noreplace: bool,
    pub(crate) selective_flag: Option<bool>,
    pub(crate) shell: ebuild_phases::ShellBackend,
}

impl<'a> Default for EmergeOpts<'a> {
    fn default() -> Self {
        Self {
            atom_args: Vec::new(),
            pretend: false,
            // --ask/-a: prompt after the list, before anything executes. Bare Enter
            // = Yes; "No"/EOF/SIGINT exits 130. Ignored under `--pretend`.
            ask: false,
            // --read-news: with `--ask`, offer `eselect news read` while calculating
            // dependencies. Alone it changes nothing.
            read_news: false,
            // --resume/-r replays the previous failed merge list; --skipfirst drops
            // its first entry. Portuale writes that list when a source merge fails.
            // Portage: _emerge/Scheduler.py
            resume: false,
            skipfirst: false,
            verbose: false,
            quiet: false,
            newuse: false,
            changed_use: false,
            nodeps: false,
            onlydeps: false,
            // `--onlydeps-with-rdeps`/`--onlydeps-with-ideps`: keep the key class for
            // an `--onlydeps` root (defaults: rdeps on, ideps off). Inert without
            // `--onlydeps`.
            onlydeps_with_rdeps: true,
            onlydeps_with_ideps: false,
            // --oneshot/-1: don't record the target in world on a merge, and don't
            // colour it as a would-be world member at --pretend.
            oneshot: false,
            // --tree/-t and --unordered-display: display-only, independent of
            // resolution (see `print_tree`).
            tree: false,
            unordered_display: false,
            // --columns: display-only like --tree; mutually exclusive with --tree
            // (checked once parsing finishes).
            columns: false,
            // --alphabetical: display-only USE ordering (see `use_suffix`).
            alphabetical: false,
            // --color y|n: a REQUIRED value, not an optional-value flag. `None` =
            // not given (fall through to the NO_COLOR/NOCOLOR/isatty gate).
            color_opt: None,
            // --depclean-lib-check y|n (default y): the `NEEDED.ELF.2` scan for
            // `--depclean`/`--prune`.
            lib_check: true,
            update: false,
            deep: portage_repo::Deep::NotRequested,
            excluded: Vec::new(),
            // --reinstall-atoms / --useoldpkg-atoms: repeatable, space-separated per
            // occurrence, like --exclude. A matching installed package is forced to
            // re-merge / prefers its binary over a newer ebuild.
            reinstall_atoms: Vec::new(),
            useoldpkg_atoms: Vec::new(),
            // --rebuild-if-new-slot (default y): the `:=` slot-operator auto-rebuild.
            rebuild_if_new_slot: true,
            // --rebuild-if-unbuilt / --rebuild-if-new-rev / --rebuild-if-new-ver (all
            // off by default). `Option` so the precedence below can resolve once
            // parsing finishes (`unbuilt` clears rev+ver; `rev` clears ver).
            rebuild_if_unbuilt: None,
            rebuild_if_new_rev: None,
            rebuild_if_new_ver: None,
            // `--binpkg-changed-deps[=y|n]`: `None` = auto (on unless `--usepkgonly`).
            // `--use-ebuild-visibility[=y|n]` (default off).
            binpkg_changed_deps_override: None,
            use_ebuild_visibility: false,
            // --rebuild-exclude (parent skipped) / --rebuild-ignore (dep never
            // triggers): same repeatable/space-separated shape as --exclude.
            rebuild_exclude: Vec::new(),
            rebuild_ignore: Vec::new(),
            // --complete-graph (off by default): force the deep walk over all known
            // dependencies. `--rebuild-if-*` imply it; `--nodeps` pops it.
            // Portage: create_depgraph_params.py
            complete_graph: false,
            // --ignore-world: drop `@world` (with `@selected`/`@system`/`@profile`)
            // from the complete-graph seeds, and skip the `@selected` uninstall
            // guard. No removal-action effect.
            ignore_world: false,
            // --complete-graph-if-new-use / --complete-graph-if-new-ver: both default
            // ON; only `=n` opts out. `None` = never given (on).
            complete_if_new_use: None,
            complete_if_new_ver: None,
            // --dynamic-deps (default y): an AlreadyInstalled package's `--deep` walk
            // reads the current ebuild; `=n` reads the vdb snapshot. `--nodeps`
            // forces it off.
            dynamic_deps: true,
            // --misspell-suggestions (default y): suggest close package names for a
            // nonexistent top-level `cat/pkg`. Only `=n` disables it.
            misspell_suggestions: true,
            // --package-moves (default y): whether `profiles/updates/` moves apply.
            // A process-global, like `--color`.
            package_moves: true,
            // --implicit-system-deps (default y): `@system` members count as implicit
            // dependencies. With `=n` the @system-first merge-order bias is skipped.
            implicit_system_deps: true,
            // --usepkg-exclude/--usepkg-include: same append shape as --exclude, but
            // scoped to binary-candidate eligibility (see `filter_usepkg_exclude_include`).
            usepkg_exclude: Vec::new(),
            usepkg_include: Vec::new(),
            // --getbinpkg-exclude/--getbinpkg-include: same append shape as
            // --exclude, validated with the strict family rule (see
            // `validate_exclude_options`). The refresh layer folds them
            // with each binrepo's own keys and filters the remote index
            // before candidates are read (S0 G cells).
            getbinpkg_exclude: Vec::new(),
            getbinpkg_include: Vec::new(),
            json: false,
            deselect: false,
            // `--deselect=n` (distinct from "not given"): consulted by `--depclean
            // <atoms>` (default keep-behavior on). Never triggers deselect itself.
            deselect_n: false,
            // --unmerge/-C, --depclean/-c, --prune/-P, --config: standalone actions.
            unmerge: false,
            depclean: false,
            prune: false,
            config_action: false,
            // --list-sets / --search / --searchdesc / --check-news: standalone
            // read-only query actions.
            list_sets: false,
            search_action: false,
            searchdesc: false,
            // --fuzzy-search (default y): `emerge --search`'s `SequenceMatcher` fuzzy
            // matching. Only `--fuzzy-search=n` disables it.
            fuzzy_search: true,
            // --regex-search-auto (default y): auto-detect a regex search key.
            regex_search_auto: true,
            // --search-similarity (a float 0..100, default 80): the minimum fuzzy-hit
            // ratio percentage.
            search_similarity: 80.0,
            check_news: false,
            // --regen: regenerate every repo's `metadata/md5-cache` (rejects
            // `--pretend`). --metadata transfers a repo's cache (a no-op here:
            // portuale reads `metadata/md5-cache` directly, with no `depcachedir`).
            // --sync is a permanent non-goal (moved to `emaint sync`).
            regen_action: false,
            metadata_action: false,
            sync_action: false,
            // --clean / --rage-clean: standalone removal actions. --rage-clean is a
            // fast --unmerge (same selection, skips CLEAN_DELAY + prerm/postrm);
            // --clean keeps only the newest version per slot.
            clean_action: false,
            rage_clean: false,
            // --info: a standalone read-only query action.
            info_action: false,
            with_bdeps: true,
            with_bdeps_given: false,
            with_bdeps_auto: true,
            changed_deps: false,
            changed_slot: false,
            // --newrepo: plain boolean, no value at all.
            newrepo: false,
            // --emptytree/-e: reinstall every atom in the deep dependency tree as
            // though nothing is installed.
            emptytree: false,
            // --buildpkgonly/-B: plain boolean, like --newrepo.
            buildpkgonly: false,
            // --buildpkg/-b: `None` = not given (fall back to `FEATURES=buildpkg`).
            // Only matters on a non-`--pretend` source merge.
            buildpkg_opt: None,
            // `--quiet-build[=y|n]`: redirect a build's phase output to
            // `${T}/build.log`. `None` = not given. Also implied by `-q` and `-j` >1.
            quiet_build: None,
            // --buildpkg-exclude: matching source entries are merged but not binpkg'd.
            buildpkg_exclude: Vec::new(),
            // --keep-going: bare/`y` form only; only meaningful alongside
            // `--buildpkgonly` (without `--pretend`).
            keep_going: false,
            // --debug/-d: `PORTAGE_DEBUG=1` in the phase env for a build; a no-op
            // for `--pretend`. Threaded into the build/merge/config/unmerge paths.
            debug: false,
            // --jobs[=N] / -j[N]: max concurrent package builds. Default 1 (serial);
            // bare `--jobs`/`-j` means unlimited; explicit `0` means the CPU count.
            // The vdb merge step stays serialized; only `--regen` and plain source
            // `emerge <atom>` consult this.
            jobs: 1,
            // --load-average[=LA] / -l[LA]: don't start an *additional* build while
            // the 1-minute load average is above `LA`. `None` = no throttle.
            // Only meaningful together with `--jobs` > 1.
            load_average: None,
            // --root-deps (+ bare form): running-root satisfiability for
            // DEPEND/BDEPEND/IDEPEND atoms. `=rdeps` is a no-op at EAPI 7+.
            // Portage: main.py; see `root_deps_satisfied_atoms`.
            root_deps: false,
            with_test_deps: false,
            changed_deps_report: false,
            // --verbose-slot-rebuilds (default y, NOT gated on `--verbose`): the "are
            // causing rebuilds:" block. Only `=n` suppresses it.
            verbose_slot_rebuilds: true,
            // --ignore-built-slot-operator-deps: skip the slot-operator auto-rebuild
            // scan entirely (debugging aid).
            ignore_built_slot_operator_deps: false,
            // --backtrack=COUNT: the resolver's retry ceiling after a solvable slot
            // conflict (`--backtrack=0` disables backtracking).
            backtrack_max: DEFAULT_BACKTRACK_MAX,
            // --solver=<portage|pubgrub|resolvo>: portuale-only pick of the solving
            // algorithm (default: the backtracking walk Portage runs).
            solver: portage_repo::SolverKind::Portage,
            // --verbose-conflicts: show *every* parent atom of a slot conflict.
            // Pure display; never reaches the resolver.
            verbose_conflicts: false,
            // --autounmask/--autounmask-keep-keywords: bare flag, `=y`/`=True`, or
            // `=n`. Defaults resolve below, where consumed.
            autounmask: None,
            autounmask_keep_keywords: None,
            autounmask_use: None,
            // --autounmask-license: REQUIRED value. OFF unless `--autounmask` is
            // explicit or `=y` is given.
            autounmask_license: None,
            // --autounmask-keep-masks: masks are KEPT by default; only `=n` unmasks.
            autounmask_keep_masks: None,
            // --autounmask-only: resolve the graph, show the problems, return 0 --
            // skipping the whole merge list. Nothing is ever built.
            autounmask_only: false,
            // --autounmask-continue (write the changes and keep merging; pretend-gated)
            // and --autounmask-backtrack (REQUIRED value, disabled by default).
            // `--autounmask-continue` also implies `--autounmask-backtrack=y`.
            // `None` = flag not given.
            autounmask_continue: None,
            autounmask_backtrack: None,
            // --usepkg/-k, --usepkgonly/-K, --binpkg-respect-use. The respect-use
            // default ("auto", effectively on without `--usepkgonly`) resolves below.
            usepkg: false,
            usepkg_given: false,
            usepkgonly: false,
            // --getbinpkg/-g, --getbinpkgonly/-G: folded into `usepkg`/`usepkgonly`;
            // `getbinpkg` additionally turns on *remote* candidate loading.
            getbinpkg: false,
            getbinpkg_given: false,
            getbinpkgonly: false,
            binpkg_respect_use: None,
            // --quickpkg-direct (+ REQUIRED value) + --quickpkg-direct-root: when
            // active, every package installed in the source root becomes a binary
            // candidate for the target-`ROOT` build.
            // Portage: actions.py
            quickpkg_direct: None,
            quickpkg_direct_root: None,
            // --rebuilt-binaries: auto-on when --usepkgonly/--deep/--update are ALL
            // given together. Resolved below, once those three are known.
            rebuilt_binaries: None,
            rebuilt_binaries_timestamp: None,
            noreplace: false,
            // `None` until an explicit `--selective[=y|n]` is given, so `n` can
            // override whatever the other triggers computed.
            selective_flag: None,
            // `--shell bash|brush` (default `bash`, portuale-only): which shell backend
            // executes phase chains. Inert under `--pretend`. Threaded into every
            // non-`--pretend` phase chain (merge, removal-hook, `--config`).
            shell: ebuild_phases::ShellBackend::default(),
        }
    }
}

/// An explicit `--jobs=0` means the CPU count. Shared by every `--jobs`
/// spelling below.
fn jobs_count(n: usize) -> usize {
    if n == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        n
    }
}

/// Prompt and process-wide flags. Returns whether the token was consumed.
fn parse_prompt_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--pretend" || arg == "-p" {
        opts.pretend = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--ask-enter-invalid" {
        // `UserQuery.query`'s `enter_invalid` gate, stored process-wide.
        ASK_ENTER_INVALID.store(true, std::sync::atomic::Ordering::Relaxed);
        *idx += 1;
        Ok(true)
    } else if arg == "--ignore-default-opts" {
        // Skip `EMERGE_DEFAULT_OPTS` entirely (handled on the raw argv);
        // accepted here as a no-op.
        // Portage: main.py
        *idx += 1;
        Ok(true)
    } else if arg == "--ask" || arg == "-a" {
        // Bare flag or `--ask=y`; a separate `y`/`n` word is consumed, a
        // separate `True` stays a positional atom (see `emerge_options`).
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.ask = true;
                *idx += 2;
            }
            Some("n") => {
                opts.ask = false;
                *idx += 2;
            }
            _ => {
                opts.ask = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--ask=y" || arg == "--ask=True" {
        opts.ask = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--ask=n" {
        opts.ask = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--read-news" {
        // Same optional-value shape as `--ask`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.read_news = true;
                *idx += 2;
            }
            Some("n") => {
                opts.read_news = false;
                *idx += 2;
            }
            _ => {
                opts.read_news = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--read-news=y" || arg == "--read-news=True" {
        opts.read_news = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--read-news=n" {
        opts.read_news = false;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Update-mode flags. Returns whether the token was consumed.
fn parse_mode_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--newuse" || arg == "-N" {
        opts.newuse = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--changed-use" || arg == "-U" {
        opts.changed_use = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--nodeps" || arg == "-O" {
        opts.nodeps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--onlydeps" || arg == "-o" {
        opts.onlydeps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--onlydeps-with-rdeps" {
        // A separate value is consumed only for `y`/`n`; `=True` is
        // accepted explicitly, any other value is a usage error.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.onlydeps_with_rdeps = true;
                *idx += 2;
            }
            Some("n") => {
                opts.onlydeps_with_rdeps = false;
                *idx += 2;
            }
            _ => {
                opts.onlydeps_with_rdeps = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--onlydeps-with-rdeps=") {
        match value {
            "y" | "True" => {
                opts.onlydeps_with_rdeps = true;
                *idx += 1;
            }
            "n" => {
                opts.onlydeps_with_rdeps = false;
                *idx += 1;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --onlydeps-with-rdeps: invalid choice: '{value}' \
                     (choose from 'True', 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else if arg == "--onlydeps-with-ideps" {
        // Same shape as `--onlydeps-with-rdeps`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.onlydeps_with_ideps = true;
                *idx += 2;
            }
            Some("n") => {
                opts.onlydeps_with_ideps = false;
                *idx += 2;
            }
            _ => {
                opts.onlydeps_with_ideps = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--onlydeps-with-ideps=") {
        match value {
            "y" | "True" => {
                opts.onlydeps_with_ideps = true;
                *idx += 1;
            }
            "n" => {
                opts.onlydeps_with_ideps = false;
                *idx += 1;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --onlydeps-with-ideps: invalid choice: '{value}' \
                     (choose from 'True', 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else if arg == "--oneshot" || arg == "-1" {
        opts.oneshot = true;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Display-only flags. Returns whether the token was consumed.
fn parse_display_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--tree" || arg == "-t" {
        opts.tree = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--unordered-display" {
        opts.unordered_display = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--columns" {
        opts.columns = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--alphabetical" {
        opts.alphabetical = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--color" || arg.starts_with("--color=") {
        // A REQUIRED `y`/`n` value, as `--color y` or `--color=y`.
        let val = if let Some(v) = arg.strip_prefix("--color=") {
            *idx += 1;
            v.to_string()
        } else if let Some(v) = args.get(*idx + 1) {
            *idx += 2;
            v.clone()
        } else {
            eprintln!("emerge: --color requires an argument (y or n)");
            return Err(ExitCode::from(2));
        };
        opts.color_opt = match val.as_str() {
            "y" => Some(true),
            "n" => Some(false),
            other => {
                return Err(parser_error(&format!(
                    "argument --color: invalid choice: '{other}' (choose from 'y', 'n')"
                )));
            }
        };
        Ok(true)
    } else if arg == "--depclean-lib-check" || arg.starts_with("--depclean-lib-check=") {
        // Bare (no value) means `y`.
        let val = if let Some(v) = arg.strip_prefix("--depclean-lib-check=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.lib_check = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Depth and backtrack limits. Returns whether the token was consumed.
fn parse_depth_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--update" || arg == "-u" {
        opts.update = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--noreplace" || arg == "-n" {
        // A plain boolean, no value; its only effect is setting selective.
        opts.noreplace = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--deep" || arg == "-D" {
        // Peeks at the next token, consuming it only if it parses as a
        // non-negative integer; otherwise unlimited depth.
        match args.get(*idx + 1).map(|s| s.parse::<u32>()) {
            Some(Ok(0)) => {
                opts.deep = portage_repo::Deep::NotRequested;
                *idx += 2;
            }
            Some(Ok(n)) => {
                opts.deep = portage_repo::Deep::Bounded(n);
                *idx += 2;
            }
            _ => {
                opts.deep = portage_repo::Deep::Unlimited;
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--deep=") {
        // The `=` form errors on a non-numeric value instead of leaving
        // the token alone.
        match value.parse::<u32>() {
            Ok(0) => {
                opts.deep = portage_repo::Deep::NotRequested;
                *idx += 1;
            }
            Ok(n) => {
                opts.deep = portage_repo::Deep::Bounded(n);
                *idx += 1;
            }
            Err(_) => {
                return Err(parser_error(&format!(
                    "Invalid --deep parameter: '{value}'\n"
                )));
            }
        }
        Ok(true)
    } else if arg == "--backtrack" {
        // Like `--deep`: the next token is consumed only if an integer;
        // a bare flag keeps the default.
        match args.get(*idx + 1).map(|s| s.parse::<u32>()) {
            Some(Ok(n)) => {
                opts.backtrack_max = n;
                *idx += 2;
            }
            _ => {
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--backtrack=") {
        // Like `--deep=`: a non-integer here is a parse error.
        match value.parse::<u32>() {
            Ok(n) => {
                opts.backtrack_max = n;
                *idx += 1;
            }
            Err(_) => {
                return Err(parser_error(&format!(
                    "Invalid --backtrack parameter: '{value}'\n"
                )));
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Repeatable atom-list flags. Returns whether the token was consumed.
fn parse_atom_list_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--exclude" || arg == "-X" {
        // Repeatable; each value is itself a space-separated atom list.
        // A missing value is a usage error.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--exclude\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.excluded
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--exclude=") {
        opts.excluded
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--reinstall-atoms" {
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--reinstall-atoms\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.reinstall_atoms
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--reinstall-atoms=") {
        opts.reinstall_atoms
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--useoldpkg-atoms" {
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--useoldpkg-atoms\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.useoldpkg_atoms
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--useoldpkg-atoms=") {
        opts.useoldpkg_atoms
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--rebuild-exclude" || arg == "--rebuild-ignore" {
        let is_exclude = arg == "--rebuild-exclude";
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"{arg}\" requires an argument");
            return Err(ExitCode::from(2));
        };
        let dst = if is_exclude {
            &mut opts.rebuild_exclude
        } else {
            &mut opts.rebuild_ignore
        };
        dst.extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--rebuild-exclude=") {
        opts.rebuild_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--rebuild-ignore=") {
        opts.rebuild_ignore
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Rebuild-trigger flags. Returns whether the token was consumed.
fn parse_rebuild_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--dynamic-deps" || arg.starts_with("--dynamic-deps=") {
        // A bare flag means `y` (Portage requires a value).
        let val = if let Some(v) = arg.strip_prefix("--dynamic-deps=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.dynamic_deps = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--misspell-suggestions" || arg.starts_with("--misspell-suggestions=") {
        let val = if let Some(v) = arg.strip_prefix("--misspell-suggestions=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.misspell_suggestions = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--package-moves" || arg.starts_with("--package-moves=") {
        let val = if let Some(v) = arg.strip_prefix("--package-moves=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.package_moves = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--implicit-system-deps" || arg.starts_with("--implicit-system-deps=") {
        // A bare flag means `y`.
        let val = if let Some(v) = arg.strip_prefix("--implicit-system-deps=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.implicit_system_deps = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--rebuild-if-new-slot" || arg.starts_with("--rebuild-if-new-slot=") {
        // Default "y"; only `=n` disables.
        let val = if let Some(v) = arg.strip_prefix("--rebuild-if-new-slot=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.rebuild_if_new_slot = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--rebuild-if-unbuilt"
        || arg.starts_with("--rebuild-if-unbuilt=")
        || arg == "--rebuild-if-new-rev"
        || arg.starts_with("--rebuild-if-new-rev=")
        || arg == "--rebuild-if-new-ver"
        || arg.starts_with("--rebuild-if-new-ver=")
    {
        // Bare / `=y` / `=True` -> on, `=n` -> off.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg, None),
        };
        let val = if let Some(v) = inline {
            *idx += 1;
            v
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        let on = matches!(val.as_str(), "y" | "True");
        match name {
            "--rebuild-if-unbuilt" => opts.rebuild_if_unbuilt = Some(on),
            "--rebuild-if-new-rev" => opts.rebuild_if_new_rev = Some(on),
            _ => opts.rebuild_if_new_ver = Some(on),
        }
        Ok(true)
    } else if arg == "--binpkg-changed-deps"
        || arg.starts_with("--binpkg-changed-deps=")
        || arg == "--use-ebuild-visibility"
        || arg.starts_with("--use-ebuild-visibility=")
    {
        // Same shape as the `--rebuild-if-*` block above; both are
        // resolver-time overrides of an automatic default.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg, None),
        };
        let val = if let Some(v) = inline {
            *idx += 1;
            v
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        let on = matches!(val.as_str(), "y" | "True");
        if name == "--binpkg-changed-deps" {
            opts.binpkg_changed_deps_override = Some(on);
        } else {
            opts.use_ebuild_visibility = on;
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Complete-graph and binary-eligibility flags. Returns whether the token was consumed.
fn parse_graph_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--complete-graph" || arg.starts_with("--complete-graph=") {
        // Bare / `=y` / `=True` -> on, `=n` -> off.
        let val = if let Some(v) = arg.strip_prefix("--complete-graph=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.complete_graph = matches!(val.as_str(), "y" | "True");
        Ok(true)
    } else if arg == "--ignore-world" || arg.starts_with("--ignore-world=") {
        // Same shape as `--complete-graph`.
        let val = if let Some(v) = arg.strip_prefix("--ignore-world=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.ignore_world = matches!(val.as_str(), "y" | "True");
        Ok(true)
    } else if arg == "--complete-graph-if-new-use"
        || arg.starts_with("--complete-graph-if-new-use=")
        || arg == "--complete-graph-if-new-ver"
        || arg.starts_with("--complete-graph-if-new-ver=")
    {
        // A bare flag means `y`; both default ON, only `=n` opts out.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg, None),
        };
        let val = if let Some(v) = inline {
            *idx += 1;
            v
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        let on = !matches!(val.as_str(), "n" | "N");
        if name == "--complete-graph-if-new-use" {
            opts.complete_if_new_use = Some(on);
        } else {
            opts.complete_if_new_ver = Some(on);
        }
        Ok(true)
    } else if arg == "--usepkg-exclude" {
        // Same append shape as --exclude; no short alias.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--usepkg-exclude\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.usepkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--usepkg-exclude=") {
        opts.usepkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--usepkg-include" {
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--usepkg-include\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.usepkg_include
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--usepkg-include=") {
        opts.usepkg_include
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkg-exclude" {
        // Same append shape as --exclude (Portage `action: "append"`).
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--getbinpkg-exclude\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.getbinpkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--getbinpkg-exclude=") {
        opts.getbinpkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkg-include" {
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--getbinpkg-include\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.getbinpkg_include
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--getbinpkg-include=") {
        opts.getbinpkg_include
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--buildpkg-exclude" {
        // Same append shape as --exclude.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--buildpkg-exclude\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.buildpkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--buildpkg-exclude=") {
        opts.buildpkg_exclude
            .extend(value.split_whitespace().map(String::from));
        *idx += 1;
        Ok(true)
    } else if arg == "--shell" || arg.starts_with("--shell=") {
        // Portuale-only; same `=`/space handling `--json` gets.
        let value = if let Some(v) = arg.strip_prefix("--shell=") {
            *idx += 1;
            v.to_string()
        } else if let Some(v) = args.get(*idx + 1) {
            *idx += 2;
            v.clone()
        } else {
            eprintln!("emerge: option '--shell' requires a value (bash or brush)");
            return Err(ExitCode::from(2));
        };
        opts.shell = match value.as_str() {
            "bash" => ebuild_phases::ShellBackend::Bash,
            "brush" => ebuild_phases::ShellBackend::Brush,
            other => {
                eprintln!("emerge: --shell: {other:?} is not \"bash\" or \"brush\"");
                return Err(ExitCode::from(1));
            }
        };
        Ok(true)
    } else if arg == "--solver" || arg.starts_with("--solver=") {
        // Portuale-only; same `=`/space handling as `--shell`.
        let value = if let Some(v) = arg.strip_prefix("--solver=") {
            *idx += 1;
            v.to_string()
        } else if let Some(v) = args.get(*idx + 1) {
            *idx += 2;
            v.clone()
        } else {
            eprintln!("emerge: option '--solver' requires a value (portage, pubgrub or resolvo)");
            return Err(ExitCode::from(2));
        };
        opts.solver = match portage_repo::SolverKind::parse(&value) {
            Some(kind) => kind,
            None => {
                eprintln!(
                    "emerge: --solver: {value:?} is not \"portage\", \"pubgrub\" or \"resolvo\""
                );
                return Err(ExitCode::from(2));
            }
        };
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Verbosity and deselect flags. Returns whether the token was consumed.
fn parse_output_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--json" {
        // Portuale-only structured-output mode, so not routed through
        // emerge_options.rs's CLI-surface tables; no short alias.
        opts.json = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose" || arg == "-v" {
        // Peeks at the next token, consuming it only if exactly `y`/`n`
        // (see `emerge_options`' `insert_optional_args` handling).
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.verbose = true;
                *idx += 2;
            }
            Some("n") => {
                opts.verbose = false;
                *idx += 2;
            }
            _ => {
                opts.verbose = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--verbose=y" || arg == "--verbose=True" {
        opts.verbose = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose=n" {
        opts.verbose = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--quiet" || arg == "-q" {
        // Same optional-value shape as `--verbose`; sets verbosity to 1
        // (see `use_suffix` / `attr_display_field` for what that changes).
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.quiet = true;
                *idx += 2;
            }
            Some("n") => {
                opts.quiet = false;
                *idx += 2;
            }
            _ => {
                opts.quiet = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--quiet=y" || arg == "--quiet=True" {
        opts.quiet = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--quiet=n" {
        opts.quiet = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--deselect" || arg == "-W" {
        // Same optional-value shape as `--verbose`, but a bare flag turns
        // this into the standalone deselect action (see `run_deselect`).
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.deselect = true;
                *idx += 2;
            }
            Some("n") => {
                opts.deselect = false;
                opts.deselect_n = true;
                *idx += 2;
            }
            _ => {
                opts.deselect = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--deselect=y" || arg == "--deselect=True" {
        opts.deselect = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--deselect=n" {
        opts.deselect = false;
        opts.deselect_n = true;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Removal actions and with-bdeps flags. Returns whether the token was consumed.
fn parse_action_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--unmerge" || arg == "-C" {
        opts.unmerge = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--depclean" || arg == "-c" {
        opts.depclean = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--prune" || arg == "-P" {
        opts.prune = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--config" {
        opts.config_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--with-bdeps" {
        // A REQUIRED closed-choice value (`y`/`n`): a missing or other
        // value is an immediate usage error, with no default to fall back to.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--with-bdeps\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.as_str() {
            "y" => {
                opts.with_bdeps = true;
                opts.with_bdeps_given = true;
                *idx += 2;
            }
            "n" => {
                opts.with_bdeps = false;
                opts.with_bdeps_given = true;
                *idx += 2;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --with-bdeps: invalid choice: '{value}' (choose from 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--with-bdeps=") {
        match value {
            "y" => {
                opts.with_bdeps = true;
                opts.with_bdeps_given = true;
                *idx += 1;
            }
            "n" => {
                opts.with_bdeps = false;
                opts.with_bdeps_given = true;
                *idx += 1;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --with-bdeps: invalid choice: '{value}' (choose from 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else if arg == "--with-bdeps-auto" {
        // Same required closed-choice shape as `--with-bdeps`.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--with-bdeps-auto\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.as_str() {
            "y" => {
                opts.with_bdeps_auto = true;
                *idx += 2;
            }
            "n" => {
                opts.with_bdeps_auto = false;
                *idx += 2;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --with-bdeps-auto: invalid choice: '{value}' (choose from 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--with-bdeps-auto=") {
        match value {
            "y" => {
                opts.with_bdeps_auto = true;
                *idx += 1;
            }
            "n" => {
                opts.with_bdeps_auto = false;
                *idx += 1;
            }
            _ => {
                return Err(parser_error(&format!(
                    "argument --with-bdeps-auto: invalid choice: '{value}' (choose from 'y', 'n')"
                )));
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Optional-value boolean toggles. Returns whether the token was consumed.
fn parse_toggle_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--changed-deps" {
        // Same optional-value shape as `--verbose`; an ordinary
        // --pretend modifier, not a standalone action.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.changed_deps = true;
                *idx += 2;
            }
            Some("n") => {
                opts.changed_deps = false;
                *idx += 2;
            }
            _ => {
                opts.changed_deps = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--changed-deps=y" || arg == "--changed-deps=True" {
        opts.changed_deps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--changed-deps=n" {
        opts.changed_deps = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--changed-deps-report" {
        // Same shape as `--changed-deps`, but never changes what gets
        // reinstalled.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.changed_deps_report = true;
                *idx += 2;
            }
            Some("n") => {
                opts.changed_deps_report = false;
                *idx += 2;
            }
            _ => {
                opts.changed_deps_report = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--changed-deps-report=y" || arg == "--changed-deps-report=True" {
        opts.changed_deps_report = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--changed-deps-report=n" {
        opts.changed_deps_report = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose-conflicts" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.verbose_conflicts = true;
                *idx += 2;
            }
            Some("n") => {
                opts.verbose_conflicts = false;
                *idx += 2;
            }
            _ => {
                opts.verbose_conflicts = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--verbose-conflicts=y" {
        opts.verbose_conflicts = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose-conflicts=n" {
        opts.verbose_conflicts = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose-slot-rebuilds" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.verbose_slot_rebuilds = true;
                *idx += 2;
            }
            Some("n") => {
                opts.verbose_slot_rebuilds = false;
                *idx += 2;
            }
            _ => {
                opts.verbose_slot_rebuilds = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--verbose-slot-rebuilds=y" {
        opts.verbose_slot_rebuilds = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--verbose-slot-rebuilds=n" {
        opts.verbose_slot_rebuilds = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--ignore-built-slot-operator-deps" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.ignore_built_slot_operator_deps = true;
                *idx += 2;
            }
            Some("n") => {
                opts.ignore_built_slot_operator_deps = false;
                *idx += 2;
            }
            _ => {
                opts.ignore_built_slot_operator_deps = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--ignore-built-slot-operator-deps=y" {
        opts.ignore_built_slot_operator_deps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--ignore-built-slot-operator-deps=n" {
        opts.ignore_built_slot_operator_deps = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--selective" {
        // Same shape as `--changed-deps`. `n` explicitly CANCELS selective
        // even if another flag already set it (applied before resolving).
        // (`-n` is the separate bare-boolean `--noreplace` spelling.)
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.selective_flag = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.selective_flag = Some(false);
                *idx += 2;
            }
            _ => {
                opts.selective_flag = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--selective=y" || arg == "--selective=True" {
        opts.selective_flag = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--selective=n" {
        opts.selective_flag = Some(false);
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Build-output flags. Returns whether the token was consumed.
fn parse_build_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--changed-slot" {
        // Same shape as `--changed-deps`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.changed_slot = true;
                *idx += 2;
            }
            Some("n") => {
                opts.changed_slot = false;
                *idx += 2;
            }
            _ => {
                opts.changed_slot = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--changed-slot=y" || arg == "--changed-slot=True" {
        opts.changed_slot = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--changed-slot=n" {
        opts.changed_slot = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--newrepo" {
        opts.newrepo = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--emptytree" || arg == "-e" {
        opts.emptytree = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--buildpkgonly" || arg == "-B" {
        opts.buildpkgonly = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--buildpkg" || arg == "-b" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.buildpkg_opt = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.buildpkg_opt = Some(false);
                *idx += 2;
            }
            _ => {
                opts.buildpkg_opt = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--buildpkg=y" {
        opts.buildpkg_opt = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--buildpkg=n" {
        opts.buildpkg_opt = Some(false);
        *idx += 1;
        Ok(true)
    } else if arg == "--quiet-build" {
        // Bare / `y` -> redirect build output to `${T}/build.log`.
        // (`n` still captures under `-j` > 1.)
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.quiet_build = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.quiet_build = Some(false);
                *idx += 2;
            }
            _ => {
                opts.quiet_build = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--quiet-build=y" || arg == "--quiet-build=True" {
        opts.quiet_build = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--quiet-build=n" {
        opts.quiet_build = Some(false);
        *idx += 1;
        Ok(true)
    } else if arg == "--keep-going" {
        opts.keep_going = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--debug" || arg == "-d" {
        // Sets `PORTAGE_DEBUG=1` in the phase env for a build; a no-op
        // for `--pretend`.
        opts.debug = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--resume" || arg == "-r" {
        opts.resume = true;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Standalone query and removal actions. Returns whether the token was consumed.
fn parse_query_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--list-sets" {
        opts.list_sets = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--search" || arg == "-s" {
        // The search terms are the remaining positional args.
        opts.search_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--searchdesc" || arg == "-S" {
        // Implies `--search`, and also matches DESCRIPTION.
        opts.search_action = true;
        opts.searchdesc = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--fuzzy-search" || arg.starts_with("--fuzzy-search=") {
        let val = if let Some(v) = arg.strip_prefix("--fuzzy-search=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.fuzzy_search = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--regex-search-auto" || arg.starts_with("--regex-search-auto=") {
        let val = if let Some(v) = arg.strip_prefix("--regex-search-auto=") {
            *idx += 1;
            v.to_string()
        } else if matches!(args.get(*idx + 1).map(String::as_str), Some("y" | "n")) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.regex_search_auto = !matches!(val.as_str(), "n" | "N");
        Ok(true)
    } else if arg == "--search-similarity" || arg.starts_with("--search-similarity=") {
        let val = if let Some(v) = arg.strip_prefix("--search-similarity=") {
            *idx += 1;
            v.to_string()
        } else if let Some(v) = args.get(*idx + 1) {
            *idx += 2;
            v.clone()
        } else {
            eprintln!("emerge: option '--search-similarity' requires a value");
            return Err(ExitCode::from(2));
        };
        // Must parse as a float in [0, 100].
        match val.parse::<f64>() {
            Ok(n) if (0.0..=100.0).contains(&n) => opts.search_similarity = n,
            Ok(_) => {
                eprintln!(
                    "emerge: Invalid --search-similarity parameter (not between 0 and 100): '{val}'"
                );
                return Err(ExitCode::from(2));
            }
            Err(_) => {
                eprintln!("emerge: Invalid --search-similarity parameter (not a number): '{val}'");
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if arg == "--check-news" {
        opts.check_news = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--regen" {
        opts.regen_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--metadata" {
        opts.metadata_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--sync" {
        opts.sync_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--clean" {
        opts.clean_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--rage-clean" {
        opts.rage_clean = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--info" {
        opts.info_action = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--skipfirst" || arg == "--skip-first" {
        opts.skipfirst = true;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Parallelism and test-dep flags. Returns whether the token was consumed.
fn parse_jobs_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--jobs" || arg == "-j" {
        // Optional integer next token, like `--deep`; bare means unlimited.
        match args.get(*idx + 1).map(|s| s.parse::<usize>()) {
            Some(Ok(n)) => {
                opts.jobs = jobs_count(n);
                *idx += 2;
            }
            _ => {
                opts.jobs = usize::MAX;
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--jobs=") {
        match value.parse::<usize>() {
            Ok(n) => {
                opts.jobs = jobs_count(n);
                *idx += 1;
            }
            Err(_) => {
                return Err(parser_error(&format!(
                    "Invalid --jobs parameter: '{value}'\n"
                )));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("-j") {
        match value.parse::<usize>() {
            Ok(n) => {
                opts.jobs = jobs_count(n);
                *idx += 1;
            }
            Err(_) => {
                eprintln!("emerge: invalid -j parameter: {value:?}");
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if arg == "--load-average" || arg == "-l" {
        // A required value (unlike `--jobs`' optional one): missing or
        // non-numeric is a usage error.
        match args.get(*idx + 1).map(|s| s.parse::<f64>()) {
            Some(Ok(la)) if la > 0.0 => {
                opts.load_average = Some(la);
                *idx += 2;
            }
            _ => {
                eprintln!("emerge: option \"--load-average\" requires a positive number");
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if let Some(value) = arg
        .strip_prefix("--load-average=")
        .or_else(|| arg.strip_prefix("-l"))
    {
        match value.parse::<f64>() {
            Ok(la) if la > 0.0 => {
                opts.load_average = Some(la);
                *idx += 1;
            }
            _ => {
                return Err(parser_error(&format!(
                    "Invalid --load-average parameter: '{value}'\n"
                )));
            }
        }
        Ok(true)
    } else if arg == "--root-deps" || arg == "--root-deps=True" || arg == "--root-deps=rdeps" {
        opts.root_deps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--with-test-deps" {
        // Same shape as `--changed-deps`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.with_test_deps = true;
                *idx += 2;
            }
            Some("n") => {
                opts.with_test_deps = false;
                *idx += 2;
            }
            _ => {
                opts.with_test_deps = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--with-test-deps=y" || arg == "--with-test-deps=True" {
        opts.with_test_deps = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--with-test-deps=n" {
        opts.with_test_deps = false;
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Autounmask driver flags. Returns whether the token was consumed.
fn parse_autounmask_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--autounmask" {
        // Bare flag means true; three accepted true spellings otherwise.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.autounmask = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.autounmask = Some(false);
                *idx += 2;
            }
            _ => {
                opts.autounmask = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--autounmask=y" {
        opts.autounmask = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--autounmask=n" {
        opts.autounmask = Some(false);
        *idx += 1;
        Ok(true)
    } else if arg == "--autounmask-only" || arg.starts_with("--autounmask-only=") {
        let val = if let Some(v) = arg.strip_prefix("--autounmask-only=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.autounmask_only = matches!(val.as_str(), "y" | "True");
        Ok(true)
    } else if arg == "--autounmask-continue" || arg.starts_with("--autounmask-continue=") {
        let val = if let Some(v) = arg.strip_prefix("--autounmask-continue=") {
            *idx += 1;
            v.to_string()
        } else if matches!(
            args.get(*idx + 1).map(String::as_str),
            Some("y" | "n" | "True")
        ) {
            *idx += 2;
            args[*idx - 1].clone()
        } else {
            *idx += 1;
            "y".to_string()
        };
        opts.autounmask_continue = Some(matches!(val.as_str(), "y" | "True"));
        Ok(true)
    } else if arg == "--autounmask-backtrack" {
        // A REQUIRED `y`/`n` value; gates the backtrack loop's autounmask
        // re-drive. Disabled by default.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--autounmask-backtrack\" requires an argument");
            return Err(ExitCode::from(2));
        };
        if !matches!(value.as_str(), "y" | "n") {
            return Err(parser_error(&format!(
                "argument --autounmask-backtrack: invalid choice: '{value}' (choose from 'y', 'n')"
            )));
        }
        opts.autounmask_backtrack = Some(value == "y");
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--autounmask-backtrack=") {
        if !matches!(value, "y" | "n") {
            return Err(parser_error(&format!(
                "argument --autounmask-backtrack: invalid choice: '{value}' (choose from 'y', 'n')"
            )));
        }
        opts.autounmask_backtrack = Some(value == "y");
        *idx += 1;
        Ok(true)
    } else if arg == "--autounmask-keep-keywords" {
        // Same shape as `--changed-deps`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.autounmask_keep_keywords = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.autounmask_keep_keywords = Some(false);
                *idx += 2;
            }
            _ => {
                opts.autounmask_keep_keywords = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--autounmask-keep-keywords=") {
        match value {
            "y" | "True" => {
                opts.autounmask_keep_keywords = Some(true);
                *idx += 1;
            }
            "n" => {
                opts.autounmask_keep_keywords = Some(false);
                *idx += 1;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-keep-keywords\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Autounmask scope flags. Returns whether the token was consumed.
fn parse_autounmask_scope_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--autounmask-use" {
        // A REQUIRED `y`/`n` value.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--autounmask-use\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.as_str() {
            "y" => {
                opts.autounmask_use = Some(true);
                *idx += 2;
            }
            "n" => {
                opts.autounmask_use = Some(false);
                *idx += 2;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--autounmask-use=") {
        match value {
            "y" => {
                opts.autounmask_use = Some(true);
                *idx += 1;
            }
            "n" => {
                opts.autounmask_use = Some(false);
                *idx += 1;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if arg == "--autounmask-license" {
        // A REQUIRED `y`/`n` value, like `--autounmask-use`.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--autounmask-license\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.as_str() {
            "y" => {
                opts.autounmask_license = Some(true);
                *idx += 2;
            }
            "n" => {
                opts.autounmask_license = Some(false);
                *idx += 2;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-license\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--autounmask-license=") {
        match value {
            "y" => {
                opts.autounmask_license = Some(true);
                *idx += 1;
            }
            "n" => {
                opts.autounmask_license = Some(false);
                *idx += 1;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-license\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if arg == "--autounmask-keep-masks" {
        // Same shape as `--changed-deps`.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.autounmask_keep_masks = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.autounmask_keep_masks = Some(false);
                *idx += 2;
            }
            _ => {
                opts.autounmask_keep_masks = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--autounmask-keep-masks=") {
        match value {
            "y" | "True" => {
                opts.autounmask_keep_masks = Some(true);
                *idx += 1;
            }
            "n" => {
                opts.autounmask_keep_masks = Some(false);
                *idx += 1;
            }
            _ => {
                eprintln!(
                    "emerge: option \"--autounmask-keep-masks\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Binary-package flags. Returns whether the token was consumed.
fn parse_binpkg_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--usepkg" || arg == "-k" {
        opts.usepkg_given = true;
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.usepkg = true;
                *idx += 2;
            }
            Some("n") => {
                opts.usepkg = false;
                *idx += 2;
            }
            _ => {
                opts.usepkg = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--usepkg=y" {
        opts.usepkg_given = true;
        opts.usepkg = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--usepkg=n" {
        opts.usepkg_given = true;
        opts.usepkg = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--usepkgonly" || arg == "-K" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.usepkgonly = true;
                *idx += 2;
            }
            Some("n") => {
                opts.usepkgonly = false;
                *idx += 2;
            }
            _ => {
                opts.usepkgonly = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--usepkgonly=y" {
        opts.usepkgonly = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--usepkgonly=n" {
        opts.usepkgonly = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkg" || arg == "-g" {
        // Folded into `usepkg`, plus `getbinpkg` for remote candidates.
        opts.getbinpkg_given = true;
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.getbinpkg = true;
                *idx += 2;
            }
            Some("n") => {
                opts.getbinpkg = false;
                *idx += 2;
            }
            _ => {
                opts.getbinpkg = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--getbinpkg=y" {
        opts.getbinpkg_given = true;
        opts.getbinpkg = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkg=n" {
        opts.getbinpkg_given = true;
        opts.getbinpkg = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkgonly" || arg == "-G" {
        // Implies binary-only (`usepkgonly`); folded the same way.
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.getbinpkgonly = true;
                *idx += 2;
            }
            Some("n") => {
                opts.getbinpkgonly = false;
                *idx += 2;
            }
            _ => {
                opts.getbinpkgonly = true;
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--getbinpkgonly=y" {
        opts.getbinpkgonly = true;
        *idx += 1;
        Ok(true)
    } else if arg == "--getbinpkgonly=n" {
        opts.getbinpkgonly = false;
        *idx += 1;
        Ok(true)
    } else if arg == "--binpkg-respect-use" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.binpkg_respect_use = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.binpkg_respect_use = Some(false);
                *idx += 2;
            }
            _ => {
                opts.binpkg_respect_use = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--binpkg-respect-use=y" {
        opts.binpkg_respect_use = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--binpkg-respect-use=n" {
        opts.binpkg_respect_use = Some(false);
        *idx += 1;
        Ok(true)
    } else if arg == "--rebuilt-binaries" {
        match args.get(*idx + 1).map(String::as_str) {
            Some("y") => {
                opts.rebuilt_binaries = Some(true);
                *idx += 2;
            }
            Some("n") => {
                opts.rebuilt_binaries = Some(false);
                *idx += 2;
            }
            _ => {
                opts.rebuilt_binaries = Some(true);
                *idx += 1;
            }
        }
        Ok(true)
    } else if arg == "--rebuilt-binaries=y" {
        opts.rebuilt_binaries = Some(true);
        *idx += 1;
        Ok(true)
    } else if arg == "--rebuilt-binaries=n" {
        opts.rebuilt_binaries = Some(false);
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Quickpkg and timestamp flags. Returns whether the token was consumed.
fn parse_quickpkg_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    args: &'a [String],
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if arg == "--rebuilt-binaries-timestamp" {
        // A required numeric Unix timestamp.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--rebuilt-binaries-timestamp\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.parse::<u64>() {
            Ok(n) => {
                opts.rebuilt_binaries_timestamp = Some(n);
                *idx += 2;
            }
            Err(_) => {
                eprintln!("emerge: invalid --rebuilt-binaries-timestamp parameter: {value:?}");
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--rebuilt-binaries-timestamp=") {
        match value.parse::<u64>() {
            Ok(n) => {
                opts.rebuilt_binaries_timestamp = Some(n);
                *idx += 1;
            }
            Err(_) => {
                eprintln!("emerge: invalid --rebuilt-binaries-timestamp parameter: {value:?}");
                return Err(ExitCode::from(2));
            }
        }
        Ok(true)
    } else if arg == "--quickpkg-direct" {
        // A REQUIRED `y`/`n` value, like `--autounmask-backtrack`.
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--quickpkg-direct\" requires an argument");
            return Err(ExitCode::from(2));
        };
        match value.as_str() {
            "y" => opts.quickpkg_direct = Some(true),
            "n" => opts.quickpkg_direct = Some(false),
            _ => {
                eprintln!(
                    "emerge: option \"--quickpkg-direct\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--quickpkg-direct=") {
        match value {
            "y" => opts.quickpkg_direct = Some(true),
            "n" => opts.quickpkg_direct = Some(false),
            _ => {
                eprintln!(
                    "emerge: option \"--quickpkg-direct\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
        }
        *idx += 1;
        Ok(true)
    } else if arg == "--quickpkg-direct-root" {
        let Some(value) = args.get(*idx + 1) else {
            eprintln!("emerge: option \"--quickpkg-direct-root\" requires an argument");
            return Err(ExitCode::from(2));
        };
        opts.quickpkg_direct_root = Some(value.clone());
        *idx += 2;
        Ok(true)
    } else if let Some(value) = arg.strip_prefix("--quickpkg-direct-root=") {
        opts.quickpkg_direct_root = Some(value.to_string());
        *idx += 1;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Positional atoms, bundles, unknown options. Returns whether the token was consumed.
fn parse_tail_opts<'a>(
    opts: &mut EmergeOpts<'a>,
    idx: &mut usize,
    arg: &'a str,
) -> Result<bool, ExitCode> {
    if !arg.starts_with('-') {
        opts.atom_args.push(arg);
        *idx += 1;
        Ok(true)
    } else if !arg.starts_with("--") && arg.len() > 2 {
        // Short-flag bundle (e.g. "-pv"): one character at a time, left to
        // right. A bundled flag never consumes a value (unlike standalone).
        for c in arg[1..].chars() {
            match c {
                'p' => opts.pretend = true,
                'a' => opts.ask = true,
                'v' => opts.verbose = true,
                'q' => opts.quiet = true,
                'N' => opts.newuse = true,
                'U' => opts.changed_use = true,
                'O' => opts.nodeps = true,
                'o' => opts.onlydeps = true,
                '1' => opts.oneshot = true,
                't' => opts.tree = true,
                'u' => opts.update = true,
                'n' => opts.noreplace = true,
                'd' => opts.debug = true,
                'D' => opts.deep = portage_repo::Deep::Unlimited,
                'e' => opts.emptytree = true,
                'k' => {
                    opts.usepkg = true;
                    opts.usepkg_given = true;
                }
                'K' => opts.usepkgonly = true,
                'g' => {
                    opts.getbinpkg = true;
                    opts.getbinpkg_given = true;
                }
                'G' => opts.getbinpkgonly = true,
                'W' => opts.deselect = true,
                'C' => opts.unmerge = true,
                'c' => opts.depclean = true,
                'P' => opts.prune = true,
                's' => opts.search_action = true,
                'S' => {
                    opts.search_action = true;
                    opts.searchdesc = true;
                }
                'B' => opts.buildpkgonly = true,
                'b' => opts.buildpkg_opt = Some(true),
                'X' => {
                    // -X requires a value, so it can't be bundled; error
                    // specifically instead of a misleading generic one.
                    eprintln!(
                        "emerge: -X (--exclude) requires an argument and can't be \
                             bundled with other short flags in portuale"
                    );
                    return Err(ExitCode::from(2));
                }
                _ => return Err(report_option(&format!("-{c}"))),
            }
        }
        *idx += 1;
        Ok(true)
    } else {
        Err(report_option(arg))
    }
}

/// Parses `args` into [`EmergeOpts`]. `Err` carries the exit code of an
/// option error, already reported.
pub(crate) fn parse_emerge_args(args: &[String]) -> Result<EmergeOpts<'_>, ExitCode> {
    let mut opts = EmergeOpts::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let _consumed = parse_prompt_opts(&mut opts, args, &mut i, arg)?
            || parse_mode_opts(&mut opts, args, &mut i, arg)?
            || parse_display_opts(&mut opts, args, &mut i, arg)?
            || parse_depth_opts(&mut opts, args, &mut i, arg)?
            || parse_atom_list_opts(&mut opts, args, &mut i, arg)?
            || parse_rebuild_opts(&mut opts, args, &mut i, arg)?
            || parse_graph_opts(&mut opts, args, &mut i, arg)?
            || parse_output_opts(&mut opts, args, &mut i, arg)?
            || parse_action_opts(&mut opts, args, &mut i, arg)?
            || parse_toggle_opts(&mut opts, args, &mut i, arg)?
            || parse_build_opts(&mut opts, args, &mut i, arg)?
            || parse_query_opts(&mut opts, args, &mut i, arg)?
            || parse_jobs_opts(&mut opts, args, &mut i, arg)?
            || parse_autounmask_opts(&mut opts, args, &mut i, arg)?
            || parse_autounmask_scope_opts(&mut opts, args, &mut i, arg)?
            || parse_binpkg_opts(&mut opts, args, &mut i, arg)?
            || parse_quickpkg_opts(&mut opts, args, &mut i, arg)?
            || parse_tail_opts(&mut opts, &mut i, arg)?;
    }
    Ok(opts)
}
