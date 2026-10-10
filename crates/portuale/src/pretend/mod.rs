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
mod merge_run;
mod merge_setup;
mod news;
mod post_merge;
mod problem_reports;
mod process_globals;
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
pub(crate) use merge_run::*;
pub(crate) use merge_setup::*;
pub use news::*;
pub(crate) use post_merge::*;
pub(crate) use problem_reports::*;
pub(crate) use process_globals::*;
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

    let useoldpkg_atoms_render = apply_process_globals(ApplyProcessGlobalsArgs {
        package_moves,
        useoldpkg_atoms,
        binpkg_changed_deps_override,
        use_ebuild_visibility,
        debug,
        pretend,
    });

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
        if let Some(code) = show_nothing_to_merge(ShowNothingToMergeArgs {
            pretend,
            show_merge_list,
            mergecount,
            deferred,
            root: &root,
            buildpkgonly,
            config_root: &config_root,
            selected_set_args: &selected_set_args,
            atom_args: &atom_args,
            entries,
            system_atoms,
            repos: &repos,
            ask,
            color,
            oneshot,
            onlydeps,
        }) {
            return code;
        }
        // Real `_emerge/actions.py:525-536`: `--ask` prompts once, after
        // the whole merge list is displayed, before anything is built.
        if ask && !ask_confirm(&color, "Would you like to merge these packages?") {
            return ExitCode::from(130);
        }
        let MergeSetupOut {
            status_mode,
            package_options,
            portage_tmpdir,
            merge_options,
        } = merge_setup(MergeSetupArgs {
            jobs,
            quiet,
            quiet_build,
            entries,
            verbose,
            shell,
            debug,
            config: &config,
            root: &root,
        });

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
            if let Some(code) = run_binary_merge_plan(RunBinaryMergePlanArgs {
                entries,
                config: &config,
                repos: &repos,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                keep_going,
                oneshot,
                onlydeps,
                atom_args: &atom_args,
                package_options: &package_options,
                merge_options: &merge_options,
                buildpkg,
                buildpkg_exclude: &buildpkg_exclude,
                status_mode,
                color,
            }) {
                return code;
            }
        } else if let Some(code) = run_source_merge_plan(RunSourceMergePlanArgs {
            entries,
            oneshot,
            onlydeps,
            root: &root,
            atom_args: &atom_args,
            repos: &repos,
            portage_tmpdir: &portage_tmpdir,
            merge_options: &merge_options,
            keep_going,
            buildpkg,
            buildpkg_exclude: &buildpkg_exclude,
            jobs,
            load_average,
            quiet_build,
            quiet,
            status_mode,
            config: &config,
            color,
        }) {
            return code;
        }

        if let Some(code) = record_world_after_merge(RecordWorldAfterMergeArgs {
            buildpkgonly,
            root: &root,
            atom_args: &atom_args,
            entries,
            system_atoms,
            repos: &repos,
            oneshot,
            onlydeps,
            config_root: &config_root,
            selected_set_args: &selected_set_args,
        }) {
            return code;
        }

        if let Some(code) = show_post_merge_notices(ShowPostMergeNoticesArgs {
            buildpkgonly,
            entries,
            config: &config,
            root: &root,
            color,
            quiet,
            verbose,
            repos: &repos,
        }) {
            return code;
        }
    }

    // Backlog #196 fix round 1: real `post_emerge.py:112-117` -- a
    // `--pretend` run prints the GLEP 42 notice at the end (the vdb
    // never changes under `--pretend`, so this arm is pretend-only;
    // the non-pretend path printed its post-merge notice above).
    pretend_end_news_notice(pretend, &repos, &root, &config, &color);

    ExitCode::SUCCESS
}
