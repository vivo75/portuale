// `emerge --pretend <category/package>`: the v1 slice (see
// crates/portage-repo/src/lib.rs for the full scope writeup --
// candidates come from every repos.conf repo, main + overlays). USE/
// ACCEPT_KEYWORDS/package.mask/.unmask/.accept_keywords/.use come from
// the real profile chain + make.conf + package.* (see portage-profile's
// doc comment for what that does and doesn't implement), not a
// hardcoded stand-in. Recursively resolves DEPEND+RDEPEND (see
// resolve_pretend_graph's doc comment for the recursion's own scope
// cuts: DEPEND+RDEPEND only, || resolves every alternative, cycle/
// dup-safe, multiple slots of the same package correctly coexist as
// separate entries) and reports (not enforces) both blocker conflicts
// against installed packages/the rest of the graph, and slot conflicts
// (two atoms needing the identical slot at incompatible versions) --
// see print_blockers/the slot_conflicts loop below and
// resolve_pretend_graph's doc comment. Output format is a documented,
// simplified subset of real emerge's --pretend output, not
// byte-identical to it.
//
// --update/-u is real and implemented too (see resolve_pretend's own doc
// comment, portage-repo, for the real `avoid_update` behavior it ports):
// without it, an already-installed version that still satisfies the
// requested atom is kept as-is rather than upgraded to a newer visible
// one -- this is real emerge's own default, not something portuale
// invented, and correcting to it was this flag's whole point.
//
// --deep/-D is real and implemented too (see portage-repo's `Deep` and
// `resolve_pretend_graph`'s own doc comment for the real depth-cutoff
// semantics it ports): without it, an already-installed package's own
// further dependencies are never walked, no matter how deep the graph
// goes -- only New/Upgrade/Reinstall packages (things that actually need
// work) recurse unconditionally either way. Like `--verbose`/`-v`, it's
// real `argument_options` with an *optional* value, not a plain boolean:
// a standalone `--deep`/`-D` peeks at the next token, consuming it only
// if it parses as a non-negative integer (matching real
// `insert_optional_args`'s own `valid_integers` check) -- anything else,
// including nothing at all, leaves depth unlimited without consuming.
// `--deep=N` (argparse's own native `=`-form) is a separate mechanism: a
// non-numeric or negative value there is a real, immediate parse error
// (exit 2), matching real `parser.error("Invalid --deep parameter")`. A
// *bundled* -D (e.g. `-pvD`) never consumes anything, always defaulting
// to unlimited depth -- the same "no ambiguity with another bundled
// flag character" reasoning already established for a bundled -v. `N==0`
// (either form) is indistinguishable from `--deep` never being given at
// all, matching real `create_depgraph_params.py`'s own `!= 0` check.
//
// --exclude/-X is real and implemented too (see portage-repo's
// `resolve_pretend`'s own doc comment for the real `excluded_pkgs`/
// `WildcardPackageSet` behavior it ports, and the documented scope cut
// relative to real depgraph.py's own ~18 call sites): an installed
// package matching an exclude atom is left exactly as-is, regardless of
// `--update`/`--newuse`/`--changed-use`, and a not-yet-installed package
// matching one is never offered as a New/Upgrade candidate either. Real
// `main.py` declares it `"action": "append"` -- repeatable, each
// occurrence's own value itself a *space-separated* atom list (real
// help text: "A space separated list of package names or slot atoms"),
// so both accumulate here too: `--exclude foo --exclude "bar baz"`
// excludes all three. Unlike `--deep`/`-D`'s own optional value, this
// one is required: a missing value (nothing left in `args`) is a real,
// immediate usage error (exit 2), not "fall back to a default." A
// *bundled* -X (e.g. `-pX`) is deliberately NOT supported at all --
// there's no sensible default the way a bundled -v/-D has, so it gets
// its own specific "requires an argument, can't be bundled" message
// instead of being silently misparsed or falling through to a
// misleading generic error.
//
// --deselect/-W is real and implemented too, but unlike every flag
// above, it isn't a modifier on ordinary --pretend resolution at all --
// real `main.py` turns a bare `--deselect`/`-W` into its own standalone
// ACTION (`if myaction is None and myoptions.deselect is True: myaction
// = "deselect"`, the same shape as `--depclean`/`--sync`), dispatched
// here to `run_deselect` before any of the ordinary target-atom/resolve
// machinery even runs. It previews under `--pretend`, and without it
// really rewrites the `world` / `world_sets` files (same "never merges"
// invariant, but the world files themselves are user config portuale
// does edit -- like `-C`/`--depclean`/`--prune`). It reports which
// world-file atoms each given target would cause real `action_deselect`
// (lib/_emerge/actions.py) to discard: every target is expanded against
// the vdb (a bare package name -- no `/` -- via real portage's own
// "null category" mechanism, scanning the world file for a same-named
// atom to borrow its category from, then `vardb.match`-equivalent
// lookup either way -- see `portage_repo::installed_candidates`), and
// each expanded, actually-installed `category/package:slot` is matched
// against every world-file atom. A documented scope cut versus real
// `Atom.intersects()`: only a narrower category/package(+slot) equality
// check is done, not the full version-range/USE-dep algebra -- sufficient
// for the dominant plain-atom case. A `@set`-prefixed world entry is
// never matched, consistent with the pre-existing `read_world_atoms` cut
// for `@world` itself (not a new one). Real `--deselect` is
// `true_y_or_n` choices (`_emerge/main.py:446-449`, `in true_y` at
// `:869-870`), the same optional-value shape `--verbose`/`-v` already
// has: a bare `--deselect`/`-W` or `--deselect y`/`--deselect True`
// enables it, `--deselect n` explicitly disables it (falling through
// to ordinary resolution instead); a bundled `-W` (e.g. `-pW`) never
// consumes a value, always enabling, the same "no ambiguity with another
// bundled flag character" reasoning as bundled `-v`/`-D`. `--ask`
// interactive confirmation and `--json` output are both out of scope for
// deselect mode -- the former needs no special-casing (it already falls
// through to the CLI's existing "not yet implemented" rejection), the
// latter simply isn't offered.
//
// --json is NOT a real emerge option at all -- real portage has no
// structured-output mode for --pretend, so unlike every other flag in
// this file, there's no real behavior to port. Built as a portuale-specific
// convenience, requested directly by name (not routed through
// emerge_options.rs's real-CLI-surface tables the way every other
// unimplemented-but-real flag is, since it isn't one). Dumps the whole
// resolved graph as one line of JSON, `{"entries": [...],
// "slot_conflicts": [...]}`, instead of the plain-text lines below --
// see `print_json`'s own doc comment for the exact shape, including two
// fields no plain-text line carries at all: `requested` and
// `required_by` (see `GraphEntry::required_by`'s own doc comment,
// portage-repo, for how the latter is tracked through the BFS). Hand-
// rolled JSON (`json_escape`/`json_string`), not a crate dependency --
// see `json_escape`'s own doc comment for why. The contract suite and
// `tests/output_invariants.py` read this output; the byte shape is pinned
// by the harvested corpus (`tests/corpus/`).
//
// A top-level atom may carry an operator/version/slot (e.g.
// `>=cat/pkg-1.2`, `cat/pkg:0`) -- resolve_pretend's own atom-vs-candidate
// matching (see portage-repo/src/lib.rs) already handles this correctly
// with zero extra code, since it's the exact same match_from_list path
// every dependency atom extracted from DEPEND/RDEPEND already uses. Only
// a blocker (`!`/`!!`) top-level atom is rejected -- real portage doesn't
// accept a bare blocker as an emerge target either, and prior to this
// slice the CLI's own bare-atom check didn't test for it at all, so
// `emerge --pretend '!!cat/pkg'` used to silently no-op (accepted by the
// CLI, then dropped by resolve_pretend_graph's own blocker skip) instead
// of being rejected -- fixed as part of tightening this same check.
// Dependency atoms extracted from DEPEND/RDEPEND are never restricted
// this way -- real dependency strings need the full atom grammar.
//
// Multiple top-level atoms (`emerge --pretend foo bar`) ARE supported --
// they seed the same BFS/dedup/slot-conflict machinery together (see
// resolve_pretend_graph's doc comment), so a dependency shared between
// two requested atoms dedupes like a diamond dependency, and a slot
// conflict between two *targets* is detected too. A top-level atom with
// no visible candidate aborts the whole run immediately (matching real
// portage's own depgraph.py "there are no ebuilds to satisfy" behavior),
// in argv order -- confirmed with the user before implementing, over the
// alternative of reporting it and still resolving the rest.
//
// CLI surface: every real emerge option/action from lib/_emerge/main.py
// (see emerge_options.rs) is recognized by name -- using one that isn't
// --pretend/-p produces a specific "real emerge option/action, not
// implemented in portuale" message, distinct from a genuinely unknown
// flag ("unrecognized option"). This lets real-world invocations that
// happen to include options portuale doesn't implement (e.g. from
// EMERGE_DEFAULT_OPTS or a script) fail with an accurate, actionable
// message instead of a generic one. See emerge_options.rs's doc comment
// for the (deliberately unfaithful) value-consumption scope cut for
// every option other than --verbose/-v.
//
// --verbose/-v is NOT a plain boolean in real emerge -- it's registered
// in main.py's `argument_options` with `choices=("True", "y", "n")`, and
// `insert_optional_args` (main.py) inserts "True" when it's given with
// no explicit value. So a standalone `--verbose`/`-v` peeks at the next
// argv token: exactly "y" or "n" is consumed as an explicit value,
// anything else (including nothing at all) leaves verbose enabled
// without consuming it -- matching real emerge exactly (verified by
// tracing insert_optional_args by hand). `--verbose=y`/`--verbose=n`
// (argparse's own native `=`-form, a separate mechanism) are handled the
// same way. A *bundled* -v (e.g. `-pv`) never consumes anything at all,
// always defaulting to enabled -- real emerge's own
// `short_arg_opts_n`/"Don't make things like '-kn' expand to '-k n'"
// comment explains why: allowing an inline or next-token value for a
// bundled single-letter flag would be ambiguous with "another bundled
// flag character". See the `--verbose`/`-v` handling and the
// bundle-handling comment, both in `run` below.
//
// Short-flag bundling (`-pv`, `-pd`, ...): a single-dash token longer
// than one character decomposes into its individual short options, one
// per character, left to right -- unlike real emerge's own
// insert_optional_args (which scans for a value-taking short option
// *anywhere* in the bundle and extracts it first via an internal
// recycling stack, regardless of position), portuale processes
// strictly left to right and reports on the first character that's
// either unimplemented-but-recognized or genuinely unrecognized, exiting
// immediately -- exactly the same two outcomes (and same messages) a
// standalone occurrence of that character would produce. This is an
// intentional simplification of the *processing order*, not the
// *outcome*: since portuale exits at the first out-of-scope input
// either way, which internal algorithm finds it first is unobservable
// except in the rare case of two DIFFERENT unimplemented flags bundled
// together, where portuale always reports the leftmost one.
//
// --help/-h is real and implemented, checked unconditionally before
// anything else in `args` -- matching real emerge's own behavior
// (`main.py`'s `parse_opts` maps `-h`/`--help` to the "help" action,
// which `main()` special-cases: `if myaction == "help": emerge_help();
// return os.EX_OK` -- checked once, after the *whole* line has already
// parsed successfully, so it wins regardless of where in argv it
// appears or what other real-but-unimplemented flags accompany it).
// Portuale's own scan is a documented simplification of that: it
// checks every token (including each character of a short-flag bundle)
// for a literal `--help`/`-h`/`h` match unconditionally, rather than
// first confirming the rest of the line would even parse -- so
// `emerge --help --this-is-not-a-real-flag-at-all` prints help here,
// where real emerge would report a parse error instead, since
// `--this-is-not-a-real-flag-at-all` would never successfully reach
// argparse's post-parse action dispatch at all. The help text itself is
// NOT a port of real emerge's own `_emerge/help.py` (157 lines of
// colorized usage syntax for its full ~130-flag surface, most of which
// portuale doesn't implement -- reproducing it here would be actively
// misleading) -- it's a short, honest, portuale-specific summary of what
// portuale actually supports, ending with a pointer to
// README.md and LLM/agent-context.md for the rest.

use crate::color::{self, Colorizer};

mod args;
mod atoms;
mod autounmask_report;
mod config_action;
mod conflicts;
mod deselect;
mod display;
mod early_actions;
mod env_config;
mod expand_targets;
mod final_gates;
mod gates;
mod help;
mod info;
mod json;
mod merge_list;
mod news;
mod problem_reports;
mod prompt;
mod removal;
mod resolve_phases;
mod resolve_plan;
mod resume;
mod search;
mod standalone_actions;
mod world;

#[cfg(test)]
mod tests;

pub(crate) use args::*;
pub(crate) use atoms::*;
pub(crate) use autounmask_report::*;
pub(crate) use config_action::*;
pub(crate) use conflicts::*;
pub(crate) use deselect::*;
pub(crate) use display::*;
pub(crate) use early_actions::*;
pub(crate) use env_config::*;
pub(crate) use expand_targets::*;
pub(crate) use final_gates::*;
pub(crate) use gates::*;
pub(crate) use help::*;
pub(crate) use info::*;
pub(crate) use json::*;
pub(crate) use merge_list::*;
pub use news::*;
pub(crate) use problem_reports::*;
pub(crate) use prompt::*;
pub(crate) use removal::*;
pub(crate) use resolve_phases::*;
pub(crate) use resolve_plan::*;
pub(crate) use resume::*;
pub(crate) use search::*;
pub(crate) use standalone_actions::*;
pub(crate) use world::*;

use crate::difflib;

use crate::ebuild_merge;

use crate::ebuild_package;

use crate::ebuild_phases;

use crate::emerge_build;

use crate::emerge_getbinpkg;

use crate::emerge_options;

use crate::needed_elf;

use portage_dep::{
    Atom, Blocker, Operator, SlotOperator, match_from_list, parse_atom, use_mismatch_flags,
};

use portage_repo::{
    BlockerConflict, BlockerSatisfiedBy, ChangedDepsReportEntry, GraphEntry, PretendOutcome,
    ResolveRequest, SlotConflict, active_resolver_for, all_installed_packages,
    config_root_from_env, ebuild_visible_at, root_from_env, slot_conflict_flag_sets, split_pf,
};

use std::collections::{BTreeMap, HashMap, HashSet};

use std::path::Path;

use std::process::ExitCode;

/// Reports and returns the exit code for a single option/action token
/// ("-x" or "--long", never a positional atom) that isn't --pretend/-p,
/// --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, or
/// --onlydeps/-o -- shared between a standalone token and one character
/// of a decomposed short-flag bundle, so both produce identical
/// messages for the same underlying flag.
fn report_option(token: &str) -> ExitCode {
    if let Some(found) = emerge_options::lookup(token) {
        // Reports and exits immediately, matching every other
        // out-of-scope-input case in portuale -- so there's no need to
        // correctly skip over this option's own value token (see
        // emerge_options.rs's doc comment): nothing after this point is
        // ever looked at.
        let kind = if found.category == emerge_options::Category::Action {
            "action"
        } else {
            "option"
        };
        eprintln!(
            "emerge: {kind} {:?} is a Portage emerge {kind}, but is not yet \
             implemented in portuale -- run \"emerge --help\" for the options \
             and actions that are.",
            found.canonical
        );
    } else {
        eprintln!("emerge: unrecognized option {token:?}");
    }
    ExitCode::from(2)
}

/// The `--backtrack` budget when the flag is absent: real Portage's own
/// default (backlog #263). Grounded in real 3.0.82.2 (`/usr/sbin/emerge`):
/// `_emerge/depgraph.py:12188` (`_backtrack_depgraph`) reads
/// `max_retries = 0 if nodeps else myopts.get("--backtrack", 20)` --
/// `--backtrack` itself is `action: store` with no default
/// (`lib/_emerge/main.py:385-389`), so the 20 lives only in that reader
/// (`0` only under `--nodeps`). Single source for the `run()` flag-absent
/// initializer below and the `--help` line's advertised default.
const DEFAULT_BACKTRACK_MAX: u32 = 20;

pub fn run(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    // `--ask-enter-invalid` is process-wide state for the prompt
    // helpers; reset per invocation so an in-process caller can't leak it
    // (the CLI parse below sets it again from argv or the defaults).
    ASK_ENTER_INVALID.store(false, std::sync::atomic::Ordering::Relaxed);

    // The live binhost-index overrides are process-wide state too
    // (review M4): a second in-process run must resolve from its own
    // refresh, not inherit the first run's in-memory indexes.
    portage_repo::clear_remote_binary_index_overrides();

    // Config resolution comes before argv parsing, matching real
    // `emerge`'s own order: its first pass only finds `--config-root`,
    // then it loads the config and re-parses with
    // `EMERGE_DEFAULT_OPTS` prepended (`_emerge/main.py:1221-1232`,
    // `:1352-1360`). Portuale has no `--config-root` CLI
    // (`PORTAGE_CONFIGROOT` is the only config-root source), so the two
    // passes collapse into this one: resolve, then prepend the
    // variable's tokens to argv. `--help` above still short-circuits
    // before any config load, exactly like real's early
    // `myaction == "help"` return (`main.py:1254`).

    // resolve_config needs the main repo's own location for
    // package.mask/.unmask's repo-level source (see its doc comment) --
    // found via the same find_repos repos.conf parsing
    // resolve_pretend_graph uses internally a few lines down; called
    // again here since portage-profile can't depend back on portage-repo
    // (portage-repo already depends on portage-profile). Resolved before
    // @world/@system expansion below: @system's own atom list lives in
    // `config` (see portage-profile's `system_packages`), so the config
    // must already exist by the time a "@system" token is seen.
    let root = root_from_env();
    let config_root = config_root_from_env();

    // The repos + resolved config (see `load_repos_and_config`): the
    // resolve already runs under `ConfigRootOverride` for the placed
    // root on the remote path.
    let (repos, mut config) = match load_repos_and_config(&config_root, &root) {
        Ok(loaded) => loaded,
        Err(message) => {
            eprintln!("emerge: {message}");
            return ExitCode::from(1);
        }
    };

    // Real `main.py:1352-1360`: `EMERGE_DEFAULT_OPTS` is prepended to
    // argv before the real parse, so argv wins for last-wins options.
    let effective_args = args_with_emerge_defaults(&config, args);
    let args: &[String] = &effective_args;

    let EmergeOpts {
        atom_args,
        mut pretend,
        mut ask,
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
        mut with_bdeps,
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
    } = match parse_emerge_args(args) {
        Ok(opts) => opts,
        Err(code) => return code,
    };

    // `--package-moves` (real `y_or_n`, default "y"): must be applied
    // before anything reads `profiles/updates/` -- see
    // `portage_repo::set_package_moves_enabled`. Harmless for every
    // action (a no-op when `y`).
    portage_repo::set_package_moves_enabled(package_moves);

    // `--useoldpkg-atoms ATOMS`: a process-global read during candidate
    // selection (see `portage_repo::set_useoldpkg_atoms`). Set once here,
    // before any `resolve_pretend_graph` call; empty is a strict no-op.
    // A clone is kept for the slot-collision `need_rebuild` trailer,
    // which re-checks the same atoms at render time (real reads
    // `frozen_config.useoldpkg_atoms` there too).
    let useoldpkg_atoms_render = useoldpkg_atoms.clone();
    portage_repo::set_useoldpkg_atoms(useoldpkg_atoms);

    // `--binpkg-changed-deps=y|n` / `--use-ebuild-visibility=y|n`:
    // resolver-time overrides of automatic defaults, applied as
    // process-globals (see those setters' own doc comments). `None` /
    // `false` (the defaults when the flag is absent) are strict no-ops.
    portage_repo::set_binpkg_changed_deps_override(binpkg_changed_deps_override);
    portage_repo::set_use_ebuild_visibility(use_ebuild_visibility);

    // `--debug`/`-d` under `--pretend`: besides `PORTAGE_DEBUG=1` in the
    // phase env (a no-op when nothing is built), real also turns on the
    // full resolver trace (`initialize_logger(logging.DEBUG)`). Portuale
    // emits the same shapes -- see `portage_repo::resolver_trace`. Gated
    // to `--pretend` only (portuale's build path doesn't route through
    // the traced resolver the way real's does). Process-global, set once
    // before any `resolve_pretend_graph` call.
    portage_repo::set_resolver_debug(debug && pretend);

    if let Some(code) = validate_and_early_actions(ValidateAndEarlyActionsArgs {
        tree,
        columns,
        pretend: &mut pretend,
        ask: &mut ask,
        sync_action,
        search_action,
        info_action,
        list_sets,
        check_news,
        regen_action,
        metadata_action,
        color_opt,
        debug,
        deselect,
        depclean,
        prune,
        unmerge,
        atom_args: &atom_args,
        jobs,
        load_average,
        config_action,
        resume,
        clean_action,
        rage_clean,
    }) {
        return code;
    }

    // Real `actions.py::adjust_configs` colour gate -- resolved once here
    // so every action path (the standalone cleanup actions below and the
    // ordinary resolve-graph path) shares one `Colorizer`.
    let color = Colorizer::new(color::resolve_havecolor(color_opt));

    // Real `_emerge/actions.py::apply_priorities` (called from
    // `run_action`, before any action): renice / ionice this process so
    // every build subprocess it spawns inherits the policy.
    apply_portage_scheduling_policy();

    if let Some(code) = dispatch_standalone_actions(DispatchStandaloneActionsArgs {
        resume,
        config: &mut config,
        root: &root,
        config_root: &config_root,
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
        config_action,
        atom_args: &atom_args,
        color: &color,
        search_action,
        repos: &repos,
        searchdesc,
        fuzzy_search,
        regex_search_auto,
        search_similarity,
        check_news,
        info_action,
        usepkg,
        usepkgonly,
        getbinpkg,
        getbinpkgonly,
        misspell_suggestions,
        clean_action,
        rage_clean,
        unmerge,
        depclean,
        verbose,
        lib_check,
        deselect_n,
        prune,
        nodeps,
    }) {
        return code;
    }
    let ExpandTargetsOut {
        expanded_atoms,
        selected_set_args,
        set_args,
    } = match expand_targets(ExpandTargetsArgs {
        autounmask_continue,
        autounmask,
        color: &color,
        atom_args: &atom_args,
        config: &config,
        root: &root,
        config_root: &config_root,
        repos: &repos,
        quiet,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };

    let ResolvePlan {
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
    } = match plan_resolve(PlanResolveArgs {
        with_bdeps_given,
        with_bdeps: &mut with_bdeps,
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
        config: &mut config,
        autounmask_backtrack,
        autounmask_continue,
        quickpkg_direct,
        quickpkg_direct_root: &quickpkg_direct_root,
        root: &root,
        binpkg_respect_use,
        rebuilt_binaries,
        deep,
        root_deps,
        repos: &repos,
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
        config_root: &config_root,
        color: &color,
        ask,
        read_news,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let result = match run_resolve_phases(RunResolvePhasesArgs {
        complete_seed_atoms: &complete_seed_atoms,
        config: &config,
        config_root: &config_root,
        root: &root,
        expanded_atoms: &expanded_atoms,
        newuse,
        changed_use,
        nodeps,
        onlydeps,
        onlydeps_with_rdeps,
        onlydeps_with_ideps,
        update,
        deep,
        excluded: &excluded,
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
        usepkg_exclude: &usepkg_exclude,
        usepkg_include: &usepkg_include,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        newrepo,
        buildpkgonly,
        root_deps_running_root: &root_deps_running_root,
        distdir: &distdir,
        emptytree,
        getbinpkg,
        ignore_built_slot_operator_deps,
        backtrack_max,
        reinstall_atoms: &reinstall_atoms,
        rebuild_if_new_slot,
        rebuild_if_unbuilt,
        rebuild_if_new_rev,
        rebuild_if_new_ver,
        rebuild_exclude: &rebuild_exclude,
        rebuild_ignore: &rebuild_ignore,
        dynamic_deps,
        implicit_system_deps,
        set_args: &set_args,
        solver,
        repos: &repos,
        misspell_suggestions,
        pretend,
        color: &color,
        complete_if_new_use,
        complete_if_new_ver,
        complete_graph,
        running_config: &running_config,
    }) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let entries = &result.entries;
    // Backlog #19 Slice 4: with the gate on, an aborted resolve renders
    // the outcome's partial list *instead of* the full entries — real
    // never calls `display()` on the abort path (`actions.py:460-462`),
    // so masked/unsat shapes (empty partial) show no list at all and the
    // cycle shape shows the stuck remainder only (`docs/history/abort-path-spec.md`
    // §4). Everything else (resolver trace, abi-rebuild maps, the
    // complete-graph decisions above, the unsolvable-blocker scan below)
    // keeps the full graph. With the gate off
    // (`PORTUALE_ABORT_PATH=0`) the legacy full list renders.
    let gated_abort_partial: Option<&Vec<portage_repo::GraphEntry>> =
        if portage_repo::abort_path_enabled() {
            match &result.outcome {
                portage_repo::ResolveOutcome::Aborted { partial, .. } => Some(partial),
                portage_repo::ResolveOutcome::Complete => None,
            }
        } else {
            None
        };
    let display_entries: &[portage_repo::GraphEntry] =
        gated_abort_partial.map_or(entries.as_slice(), Vec::as_slice);
    // Real `Display.__call__` runs (and prints `Total:`) only when the
    // merge list is displayed at all: the masked/unsat abort shows
    // neither list nor counters, while the cycle abort counts over the
    // partial rows only.
    let display_list_suppressed = gated_abort_partial.is_some() && display_entries.is_empty();

    // `--autounmask-only` (real `actions.py:456`): the normal merge
    // list is skipped and the run exits 0 right after the
    // `display_problems()` equivalent (slot-conflict notice + autounmask
    // blocks). But real `_display_autounmask` re-shows the list itself
    // via `_show_merge_list` (`depgraph.py:10625`, `:10488`) whenever it
    // has changes to report -- so under `--pretend --autounmask
    // --autounmask-only` the list IS printed (backlog #271; grounded on
    // the fresh host 3.0.82.2 staged-fixture probe,
    // `LLM/evidence/2026-09-30-271/`). Also forces the dry-run path:
    // nothing is ever built.
    //
    // Backlog #185: real `_emerge/actions.py:464-469` (`mergelist_shown`
    // branch) shows the merge list before a non-`--pretend` merge only
    // when `--ask`, `--tree` or `--verbose` is given, and never when
    // `--quiet` is given without `--ask`. A `--pretend` run always
    // shows it (the long-standing, corpus-pinned behavior above). An
    // aborted resolve shows no list on a real non-`--pretend` run
    // either (real `actions.py:460-462` fails before any `display()`).
    let has_autounmask_changes = !result.autounmask_keyword_changes.is_empty()
        || !result.autounmask_mask_changes.is_empty()
        || !result.autounmask_use_changes.is_empty()
        || !result.autounmask_license_changes.is_empty();
    let show_merge_list = (!autounmask_only
        && merge_list_shown(pretend, ask, tree, verbose, quiet)
        && (pretend || gated_abort_partial.is_none()))
        || autounmask_only_reshows_merge_list(pretend, autounmask_only, has_autounmask_changes);
    show_resolution_header(ShowResolutionHeaderArgs {
        pretend,
        quiet,
        ask,
        tree,
        verbose,
        buildpkgonly,
        unordered_display,
        nodeps,
        result: &result,
    });

    show_pprovided_notice(&result, &color);

    // Which (category, package) pairs were directly requested (as opposed
    // to reached only as a dependency) -- a top-level atom that's
    // AlreadyInstalled gets its own "nothing to do" line, unlike a
    // dependency-level one, which stays silent below. A top-level atom's
    // own NoVisibleCandidate never reaches here at all: resolve_pretend_graph
    // already aborted the whole call for that case (see its doc comment).
    let top_level_pkgs: HashSet<(String, String)> = expanded_atoms
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    if let Some(code) = print_json_result(PrintJsonResultArgs {
        json,
        display_entries,
        result: &result,
        top_level_pkgs: &top_level_pkgs,
        verbose,
        root_deps_running_root: &root_deps_running_root,
        has_autounmask_changes,
        autounmask_only,
        pretend,
        autounmask_continue,
        backtrack_max,
        buildpkgonly,
        nodeps,
        gated_abort_partial,
    }) {
        return code;
    }

    // Real portage resolves COLUMNWIDTH (and warns on an unparsable
    // value) as part of general display setup, unconditionally --
    // never gated on --columns itself actually being given. Mirrored
    // here the same way, even though the value only ever affects
    // anything below when `columns` is true.
    let columnwidth = columnwidth_from_env();
    // `Display.pkgprint`'s `@system`/world inputs (`color` already
    // resolved above): `@system` = the profile's own package set
    // (`config.system_packages`); world = `var/lib/portage/world` (a
    // missing file is a valid empty world, same as everywhere else).
    let world_atoms: Vec<String> = read_world_atoms(&root)
        .unwrap_or_default()
        .iter()
        .map(|a| portage_repo::apply_updates_to_atom(a))
        .collect();
    let system_atoms = &config.system_packages;
    // Real `PkgAttrDisplay.force_reinstall` (the red `r` bracket column):
    // every `(cat, pkg)` on either side of a slot-operator ABI-rebuild
    // edge -- the triggering provider upgrade AND every forced consumer
    // rebuild in the cascade. Real yields the
    // `@__auto_slot_operator_replace_installed__` set with
    // `force_reinstall=True` and tags the provider via `_forced_rebuilds`.
    // Backlog #214: real only tags a merge that is not new
    // (`resolver/output.py:812-816`, `not pkg_info.attr_display.new`) --
    // a provider with nothing installed anywhere (a pure `New` install
    // that still caused a rebuild, like `abiprov-2`) stays `N`.
    // #255: real's `new` comes from `_get_installed_best`
    // (`output.py:708-768`), which is slot-keyed: a provider installed
    // only in *another* slot is `new_slot` and therefore still `new`
    // (`NS`, no `r`). So the gate is "nothing installed in the
    // provider's own slot".
    let force_reinstall_cps: HashSet<(String, String)> = result
        .abi_rebuilds
        .iter()
        .flat_map(|(provider, consumer)| [(provider, true), (consumer, false)])
        .filter_map(|(cpv, is_provider)| {
            let (c, p, v) = portage_repo::split_cpv(cpv)?;
            if is_provider {
                let slot = result
                    .entries
                    .iter()
                    .find(|e| {
                        e.category == c
                            && e.package == p
                            && match &e.outcome {
                                portage_repo::PretendOutcome::New { version }
                                | portage_repo::PretendOutcome::Reinstall { version, .. } => {
                                    *version == v
                                }
                                portage_repo::PretendOutcome::Upgrade { to, .. }
                                | portage_repo::PretendOutcome::Downgrade { to, .. } => *to == v,
                                _ => false,
                            }
                    })
                    .and_then(|e| e.slot.clone());
                let in_slot = portage_repo::installed_candidates(&root, &c, &p)
                    .iter()
                    .any(|(_, s, _)| slot.as_ref().is_none_or(|want| s == want));
                if !in_slot {
                    return None;
                }
            }
            Some((c, p))
        })
        .collect();
    // Real `Display.blockers`: blocker lines are collected while walking
    // the entries and printed as one group after every package line (see
    // `format_blocker_lines`).
    let mut blocker_lines: Vec<String> = Vec::new();
    // Backlog #244: abort-path unsatisfied-dependency lines held back
    // until after the slot-conflict block (real `display_problems`
    // order).
    let mut deferred_abort_unsat: Vec<usize> = Vec::new();
    // Backlog #206 S2: real `_show_circular_deps` (`depgraph.py:10425`)
    // pops `--quiet` and forces `--verbose` + `--tree` before
    // `display(handler.merge_list)` — the stuck remainder renders as a
    // verbose tree no matter what display flags the user passed. When
    // this resolve ends in the circular block, the merge list above IS
    // that display, so it renders with the forced flags (and forced
    // sizes — real `verbose_size` runs at verbosity 3). The legacy
    // (gate-off) circular path keeps the user-flagged list plus its own
    // re-display below, exactly as before.
    let circular_forced_display = gated_abort_partial.is_some()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted {
                reason: portage_repo::AbortReason::UnserializableCycle { .. },
                ..
            }
        );
    let (disp_tree, disp_verbose, disp_quiet) = if circular_forced_display {
        (true, true, false)
    } else {
        (tree, verbose, quiet)
    };
    // Backlog #206 S2: the rendered entry rows of the tree walk, for the
    // circular `Total:` recount only (real counts displayed merge rows —
    // repeats included, `nomerge` excluded — not entries). Empty unless
    // the tree branch below runs.
    let mut rendered_rows: Vec<(usize, bool)> = Vec::new();
    render_merge_list(RenderMergeListArgs {
        show_merge_list,
        disp_tree,
        circular_forced_display,
        display_entries,
        expanded_atoms: &expanded_atoms,
        config: &config,
        root: &root,
        implicit_system_deps,
        repos: &repos,
        dynamic_deps,
        result: &result,
        rendered_rows: &mut rendered_rows,
        top_level_pkgs: &top_level_pkgs,
        onlydeps,
        oneshot,
        unordered_display,
        disp_verbose,
        disp_quiet,
        alphabetical,
        root_deps_running_root: &root_deps_running_root,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
        blocker_lines: &mut blocker_lines,
        quiet,
        columns,
        columnwidth,
        pretend,
        gated_abort_partial,
        deferred_abort_unsat: &mut deferred_abort_unsat,
        verbose,
    });

    show_merge_list_summary(ShowMergeListSummaryArgs {
        circular_forced_display,
        rendered_rows: &rendered_rows,
        display_entries,
        show_merge_list,
        disp_verbose,
        disp_quiet,
        display_list_suppressed,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        onlydeps,
        color: &color,
        result: &result,
        disp_tree,
        pretend,
        ask,
    });

    // Real `_show_circular_deps`' cycle-only re-display
    // (`self.display(handler.merge_list)` with `--verbose --tree`
    // forced): the stuck remainder isolated as its own list between the
    // merge list and the error block. Portuale re-displays
    // `result.cycle_display` (cycle members plus their transitive
    // requirers, leaf-drain order) as flat merge lines -- no tree
    // nesting or `[nomerge]` marking (portuale's tree model dedups
    // shared nodes by design and never abandons the merge list, so
    // there is no partial scheduler state to show); isolating *which*
    // packages is what's ported. A throwaway blocker sink keeps
    // re-displayed blocker owners from doubling the collected lines.
    // Non-empty exactly when a hard cycle was reported (the driver only
    // builds the display then), so no extra gating is needed.
    //
    // Backlog #19 Slice 4: with the gate on, an aborted cycle renders
    // the remainder as the merge list itself (see `display_entries`
    // above), so the re-display would duplicate it line for line and is
    // skipped — real shows the reduced list exactly once too
    // (`_show_circular_deps`' single `display(handler.merge_list)`).
    // Gate off keeps the legacy list-plus-redisplay.
    let skip_redisplay = gated_abort_partial.is_some()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted {
                reason: portage_repo::AbortReason::UnserializableCycle { .. },
                ..
            }
        );
    show_cycle_redisplay(ShowCycleRedisplayArgs {
        show_merge_list,
        result: &result,
        skip_redisplay,
        display_entries,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        columnwidth,
        root_deps_running_root: &root_deps_running_root,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
    });

    show_slot_conflicts(ShowSlotConflictsArgs {
        result: &result,
        root: &root,
        repos: &repos,
        config: &config,
        verbose_conflicts,
        excluded: &excluded,
        useoldpkg_atoms_render: &useoldpkg_atoms_render,
        usepkgonly,
        color: &color,
        backtrack_max,
    });
    show_deferred_unsat(ShowDeferredUnsatArgs {
        deferred_abort_unsat: &deferred_abort_unsat,
        result: &result,
        root: &root,
        top_level_pkgs: &top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        quiet,
        alphabetical,
        columns,
        columnwidth,
        root_deps_running_root: &root_deps_running_root,
        color: &color,
        system_atoms,
        world_atoms: &world_atoms,
        force_reinstall_cps: &force_reinstall_cps,
    });

    show_skipped_updates(ShowSkippedUpdatesArgs {
        quiet,
        debug,
        result: &result,
        root: &root,
        root_deps_running_root: &root_deps_running_root,
        running_config: &running_config,
        repos: &repos,
    });

    show_skipped_missing_deps(quiet, debug, &result);

    show_missing_args_and_aborts(&result, &root, &color, &repos, &config);

    if let Some(code) = show_autounmask(ShowAutounmaskArgs {
        color: &color,
        result: &result,
        autounmask_only,
        pretend,
        repos: &repos,
        root: &root,
        config: &config,
        autounmask_continue,
        has_autounmask_changes,
    }) {
        return code;
    }

    if let Some(code) = failure_gates(FailureGatesArgs {
        result: &result,
        show_merge_list,
        color: &color,
        pretend,
        repos: &repos,
        root: &root,
        config: &config,
        backtrack_max,
        buildpkgonly,
        nodeps,
    }) {
        return code;
    }

    print_resolver_debug(&root, entries, &reinstall_atoms, &result);

    if let Some(code) = final_gates(FinalGatesArgs {
        verbose_slot_rebuilds,
        result: &result,
        entries,
        root: &root,
        pretend,
        repos: &repos,
        config: &config,
        color: &color,
    }) {
        return code;
    }

    // Real execution: only reachable when `!pretend`, which the gate at
    // the top of this function only ever lets through when `buildpkgonly`
    // is also `true` -- see `emerge_build.rs`'s own module doc comment
    // for what this actually does (and doesn't) build.
    if !pretend {
        // Backlog #225 (real `_emerge/actions.py:496-521`): the merge
        // list has just been displayed (see `show_merge_list` above),
        // and when nothing in it is merge-bound real prints `Nothing to
        // merge; quitting.` on stdout and returns `EX_OK` before the
        // `resume_backup` rotation below and before `Scheduler` -- so a
        // stale resume list (and its backup) come back untouched. This
        // sits after every failure gate above (real's own `if not
        // success: display_problems(); return 1` precedes its display
        // branch), and `--resume` never reaches here (it dispatches to
        // `run_resume` far above, with its own `nothing to resume`
        // message).
        //
        // Residue (deliberate scope boundary): the deferral arm above
        // restores real's rotation plus `saveNomergeFavorites`'s record
        // (`actions.py:514-516` + `:664-675` + `depgraph.py:11305-11402`)
        // within portuale's existing world-file cuts -- the
        // `world_file_additions`/`update_world_file` v1 cuts
        // (version-pinned slot args, the vdb-only multislot fallback,
        // system-virtual exclusion). A `world-candidate = false` user
        // set is filtered at the gate and the record alike (see
        // `usersets_world_candidate`); per-set single sections and repo
        // `sets.conf` files stay unconsulted cuts (see its doc comment).
        let mergecount = display_entries
            .iter()
            .filter_map(emerge_build::resume_cpv)
            .count();
        // Backlog #232: real's selective-without-`--oneshot` world
        // candidates -- a plain (non-`@`) favorite, which real's own
        // `world_candidates` filter always keeps (`actions.py:498-513`),
        // or a user `@set` (`selected_set_args`; real defaults
        // `usersets` to `world-candidate = true`, honoring a `sets.conf`
        // override -- see `usersets_world_candidate`). Built-in
        // `@world`/`@system`/`@profile` never count on either side.
        let deferred = selective_noop_deferred(
            selective,
            // Real's folded gate variable (`actions.py:257`): `--onlydeps`
            // keeps the deferral at bay exactly like `--oneshot`.
            oneshot || onlydeps,
            atom_args.iter().any(|a| !a.starts_with('@'))
                || (usersets_world_candidate(&config_root) && !selected_set_args.is_empty()),
        );
        if nothing_to_merge(pretend, show_merge_list, mergecount) {
            if deferred {
                // Real `actions.py:526`: the blank line prints on the
                // prompt shape too (`prompt` stays `None` on this arm,
                // so `UserQuery` is skipped), then the run falls through
                // to the `resume_backup` rotation and
                // `saveNomergeFavorites` -- only `Scheduler` is skipped
                // (`:664-675`).
                println!();
                crate::mtimedb::rotate_resume_to_backup(&root);
                // Backlog #232, second half: real
                // `depgraph.saveNomergeFavorites`
                // (`depgraph.py:11305-11402`) records the world-candidate
                // favorites that stayed unmerged. Its own suppression set
                // (`:11308-11317`) is `--buildpkgonly` / `--fetchonly` /
                // `--fetch-all-uri` / `--oneshot` / `--onlydeps` /
                // `--pretend`: `--pretend` never reaches this branch,
                // `--oneshot` / `--onlydeps` already keep the deferral
                // gate above at bay (real's folded variable,
                // `actions.py:257`), and `--fetchonly` / `--fetch-all-uri`
                // are portuale-unimplemented options (a usage error long
                // before this point), so only `--buildpkgonly` still
                // suppresses here. Under `--ask` the
                // prompt moves here (`:11376-11386`): the additions list
                // prints first, then `Would you like to add these
                // packages to your world favorites?`, and a "No" (or EOF,
                // which `ask_yes_no` folds into the same decline) records
                // nothing -- the run still succeeds, the rotation above
                // already happened.
                if !buildpkgonly {
                    // Backlog #232 review (Important-2): real's record
                    // skips non-candidate sets (`depgraph.py:11366`) --
                    // the same `usersets_world_candidate` filter as the
                    // gate above, so only the gate-passing sets record.
                    let record_sets: &[String] = if usersets_world_candidate(&config_root) {
                        &selected_set_args
                    } else {
                        &[]
                    };
                    let fav_refs: Vec<&str> = atom_args.to_vec();
                    let mut additions = match read_world_atoms(&root) {
                        Ok(current) => {
                            world_file_additions(&current, &fav_refs, entries, system_atoms, &repos)
                        }
                        Err(e) => {
                            eprintln!("emerge: {e}");
                            return ExitCode::from(1);
                        }
                    };
                    match read_world_sets(&root) {
                        Ok(names) => {
                            let current: Vec<String> =
                                names.into_iter().map(|n| format!("@{n}")).collect();
                            additions.extend(world_sets_additions(&current, record_sets));
                        }
                        Err(e) => {
                            eprintln!("emerge: {e}");
                            return ExitCode::from(1);
                        }
                    }
                    if ask && !additions.is_empty() {
                        println!();
                        let mut shown = additions.clone();
                        shown.sort();
                        shown.dedup();
                        for a in &shown {
                            println!(" {} {}", color.c("GOOD", "*"), a);
                        }
                        println!();
                        if ask_yes_no(
                            &color,
                            "Would you like to add these packages to your world favorites?",
                        ) != Some(true)
                        {
                            return ExitCode::SUCCESS;
                        }
                    }
                    // The writers below re-read the world files and
                    // recompute the same additions instead of reusing
                    // `additions`: deliberate -- single-threaded, no
                    // intervening write, so the recompute is identical,
                    // and it keeps one writer entry point for both this
                    // arm and the success path.
                    if let Err(e) = update_world_file(
                        &root,
                        &fav_refs,
                        entries,
                        system_atoms,
                        &repos,
                        oneshot,
                        onlydeps,
                    ) {
                        eprintln!("emerge: {e}");
                        return ExitCode::from(1);
                    }
                    if let Err(e) = update_world_sets_file(&root, record_sets, oneshot, onlydeps) {
                        eprintln!("emerge: {e}");
                        return ExitCode::from(1);
                    }
                }
                return ExitCode::SUCCESS;
            }
            // Real's own `print()` before the message
            // (`actions.py:518`): `--ask` reaches here with no blank
            // line printed yet (the #185 `actions.py:526` blank only
            // covers the no-prompt shape, and `ask_confirm`'s own
            // opener never runs).
            if ask {
                println!();
            }
            println!("Nothing to merge; quitting.");
            println!();
            return ExitCode::SUCCESS;
        }
        // Real `_emerge/actions.py:525-536`: `--ask` prompts once, after
        // the whole merge list is displayed, before anything is built.
        if ask && !ask_confirm(&color, "Would you like to merge these packages?") {
            return ExitCode::from(130);
        }
        // Backlog #185/#197: real `Scheduler._status_msg`
        // (`Scheduler.py:2387-2397`) precedes every status line -- the
        // first `>>> Emerging ...` among them -- with a blank line,
        // unless the scheduler is in background mode (real
        // `_background_mode`: parallel `--jobs`, `--quiet`, or
        // `--quiet-build=y`, except a one-package mergelist without
        // `--quiet` -- see `emerge_build::scheduler_status_mode`).
        // #185 ported only the first blank with a `jobs`/`quiet`
        // approximation; #197 generalizes the rule to every status
        // line, and the merge functions own every blank through their
        // `StatusDisplay::status()` -- including the first, so no
        // blank prints here (fix round 1 removed the duplicate that
        // printed a second blank before the first `>>>` line). The
        // exact background predicate still computes here.
        let status_mode = crate::emerge_build::scheduler_status_mode(
            jobs,
            quiet,
            quiet_build,
            entries.len(),
            verbose,
        );
        // Real BINPKG_COMPRESS/BINPKG_COMPRESS_FLAGS[_<NAME>]/
        // PORTAGE_BZIP2_COMMAND/PKGDIR/... resolution -- see
        // `package_options_from_env`.
        let package_options = package_options_from_env(
            shell,
            debug,
            Some(&config_features_string(&config)),
            Some(&config),
        );
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(&config));
        let mut merge_options = ebuild_merge::MergeOptions::from_env(shell, debug);
        // The compiler / make flags real portage puts in every build
        // phase's environment, from the resolved config (make.conf +
        // profile `make.defaults` + the `env` layer -- all folded into
        // `Config::other_vars`). `merge_one_source_entry` appends the
        // per-entry `USE` on top. Only the deterministic build-var set --
        // not every scalar (`PATH`/`HOME`/... are handled by
        // `phase_env_vars` itself), and `FEATURES` stays out (portuale
        // models it via its own `feature_enabled`, forcing `""` here).
        merge_options.build_env = crate::emerge_build::run_wide_phase_env(&config);
        merge_options.resolved_config = Some(std::sync::Arc::new(config.clone()));
        // Real `preinst_mask()`: `INSTALL_MASK` + the `no{man,info,doc}`
        // `FEATURES` fold, from the resolved config (not the process
        // env). Applied to every merged image before `CONTENTS` is
        // recorded -- see `ebuild_merge::apply_install_mask`.
        (
            merge_options.install_mask,
            merge_options.install_mask_prunes_usr_share,
        ) = config_install_mask(&config);
        merge_options.set_resolved_features(&config_features_string(&config));
        // #333: real `dblink` protects `settings["CONFIG_PROTECT"]`, the
        // resolved fold (calling env included), not the raw process env
        // `from_env` read (`MergeOptions::apply_config_protect`).
        merge_options.apply_config_protect(&config);
        // Real `_grab_pkg_env` into `configdict["pkg"]`: a `package.env`
        // entry matching a build-bound package layers its env file's
        // build vars over the run-wide set above.
        // `emerge_build::entry_package_env_vars` does the per-entry atom
        // match.
        merge_options.package_env_vars = config.package_env_vars.clone();
        // Real `actions.py:664-672`: right before a fresh, non-`--resume`
        // merge starts, an existing multi-package `mtimedb["resume"]`
        // (left over from some earlier, abandoned run) is preserved as
        // `resume_backup` rather than just being silently overwritten
        // once this run's own outcome gets recorded -- `emerge --resume`
        // can still recover it later if this run wasn't actually meant
        // to replace it. This whole branch is only ever reached for a
        // non-`--resume` action (`--resume` returns early, well above).
        crate::mtimedb::rotate_resume_to_backup(&root);

        // Real `FEATURES=buildpkg` / `--buildpkg`/`-b` (real
        // `_emerge/EbuildBinpkg`): a binpkg of each source entry is
        // written into `$PKGDIR` as a side effect of the merge.
        // `--buildpkg=n` wins over the FEATURE.
        let buildpkg_on = buildpkg_from_config(buildpkg_opt, &config);
        let buildpkg = buildpkg_on.then_some(&package_options);
        // `emerge -k <atom>` (`--usepkg`, no `--getbinpkg`): the resolver
        // put a local-`$PKGDIR` binary on the plan. Real portage merges a
        // local `$PKGDIR` binary on `-k`/`-K` alone -- `--getbinpkg` is
        // only about *also* consulting a remote binhost -- so the
        // per-entry binary-vs-source executor (`run_merge_plan`) must run
        // here too, not just under `getbinpkg`. `merge_one_binary_entry`
        // prefers the on-disk `$PKGDIR` file and only touches a binhost
        // when it is missing, so this stays offline for a pure local
        // merge. (L1-b.)
        let has_binary_entry = entries
            .iter()
            .any(|e| e.source == portage_repo::CandidateSource::Binary);
        if buildpkgonly {
            if let Err(e) = emerge_build::run_buildpkgonly(
                entries,
                &config,
                &repos,
                &root,
                &portage_tmpdir,
                &package_options,
                keep_going,
                status_mode,
            ) {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        } else if getbinpkg || has_binary_entry {
            // Remote execution (`mrg --remote-hostname` + `--getbinpkgonly`,
            // published via the `REMOTE_EXEC` handoff): the same resolved
            // entries run through the remote plan instead. Binary-only is
            // enforced (usage error, exit 2); resume/world bookkeeping is
            // skipped (the client owns that state -- see remote.rs).
            // The download/ledger `$PKGDIR` is the placed config's own
            // `PKGDIR` (`config.pkgdir`, honouring the resolve's
            // `ConfigRootOverride`), not the process `$PKGDIR` env the
            // local plan uses -- the server stores against the config it
            // resolved against (plan §7/§8).
            if let Some(mut rctx) = crate::remote::take_remote_exec() {
                if let Err(e) = crate::remote::check_binary_plan(entries) {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(2);
                }
                // Slice 6: CONFIG_PROTECT comes from the placed config
                // (`Config::resolved_incremental` folds the profile +
                // make.conf stack) unless explicitly flagged -- the
                // client merge then protects what the client config
                // protects, not the flag defaults.
                if !rctx.config_protect_explicit
                    && let Some(v) = config.resolved_incremental("CONFIG_PROTECT")
                    && !v.is_empty()
                {
                    rctx.config_protect = v.join(" ");
                }
                if !rctx.config_protect_mask_explicit
                    && let Some(v) = config.resolved_incremental("CONFIG_PROTECT_MASK")
                    && !v.is_empty()
                {
                    rctx.config_protect_mask = v.join(" ");
                }
                if let Err(e) = crate::remote::run_remote_plan(
                    entries,
                    &config,
                    &repos,
                    &root,
                    &std::path::PathBuf::from(&config.pkgdir),
                    &portage_tmpdir,
                    &rctx,
                    keep_going,
                ) {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(1);
                }
                // Backlog #231 (b): real `post_emerge`'s tail (info regen,
                // preserved-libs notice, config-file check, the GLEP 42
                // notice at `post_emerge.py:155`) runs only once the vdb
                // changed (`post_emerge.py:112-117` early-returns
                // otherwise -- printing just the notice, and only under
                // `--pretend`). The remote plan merges onto the *remote*
                // host (`run_remote_plan` ships every unit through the
                // transport; the local `${ROOT}/var/db/pkg` is never
                // touched), so from this process's vdb nothing changed:
                // real's gate evaluates false on both the success and
                // the failure returns above, and -- being non-`--pretend`
                // here (every `--pretend` exit returned long before this
                // dispatch) -- the tail would print nothing. The early
                // return is therefore the faithful shape; no post-merge
                // notice here by construction, pinned by the
                // local-transport contract test. (The pre-resolution
                // notice already printed before the resolve, like real.)
                return ExitCode::from(0);
            }
            // Real `Scheduler.merge() -> _save_resume_list()` (#168):
            // persist the full mergelist BEFORE the first merge runs, so a
            // SIGKILL mid-merge still leaves `--resume` something to replay
            // (nothing was saved here before -- only the failure path wrote
            // anything). Each successful merge below shrinks it again
            // (`run_merge_loop` / the `-j` scheduler); the failure path
            // re-saves the tail. Silent on write failure, like the
            // `rotate_resume_to_backup` above (a later failure write warns).
            // `--buildpkgonly` and remote execution are deliberately
            // untouched: no save here covers them. The write is
            // unconditional (real always assigns, #179): an all-noop
            // plan records the empty list instead of leaving a stale one.
            let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
                .iter()
                .filter_map(emerge_build::resume_cpv)
                .collect();
            let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
            let _ = crate::mtimedb::write_resume_list(
                &root,
                &atom_args,
                &resume_mergelist,
                &resume_opts,
            );
            // `emerge --getbinpkg`/`-g` (and `-G`, binary-only): merge
            // every resolved entry, per-entry `Binary` vs `Source` --
            // see `emerge_getbinpkg::run_merge_plan`.
            if let Err(e) = emerge_getbinpkg::run_merge_plan(
                entries,
                &config,
                &repos,
                &root,
                &package_options.pkgdir,
                &portage_tmpdir,
                &merge_options,
                keep_going,
                buildpkg,
                &buildpkg_exclude,
                status_mode,
            ) {
                // Backlog #174: a merge-time binpkg digest failure
                // already printed real's full tail (digest block +
                // `>>> Failed to emerge ...`, real
                // `Scheduler._failed_pkg_msg`) and needs no resume-list
                // notice or `emerge:` line -- real prints neither for a
                // merge-time package failure. Backlog #187 extends the
                // same silence to the missing-file sentinel (real
                // `BinpkgVerifier`'s ENOENT arm). The up-front save above
                // (minus per-success removals) already leaves the tail
                // for `--resume`, like real's own start-of-merge save.
                if crate::emerge_getbinpkg::is_binpkg_digest_failure(&e)
                    || crate::emerge_getbinpkg::is_binpkg_missing_failure(&e)
                {
                    return ExitCode::from(1);
                }
                // Real `Scheduler._save_resume_list`: on a merge
                // failure, record every still-unmerged package (source
                // *or* binary -- `entries_not_merged`'s own
                // `ResumeEntryKind` tag) so `emerge --resume` can pick
                // up where this left off. Previously missing entirely
                // for this `--getbinpkg` path -- a failed mixed-source-
                // and-binary run left nothing for `--resume` to find.
                // The empty-`unmerged` guard is the failure path's own:
                // `write_resume_list` itself is unconditional now (real
                // `_save_resume_list` always assigns, #179), but a
                // failure with nothing unmerged must leave the
                // per-merge shrink's own tail alone, not overwrite it
                // with an empty list.
                let unmerged = entries_not_merged(&root, entries);
                let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
                if !unmerged.is_empty() {
                    if let Err(w) = crate::mtimedb::write_resume_list(
                        &root,
                        &atom_args,
                        &unmerged,
                        &resume_opts,
                    ) {
                        eprintln!("emerge: warning: could not save the resume list: {w}");
                    } else {
                        eprintln!(
                            "\n * The resume list contains packages that could not be \
                             merged.\n * Use `emerge --resume` to retry, or `emerge --resume \
                             --skipfirst` to skip the first one."
                        );
                    }
                }
                // Backlog #196 fix round 1: real `post_emerge.py:155`
                // prints the GLEP 42 notice regardless of retval -- gated
                // on the vdb having changed (`:112-117`), which here is
                // `unmerged.len() < entries.len()` (at least one entry
                // landed; the same structural approximation as the
                // success path's `!entries.is_empty()` below).
                merge_failure_news_notice(
                    unmerged.len() < entries.len(),
                    &repos,
                    &root,
                    &config,
                    &color,
                );
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        } else {
            // Plain `emerge <atom>`: real source build + merge (see
            // `emerge_build::run_source_merge`). The up-front resume save
            // is real `Scheduler.merge() -> _save_resume_list()` (#168),
            // same as the `--getbinpkg` arm above -- unconditional (an
            // all-noop plan records the empty list, #179).
            let resume_mergelist: Vec<crate::mtimedb::ResumeCpv> = entries
                .iter()
                .filter_map(emerge_build::resume_cpv)
                .collect();
            let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
            let _ = crate::mtimedb::write_resume_list(
                &root,
                &atom_args,
                &resume_mergelist,
                &resume_opts,
            );
            if let Err(e) = emerge_build::run_source_merge(
                entries,
                &repos,
                &root,
                &portage_tmpdir,
                &merge_options,
                keep_going,
                buildpkg,
                &buildpkg_exclude,
                jobs,
                load_average,
                // Real `Scheduler._background_mode`: capture per-package
                // build output when `--quiet-build=y` or `-q` is set (a
                // `-j` >1 run always captures regardless).
                quiet_build.unwrap_or(false) || quiet,
                status_mode,
            ) {
                // Real `Scheduler._save_resume_list`: on a merge failure,
                // record every still-unmerged package so `emerge --resume`
                // can pick up where this left off. The empty-`unmerged`
                // guard is the failure path's own (`write_resume_list`
                // itself is unconditional now, #179): a failure with
                // nothing unmerged leaves the per-merge shrink's own
                // tail alone.
                let unmerged = entries_not_merged(&root, entries);
                let resume_opts = crate::mtimedb::ResumeOpts { oneshot, onlydeps };
                if !unmerged.is_empty() {
                    if let Err(w) = crate::mtimedb::write_resume_list(
                        &root,
                        &atom_args,
                        &unmerged,
                        &resume_opts,
                    ) {
                        eprintln!("emerge: warning: could not save the resume list: {w}");
                    } else {
                        eprintln!(
                            "\n * The resume list contains packages that could not be \
                             merged.\n * Use `emerge --resume` to retry, or `emerge --resume \
                             --skipfirst` to skip the first one."
                        );
                    }
                }
                // Backlog #196 fix round 1: same failure-path notice as
                // the `--getbinpkg` arm above (real `post_emerge.py:155`
                // regardless of retval, gated on the vdb having changed).
                merge_failure_news_notice(
                    unmerged.len() < entries.len(),
                    &repos,
                    &root,
                    &config,
                    &color,
                );
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        }

        // Real `Scheduler._world_atom` / `saveNomergeFavorites`: record
        // each directly-requested plain target in the world file. Real's
        // own suppression set includes `--buildpkgonly` (which builds but
        // never merges, so a package that isn't installed can't be a
        // world member yet); `--oneshot`/`--onlydeps` are handled inside.
        if !buildpkgonly {
            if let Err(e) = update_world_file(
                &root,
                &atom_args,
                entries,
                system_atoms,
                &repos,
                oneshot,
                onlydeps,
            ) {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
            // Real `saveNomergeFavorites`'s `@set` half: a directly-given
            // `@name` set arg is recorded in `world_sets` (same
            // suppression set as the world file). Backlog #232 review
            // (Important-2): non-candidate sets are skipped
            // (`depgraph.py:11366`) -- the same `usersets_world_candidate`
            // filter as the deferral arm above.
            let record_sets: &[String] = if usersets_world_candidate(&config_root) {
                &selected_set_args
            } else {
                &[]
            };
            if let Err(e) = update_world_sets_file(&root, record_sets, oneshot, onlydeps) {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        }

        // `elog_process` is no longer batched here: real runs it
        // per-package inside `dblink.merge()` (before the post-merge
        // `clean`, `vartree.py:6160-6198`), and backlog #42's clean now
        // removes `${T}` -- so `ebuild_merge::process_merge_elog` is
        // called from `merge_after_install`/`merge_binpkg` themselves.
        // The `echo` summary still reaches stdout, the `save`/
        // `save_summary` files the logdir, and `mail`/`mail_summary`
        // the MTA exactly as before (see `elog.rs`).
        if !buildpkgonly {
            // Real `post_emerge()` (`post_emerge.py:126-130`): once the
            // merge changed the vdb, regenerate any stale GNU info
            // directory index *before* the preserved-libs notice below.
            // The vdb-changed gate is structural here: every merge
            // failure returned early above, `--pretend` never reaches
            // this branch, `buildpkgonly` merges nothing, and an empty
            // mergelist changed nothing either.
            if !entries.is_empty() {
                let noinfo = config_features_list(&config).iter().any(|t| t == "noinfo");
                if let Err(e) =
                    crate::info_files::post_merge_info_update(&root, &color, quiet, noinfo)
                {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(1);
                }
            }
            // Real `post_emerge()` (`post_emerge.py:141-152`): once the
            // merge changed the vdb, warn about any library the merge/
            // unmerge just preserved and point the user at
            // `emerge @preserved-rebuild`.
            crate::preserved_libs::show_preserved_libs_notice(&root, &color, quiet, verbose);
            // Backlog #196: real `post_emerge()` (`post_emerge.py:155`)
            // prints the GLEP 42 count notice a second time once the
            // merge changed the vdb (after the preserved-libs notice and
            // the config-file check, both mirrored above) -- same
            // FEATURES-plus-nonzero-count gates, no `--quiet` gate. The
            // vdb-changed gate is structural here: `--buildpkgonly`
            // builds but never merges (real's `_pkgs_changed` stays
            // false, so its `post_emerge` early-returns with no notice),
            // and an empty mergelist changed nothing either. A merge
            // failure prints the notice too when the vdb changed (real
            // prints regardless of retval, `post_emerge.py:104-108,155`)
            // -- see `merge_failure_news_notice` on the two failure
            // arms above.
            if !entries.is_empty() {
                display_news_notice_if_any(&repos, &root, &config, &color);
            }
        }
    }

    // Backlog #196 fix round 1: real `post_emerge.py:112-117` -- a
    // `--pretend` run prints the GLEP 42 notice at the end (the vdb
    // never changes under `--pretend`, so this arm is pretend-only;
    // the non-pretend path printed its post-merge notice above).
    pretend_end_news_notice(pretend, &repos, &root, &config, &color);

    ExitCode::SUCCESS
}
