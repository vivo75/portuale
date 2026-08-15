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
            usepkgonly: false,
            // --getbinpkg/-g, --getbinpkgonly/-G: folded into `usepkg`/`usepkgonly`;
            // `getbinpkg` additionally turns on *remote* candidate loading.
            getbinpkg: false,
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
