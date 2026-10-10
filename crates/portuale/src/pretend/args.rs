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
    // --ask/-a (real `true_y_or_n`): after the merge/removal list is
    // displayed and before anything is actually built/merged/removed,
    // prompt `Would you like to ...? [Yes/No]` (real `UserQuery.query`,
    // `_emerge/actions.py:525` / `unmerge.py:621`). Bare Enter = Yes. "No"
    // prints `Quitting.` and exits 130, while EOF/SIGINT prints only
    // `Interrupted.` and exits 130 (real exits from inside `query`, so
    // its `Quitting.` never runs -- backlog #240).
    // (`128 + SIGINT`). Ignored under `--pretend` (nothing executes
    // anyway).
    let mut ask = false;
    // --read-news (real `y_or_n` insert default, `main.py:175`, with
    // `choices: true_y_or_n`, `main.py:625`, normalized to `True`/`None`
    // at `main.py:950-953`): with `--ask`,
    // offer `eselect news read` while calculating dependencies (real
    // `actions.py:4266-4281`, backlog #231). Alone it changes nothing --
    // real's prompt requires both flags.
    let mut read_news = false;
    // --resume/-r + --skipfirst (real `_emerge/Scheduler.py` resume
    // handling): `--resume` replays `mtimedb["resume"]["mergelist"]` --
    // the packages a previous failed `emerge <atom>` left unmerged;
    // `--skipfirst` drops the first (the one that failed) before
    // continuing. When a source merge fails portuale writes that list
    // (see `mtimedb::write_resume_list`).
    let mut resume = false;
    let mut skipfirst = false;
    let mut verbose = false;
    let mut quiet = false;
    let mut newuse = false;
    let mut changed_use = false;
    let mut nodeps = false;
    let mut onlydeps = false;
    // `--onlydeps-with-rdeps` / `--onlydeps-with-ideps` (real `main.py`
    // `argument_options`, `choices: true_y_or_n` = `("True", "y", "n")`,
    // no default): `true` = keep the key class, `false` = blank it for
    // an `--onlydeps` root. Defaults are real's own (`man emerge.1`:
    // with-rdeps enabled, with-ideps disabled); without `--onlydeps`
    // both are inert (real only ever consults them behind
    // `pkg.onlydeps`). See the parse arms below and
    // `portage_repo::ResolveRequest`'s own doc comments.
    let mut onlydeps_with_rdeps = true;
    let mut onlydeps_with_ideps = false;
    // --oneshot/-1: don't record the target in the world file on a real
    // merge, and (at --pretend) don't colour it as a would-be world
    // member -- real `Scheduler._world_atom` / `_DisplayConfig.oneshot`.
    let mut oneshot = false;
    // --tree/-t and --unordered-display: display-only, entirely
    // independent of resolution itself (real portage's own equivalent,
    // output_helpers.py's _tree_display, lives in the display layer too,
    // never depgraph.py's core resolution) -- see print_tree's own doc
    // comment for the full portuale-specific design this needed.
    let mut tree = false;
    let mut unordered_display = false;
    // --columns: display-only, same "entirely independent of resolution"
    // shape as --tree above (real output_helpers.py's own
    // MergeListItem.conf.columns is a display-layer flag, never consulted
    // anywhere in depgraph.py) -- mutually exclusive with --tree, checked
    // once parsing finishes (see the "can't specify both" check below).
    let mut columns = false;
    // --alphabetical: display-only, real `output_helpers.py`'s
    // `conf.alphabetical` -- see use_suffix.
    let mut alphabetical = false;
    // --color y|n: real `main.py`'s own `argument_options` entry
    // (`"choices": ("y", "n")`), a REQUIRED value -- not one of the
    // optional-value flags. `None` = not given (fall through to the
    // NO_COLOR/NOCOLOR/isatty gate, see `color::resolve_havecolor`).
    let mut color_opt: Option<bool> = None;
    // --depclean-lib-check y|n: real `main.py:442` (`_DEPCLEAN_LIB_CHECK_
    // DEFAULT = True`). Only consulted by `--depclean`/`--prune`. `n`
    // disables the `NEEDED.ELF.2` soname-consumer scan (and, no-args,
    // adds the `Depclean may break link level dependencies` advisory
    // paragraph).
    let mut lib_check = true;
    let mut update = false;
    let mut deep = portage_repo::Deep::NotRequested;
    let mut excluded: Vec<String> = Vec::new();
    // --reinstall-atoms ATOMS (real `main.py`, `action: "append"` ->
    // `depgraph.py:363-365` `WildcardPackageSet`): same repeatable,
    // space-separated-per-occurrence shape as --exclude. A matching
    // already-installed package is forced to re-merge (see
    // `resolve_pretend_graph`'s own `reinstall_atoms` doc comment).
    let mut reinstall_atoms: Vec<String> = Vec::new();
    // --useoldpkg-atoms ATOMS (real `main.py:713`, `action: "append"` ->
    // `depgraph.py:370-371` `WildcardPackageSet`): same repeatable,
    // space-separated-per-occurrence shape as --exclude / --reinstall-atoms.
    // For a matching package, prefer an existing binary package over a
    // newer unbuilt ebuild (see `portage_repo::set_useoldpkg_atoms`).
    let mut useoldpkg_atoms: Vec<String> = Vec::new();
    // --rebuild-if-new-slot (real `y_or_n`, default "y" -- `main.py:955`):
    // the `:=` slot-operator auto-rebuild. Only `=n` disables it.
    let mut rebuild_if_new_slot = true;
    // --rebuild-if-unbuilt / --rebuild-if-new-rev / --rebuild-if-new-ver
    // (real `true_y_or_n`, all OFF by default). Tracked as Option so the
    // real precedence (`unbuilt` clears rev+ver; `rev` clears ver;
    // `main.py:958-975`) can be resolved once parsing finishes.
    let mut rebuild_if_unbuilt: Option<bool> = None;
    let mut rebuild_if_new_rev: Option<bool> = None;
    let mut rebuild_if_new_ver: Option<bool> = None;
    // `--binpkg-changed-deps[=y|n]` (real `true_y_or_n`): `None` = auto
    // (real `create_depgraph_params.py`'s "enabled unless `--usepkgonly`"),
    // `Some(true|false)` = explicit force on/off.
    // `--use-ebuild-visibility[=y|n]` (real `true_y_or_n`, default off).
    let mut binpkg_changed_deps_override: Option<bool> = None;
    let mut use_ebuild_visibility = false;
    // --rebuild-exclude ATOMS (parent skipped) / --rebuild-ignore ATOMS
    // (dep never triggers) -- same repeatable/space-separated shape as
    // --exclude.
    let mut rebuild_exclude: Vec<String> = Vec::new();
    let mut rebuild_ignore: Vec<String> = Vec::new();
    // --complete-graph (real `true_y_or_n`, `main.py:155`/`878-881`, OFF
    // by default -- only a `y`-ish value turns it on). "completely account
    // for all known dependencies": real `create_depgraph_params.py:169-175`
    // sets `myparams["complete"]`, which `depgraph.py::_complete_graph`
    // turns into a forced deep walk (see `resolve_pretend_graph`'s
    // `complete` param). `--rebuild-if-{unbuilt,new-rev,new-ver}` imply it
    // too (same `create_depgraph_params.py` block); `--nodeps` pops it.
    let mut complete_graph = false;
    // --ignore-world (real `y_or_n` in `default_arg_opts` so a bare flag
    // inserts `"True"`, `true_y_or_n` choices in `argument_options`,
    // `main.py:164`/`486`/`928-929`: only a `true_y` value flips it to
    // `True`; `create_depgraph_params.py:128-129` sets
    // `myparams["ignore_world"]` only on `is True`). "Ignore the @world
    // package set and its dependencies" (`man/emerge.1`): real
    // `depgraph.py:357-360` empties `_required_set_names` (dropping
    // `@world` -- which nests `@selected`/`@system`/`@profile`, real
    // `_sets/__init__.py:97-98` -- from the complete-graph seeds, so the
    // complete walk covers only the deep dependencies of the args) and
    // `:10107` skips the `@selected` uninstall guard. No `actions.py`
    // handling, no `--complete-graph`/`--deep`/`--update` interplay, and
    // no removal-action effect (real `create_depgraph_params.py`
    // returns early for `remove` before reaching the flag).
    let mut ignore_world = false;
    // --complete-graph-if-new-use / --complete-graph-if-new-ver (real
    // `y_or_n`, `main.py:157-158`). Both default ON -- real
    // `_complete_graph` reads `myparams.get("complete_if_new_*", "y")`, so
    // an absent flag behaves as `"y"`. Only `=n` opts out of that
    // auto-enable trigger. `None` = never given (on); `Some(false)` = `=n`.
    let mut complete_if_new_use: Option<bool> = None;
    let mut complete_if_new_ver: Option<bool> = None;
    // --dynamic-deps (real `y_or_n`; real default is "y" for a source
    // install -- see create_depgraph_params.py). ON is portuale's own
    // long-standing behaviour (an AlreadyInstalled package's `--deep`
    // walk reads the current ebuild); only `=n` flips it to the vdb
    // snapshot. `--nodeps` forces it off (real
    // `create_depgraph_params.py:122`).
    let mut dynamic_deps = true;
    // --misspell-suggestions (real `y_or_n`, `depgraph.py:7037`, default
    // "y"): when a top-level atom names a `cat/pkg` that doesn't exist,
    // suggest close package names (`difflib.get_close_matches` via
    // `_similar_name_search`). Only `=n` disables it.
    let mut misspell_suggestions = true;
    // --package-moves (real `y_or_n`, `main.py:936`, default "y"): whether
    // `profiles/updates/` moves are applied. Only `=n` disables. Threaded
    // via `portage_repo::set_package_moves_enabled` (a process-global,
    // like `--color`), not the resolver signature.
    let mut package_moves = true;
    // --implicit-system-deps (real `y_or_n`, `main.py:490`, default "y"):
    // whether `@system` members are assumed to be implicit dependencies.
    // Only `=n` disables -- real `create_depgraph_params.py:120`
    // (`myopts.get("--implicit-system-deps", "y") != "n"`), and for a
    // `--pretend` merge list that only shows up in `_merge_order_bias`
    // (depgraph.py:9279 early-returns): with `=n`, the @system-first /
    // reference-count sort is skipped and the list stays in discovery
    // order. Threaded into the resolver.
    let mut implicit_system_deps = true;
    // --usepkg-exclude/--usepkg-include: same "action": "append",
    // space-separated-per-occurrence shape as --exclude/-X above (real
    // main.py: "A space separated list of package names or slot atoms"),
    // but scoped to binary-candidate eligibility specifically -- see
    // `filter_usepkg_exclude_include`'s own doc comment, portage-repo.
    let mut usepkg_exclude: Vec<String> = Vec::new();
    let mut usepkg_include: Vec<String> = Vec::new();
    let mut json = false;
    let mut deselect = false;
    // Real `--deselect=n` / `--deselect n` -- distinct from "not given":
    // consulted by `--depclean <atoms>` (real `action_depclean`'s
    // `deselect = myopts.get("--deselect") != "n"`, default keep-behavior
    // on). Never triggers the standalone deselect action.
    let mut deselect_n = false;
    // --unmerge/-C: a standalone action (see run_unmerge_pretend).
    let mut unmerge = false;
    // --depclean/-c: a standalone action (see run_depclean_pretend).
    let mut depclean = false;
    // --prune/-P: a standalone action (see run_prune_pretend).
    let mut prune = false;
    // --config: a standalone action (see run_config_action). Ignores
    // --pretend entirely -- real `action_config` never checks it.
    let mut config_action = false;
    // --list-sets / --search / --searchdesc / --check-news: standalone
    // read-only query actions (see run_list_sets / run_search /
    // run_check_news).
    let mut list_sets = false;
    let mut search_action = false;
    let mut searchdesc = false;
    // --fuzzy-search (real `true_y_or_n`): `emerge --search`'s own
    // `difflib.SequenceMatcher` fuzzy matching. Real `action_search`:
    // `fuzzy = myopts.get("--fuzzy-search") != "n"` -- ON by default,
    // only `--fuzzy-search=n` disables it.
    let mut fuzzy_search = true;
    // --regex-search-auto (real `y_or_n`, `"default": "y"`): auto-detect
    // a regular-expression search key. Real `action_search`: `regex_auto
    // = myopts.get("--regex-search-auto") != "n"`.
    let mut regex_search_auto = true;
    // --search-similarity (real: a float 0..100, default 80): the minimum
    // `SequenceMatcher.ratio()` percentage a fuzzy hit needs. Real
    // `search.py`: `self.search_similarity = 80 if search_similarity is
    // None else search_similarity`.
    let mut search_similarity: f64 = 80.0;
    let mut check_news = false;
    // --regen (real `action_regen`): regenerate every repo's
    // `metadata/md5-cache` by running each ebuild's `depend` phase. A
    // real write action -- rejects `--pretend`. --metadata (real
    // `action_metadata`) transfers a repo's pre-generated cache into
    // portage's own `depcachedir`; portuale reads `metadata/md5-cache`
    // directly and has no `depcachedir`, so the transfer is a no-op.
    // --sync is a permanent non-goal -- `emerge --sync` prints
    // "Functionality has moved to `emaint sync`." and exits 1.
    let mut regen_action = false;
    let mut metadata_action = false;
    let mut sync_action = false;
    // --clean / --rage-clean: standalone removal actions. --rage-clean
    // is a fast --unmerge (same selection, skips CLEAN_DELAY + prerm/
    // postrm -- invisible at --pretend); --clean keeps only the newest
    // version per slot. See run_clean_pretend / run_unmerge_pretend.
    let mut clean_action = false;
    let mut rage_clean = false;
    // --info: a standalone read-only query action (see run_info).
    let mut info_action = false;
    let mut with_bdeps = true;
    let mut with_bdeps_given = false;
    let mut with_bdeps_auto = true;
    let mut changed_deps = false;
    let mut changed_slot = false;
    // --newrepo: real main.py's own plain boolean "options" list, no
    // value at all (same shape as --changed-use/-U above) -- unlike
    // --changed-slot/--rebuilt-binaries, which are real "true_y_or_n".
    let mut newrepo = false;
    // --emptytree/-e: real `main.py`'s own plain-boolean "options" list
    // (short alias `e`, `main.py:58`). Reinstalls every atom in the deep
    // dependency tree as though nothing is installed
    // (`create_depgraph_params.py:176-179`) -- useful for byte-for-byte
    // comparison against real portage and for debugging resolution.
    let mut emptytree = false;
    // --buildpkgonly/-B: same plain-boolean shape as --newrepo above.
    let mut buildpkgonly = false;
    // --buildpkg/-b: real `true_y_or_n`. `None` = not given (fall back
    // to `FEATURES=buildpkg`); `Some(true/false)` = explicit. Only
    // matters on a real (non-`--pretend`) source merge -- see the
    // `!pretend` block below.
    let mut buildpkg_opt: Option<bool> = None;
    // `--quiet-build[=y|n]` (real `true_y_or_n`): redirect a build's phase
    // output to `${T}/build.log` instead of the terminal. `None` = not
    // given. Also implied by `-q`/`--quiet` and (already) by `-j` >1.
    let mut quiet_build: Option<bool> = None;
    // --buildpkg-exclude: source entries matching one of these atoms are
    // merged but not binpkg'd (real `--buildpkg-exclude`).
    let mut buildpkg_exclude: Vec<String> = Vec::new();
    // --keep-going: real main.py's own `y_or_n` validator, but this
    // portuale's own transcription (`emerge_options::BOOLEAN_OPTIONS`)
    // already narrows it to the bare/`y` form only, the same shape
    // `--newrepo`/`--buildpkgonly` have -- only meaningful alongside
    // `--buildpkgonly` (without `--pretend`), see `emerge_build::
    // run_buildpkgonly`'s own doc comment for why portuale's own
    // simplified, no-cross-entry-ordering context makes the real
    // semantics much narrower than real portage's own general
    // mergelist-recalculation/resume-state machinery.
    let mut keep_going = false;
    // --debug / -d (real `main.py:1235`): `PORTAGE_DEBUG=1` in the phase
    // env for a real build (real `bin/ebuild.sh` `set -x`); a no-op for
    // `--pretend` (real's `logging.DEBUG` trace has no portuale
    // equivalent). Threaded into the build/merge/config/unmerge paths.
    let mut debug = false;
    // --jobs[=N] / -j[N] (real `main.py`, `valid_integers`): the maximum
    // number of concurrent jobs -- package *builds* for a plain source
    // `emerge <atom>` (real `_emerge/Scheduler.py`'s `_max_jobs`), `depend`
    // phases for `emerge --regen` (real `MetadataRegen`'s `max_jobs` --
    // see `regen.rs`). Default 1 = portuale's long-standing
    // strictly-serial build+merge loop; a bare `--jobs`/`-j` means "as many
    // as the work allows" (`usize::MAX`, capped by the scheduler); an
    // explicit `0` means the CPU count (real `main.py:1036-1039`). The vdb
    // merge step is always serialized regardless -- only the `install`
    // phase runs in parallel, matching real portage. Only consulted for a
    // plain source `emerge <atom>` and `--regen`; `--buildpkgonly` and the
    // binary-merge paths stay serial (nothing to build in parallel).
    let mut jobs: usize = 1;
    /// Real `main.py:1036-1039`: an explicit `--jobs=0` means the CPU
    /// count (not "one job"). Shared by every `--jobs` spelling below.
    fn jobs_count(n: usize) -> usize {
        if n == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            n
        }
    }
    // --load-average[=LA] / -l[LA] (real `main.py`, `type=float`): the
    // scheduler will not start an *additional* build (beyond the one it
    // always allows, so it can never deadlock) while the system's 1-minute
    // load average is above `LA`. Real `_emerge/Scheduler.py`'s
    // `_is_work_scheduled` / `JobStatusDisplay` load gate. `None` = no
    // throttle (portuale's default). Only meaningful together with
    // `--jobs` > 1.
    let mut load_average: Option<f64> = None;
    // --root-deps: real main.py's own `choices: ("True", "rdeps")`, plus
    // a bare form (no `=value` at all). Portuale doesn't distinguish
    // "True" (fold DEPEND/BDEPEND/IDEPEND into RDEPEND) from "rdeps" --
    // and for this EAPI-7+-only fork it never needs to: at EAPI 7+
    // (`eapi_attrs.bdepend`, `depgraph.py:4218-4238`) the `--root-deps ==
    // "rdeps"` `ignore_depend_deps` branch is inside `else: if
    // eapi_attrs.bdepend`, so `=rdeps` is a **complete no-op**. Every
    // accepted form just enables the one behavior portuale implements:
    // real running-root satisfiability for DEPEND/BDEPEND/IDEPEND atoms
    // (which at EAPI 7+ genuinely target `/` for BDEPEND/IDEPEND and
    // `ESYSROOT` ≈ ROOT for DEPEND). See `root_deps_satisfied_atoms`'s
    // own doc comment.
    let mut root_deps = false;
    let mut with_test_deps = false;
    let mut changed_deps_report = false;
    // --verbose-slot-rebuilds: real `y_or_n`, default `"y"` -- NOT gated
    // on `--verbose`. Only `=n` suppresses the "The following packages
    // are causing rebuilds:" block (`_show_abi_rebuild_info`).
    let mut verbose_slot_rebuilds = true;
    // --ignore-built-slot-operator-deps: real `y_or_n` (default `"n"`,
    // `main.py:470`). "Intended only for debugging purposes" -- when `y`,
    // the slot-operator auto-rebuild scan is skipped entirely (real
    // portage strips the built `:=` operator parts so the scan finds
    // nothing; same net effect). Threaded into `resolve_pretend_graph`.
    let mut ignore_built_slot_operator_deps = false;
    // --backtrack=COUNT: real `type=int` / `valid_integers` (`main.py`).
    // The resolver's retry ceiling after a solvable slot conflict; real
    // portage's default (flag absent) is 20 (`_backtrack_depgraph` reads
    // `myopts.get("--backtrack", 20)`), `--backtrack=0` disables
    // backtracking. Threaded into `resolve_pretend_graph`.
    let mut backtrack_max: u32 = DEFAULT_BACKTRACK_MAX;
    // --solver=<portage|pubgrub|resolvo>: portuale-only (real `emerge`
    // has no `--solver`), same "special-cased, not in emerge_options.rs"
    // treatment `--shell`/`--json` get. Picks the dependency-solving
    // algorithm: the backtracking walk (default, the one real Portage
    // runs) or lu-zero's PubGrub / resolvo bridges over the same repo
    // facts (see `solver_bridge.rs`).
    let mut solver = portage_repo::SolverKind::Portage;
    // --verbose-conflicts: real bare boolean (`main.py`'s `y_or_n` group,
    // so `--verbose-conflicts` / `=y` / `=n`). Pure display: real
    // `_prepare_conflict_msg_and_check_for_specificity` shows *every*
    // parent atom of a slot conflict instead of one representative per
    // collision reason, and drops the "(and N more ...)" / "NOTE: Use the
    // '--verbose-conflicts' option" trailer. Never reaches the resolver.
    let mut verbose_conflicts = false;
    // --autounmask/--autounmask-keep-keywords: real "true_y_or_n"
    // (bare flag, "=y", "=True", or "=n") for both -- see the on/off
    // default-resolution logic just below where these are actually
    // consumed, grounded against real create_depgraph_params.py's own
    // autounmask/autounmask_keep_keywords computation.
    let mut autounmask: Option<bool> = None;
    let mut autounmask_keep_keywords: Option<bool> = None;
    let mut autounmask_use: Option<bool> = None;
    // --autounmask-license: real `y_or_n` (REQUIRED value). Real default
    // `"y" if autounmask is True else "n"` (create_depgraph_params.py) --
    // so OFF unless `--autounmask` itself is explicit or `=y` is given.
    let mut autounmask_license: Option<bool> = None;
    // --autounmask-keep-masks: real `true_y_or_n` (bare flag, "=y",
    // "=True", or "=n"). Real KEEPS masks by default
    // (`autounmask_keep_masks` defaults `True`); only `=n` unmasks.
    let mut autounmask_keep_masks: Option<bool> = None;
    // --autounmask-only (real `true_y_or_n`, `main.py:813`): "only perform
    // --autounmask" -- real `actions.py:456`: resolve the graph, then
    // `mydepgraph.display_problems(); return 0`, skipping the whole merge
    // list. Nothing is ever built (it forces the dry-run path). The
    // portuale's `display_problems` equivalent = the slot-conflict notice +
    // the autounmask suggestion blocks (+ the unsatisfied-dep stderr
    // notes already printed during resolution).
    let mut autounmask_only = false;
    // --autounmask-continue (real `true_y_or_n`, `main.py:345`/`810`) and
    // --autounmask-backtrack (real `choices: ("y", "n")`, a REQUIRED
    // value, `main.py:338`, **disabled by default** -- `man emerge`).
    // `--autounmask-continue`'s "write the changes and keep merging" path
    // is gated on `"--pretend" not in myopts` (`depgraph.py:5796`), so
    // under `--pretend` its only observable is the `actions.py:3772`
    // warning against `--autounmask=n` -- but it also *implies*
    // `--autounmask-backtrack=y` (`man emerge`), and that half IS
    // observable here: it gates the `'backtrack` loop's autounmask
    // re-drive (`Config::autounmask_backtrack`). `None` = flag not given.
    let mut autounmask_continue: Option<bool> = None;
    let mut autounmask_backtrack: Option<bool> = None;
    // --usepkg/-k, --usepkgonly/-K, --binpkg-respect-use: all three real
    // "true_y_or_n" (bare flag, "=y", or "=n"), same shape --autounmask
    // already has. --binpkg-respect-use's own real default ("auto",
    // effectively on, whenever --usepkgonly is NOT given -- see
    // create_depgraph_params.py:47-55) is resolved below, once usepkgonly
    // itself is known.
    let mut usepkg = false;
    let mut usepkgonly = false;
    // --getbinpkg/-g, --getbinpkgonly/-G (real `main.py`, `y_or_n`).
    // Folded into `usepkg`/`usepkgonly` below (real depgraph treats a
    // binrepo package the same as a $PKGDIR one for pool eligibility);
    // `getbinpkg` additionally turns on *remote* candidate loading.
    let mut getbinpkg = false;
    let mut getbinpkgonly = false;
    let mut binpkg_respect_use: Option<bool> = None;
    // --quickpkg-direct (real `y_or_n`, REQUIRED value, `main.py:604`,
    // default "n") + --quickpkg-direct-root (real `store`, `main.py:608`,
    // default = the running root's `ROOT`). When active (real
    // `actions.py:145-149`: `--usepkg` is True AND `--quickpkg-direct ==
    // "y"` AND target `ROOT` != the source root), every package installed
    // in the source root becomes a binary candidate for the target-`ROOT`
    // build -- see `portage_repo::set_quickpkg_direct_root`.
    let mut quickpkg_direct: Option<bool> = None;
    let mut quickpkg_direct_root: Option<String> = None;
    // --rebuilt-binaries's own real default ("auto-on" whenever
    // --usepkgonly/--deep/--update are ALL given together, even with no
    // explicit --rebuilt-binaries at all -- create_depgraph_params.py:
    // 185-193) is resolved below, once those three are known.
    let mut rebuilt_binaries: Option<bool> = None;
    let mut rebuilt_binaries_timestamp: Option<u64> = None;
    let mut noreplace = false;
    // `None` until an explicit `--selective`/`--selective=y`/`--selective=n`
    // is given, so `n` can override whatever `update`/`newuse`/
    // `changed_use`/`changed_deps`/`changed_slot`/`noreplace` computed --
    // matching real `create_depgraph_params.py`'s own unconditional
    // `if myopts.get("--selective") == "n": myparams.pop("selective",
    // None)`, checked after every other trigger. See `selective`'s own
    // computation just before the `resolve_pretend_graph` call below.
    let mut selective_flag: Option<bool> = None;
    // `--shell bash|brush` (default `bash`): which real shell backend
    // executes the phase chain of a real (non-`--pretend`) `emerge <atom>`
    // / `--getbinpkg` / `--buildpkgonly` / `--resume` merge -- see
    // `ebuild_phases::ShellBackend`'s doc comment (including why bash is
    // the default). A portuale-only flag (real `emerge` has no `--shell`),
    // so it's special-cased in the parse loop rather than routed through
    // `emerge_options.rs`, the same treatment `--json` gets. Inert under
    // `--pretend` (nothing executes). Also threaded into the removal-hook
    // (`pkg_prerm`/`pkg_postrm` under `-C`/`--unmerge`/`--depclean`/
    // `--prune`/`--clean`/`--rage-clean`, plus the `FEATURES=unmerge-backup`
    // `quickpkg`) and `--config` (`pkg_config`) paths -- every real
    // (non-`--pretend`) phase chain `emerge` can drive picks up the same
    // backend.
    let mut shell = ebuild_phases::ShellBackend::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--pretend" || arg == "-p" {
            pretend = true;
            i += 1;
        } else if arg == "--ask-enter-invalid" {
            // Real boolean flag (no value): `UserQuery.query`'s
            // `enter_invalid` gate. Stored process-wide for the prompt
            // helpers, reset at `run` entry.
            ASK_ENTER_INVALID.store(true, std::sync::atomic::Ordering::Relaxed);
            i += 1;
        } else if arg == "--ignore-default-opts" {
            // Real `main.py:1352`: skip `EMERGE_DEFAULT_OPTS` entirely.
            // The skip itself happens on the raw argv in
            // `args_with_emerge_defaults`; accepted here as a no-op.
            i += 1;
        } else if arg == "--ask" || arg == "-a" {
            // Real `true_y_or_n`: a bare flag (real
            // `insert_optional_args` inserts `"True"`), `--ask=y` /
            // `--ask=True` / `--ask=n`, or a separate `y` / `n` word
            // (only `y`/`n` are consumed -- real's insert `y_or_n`
            // set; a separate `True` stays a positional atom, same as
            // here where it falls through to the atom args below).
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
            // Real `true_y_or_n`: a bare flag (real
            // `insert_optional_args` inserts `"True"`), `--read-news=y`
            // / `--read-news=True` / `--read-news=n`, or a separate `y`
            // / `n` word -- same separate-word shape as `--ask` above.
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
            // Real `main.py`: `default_arg_opts` `y_or_n` (a bare flag
            // inserts `"True"`) + `argument_options`
            // `choices: true_y_or_n` (`"True"`, `"y"`, `"n"`). A
            // space-separated value is only consumed when it is `y` or
            // `n` (real `insert_optional_args`); anything else leaves
            // the bare form (`"True"`) in place and the token to the
            // positional parser -- the same shape every other
            // optional-value flag in this loop already has. `=True` is
            // accepted explicitly (real argparse's own `=` form); any
            // other value is a real, immediate usage error (real
            // argparse's own choices validation), rc 2 either way.
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
            // Same real shape as `--onlydeps-with-rdeps` above
            // (`default_arg_opts` `y_or_n` + `choices: true_y_or_n`).
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
            // Real `main.py`'s own plain-boolean "options" list -- only
            // affects `_create_use_string`'s `USE="…"` ordering (one
            // combined bare-name-sorted list instead of enabled-first).
            alphabetical = true;
            i += 1;
        } else if arg == "--color" || arg.starts_with("--color=") {
            // Real `emerge --color y|n` (`main.py:421`): the explicit
            // override that wins over `NO_COLOR`/`NOCOLOR`/isatty. A
            // required value -- `y` or `n`, as `--color y` or `--color=y`.
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
            // Real `main.py`: `"choices": true_y_or_n` -- a value flag
            // (`y`/`n`/`True`). Bare (no value) is lenient here -> `y`.
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
            // Real "--noreplace"/"-n": a plain boolean, no value at all
            // (real main.py's own boolean-options list) -- unlike
            // "--selective" below, which has the same name/meaning but a
            // real optional y_or_n value. Its entire real effect is
            // setting `selective` -- see `resolve_pretend`'s own doc
            // comment (portage-repo).
            noreplace = true;
            i += 1;
        } else if arg == "--deep" || arg == "-D" {
            // Peeks at the next token, consuming it only if it parses as
            // a non-negative integer -- see the module doc comment (real
            // `valid_integers`'s own `__contains__`, checked by
            // `insert_optional_args` before optparse ever sees the
            // value). A bare `--deep`/`-D`, or one followed by anything
            // that doesn't parse this way, means unlimited depth,
            // matching real `myoptions.deep == "True"`.
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
            // argparse's own native `=`-form -- a separate mechanism from
            // the optional-next-token one above, so a non-numeric value
            // here is a real, immediate parse error (matching real
            // `parser.error("Invalid --deep parameter: ...")`, unlike a
            // non-numeric *next token* above, which just means "no value
            // given" and is left alone).
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
            // Real `main.py` `--backtrack`: `type=int`, and listed in
            // `insert_optional_args`'s `valid_integers` set, so the next
            // token is consumed only if it parses as a non-negative
            // integer -- exactly like `--deep`/`-D` above. A bare
            // `--backtrack`, or one followed by a non-integer, leaves the
            // default (10) in place.
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
            // argparse's native `=` form -- a non-integer here is an
            // immediate parse error (real `parser.error`), unlike a
            // non-integer *next token* above.
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
            // Real "action": "append" -- repeatable, each occurrence's own
            // value is itself a *space-separated* atom list (real
            // bin/emerge's own help text: "A space separated list of
            // package names or slot atoms"), so both accumulate: multiple
            // `--exclude`/`-X` occurrences, and multiple atoms within one
            // occurrence's value. Unlike `--deep`/`-D`'s own optional
            // value, this one is required -- a missing value is a real,
            // immediate usage error, not "no value given, fall back to a
            // default."
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
            // Real `y_or_n` (a value is required in real `argparse`; the
            // portuale is lenient and treats a bare flag as `y`).
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
            // Real `y_or_n` (a value is required in real `argparse`); the
            // portuale is lenient and treats a bare flag as `y`.
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
            // Real `y_or_n`, default "y" -- only `=n` disables.
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
            // Real `true_y_or_n` -- bare / `=y` / `=True` -> on, `=n` -> off.
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
            // Real `true_y_or_n` (`main.py:390`/`709`) + `default_arg_opts`
            // (bare -> `y`): same bare / `=y` / `y` / `=n` / `n` / `True`
            // shape as the `--rebuild-if-*` block above. Both are
            // resolver-time overrides of an automatic default -- applied
            // via `portage_repo::set_binpkg_changed_deps_override` /
            // `set_use_ebuild_visibility` (process-globals, the same
            // env-free pattern `--useoldpkg-atoms` uses).
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
            // Real `true_y_or_n` -- bare / `=y` / `=True` -> on, `=n` -> off
            // (real `main.py:878-881`: default `None`, only a `true_y`
            // value flips it to `True`). Same bare/`=y`/`=n` shape as the
            // `--rebuild-if-*` block above.
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
            // Same `true_y_or_n` bare/`=y`/`=True` -> on, `=n` -> off shape
            // as `--complete-graph` above (real `main.py:164`/`486`/
            // `928-929` + `create_depgraph_params.py:128-129`).
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
            // Real `y_or_n` (a value is required in real `argparse`; the
            // portuale is lenient and treats a bare flag as `y`, like
            // `--dynamic-deps`). Both default ON, only `=n` opts out.
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
            // Same "action": "append", space-separated-per-occurrence
            // shape as --exclude above -- no short alias, real main.py
            // never gives it one.
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
            // Same "action": "append", space-separated-per-occurrence
            // shape as --exclude. A source entry matching one of these is
            // merged normally but no binpkg is written for it (real
            // `--buildpkg-exclude`'s own `InternalPackageSet`).
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
            // Portuale-only (real emerge has no `--shell`), same
            // "special-cased, not in emerge_options.rs" treatment `--json`
            // gets -- see `shell`'s own declaration above.
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
            // Portuale-only (real `emerge` has no `--solver`), same
            // `=`/space handling as `--shell` above.
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
            // NOT a real emerge option at all -- real portage has no
            // structured-output mode for --pretend. Portuale-specific, so
            // deliberately not routed through emerge_options.rs's
            // real-CLI-surface tables at all (unlike every other flag
            // here), and given no short alias (nothing to bundle).
            json = true;
            i += 1;
        } else if arg == "--verbose" || arg == "-v" {
            // Peeks at the next token, consuming it only if it's exactly
            // "y"/"n" -- see the module doc comment on why (real
            // insert_optional_args behavior for a standalone, non-bundled
            // occurrence).
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
            // Real `--quiet`/`-q`: `true_y_or_n` choices
            // (`_emerge/main.py:612-614`, normalized `in true_y` at
            // `:939-941`); a bare occurrence inserts `"True"` via
            // `insert_optional_args` (`:172` is in the `y_or_n` insert
            // set, so only a separate `y`/`n` is consumed and a
            // separate `True` stays positional) -- the same
            // optional-value shape `--verbose`/`-v` has. Sets real
            // `_DisplayConfig` verbosity to 1 (see `render` / `use_suffix`
            // / `attr_display_field` for what that changes).
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
            // Real "--deselect": `true_y_or_n` choices
            // (`_emerge/main.py:446-449`, normalized `in true_y` at
            // `:869-870`), the same optional-value shape
            // "--verbose"/"-v" already has -- a bare `--deselect`/`-W`
            // inserts `"True"` via `insert_optional_args` (`:158` is in
            // the `y_or_n` insert set, so only a separate `y`/`n` is
            // consumed and a separate `True` stays positional; see that
            // branch's own comment). Unlike "--verbose", a bare
            // "--deselect"/"-W" turns this whole invocation into a
            // different, standalone action (see run_deselect's own doc
            // comment) rather than modifying the ordinary --pretend
            // resolution -- real main.py's own "if myaction is None and
            // myoptions.deselect is True: myaction = 'deselect'".
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
            // Real `main.py`: `--unmerge`/`-C` is a standalone ACTION
            // (`_UnmergeAction`, `myaction = "unmerge"`), the same shape
            // as `--deselect`/`--depclean` -- dispatched to
            // `run_unmerge_pretend` below, not a modifier on ordinary
            // resolution. Plain boolean (no value).
            unmerge = true;
            i += 1;
        } else if arg == "--depclean" || arg == "-c" {
            // Real `main.py`: `--depclean`/`-c` is a standalone ACTION
            // (`myaction = "depclean"`), dispatched to
            // `run_depclean_pretend` below. Plain boolean.
            depclean = true;
            i += 1;
        } else if arg == "--prune" || arg == "-P" {
            // Real `main.py`: `--prune`/`-P` is a standalone ACTION
            // (`myaction = "prune"`), routed through the same
            // `action_depclean` as `--depclean` -- dispatched to
            // `run_prune_pretend` below. Plain boolean.
            prune = true;
            i += 1;
        } else if arg == "--config" {
            // Real `main.py`: `--config` is a standalone ACTION
            // (`myaction = "config"`, `action_config`) -- run `pkg_config`
            // for one installed package. No short alias. Dispatched to
            // `run_config_action` below.
            config_action = true;
            i += 1;
        } else if arg == "--with-bdeps" {
            // Real "argument_options" with `"choices": ("y", "n")` --
            // unlike --exclude (arbitrary text) or --deep/--verbose
            // (either an optional peek, or values beyond y/n), this is a
            // REQUIRED, closed-choice value: a missing value is a real,
            // immediate usage error (same shape as --exclude's own), and
            // a value that's neither "y" nor "n" is *also* a real,
            // immediate usage error (real argparse's own choices
            // validation) -- there's no "not given at all" default to
            // silently fall back to for either failure mode.
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
            // Real "--with-bdeps-auto": the identical required,
            // closed-choice ("y"/"n") shape "--with-bdeps" itself has --
            // both live in real main.py's own "argument_options" table,
            // registered the same way, not the optional-value "y_or_n"
            // shape --changed-slot/--with-test-deps have.
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
            // Real "--changed-deps": `true_y_or_n` choices
            // (`_emerge/main.py:405-407`, normalized `in true_y` at
            // `:848-851`), the same optional-value shape
            // "--verbose"/"-v" and "--deselect"/"-W" already have -- a
            // bare occurrence inserts `"True"` via
            // `insert_optional_args` (`:152` is in the `y_or_n` insert
            // set, so only a separate `y`/`n` is consumed and a
            // separate `True` stays positional). No short alias, though
            // (real main.py declares none). Unlike --deselect, this stays
            // an ordinary --pretend modifier, not a standalone action.
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
            // Real "--changed-deps-report": `true_y_or_n` choices
            // (`_emerge/main.py:409-411`, normalized `in true_y` at
            // `:854-857`), the identical optional-value shape
            // "--changed-deps" already has -- a bare occurrence inserts
            // `"True"` via `insert_optional_args` (`:154` is in the
            // `y_or_n` insert set, so only a separate `y`/`n` is
            // consumed). No short alias (real main.py declares none).
            // Unlike --changed-deps, this never changes what gets
            // reinstalled -- see resolve_pretend_graph's own doc comment.
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
            // Real bare boolean; portuale accepts the same permissive
            // bare / `y` / `n` shape as its sibling display flags.
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
            // Real `y_or_n` (default "y"), same optional-value shape.
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
            // Real `y_or_n` (no default arg); portuale accepts the bare
            // form as `y`, same permissive shape as its sibling flags.
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
            // Real "--selective": `true_y_or_n` choices
            // (`_emerge/main.py:697`, normalized `in true_y` at
            // `:988-989`), the same optional-value shape
            // "--changed-deps" already has -- a bare occurrence inserts
            // `"True"` via `insert_optional_args` (`:183` is in the
            // `y_or_n` insert set, so only a separate `y`/`n` is
            // consumed). No short alias for this exact spelling (real
            // main.py declares none; "-n" is "--noreplace" above, real
            // portage's own separate, bare-boolean spelling of the
            // identical meaning). "n" here explicitly CANCELS `selective`
            // even if some other flag already set it -- see
            // `resolve_pretend`'s own doc comment (portage-repo) and this
            // override's own application just before the
            // `resolve_pretend_graph` call below.
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
            // Real "--changed-slot": `true_y_or_n` choices
            // (`_emerge/main.py:413-415`, normalized `in true_y` at
            // `:860-863`), the identical optional-value shape
            // "--changed-deps" already has -- a bare occurrence inserts
            // `"True"` via `insert_optional_args` (`:153` is in the
            // `y_or_n` insert set, so only a separate `y`/`n` is
            // consumed). No short alias (real main.py declares none).
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
            // Real `true_y_or_n`: bare / `y` -> on, `n` -> off.
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
            // Real `true_y_or_n` (`main.py`): bare / `y` -> redirect build
            // output to `${T}/build.log`, `n` -> stream it. Real
            // `Scheduler._background_mode`: `n` never *disables* capture
            // under `-j` >1 (`parallel_jobs` still forces it) -- it only
            // matters for a single-job build.
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
            // Real `main.py:1235`: `--debug`/`-d` sets `PORTAGE_DEBUG=1`
            // (real `bin/ebuild.sh` then runs `set -x` in every phase --
            // portuale threads it into the phase env, same as `ebuild
            // --debug`) and `initialize_logger(logging.DEBUG)` (portage's
            // internal `logging.DEBUG` trace -- Python-implementation
            // noise with no portuale equivalent and not part of the
            // "same behaviour" contract, a documented no-op here). So
            // `emerge --pretend --debug` is byte-identical to `emerge
            // --pretend`; the flag only bites on a real build.
            debug = true;
            i += 1;
        } else if arg == "--resume" || arg == "-r" {
            resume = true;
            i += 1;
        } else if arg == "--list-sets" {
            // Real `main.py`: `--list-sets` is a standalone ACTION
            // (`myaction = "list-sets"`) -- print every defined package
            // set name, sorted, one per line (`actions.py:3839`).
            list_sets = true;
            i += 1;
        } else if arg == "--search" || arg == "-s" {
            // Real `main.py`: `--search`/`-s` is a standalone ACTION
            // (`action_search`). The search terms are the remaining
            // positional args.
            search_action = true;
            i += 1;
        } else if arg == "--searchdesc" || arg == "-S" {
            // Real `main.py`: `--searchdesc`/`-S` implies `--search` and
            // also matches DESCRIPTION (`action_search`, `searchdesc`).
            search_action = true;
            searchdesc = true;
            i += 1;
        } else if arg == "--fuzzy-search" || arg.starts_with("--fuzzy-search=") {
            // Real `true_y_or_n` -- bare / `=y` / `=n` (`y`/`n`/`True`).
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
            // Real `y_or_n` with `"default": "y"`.
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
            // Real `main.py:1088-1099`: must parse as a float in [0, 100].
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
            // Real `main.py`: `--check-news` is a standalone ACTION
            // (`actions.py:3844`) -- count unread relevant GLEP 42 news
            // items per repo.
            check_news = true;
            i += 1;
        } else if arg == "--regen" {
            // Real `main.py`: `--regen` is a standalone ACTION
            // (`action_regen` -> `MetadataRegen`).
            regen_action = true;
            i += 1;
        } else if arg == "--metadata" {
            // Real `main.py`: `--metadata` is a standalone ACTION
            // (`action_metadata` -- the post-`--sync` cache transfer).
            metadata_action = true;
            i += 1;
        } else if arg == "--sync" {
            // Real `main.py`: `--sync` is a standalone ACTION. A permanent
            // non-goal in portuale -- see the dispatch below.
            sync_action = true;
            i += 1;
        } else if arg == "--clean" {
            // Real `main.py`: `--clean` is a standalone ACTION
            // (`action_uninstall` -> `unmerge` `unmerge_action="clean"`)
            // -- per slot, remove every version older than the newest.
            clean_action = true;
            i += 1;
        } else if arg == "--rage-clean" {
            // Real `main.py`: `--rage-clean` is a standalone ACTION -- a
            // fast `--unmerge` (same selection; skips `CLEAN_DELAY` +
            // prerm/postrm).
            rage_clean = true;
            i += 1;
        } else if arg == "--info" {
            // Real `main.py`: `--info` is a standalone ACTION
            // (`action_info`). No short alias.
            info_action = true;
            i += 1;
        } else if arg == "--skipfirst" || arg == "--skip-first" {
            skipfirst = true;
            i += 1;
        } else if arg == "--jobs" || arg == "-j" {
            // Optional integer next token (real `valid_integers` /
            // `insert_optional_args`), exactly like `--deep`: a bare
            // `--jobs`/`-j`, or one followed by a non-integer, means
            // unlimited.
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
            // argparse's attached short-option form (`-j4`).
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
            // Real `main.py` `--load-average`: `type=float`. A required
            // value (unlike `--jobs`' optional one) -- a missing or
            // non-numeric value is an immediate usage error.
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
            // Real "--with-test-deps": `true_y_or_n` choices
            // (`_emerge/main.py:745-747`, normalized `in true_y` at
            // `:1133-1136`), the identical optional-value shape
            // "--changed-deps"/"--changed-slot" already have -- a bare
            // occurrence inserts `"True"` via `insert_optional_args`
            // (`:191` is in the `y_or_n` insert set, so only a separate
            // `y`/`n` is consumed). No short alias (real main.py
            // declares none).
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
            // Real "--autounmask": choices=true_y_or_n ("True", "y",
            // "n") -- a bare flag means true (real argparse's own
            // const="True" for this option), same optional-value shape
            // "--changed-slot"/"--with-test-deps" already have, just
            // with three accepted spellings for "true" instead of one.
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
            // Real `true_y_or_n` -- bare / `=y` / `=True` -> on, `=n` -> off.
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
            // Real `true_y_or_n` (`main.py:345`) -- bare / `=y` / `=True`
            // -> on, `=n` -> off. Same shape as `--autounmask-only`.
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
            // Real `choices: ("y", "n")` (`main.py:338`) -- a REQUIRED
            // value, the same shape `--autounmask-keep-keywords` has.
            // Gates the `'backtrack` loop's autounmask re-drive (see
            // `Config::autounmask_backtrack`); disabled by default.
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
            // Real "--autounmask-keep-keywords": `true_y_or_n` choices
            // (`_emerge/main.py:365-367`, normalized `in true_y` at
            // `:821-822`) -- a bare occurrence inserts `"True"` via
            // `insert_optional_args` (`:146` is in the `y_or_n` insert
            // set, so only a separate `y`/`n` is consumed and a
            // separate `True` stays positional), the same
            // optional-value shape "--changed-deps" already has.
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
            // Real "--autounmask-use": plain y_or_n, a REQUIRED value --
            // same shape as "--autounmask-keep-keywords" above (real
            // `lib/_emerge/main.py`'s own `"choices": y_or_n`, not
            // `true_y_or_n`).
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
            // Real `y_or_n`, REQUIRED value -- same shape as
            // `--autounmask-use`.
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
            // Real "--autounmask-keep-masks": `true_y_or_n` choices
            // (`_emerge/main.py:369-371`, normalized `in true_y` at
            // `:824-825`) -- a bare occurrence inserts `"True"` via
            // `insert_optional_args` (`:147` is in the `y_or_n` insert
            // set, so only a separate `y`/`n` is consumed and a
            // separate `True` stays positional), the same
            // optional-value shape "--changed-deps" already has.
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
            // Real `main.py`: `--getbinpkg`/`-g` (`y_or_n`) -- distinct
            // from `--usepkg`, but real depgraph makes a `--getbinpkg`
            // binrepo's packages eligible the same way `--usepkg` does
            // for `$PKGDIR`, so portuale folds it into `usepkg` and
            // additionally passes `getbinpkg` (-> remote candidates).
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
            // Real `--getbinpkgonly` implies binary-only (`usepkgonly`).
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
            // Real "action": "store" -- a required value, same shape as
            // --exclude's own required argument, but numeric (a Unix
            // timestamp real BUILD_TIME values are compared against).
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
            // Real `y_or_n` (`main.py:604`) -- a REQUIRED value, same shape
            // as `--autounmask-backtrack`.
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
            // Real "action": "store" (`main.py:608`) -- a required path.
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
            // Short-flag bundle (e.g. "-pv") -- decomposed one character
            // at a time, left to right; see the module doc comment for
            // how this differs from real emerge's own recycling-based
            // algorithm (same outcomes, different internal order) and
            // why a bundled -v never consumes a value.
            for c in arg[1..].chars() {
                match c {
                    'p' => pretend = true,
                    // A bundled `-a` (e.g. `-pa`, `-1a`, `-avuDN`) never
                    // consumes a `y`/`n` value -- same "no ambiguity with
                    // another bundled flag character" reasoning as a
                    // bundled `-v`/`-D`/`-W`. The standalone `-a`/`--ask`
                    // handler still accepts the optional `-a y` / `-a n`.
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
                        // Unlike every other bundle-compatible short flag
                        // here, -X's own value is *required*, not
                        // optional -- there's no sensible "just default
                        // it" behavior the way a bundled -v/-D has, so
                        // portuale deliberately doesn't support
                        // bundling -X at all, with a specific message
                        // instead of a misleading generic one.
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
