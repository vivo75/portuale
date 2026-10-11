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
    pub(crate) usepkgonly: bool,
    pub(crate) getbinpkg: bool,
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

/// Parses `args` into [`EmergeOpts`]. `Err` carries the exit code of an
/// option error, already reported.
pub(crate) fn parse_emerge_args(args: &[String]) -> Result<EmergeOpts<'_>, ExitCode> {
    let mut atom_args: Vec<&str> = Vec::new();
    let mut pretend = false;
    // --ask/-a: prompt after the list, before anything executes. Bare Enter
    // = Yes; "No"/EOF/SIGINT exits 130. Ignored under `--pretend`.
    let mut ask = false;
    // --read-news: with `--ask`, offer `eselect news read` while calculating
    // dependencies. Alone it changes nothing.
    let mut read_news = false;
    // --resume/-r replays the previous failed merge list; --skipfirst drops
    // its first entry. Portuale writes that list when a source merge fails.
    // Portage: _emerge/Scheduler.py
    let mut resume = false;
    let mut skipfirst = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut newuse = false;
    let mut changed_use = false;
    let mut nodeps = false;
    let mut onlydeps = false;
    // `--onlydeps-with-rdeps`/`--onlydeps-with-ideps`: keep the key class for
    // an `--onlydeps` root (defaults: rdeps on, ideps off). Inert without
    // `--onlydeps`.
    let mut onlydeps_with_rdeps = true;
    let mut onlydeps_with_ideps = false;
    // --oneshot/-1: don't record the target in world on a merge, and don't
    // colour it as a would-be world member at --pretend.
    let mut oneshot = false;
    // --tree/-t and --unordered-display: display-only, independent of
    // resolution (see `print_tree`).
    let mut tree = false;
    let mut unordered_display = false;
    // --columns: display-only like --tree; mutually exclusive with --tree
    // (checked once parsing finishes).
    let mut columns = false;
    // --alphabetical: display-only USE ordering (see `use_suffix`).
    let mut alphabetical = false;
    // --color y|n: a REQUIRED value, not an optional-value flag. `None` =
    // not given (fall through to the NO_COLOR/NOCOLOR/isatty gate).
    let mut color_opt: Option<bool> = None;
    // --depclean-lib-check y|n (default y): the `NEEDED.ELF.2` scan for
    // `--depclean`/`--prune`.
    let mut lib_check = true;
    let mut update = false;
    let mut deep = portage_repo::Deep::NotRequested;
    let mut excluded: Vec<String> = Vec::new();
    // --reinstall-atoms / --useoldpkg-atoms: repeatable, space-separated per
    // occurrence, like --exclude. A matching installed package is forced to
    // re-merge / prefers its binary over a newer ebuild.
    let mut reinstall_atoms: Vec<String> = Vec::new();
    let mut useoldpkg_atoms: Vec<String> = Vec::new();
    // --rebuild-if-new-slot (default y): the `:=` slot-operator auto-rebuild.
    let mut rebuild_if_new_slot = true;
    // --rebuild-if-unbuilt / --rebuild-if-new-rev / --rebuild-if-new-ver (all
    // off by default). `Option` so the precedence below can resolve once
    // parsing finishes (`unbuilt` clears rev+ver; `rev` clears ver).
    let mut rebuild_if_unbuilt: Option<bool> = None;
    let mut rebuild_if_new_rev: Option<bool> = None;
    let mut rebuild_if_new_ver: Option<bool> = None;
    // `--binpkg-changed-deps[=y|n]`: `None` = auto (on unless `--usepkgonly`).
    // `--use-ebuild-visibility[=y|n]` (default off).
    let mut binpkg_changed_deps_override: Option<bool> = None;
    let mut use_ebuild_visibility = false;
    // --rebuild-exclude (parent skipped) / --rebuild-ignore (dep never
    // triggers): same repeatable/space-separated shape as --exclude.
    let mut rebuild_exclude: Vec<String> = Vec::new();
    let mut rebuild_ignore: Vec<String> = Vec::new();
    // --complete-graph (off by default): force the deep walk over all known
    // dependencies. `--rebuild-if-*` imply it; `--nodeps` pops it.
    // Portage: create_depgraph_params.py
    let mut complete_graph = false;
    // --ignore-world: drop `@world` (with `@selected`/`@system`/`@profile`)
    // from the complete-graph seeds, and skip the `@selected` uninstall
    // guard. No removal-action effect.
    let mut ignore_world = false;
    // --complete-graph-if-new-use / --complete-graph-if-new-ver: both default
    // ON; only `=n` opts out. `None` = never given (on).
    let mut complete_if_new_use: Option<bool> = None;
    let mut complete_if_new_ver: Option<bool> = None;
    // --dynamic-deps (default y): an AlreadyInstalled package's `--deep` walk
    // reads the current ebuild; `=n` reads the vdb snapshot. `--nodeps`
    // forces it off.
    let mut dynamic_deps = true;
    // --misspell-suggestions (default y): suggest close package names for a
    // nonexistent top-level `cat/pkg`. Only `=n` disables it.
    let mut misspell_suggestions = true;
    // --package-moves (default y): whether `profiles/updates/` moves apply.
    // A process-global, like `--color`.
    let mut package_moves = true;
    // --implicit-system-deps (default y): `@system` members count as implicit
    // dependencies. With `=n` the @system-first merge-order bias is skipped.
    let mut implicit_system_deps = true;
    // --usepkg-exclude/--usepkg-include: same append shape as --exclude, but
    // scoped to binary-candidate eligibility (see `filter_usepkg_exclude_include`).
    let mut usepkg_exclude: Vec<String> = Vec::new();
    let mut usepkg_include: Vec<String> = Vec::new();
    let mut json = false;
    let mut deselect = false;
    // `--deselect=n` (distinct from "not given"): consulted by `--depclean
    // <atoms>` (default keep-behavior on). Never triggers deselect itself.
    let mut deselect_n = false;
    // --unmerge/-C, --depclean/-c, --prune/-P, --config: standalone actions.
    let mut unmerge = false;
    let mut depclean = false;
    let mut prune = false;
    let mut config_action = false;
    // --list-sets / --search / --searchdesc / --check-news: standalone
    // read-only query actions.
    let mut list_sets = false;
    let mut search_action = false;
    let mut searchdesc = false;
    // --fuzzy-search (default y): `emerge --search`'s `SequenceMatcher` fuzzy
    // matching. Only `--fuzzy-search=n` disables it.
    let mut fuzzy_search = true;
    // --regex-search-auto (default y): auto-detect a regex search key.
    let mut regex_search_auto = true;
    // --search-similarity (a float 0..100, default 80): the minimum fuzzy-hit
    // ratio percentage.
    let mut search_similarity: f64 = 80.0;
    let mut check_news = false;
    // --regen: regenerate every repo's `metadata/md5-cache` (rejects
    // `--pretend`). --metadata transfers a repo's cache (a no-op here:
    // portuale reads `metadata/md5-cache` directly, with no `depcachedir`).
    // --sync is a permanent non-goal (moved to `emaint sync`).
    let mut regen_action = false;
    let mut metadata_action = false;
    let mut sync_action = false;
    // --clean / --rage-clean: standalone removal actions. --rage-clean is a
    // fast --unmerge (same selection, skips CLEAN_DELAY + prerm/postrm);
    // --clean keeps only the newest version per slot.
    let mut clean_action = false;
    let mut rage_clean = false;
    // --info: a standalone read-only query action.
    let mut info_action = false;
    let mut with_bdeps = true;
    let mut with_bdeps_given = false;
    let mut with_bdeps_auto = true;
    let mut changed_deps = false;
    let mut changed_slot = false;
    // --newrepo: plain boolean, no value at all.
    let mut newrepo = false;
    // --emptytree/-e: reinstall every atom in the deep dependency tree as
    // though nothing is installed.
    let mut emptytree = false;
    // --buildpkgonly/-B: plain boolean, like --newrepo.
    let mut buildpkgonly = false;
    // --buildpkg/-b: `None` = not given (fall back to `FEATURES=buildpkg`).
    // Only matters on a non-`--pretend` source merge.
    let mut buildpkg_opt: Option<bool> = None;
    // `--quiet-build[=y|n]`: redirect a build's phase output to
    // `${T}/build.log`. `None` = not given. Also implied by `-q` and `-j` >1.
    let mut quiet_build: Option<bool> = None;
    // --buildpkg-exclude: matching source entries are merged but not binpkg'd.
    let mut buildpkg_exclude: Vec<String> = Vec::new();
    // --keep-going: bare/`y` form only; only meaningful alongside
    // `--buildpkgonly` (without `--pretend`).
    let mut keep_going = false;
    // --debug/-d: `PORTAGE_DEBUG=1` in the phase env for a build; a no-op
    // for `--pretend`. Threaded into the build/merge/config/unmerge paths.
    let mut debug = false;
    // --jobs[=N] / -j[N]: max concurrent package builds. Default 1 (serial);
    // bare `--jobs`/`-j` means unlimited; explicit `0` means the CPU count.
    // The vdb merge step stays serialized; only `--regen` and plain source
    // `emerge <atom>` consult this.
    let mut jobs: usize = 1;
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
    // --load-average[=LA] / -l[LA]: don't start an *additional* build while
    // the 1-minute load average is above `LA`. `None` = no throttle.
    // Only meaningful together with `--jobs` > 1.
    let mut load_average: Option<f64> = None;
    // --root-deps (+ bare form): running-root satisfiability for
    // DEPEND/BDEPEND/IDEPEND atoms. `=rdeps` is a no-op at EAPI 7+.
    // Portage: main.py; see `root_deps_satisfied_atoms`.
    let mut root_deps = false;
    let mut with_test_deps = false;
    let mut changed_deps_report = false;
    // --verbose-slot-rebuilds (default y, NOT gated on `--verbose`): the "are
    // causing rebuilds:" block. Only `=n` suppresses it.
    let mut verbose_slot_rebuilds = true;
    // --ignore-built-slot-operator-deps: skip the slot-operator auto-rebuild
    // scan entirely (debugging aid).
    let mut ignore_built_slot_operator_deps = false;
    // --backtrack=COUNT: the resolver's retry ceiling after a solvable slot
    // conflict (`--backtrack=0` disables backtracking).
    let mut backtrack_max: u32 = DEFAULT_BACKTRACK_MAX;
    // --solver=<portage|pubgrub|resolvo>: portuale-only pick of the solving
    // algorithm (default: the backtracking walk Portage runs).
    let mut solver = portage_repo::SolverKind::Portage;
    // --verbose-conflicts: show *every* parent atom of a slot conflict.
    // Pure display; never reaches the resolver.
    let mut verbose_conflicts = false;
    // --autounmask/--autounmask-keep-keywords: bare flag, `=y`/`=True`, or
    // `=n`. Defaults resolve below, where consumed.
    let mut autounmask: Option<bool> = None;
    let mut autounmask_keep_keywords: Option<bool> = None;
    let mut autounmask_use: Option<bool> = None;
    // --autounmask-license: REQUIRED value. OFF unless `--autounmask` is
    // explicit or `=y` is given.
    let mut autounmask_license: Option<bool> = None;
    // --autounmask-keep-masks: masks are KEPT by default; only `=n` unmasks.
    let mut autounmask_keep_masks: Option<bool> = None;
    // --autounmask-only: resolve the graph, show the problems, return 0 --
    // skipping the whole merge list. Nothing is ever built.
    let mut autounmask_only = false;
    // --autounmask-continue (write the changes and keep merging; pretend-gated)
    // and --autounmask-backtrack (REQUIRED value, disabled by default).
    // `--autounmask-continue` also implies `--autounmask-backtrack=y`.
    // `None` = flag not given.
    let mut autounmask_continue: Option<bool> = None;
    let mut autounmask_backtrack: Option<bool> = None;
    // --usepkg/-k, --usepkgonly/-K, --binpkg-respect-use. The respect-use
    // default ("auto", effectively on without `--usepkgonly`) resolves below.
    let mut usepkg = false;
    let mut usepkgonly = false;
    // --getbinpkg/-g, --getbinpkgonly/-G: folded into `usepkg`/`usepkgonly`;
    // `getbinpkg` additionally turns on *remote* candidate loading.
    let mut getbinpkg = false;
    let mut getbinpkgonly = false;
    let mut binpkg_respect_use: Option<bool> = None;
    // --quickpkg-direct (+ REQUIRED value) + --quickpkg-direct-root: when
    // active, every package installed in the source root becomes a binary
    // candidate for the target-`ROOT` build.
    // Portage: actions.py
    let mut quickpkg_direct: Option<bool> = None;
    let mut quickpkg_direct_root: Option<String> = None;
    // --rebuilt-binaries: auto-on when --usepkgonly/--deep/--update are ALL
    // given together. Resolved below, once those three are known.
    let mut rebuilt_binaries: Option<bool> = None;
    let mut rebuilt_binaries_timestamp: Option<u64> = None;
    let mut noreplace = false;
    // `None` until an explicit `--selective[=y|n]` is given, so `n` can
    // override whatever the other triggers computed.
    let mut selective_flag: Option<bool> = None;
    // `--shell bash|brush` (default `bash`, portuale-only): which shell backend
    // executes phase chains. Inert under `--pretend`. Threaded into every
    // non-`--pretend` phase chain (merge, removal-hook, `--config`).
    let mut shell = ebuild_phases::ShellBackend::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--pretend" || arg == "-p" {
            pretend = true;
            i += 1;
        } else if arg == "--ask-enter-invalid" {
            // `UserQuery.query`'s `enter_invalid` gate, stored process-wide.
            ASK_ENTER_INVALID.store(true, std::sync::atomic::Ordering::Relaxed);
            i += 1;
        } else if arg == "--ignore-default-opts" {
            // Skip `EMERGE_DEFAULT_OPTS` entirely (handled on the raw argv);
            // accepted here as a no-op.
            // Portage: main.py
            i += 1;
        } else if arg == "--ask" || arg == "-a" {
            // Bare flag or `--ask=y`; a separate `y`/`n` word is consumed, a
            // separate `True` stays a positional atom (see `emerge_options`).
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    ask = true;
                    i += 2;
                }
                Some("n") => {
                    ask = false;
                    i += 2;
                }
                _ => {
                    ask = true;
                    i += 1;
                }
            }
        } else if arg == "--ask=y" || arg == "--ask=True" {
            ask = true;
            i += 1;
        } else if arg == "--ask=n" {
            ask = false;
            i += 1;
        } else if arg == "--read-news" {
            // Same optional-value shape as `--ask`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    read_news = true;
                    i += 2;
                }
                Some("n") => {
                    read_news = false;
                    i += 2;
                }
                _ => {
                    read_news = true;
                    i += 1;
                }
            }
        } else if arg == "--read-news=y" || arg == "--read-news=True" {
            read_news = true;
            i += 1;
        } else if arg == "--read-news=n" {
            read_news = false;
            i += 1;
        } else if arg == "--newuse" || arg == "-N" {
            newuse = true;
            i += 1;
        } else if arg == "--changed-use" || arg == "-U" {
            changed_use = true;
            i += 1;
        } else if arg == "--nodeps" || arg == "-O" {
            nodeps = true;
            i += 1;
        } else if arg == "--onlydeps" || arg == "-o" {
            onlydeps = true;
            i += 1;
        } else if arg == "--onlydeps-with-rdeps" {
            // A separate value is consumed only for `y`/`n`; `=True` is
            // accepted explicitly, any other value is a usage error.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    onlydeps_with_rdeps = true;
                    i += 2;
                }
                Some("n") => {
                    onlydeps_with_rdeps = false;
                    i += 2;
                }
                _ => {
                    onlydeps_with_rdeps = true;
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--onlydeps-with-rdeps=") {
            match value {
                "y" | "True" => {
                    onlydeps_with_rdeps = true;
                    i += 1;
                }
                "n" => {
                    onlydeps_with_rdeps = false;
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--onlydeps-with-rdeps\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--onlydeps-with-ideps" {
            // Same shape as `--onlydeps-with-rdeps`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    onlydeps_with_ideps = true;
                    i += 2;
                }
                Some("n") => {
                    onlydeps_with_ideps = false;
                    i += 2;
                }
                _ => {
                    onlydeps_with_ideps = true;
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--onlydeps-with-ideps=") {
            match value {
                "y" | "True" => {
                    onlydeps_with_ideps = true;
                    i += 1;
                }
                "n" => {
                    onlydeps_with_ideps = false;
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--onlydeps-with-ideps\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--oneshot" || arg == "-1" {
            oneshot = true;
            i += 1;
        } else if arg == "--tree" || arg == "-t" {
            tree = true;
            i += 1;
        } else if arg == "--unordered-display" {
            unordered_display = true;
            i += 1;
        } else if arg == "--columns" {
            columns = true;
            i += 1;
        } else if arg == "--alphabetical" {
            alphabetical = true;
            i += 1;
        } else if arg == "--color" || arg.starts_with("--color=") {
            // A REQUIRED `y`/`n` value, as `--color y` or `--color=y`.
            let val = if let Some(v) = arg.strip_prefix("--color=") {
                i += 1;
                v.to_string()
            } else if let Some(v) = args.get(i + 1) {
                i += 2;
                v.clone()
            } else {
                eprintln!("emerge: --color requires an argument (y or n)");
                return Err(ExitCode::from(2));
            };
            color_opt = match val.as_str() {
                "y" => Some(true),
                "n" => Some(false),
                other => {
                    eprintln!("emerge: --color: invalid choice: {other:?} (choose from 'y', 'n')");
                    return Err(ExitCode::from(2));
                }
            };
        } else if arg == "--depclean-lib-check" || arg.starts_with("--depclean-lib-check=") {
            // Bare (no value) means `y`.
            let val = if let Some(v) = arg.strip_prefix("--depclean-lib-check=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            lib_check = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--update" || arg == "-u" {
            update = true;
            i += 1;
        } else if arg == "--noreplace" || arg == "-n" {
            // A plain boolean, no value; its only effect is setting selective.
            noreplace = true;
            i += 1;
        } else if arg == "--deep" || arg == "-D" {
            // Peeks at the next token, consuming it only if it parses as a
            // non-negative integer; otherwise unlimited depth.
            match args.get(i + 1).map(|s| s.parse::<u32>()) {
                Some(Ok(0)) => {
                    deep = portage_repo::Deep::NotRequested;
                    i += 2;
                }
                Some(Ok(n)) => {
                    deep = portage_repo::Deep::Bounded(n);
                    i += 2;
                }
                _ => {
                    deep = portage_repo::Deep::Unlimited;
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--deep=") {
            // The `=` form errors on a non-numeric value instead of leaving
            // the token alone.
            match value.parse::<u32>() {
                Ok(0) => {
                    deep = portage_repo::Deep::NotRequested;
                    i += 1;
                }
                Ok(n) => {
                    deep = portage_repo::Deep::Bounded(n);
                    i += 1;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --deep parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--backtrack" {
            // Like `--deep`: the next token is consumed only if an integer;
            // a bare flag keeps the default.
            match args.get(i + 1).map(|s| s.parse::<u32>()) {
                Some(Ok(n)) => {
                    backtrack_max = n;
                    i += 2;
                }
                _ => {
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--backtrack=") {
            // Like `--deep=`: a non-integer here is a parse error.
            match value.parse::<u32>() {
                Ok(n) => {
                    backtrack_max = n;
                    i += 1;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --backtrack parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--exclude" || arg == "-X" {
            // Repeatable; each value is itself a space-separated atom list.
            // A missing value is a usage error.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--exclude\" requires an argument");
                return Err(ExitCode::from(2));
            };
            excluded.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--exclude=") {
            excluded.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--reinstall-atoms" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--reinstall-atoms\" requires an argument");
                return Err(ExitCode::from(2));
            };
            reinstall_atoms.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--reinstall-atoms=") {
            reinstall_atoms.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--useoldpkg-atoms" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--useoldpkg-atoms\" requires an argument");
                return Err(ExitCode::from(2));
            };
            useoldpkg_atoms.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--useoldpkg-atoms=") {
            useoldpkg_atoms.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--rebuild-exclude" || arg == "--rebuild-ignore" {
            let is_exclude = arg == "--rebuild-exclude";
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"{arg}\" requires an argument");
                return Err(ExitCode::from(2));
            };
            let dst = if is_exclude {
                &mut rebuild_exclude
            } else {
                &mut rebuild_ignore
            };
            dst.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--rebuild-exclude=") {
            rebuild_exclude.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if let Some(value) = arg.strip_prefix("--rebuild-ignore=") {
            rebuild_ignore.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--dynamic-deps" || arg.starts_with("--dynamic-deps=") {
            // A bare flag means `y` (Portage requires a value).
            let val = if let Some(v) = arg.strip_prefix("--dynamic-deps=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            dynamic_deps = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--misspell-suggestions" || arg.starts_with("--misspell-suggestions=") {
            let val = if let Some(v) = arg.strip_prefix("--misspell-suggestions=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            misspell_suggestions = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--package-moves" || arg.starts_with("--package-moves=") {
            let val = if let Some(v) = arg.strip_prefix("--package-moves=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            package_moves = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--implicit-system-deps" || arg.starts_with("--implicit-system-deps=") {
            // A bare flag means `y`.
            let val = if let Some(v) = arg.strip_prefix("--implicit-system-deps=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            implicit_system_deps = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--rebuild-if-new-slot" || arg.starts_with("--rebuild-if-new-slot=") {
            // Default "y"; only `=n` disables.
            let val = if let Some(v) = arg.strip_prefix("--rebuild-if-new-slot=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            rebuild_if_new_slot = !matches!(val.as_str(), "n" | "N");
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
                i += 1;
                v
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            let on = matches!(val.as_str(), "y" | "True");
            match name {
                "--rebuild-if-unbuilt" => rebuild_if_unbuilt = Some(on),
                "--rebuild-if-new-rev" => rebuild_if_new_rev = Some(on),
                _ => rebuild_if_new_ver = Some(on),
            }
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
                i += 1;
                v
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            let on = matches!(val.as_str(), "y" | "True");
            if name == "--binpkg-changed-deps" {
                binpkg_changed_deps_override = Some(on);
            } else {
                use_ebuild_visibility = on;
            }
        } else if arg == "--complete-graph" || arg.starts_with("--complete-graph=") {
            // Bare / `=y` / `=True` -> on, `=n` -> off.
            let val = if let Some(v) = arg.strip_prefix("--complete-graph=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            complete_graph = matches!(val.as_str(), "y" | "True");
        } else if arg == "--ignore-world" || arg.starts_with("--ignore-world=") {
            // Same shape as `--complete-graph`.
            let val = if let Some(v) = arg.strip_prefix("--ignore-world=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            ignore_world = matches!(val.as_str(), "y" | "True");
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
                i += 1;
                v
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            let on = !matches!(val.as_str(), "n" | "N");
            if name == "--complete-graph-if-new-use" {
                complete_if_new_use = Some(on);
            } else {
                complete_if_new_ver = Some(on);
            }
        } else if arg == "--usepkg-exclude" {
            // Same append shape as --exclude; no short alias.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--usepkg-exclude\" requires an argument");
                return Err(ExitCode::from(2));
            };
            usepkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--usepkg-exclude=") {
            usepkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--usepkg-include" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--usepkg-include\" requires an argument");
                return Err(ExitCode::from(2));
            };
            usepkg_include.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--usepkg-include=") {
            usepkg_include.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--buildpkg-exclude" {
            // Same append shape as --exclude.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--buildpkg-exclude\" requires an argument");
                return Err(ExitCode::from(2));
            };
            buildpkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--buildpkg-exclude=") {
            buildpkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--shell" || arg.starts_with("--shell=") {
            // Portuale-only; same `=`/space handling `--json` gets.
            let value = if let Some(v) = arg.strip_prefix("--shell=") {
                i += 1;
                v.to_string()
            } else if let Some(v) = args.get(i + 1) {
                i += 2;
                v.clone()
            } else {
                eprintln!("emerge: option '--shell' requires a value (bash or brush)");
                return Err(ExitCode::from(2));
            };
            shell = match value.as_str() {
                "bash" => ebuild_phases::ShellBackend::Bash,
                "brush" => ebuild_phases::ShellBackend::Brush,
                other => {
                    eprintln!("emerge: --shell: {other:?} is not \"bash\" or \"brush\"");
                    return Err(ExitCode::from(1));
                }
            };
        } else if arg == "--solver" || arg.starts_with("--solver=") {
            // Portuale-only; same `=`/space handling as `--shell`.
            let value = if let Some(v) = arg.strip_prefix("--solver=") {
                i += 1;
                v.to_string()
            } else if let Some(v) = args.get(i + 1) {
                i += 2;
                v.clone()
            } else {
                eprintln!(
                    "emerge: option '--solver' requires a value (portage, pubgrub or resolvo)"
                );
                return Err(ExitCode::from(2));
            };
            solver = match portage_repo::SolverKind::parse(&value) {
                Some(kind) => kind,
                None => {
                    eprintln!(
                        "emerge: --solver: {value:?} is not \"portage\", \"pubgrub\" or \"resolvo\""
                    );
                    return Err(ExitCode::from(2));
                }
            };
        } else if arg == "--json" {
            // Portuale-only structured-output mode, so not routed through
            // emerge_options.rs's CLI-surface tables; no short alias.
            json = true;
            i += 1;
        } else if arg == "--verbose" || arg == "-v" {
            // Peeks at the next token, consuming it only if exactly `y`/`n`
            // (see `emerge_options`' `insert_optional_args` handling).
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    verbose = true;
                    i += 2;
                }
                Some("n") => {
                    verbose = false;
                    i += 2;
                }
                _ => {
                    verbose = true;
                    i += 1;
                }
            }
        } else if arg == "--verbose=y" || arg == "--verbose=True" {
            verbose = true;
            i += 1;
        } else if arg == "--verbose=n" {
            verbose = false;
            i += 1;
        } else if arg == "--quiet" || arg == "-q" {
            // Same optional-value shape as `--verbose`; sets verbosity to 1
            // (see `use_suffix` / `attr_display_field` for what that changes).
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    quiet = true;
                    i += 2;
                }
                Some("n") => {
                    quiet = false;
                    i += 2;
                }
                _ => {
                    quiet = true;
                    i += 1;
                }
            }
        } else if arg == "--quiet=y" || arg == "--quiet=True" {
            quiet = true;
            i += 1;
        } else if arg == "--quiet=n" {
            quiet = false;
            i += 1;
        } else if arg == "--deselect" || arg == "-W" {
            // Same optional-value shape as `--verbose`, but a bare flag turns
            // this into the standalone deselect action (see `run_deselect`).
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    deselect = true;
                    i += 2;
                }
                Some("n") => {
                    deselect = false;
                    deselect_n = true;
                    i += 2;
                }
                _ => {
                    deselect = true;
                    i += 1;
                }
            }
        } else if arg == "--deselect=y" || arg == "--deselect=True" {
            deselect = true;
            i += 1;
        } else if arg == "--deselect=n" {
            deselect = false;
            deselect_n = true;
            i += 1;
        } else if arg == "--unmerge" || arg == "-C" {
            unmerge = true;
            i += 1;
        } else if arg == "--depclean" || arg == "-c" {
            depclean = true;
            i += 1;
        } else if arg == "--prune" || arg == "-P" {
            prune = true;
            i += 1;
        } else if arg == "--config" {
            config_action = true;
            i += 1;
        } else if arg == "--with-bdeps" {
            // A REQUIRED closed-choice value (`y`/`n`): a missing or other
            // value is an immediate usage error, with no default to fall back to.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--with-bdeps\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.as_str() {
                "y" => {
                    with_bdeps = true;
                    with_bdeps_given = true;
                    i += 2;
                }
                "n" => {
                    with_bdeps = false;
                    with_bdeps_given = true;
                    i += 2;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--with-bdeps\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--with-bdeps=") {
            match value {
                "y" => {
                    with_bdeps = true;
                    with_bdeps_given = true;
                    i += 1;
                }
                "n" => {
                    with_bdeps = false;
                    with_bdeps_given = true;
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--with-bdeps\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--with-bdeps-auto" {
            // Same required closed-choice shape as `--with-bdeps`.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--with-bdeps-auto\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.as_str() {
                "y" => {
                    with_bdeps_auto = true;
                    i += 2;
                }
                "n" => {
                    with_bdeps_auto = false;
                    i += 2;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--with-bdeps-auto\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--with-bdeps-auto=") {
            match value {
                "y" => {
                    with_bdeps_auto = true;
                    i += 1;
                }
                "n" => {
                    with_bdeps_auto = false;
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--with-bdeps-auto\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--changed-deps" {
            // Same optional-value shape as `--verbose`; an ordinary
            // --pretend modifier, not a standalone action.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    changed_deps = true;
                    i += 2;
                }
                Some("n") => {
                    changed_deps = false;
                    i += 2;
                }
                _ => {
                    changed_deps = true;
                    i += 1;
                }
            }
        } else if arg == "--changed-deps=y" || arg == "--changed-deps=True" {
            changed_deps = true;
            i += 1;
        } else if arg == "--changed-deps=n" {
            changed_deps = false;
            i += 1;
        } else if arg == "--changed-deps-report" {
            // Same shape as `--changed-deps`, but never changes what gets
            // reinstalled.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    changed_deps_report = true;
                    i += 2;
                }
                Some("n") => {
                    changed_deps_report = false;
                    i += 2;
                }
                _ => {
                    changed_deps_report = true;
                    i += 1;
                }
            }
        } else if arg == "--changed-deps-report=y" || arg == "--changed-deps-report=True" {
            changed_deps_report = true;
            i += 1;
        } else if arg == "--changed-deps-report=n" {
            changed_deps_report = false;
            i += 1;
        } else if arg == "--verbose-conflicts" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    verbose_conflicts = true;
                    i += 2;
                }
                Some("n") => {
                    verbose_conflicts = false;
                    i += 2;
                }
                _ => {
                    verbose_conflicts = true;
                    i += 1;
                }
            }
        } else if arg == "--verbose-conflicts=y" {
            verbose_conflicts = true;
            i += 1;
        } else if arg == "--verbose-conflicts=n" {
            verbose_conflicts = false;
            i += 1;
        } else if arg == "--verbose-slot-rebuilds" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    verbose_slot_rebuilds = true;
                    i += 2;
                }
                Some("n") => {
                    verbose_slot_rebuilds = false;
                    i += 2;
                }
                _ => {
                    verbose_slot_rebuilds = true;
                    i += 1;
                }
            }
        } else if arg == "--verbose-slot-rebuilds=y" {
            verbose_slot_rebuilds = true;
            i += 1;
        } else if arg == "--verbose-slot-rebuilds=n" {
            verbose_slot_rebuilds = false;
            i += 1;
        } else if arg == "--ignore-built-slot-operator-deps" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    ignore_built_slot_operator_deps = true;
                    i += 2;
                }
                Some("n") => {
                    ignore_built_slot_operator_deps = false;
                    i += 2;
                }
                _ => {
                    ignore_built_slot_operator_deps = true;
                    i += 1;
                }
            }
        } else if arg == "--ignore-built-slot-operator-deps=y" {
            ignore_built_slot_operator_deps = true;
            i += 1;
        } else if arg == "--ignore-built-slot-operator-deps=n" {
            ignore_built_slot_operator_deps = false;
            i += 1;
        } else if arg == "--selective" {
            // Same shape as `--changed-deps`. `n` explicitly CANCELS selective
            // even if another flag already set it (applied before resolving).
            // (`-n` is the separate bare-boolean `--noreplace` spelling.)
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    selective_flag = Some(true);
                    i += 2;
                }
                Some("n") => {
                    selective_flag = Some(false);
                    i += 2;
                }
                _ => {
                    selective_flag = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--selective=y" || arg == "--selective=True" {
            selective_flag = Some(true);
            i += 1;
        } else if arg == "--selective=n" {
            selective_flag = Some(false);
            i += 1;
        } else if arg == "--changed-slot" {
            // Same shape as `--changed-deps`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    changed_slot = true;
                    i += 2;
                }
                Some("n") => {
                    changed_slot = false;
                    i += 2;
                }
                _ => {
                    changed_slot = true;
                    i += 1;
                }
            }
        } else if arg == "--changed-slot=y" || arg == "--changed-slot=True" {
            changed_slot = true;
            i += 1;
        } else if arg == "--changed-slot=n" {
            changed_slot = false;
            i += 1;
        } else if arg == "--newrepo" {
            newrepo = true;
            i += 1;
        } else if arg == "--emptytree" || arg == "-e" {
            emptytree = true;
            i += 1;
        } else if arg == "--buildpkgonly" || arg == "-B" {
            buildpkgonly = true;
            i += 1;
        } else if arg == "--buildpkg" || arg == "-b" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    buildpkg_opt = Some(true);
                    i += 2;
                }
                Some("n") => {
                    buildpkg_opt = Some(false);
                    i += 2;
                }
                _ => {
                    buildpkg_opt = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--buildpkg=y" {
            buildpkg_opt = Some(true);
            i += 1;
        } else if arg == "--buildpkg=n" {
            buildpkg_opt = Some(false);
            i += 1;
        } else if arg == "--quiet-build" {
            // Bare / `y` -> redirect build output to `${T}/build.log`.
            // (`n` still captures under `-j` > 1.)
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    quiet_build = Some(true);
                    i += 2;
                }
                Some("n") => {
                    quiet_build = Some(false);
                    i += 2;
                }
                _ => {
                    quiet_build = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--quiet-build=y" || arg == "--quiet-build=True" {
            quiet_build = Some(true);
            i += 1;
        } else if arg == "--quiet-build=n" {
            quiet_build = Some(false);
            i += 1;
        } else if arg == "--keep-going" {
            keep_going = true;
            i += 1;
        } else if arg == "--debug" || arg == "-d" {
            // Sets `PORTAGE_DEBUG=1` in the phase env for a build; a no-op
            // for `--pretend`.
            debug = true;
            i += 1;
        } else if arg == "--resume" || arg == "-r" {
            resume = true;
            i += 1;
        } else if arg == "--list-sets" {
            list_sets = true;
            i += 1;
        } else if arg == "--search" || arg == "-s" {
            // The search terms are the remaining positional args.
            search_action = true;
            i += 1;
        } else if arg == "--searchdesc" || arg == "-S" {
            // Implies `--search`, and also matches DESCRIPTION.
            search_action = true;
            searchdesc = true;
            i += 1;
        } else if arg == "--fuzzy-search" || arg.starts_with("--fuzzy-search=") {
            let val = if let Some(v) = arg.strip_prefix("--fuzzy-search=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            fuzzy_search = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--regex-search-auto" || arg.starts_with("--regex-search-auto=") {
            let val = if let Some(v) = arg.strip_prefix("--regex-search-auto=") {
                i += 1;
                v.to_string()
            } else if matches!(args.get(i + 1).map(String::as_str), Some("y" | "n")) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            regex_search_auto = !matches!(val.as_str(), "n" | "N");
        } else if arg == "--search-similarity" || arg.starts_with("--search-similarity=") {
            let val = if let Some(v) = arg.strip_prefix("--search-similarity=") {
                i += 1;
                v.to_string()
            } else if let Some(v) = args.get(i + 1) {
                i += 2;
                v.clone()
            } else {
                eprintln!("emerge: option '--search-similarity' requires a value");
                return Err(ExitCode::from(2));
            };
            // Must parse as a float in [0, 100].
            match val.parse::<f64>() {
                Ok(n) if (0.0..=100.0).contains(&n) => search_similarity = n,
                Ok(_) => {
                    eprintln!(
                        "emerge: Invalid --search-similarity parameter (not between 0 and 100): '{val}'"
                    );
                    return Err(ExitCode::from(2));
                }
                Err(_) => {
                    eprintln!(
                        "emerge: Invalid --search-similarity parameter (not a number): '{val}'"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--check-news" {
            check_news = true;
            i += 1;
        } else if arg == "--regen" {
            regen_action = true;
            i += 1;
        } else if arg == "--metadata" {
            metadata_action = true;
            i += 1;
        } else if arg == "--sync" {
            sync_action = true;
            i += 1;
        } else if arg == "--clean" {
            clean_action = true;
            i += 1;
        } else if arg == "--rage-clean" {
            rage_clean = true;
            i += 1;
        } else if arg == "--info" {
            info_action = true;
            i += 1;
        } else if arg == "--skipfirst" || arg == "--skip-first" {
            skipfirst = true;
            i += 1;
        } else if arg == "--jobs" || arg == "-j" {
            // Optional integer next token, like `--deep`; bare means unlimited.
            match args.get(i + 1).map(|s| s.parse::<usize>()) {
                Some(Ok(n)) => {
                    jobs = jobs_count(n);
                    i += 2;
                }
                _ => {
                    jobs = usize::MAX;
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--jobs=") {
            match value.parse::<usize>() {
                Ok(n) => {
                    jobs = jobs_count(n);
                    i += 1;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --jobs parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("-j") {
            match value.parse::<usize>() {
                Ok(n) => {
                    jobs = jobs_count(n);
                    i += 1;
                }
                Err(_) => {
                    eprintln!("emerge: invalid -j parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--load-average" || arg == "-l" {
            // A required value (unlike `--jobs`' optional one): missing or
            // non-numeric is a usage error.
            match args.get(i + 1).map(|s| s.parse::<f64>()) {
                Some(Ok(la)) if la > 0.0 => {
                    load_average = Some(la);
                    i += 2;
                }
                _ => {
                    eprintln!("emerge: option \"--load-average\" requires a positive number");
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg
            .strip_prefix("--load-average=")
            .or_else(|| arg.strip_prefix("-l"))
        {
            match value.parse::<f64>() {
                Ok(la) if la > 0.0 => {
                    load_average = Some(la);
                    i += 1;
                }
                _ => {
                    eprintln!("emerge: invalid --load-average parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--root-deps" || arg == "--root-deps=True" || arg == "--root-deps=rdeps" {
            root_deps = true;
            i += 1;
        } else if arg == "--with-test-deps" {
            // Same shape as `--changed-deps`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    with_test_deps = true;
                    i += 2;
                }
                Some("n") => {
                    with_test_deps = false;
                    i += 2;
                }
                _ => {
                    with_test_deps = true;
                    i += 1;
                }
            }
        } else if arg == "--with-test-deps=y" || arg == "--with-test-deps=True" {
            with_test_deps = true;
            i += 1;
        } else if arg == "--with-test-deps=n" {
            with_test_deps = false;
            i += 1;
        } else if arg == "--autounmask" {
            // Bare flag means true; three accepted true spellings otherwise.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    autounmask = Some(true);
                    i += 2;
                }
                Some("n") => {
                    autounmask = Some(false);
                    i += 2;
                }
                _ => {
                    autounmask = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--autounmask=y" {
            autounmask = Some(true);
            i += 1;
        } else if arg == "--autounmask=n" {
            autounmask = Some(false);
            i += 1;
        } else if arg == "--autounmask-only" || arg.starts_with("--autounmask-only=") {
            let val = if let Some(v) = arg.strip_prefix("--autounmask-only=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            autounmask_only = matches!(val.as_str(), "y" | "True");
        } else if arg == "--autounmask-continue" || arg.starts_with("--autounmask-continue=") {
            let val = if let Some(v) = arg.strip_prefix("--autounmask-continue=") {
                i += 1;
                v.to_string()
            } else if matches!(
                args.get(i + 1).map(String::as_str),
                Some("y" | "n" | "True")
            ) {
                i += 2;
                args[i - 1].clone()
            } else {
                i += 1;
                "y".to_string()
            };
            autounmask_continue = Some(matches!(val.as_str(), "y" | "True"));
        } else if arg == "--autounmask-backtrack" {
            // A REQUIRED `y`/`n` value; gates the backtrack loop's autounmask
            // re-drive. Disabled by default.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-backtrack\" requires an argument");
                return Err(ExitCode::from(2));
            };
            if !matches!(value.as_str(), "y" | "n") {
                eprintln!(
                    "emerge: option \"--autounmask-backtrack\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
            autounmask_backtrack = Some(value == "y");
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--autounmask-backtrack=") {
            if !matches!(value, "y" | "n") {
                eprintln!(
                    "emerge: option \"--autounmask-backtrack\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return Err(ExitCode::from(2));
            }
            autounmask_backtrack = Some(value == "y");
            i += 1;
        } else if arg == "--autounmask-keep-keywords" {
            // Same shape as `--changed-deps`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    autounmask_keep_keywords = Some(true);
                    i += 2;
                }
                Some("n") => {
                    autounmask_keep_keywords = Some(false);
                    i += 2;
                }
                _ => {
                    autounmask_keep_keywords = Some(true);
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--autounmask-keep-keywords=") {
            match value {
                "y" | "True" => {
                    autounmask_keep_keywords = Some(true);
                    i += 1;
                }
                "n" => {
                    autounmask_keep_keywords = Some(false);
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-keep-keywords\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--autounmask-use" {
            // A REQUIRED `y`/`n` value.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-use\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.as_str() {
                "y" => {
                    autounmask_use = Some(true);
                    i += 2;
                }
                "n" => {
                    autounmask_use = Some(false);
                    i += 2;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--autounmask-use=") {
            match value {
                "y" => {
                    autounmask_use = Some(true);
                    i += 1;
                }
                "n" => {
                    autounmask_use = Some(false);
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--autounmask-license" {
            // A REQUIRED `y`/`n` value, like `--autounmask-use`.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-license\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.as_str() {
                "y" => {
                    autounmask_license = Some(true);
                    i += 2;
                }
                "n" => {
                    autounmask_license = Some(false);
                    i += 2;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-license\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--autounmask-license=") {
            match value {
                "y" => {
                    autounmask_license = Some(true);
                    i += 1;
                }
                "n" => {
                    autounmask_license = Some(false);
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-license\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--autounmask-keep-masks" {
            // Same shape as `--changed-deps`.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    autounmask_keep_masks = Some(true);
                    i += 2;
                }
                Some("n") => {
                    autounmask_keep_masks = Some(false);
                    i += 2;
                }
                _ => {
                    autounmask_keep_masks = Some(true);
                    i += 1;
                }
            }
        } else if let Some(value) = arg.strip_prefix("--autounmask-keep-masks=") {
            match value {
                "y" | "True" => {
                    autounmask_keep_masks = Some(true);
                    i += 1;
                }
                "n" => {
                    autounmask_keep_masks = Some(false);
                    i += 1;
                }
                _ => {
                    eprintln!(
                        "emerge: option \"--autounmask-keep-masks\": invalid choice: {value:?} (choose from \"True\", \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--usepkg" || arg == "-k" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    usepkg = true;
                    i += 2;
                }
                Some("n") => {
                    usepkg = false;
                    i += 2;
                }
                _ => {
                    usepkg = true;
                    i += 1;
                }
            }
        } else if arg == "--usepkg=y" {
            usepkg = true;
            i += 1;
        } else if arg == "--usepkg=n" {
            usepkg = false;
            i += 1;
        } else if arg == "--usepkgonly" || arg == "-K" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    usepkgonly = true;
                    i += 2;
                }
                Some("n") => {
                    usepkgonly = false;
                    i += 2;
                }
                _ => {
                    usepkgonly = true;
                    i += 1;
                }
            }
        } else if arg == "--usepkgonly=y" {
            usepkgonly = true;
            i += 1;
        } else if arg == "--usepkgonly=n" {
            usepkgonly = false;
            i += 1;
        } else if arg == "--getbinpkg" || arg == "-g" {
            // Folded into `usepkg`, plus `getbinpkg` for remote candidates.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    getbinpkg = true;
                    i += 2;
                }
                Some("n") => {
                    getbinpkg = false;
                    i += 2;
                }
                _ => {
                    getbinpkg = true;
                    i += 1;
                }
            }
        } else if arg == "--getbinpkg=y" {
            getbinpkg = true;
            i += 1;
        } else if arg == "--getbinpkg=n" {
            getbinpkg = false;
            i += 1;
        } else if arg == "--getbinpkgonly" || arg == "-G" {
            // Implies binary-only (`usepkgonly`); folded the same way.
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    getbinpkgonly = true;
                    i += 2;
                }
                Some("n") => {
                    getbinpkgonly = false;
                    i += 2;
                }
                _ => {
                    getbinpkgonly = true;
                    i += 1;
                }
            }
        } else if arg == "--getbinpkgonly=y" {
            getbinpkgonly = true;
            i += 1;
        } else if arg == "--getbinpkgonly=n" {
            getbinpkgonly = false;
            i += 1;
        } else if arg == "--binpkg-respect-use" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    binpkg_respect_use = Some(true);
                    i += 2;
                }
                Some("n") => {
                    binpkg_respect_use = Some(false);
                    i += 2;
                }
                _ => {
                    binpkg_respect_use = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--binpkg-respect-use=y" {
            binpkg_respect_use = Some(true);
            i += 1;
        } else if arg == "--binpkg-respect-use=n" {
            binpkg_respect_use = Some(false);
            i += 1;
        } else if arg == "--rebuilt-binaries" {
            match args.get(i + 1).map(String::as_str) {
                Some("y") => {
                    rebuilt_binaries = Some(true);
                    i += 2;
                }
                Some("n") => {
                    rebuilt_binaries = Some(false);
                    i += 2;
                }
                _ => {
                    rebuilt_binaries = Some(true);
                    i += 1;
                }
            }
        } else if arg == "--rebuilt-binaries=y" {
            rebuilt_binaries = Some(true);
            i += 1;
        } else if arg == "--rebuilt-binaries=n" {
            rebuilt_binaries = Some(false);
            i += 1;
        } else if arg == "--rebuilt-binaries-timestamp" {
            // A required numeric Unix timestamp.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--rebuilt-binaries-timestamp\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.parse::<u64>() {
                Ok(n) => {
                    rebuilt_binaries_timestamp = Some(n);
                    i += 2;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --rebuilt-binaries-timestamp parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--rebuilt-binaries-timestamp=") {
            match value.parse::<u64>() {
                Ok(n) => {
                    rebuilt_binaries_timestamp = Some(n);
                    i += 1;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --rebuilt-binaries-timestamp parameter: {value:?}");
                    return Err(ExitCode::from(2));
                }
            }
        } else if arg == "--quickpkg-direct" {
            // A REQUIRED `y`/`n` value, like `--autounmask-backtrack`.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--quickpkg-direct\" requires an argument");
                return Err(ExitCode::from(2));
            };
            match value.as_str() {
                "y" => quickpkg_direct = Some(true),
                "n" => quickpkg_direct = Some(false),
                _ => {
                    eprintln!(
                        "emerge: option \"--quickpkg-direct\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--quickpkg-direct=") {
            match value {
                "y" => quickpkg_direct = Some(true),
                "n" => quickpkg_direct = Some(false),
                _ => {
                    eprintln!(
                        "emerge: option \"--quickpkg-direct\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return Err(ExitCode::from(2));
                }
            }
            i += 1;
        } else if arg == "--quickpkg-direct-root" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--quickpkg-direct-root\" requires an argument");
                return Err(ExitCode::from(2));
            };
            quickpkg_direct_root = Some(value.clone());
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--quickpkg-direct-root=") {
            quickpkg_direct_root = Some(value.to_string());
            i += 1;
        } else if !arg.starts_with('-') {
            atom_args.push(arg);
            i += 1;
        } else if !arg.starts_with("--") && arg.len() > 2 {
            // Short-flag bundle (e.g. "-pv"): one character at a time, left to
            // right. A bundled flag never consumes a value (unlike standalone).
            for c in arg[1..].chars() {
                match c {
                    'p' => pretend = true,
                    'a' => ask = true,
                    'v' => verbose = true,
                    'q' => quiet = true,
                    'N' => newuse = true,
                    'U' => changed_use = true,
                    'O' => nodeps = true,
                    'o' => onlydeps = true,
                    '1' => oneshot = true,
                    't' => tree = true,
                    'u' => update = true,
                    'n' => noreplace = true,
                    'd' => debug = true,
                    'D' => deep = portage_repo::Deep::Unlimited,
                    'e' => emptytree = true,
                    'k' => usepkg = true,
                    'K' => usepkgonly = true,
                    'g' => getbinpkg = true,
                    'G' => getbinpkgonly = true,
                    'W' => deselect = true,
                    'C' => unmerge = true,
                    'c' => depclean = true,
                    'P' => prune = true,
                    's' => search_action = true,
                    'S' => {
                        search_action = true;
                        searchdesc = true;
                    }
                    'B' => buildpkgonly = true,
                    'b' => buildpkg_opt = Some(true),
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
            i += 1;
        } else {
            return Err(report_option(arg));
        }
    }

    Ok(EmergeOpts {
        atom_args,
        pretend,
        ask,
        read_news,
        resume,
        skipfirst,
        verbose,
        quiet,
        newuse,
        changed_use,
        nodeps,
        onlydeps,
        onlydeps_with_rdeps,
        onlydeps_with_ideps,
        oneshot,
        tree,
        unordered_display,
        columns,
        alphabetical,
        color_opt,
        lib_check,
        update,
        deep,
        excluded,
        reinstall_atoms,
        useoldpkg_atoms,
        rebuild_if_new_slot,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        binpkg_changed_deps_override,
        use_ebuild_visibility,
        rebuild_exclude,
        rebuild_ignore,
        complete_graph,
        ignore_world,
        complete_if_new_use,
        complete_if_new_ver,
        dynamic_deps,
        misspell_suggestions,
        package_moves,
        implicit_system_deps,
        usepkg_exclude,
        usepkg_include,
        json,
        deselect,
        deselect_n,
        unmerge,
        depclean,
        prune,
        config_action,
        list_sets,
        search_action,
        searchdesc,
        fuzzy_search,
        regex_search_auto,
        search_similarity,
        check_news,
        regen_action,
        metadata_action,
        sync_action,
        clean_action,
        rage_clean,
        info_action,
        with_bdeps,
        with_bdeps_given,
        with_bdeps_auto,
        changed_deps,
        changed_slot,
        newrepo,
        emptytree,
        buildpkgonly,
        buildpkg_opt,
        quiet_build,
        buildpkg_exclude,
        keep_going,
        debug,
        jobs,
        load_average,
        root_deps,
        with_test_deps,
        changed_deps_report,
        verbose_slot_rebuilds,
        ignore_built_slot_operator_deps,
        backtrack_max,
        solver,
        verbose_conflicts,
        autounmask,
        autounmask_keep_keywords,
        autounmask_use,
        autounmask_license,
        autounmask_keep_masks,
        autounmask_only,
        autounmask_continue,
        autounmask_backtrack,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
        binpkg_respect_use,
        quickpkg_direct,
        quickpkg_direct_root,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        noreplace,
        selective_flag,
        shell,
    })
}
