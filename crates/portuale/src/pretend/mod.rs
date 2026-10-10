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

mod atoms;
mod config_action;
mod conflicts;
mod deselect;
mod display;
mod env_config;
mod gates;
mod help;
mod info;
mod json;
mod news;
mod prompt;
mod removal;
mod resume;
mod search;
mod world;

#[cfg(test)]
mod tests;

pub(crate) use atoms::*;
pub(crate) use config_action::*;
pub(crate) use conflicts::*;
pub(crate) use deselect::*;
pub(crate) use display::*;
pub(crate) use env_config::*;
pub(crate) use gates::*;
pub(crate) use help::*;
pub(crate) use info::*;
pub(crate) use json::*;
pub use news::*;
pub(crate) use prompt::*;
pub(crate) use removal::*;
pub(crate) use resume::*;
pub(crate) use search::*;
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            color_opt = match val.as_str() {
                "y" => Some(true),
                "n" => Some(false),
                other => {
                    eprintln!("emerge: --color: invalid choice: {other:?} (choose from 'y', 'n')");
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            excluded.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--exclude=") {
            excluded.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--reinstall-atoms" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--reinstall-atoms\" requires an argument");
                return ExitCode::from(2);
            };
            reinstall_atoms.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--reinstall-atoms=") {
            reinstall_atoms.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--useoldpkg-atoms" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--useoldpkg-atoms\" requires an argument");
                return ExitCode::from(2);
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
                return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            usepkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--usepkg-exclude=") {
            usepkg_exclude.extend(value.split_whitespace().map(String::from));
            i += 1;
        } else if arg == "--usepkg-include" {
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--usepkg-include\" requires an argument");
                return ExitCode::from(2);
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
                return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            shell = match value.as_str() {
                "bash" => ebuild_phases::ShellBackend::Bash,
                "brush" => ebuild_phases::ShellBackend::Brush,
                other => {
                    eprintln!("emerge: --shell: {other:?} is not \"bash\" or \"brush\"");
                    return ExitCode::from(1);
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
                return ExitCode::from(2);
            };
            solver = match portage_repo::SolverKind::parse(&value) {
                Some(kind) => kind,
                None => {
                    eprintln!(
                        "emerge: --solver: {value:?} is not \"portage\", \"pubgrub\" or \"resolvo\""
                    );
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            // Real `main.py:1088-1099`: must parse as a float in [0, 100].
            match val.parse::<f64>() {
                Ok(n) if (0.0..=100.0).contains(&n) => search_similarity = n,
                Ok(_) => {
                    eprintln!(
                        "emerge: Invalid --search-similarity parameter (not between 0 and 100): '{val}'"
                    );
                    return ExitCode::from(2);
                }
                Err(_) => {
                    eprintln!(
                        "emerge: Invalid --search-similarity parameter (not a number): '{val}'"
                    );
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            if !matches!(value.as_str(), "y" | "n") {
                eprintln!(
                    "emerge: option \"--autounmask-backtrack\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return ExitCode::from(2);
            }
            autounmask_backtrack = Some(value == "y");
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--autounmask-backtrack=") {
            if !matches!(value, "y" | "n") {
                eprintln!(
                    "emerge: option \"--autounmask-backtrack\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                );
                return ExitCode::from(2);
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
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--autounmask-use" {
            // Real "--autounmask-use": plain y_or_n, a REQUIRED value --
            // same shape as "--autounmask-keep-keywords" above (real
            // `lib/_emerge/main.py`'s own `"choices": y_or_n`, not
            // `true_y_or_n`).
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-use\" requires an argument");
                return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--autounmask-license" {
            // Real `y_or_n`, REQUIRED value -- same shape as
            // `--autounmask-use`.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-license\" requires an argument");
                return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
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
                return ExitCode::from(2);
            };
            match value.parse::<u64>() {
                Ok(n) => {
                    rebuilt_binaries_timestamp = Some(n);
                    i += 2;
                }
                Err(_) => {
                    eprintln!("emerge: invalid --rebuilt-binaries-timestamp parameter: {value:?}");
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--quickpkg-direct" {
            // Real `y_or_n` (`main.py:604`) -- a REQUIRED value, same shape
            // as `--autounmask-backtrack`.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--quickpkg-direct\" requires an argument");
                return ExitCode::from(2);
            };
            match value.as_str() {
                "y" => quickpkg_direct = Some(true),
                "n" => quickpkg_direct = Some(false),
                _ => {
                    eprintln!(
                        "emerge: option \"--quickpkg-direct\": invalid choice: {value:?} (choose from \"y\", \"n\")"
                    );
                    return ExitCode::from(2);
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
                    return ExitCode::from(2);
                }
            }
            i += 1;
        } else if arg == "--quickpkg-direct-root" {
            // Real "action": "store" (`main.py:608`) -- a required path.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--quickpkg-direct-root\" requires an argument");
                return ExitCode::from(2);
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
                        return ExitCode::from(2);
                    }
                    _ => return report_option(&format!("-{c}")),
                }
            }
            i += 1;
        } else {
            return report_option(arg);
        }
    }

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

    // Real actions.py: "if '--tree' in emerge_config.opts and '--columns'
    // in emerge_config.opts: print(...); return 1" -- checked once
    // parsing finishes (order-independent: works whichever flag came
    // first in argv), right after option parsing and before any other
    // validation, matching real portage's own placement. Portuale's
    // own CLI-usage-error convention (see the contract suite's own doc
    // comment: exit 2, stderr) differs deliberately from real portage's
    // literal `return 1`/stdout here, matching every other CLI-usage
    // error portuale already reports (`--exclude` requires an
    // argument, an invalid `--deep` value, etc.) rather than real
    // portage's own inconsistent mix of exit codes for different
    // usage errors.
    if tree && columns {
        eprintln!("emerge: can't specify both of \"--tree\" and \"--columns\".");
        return ExitCode::from(2);
    }

    // Real `actions.py:3917-3926`, checked in the same order right after
    // option parsing finishes: `-p` first silently removes `--ask` (a
    // `--pretend` run never does anything an actual confirmation would
    // gate), and only once that's settled is `--ask` checked against
    // `stdin.isatty()` -- so `--ask --pretend` on a non-terminal stdin
    // is fine (ask is already `false` by the time the isatty check
    // runs), but a bare `--ask` piped from a non-terminal is a hard,
    // immediate exit rather than a prompt nobody typing into a pipe
    // could ever answer.
    if pretend {
        ask = false;
    }
    if ask && !stdin_is_tty() {
        eprintln!("!!! \"--ask\" should only be used in a terminal. Exiting.");
        return ExitCode::from(1);
    }

    // `--sync`: repo syncing will never be part of portuale -- point the
    // user at the tool that owns it (matching real portage's own
    // long-standing split: `emerge --sync` delegates to `emaint sync`).
    if sync_action {
        eprintln!("Functionality has moved to `emaint sync`.");
        return ExitCode::from(1);
    }

    // Real `actions.py:3964-4012`: an action that writes into a
    // root-owned filesystem -- any real merge, plus `--unmerge`/-C,
    // `--depclean`/-c, `--prune`/-P, `--deselect`/-W (world-file
    // rewrite), `--config`, `--clean` -- requires `secpass == 2`
    // (`uid == 0`, or non-root ownership of the target root; see
    // `privileges::is_privileged`). `--pretend` and the read-only query
    // actions are exempt. `--regen` rewrites the repo cache, not the
    // root, so it is not gated here.
    let write_action = !pretend
        && !search_action
        && !info_action
        && !list_sets
        && !check_news
        && !regen_action
        && !metadata_action;
    if write_action && !crate::privileges::is_privileged(&root_from_env()) {
        // Real `_emerge/actions.py:3983-4006` (backlog #246): with
        // `--ask`, a non-privileged caller is offered `Would you like
        // to add --pretend to options?` instead of being refused. Real
        // prints `This action requires {access_desc} access...` to
        // stdout first; portuale has a single privilege tier (no
        // portage-group tier -- see `privileges.rs`), so `access_desc`
        // is always `superuser` here. The prompt itself goes through
        // the shared `UserQuery.query` port (`ask_yes_no`, backlog
        // #240), so "No" exits `128 + SIGINT` with nothing else
        // printed, and EOF/SIGINT prints `Interrupted.` and exits the
        // same way; "Yes" sets `--pretend` and drops `--ask`, and the
        // run continues as a pretend run. (Host probe 2026-09-29,
        // `/usr/sbin/emerge` 3.0.82.2 as uid 1000, secpass 0: exact
        // bytes in the pin docstrings below.)
        if ask {
            println!("This action requires superuser access...");
            match ask_yes_no(
                &Colorizer::new(color::resolve_havecolor(color_opt)),
                "Would you like to add --pretend to options?",
            ) {
                Some(true) => {
                    pretend = true;
                    ask = false;
                    portage_repo::set_resolver_debug(debug && pretend);
                }
                Some(false) => return ExitCode::from(130),
                None => return ExitCode::from(130),
            }
        } else {
            // Real `actions.py:4007-4012` without `--ask`: `emerge:
            // superuser access is required` on stderr, exit 1 -- and no
            // hint line (host probe 2026-09-29), so this no longer goes
            // through `privileges::deny_superuser` (kept for `ebuild`,
            // whose refusal is unprobed).
            eprintln!("emerge: superuser access is required");
            return ExitCode::from(1);
        }
    }

    // Real `actions.py:4106-4111`: the `config`, `metadata` and `regen`
    // actions reject `--pretend` outright (they only ever do real work,
    // so a dry run is meaningless). `--config` already runs ignoring
    // `--pretend` in portuale -- kept as-is for compatibility; the two
    // others are gated here.
    if pretend {
        for (flag, name) in [(regen_action, "regen"), (metadata_action, "metadata")] {
            if flag {
                eprintln!("emerge: The '{name}' action does not support '--pretend'.");
                return ExitCode::from(1);
            }
        }
    }

    // `--unmerge`/`-C`, `--depclean`/`-c`, `--prune`/`-P` and now
    // `--deselect`/`-W` WITHOUT `--pretend` are all real writes:
    // `run_unmerge_pretend` / `run_depclean_pretend` / `run_prune_pretend`
    // / the `--nodeps` variant each run `execute_unmerge` after the
    // display when `pretend` is `false`, and `run_deselect` rewrites the
    // `world` / `world_sets` files (real `action_deselect`'s own
    // `world_set.replace(remaining)`). No `requires --pretend` gate left.

    // Real non-dry-run execution paths portuale implements for `emerge`
    // itself: `--buildpkgonly` (builds a binary package, never merges --
    // `emerge_build.rs`), `--getbinpkgonly` (downloads remote binpkgs and
    // merges them -- `emerge_getbinpkg.rs`), and a plain `emerge <atom>`
    // (real source build + merge -- `emerge_build::run_source_merge`).
    // Dispatched from the `!pretend` block far below, after resolution.

    // Real `main.py`: `--deselect` becomes a standalone action only when
    // `myaction is None` -- `--depclean`/`--prune`/`--unmerge` each set
    // their own action first, and then `--deselect=y|n` is just a
    // modifier on it (real `action_depclean`'s `deselect` -- see
    // `run_depclean_pretend`).
    if deselect && !depclean && !prune && !unmerge {
        return run_deselect(
            &atom_args,
            &root_from_env(),
            pretend,
            ask,
            &Colorizer::new(color::resolve_havecolor(color_opt)),
        );
    }

    // `--list-sets`: a standalone action needing nothing from the
    // resolved config -- dispatch before the "expected a package atom"
    // guard and all the repos.conf/profile machinery.
    if list_sets {
        return run_list_sets(&config_root_from_env());
    }

    // `--regen` (real `action_regen`): a real write action -- regenerate
    // every repo's `metadata/md5-cache` by running each ebuild's `depend`
    // phase. `--pretend` was already rejected above. `--jobs` /
    // `--load-average` thread straight through (real
    // `action_regen(settings, portdb, max_jobs, max_load)`).
    if regen_action {
        return crate::regen::run(
            &config_root_from_env(),
            &root_from_env(),
            debug,
            jobs,
            load_average,
        );
    }
    // `--metadata` (real `action_metadata`): transfers a repo's
    // pre-generated cache into portage's own `depcachedir`. Portuale
    // reads `metadata/md5-cache` directly and models no `depcachedir`, so
    // there is nothing to transfer -- real portage's own
    // `>>> Updating Portage cache` header, then done.
    if metadata_action {
        println!("\n>>> Updating Portage cache");
        return ExitCode::SUCCESS;
    }

    if atom_args.is_empty()
        && !unmerge
        && !depclean
        && !prune
        && !config_action
        && !resume
        && !search_action
        && !check_news
        && !clean_action
        && !rage_clean
        && !info_action
    {
        eprintln!(
            "emerge: no targets given (an atom, an @set, or an installed file / \
             ebuild / tbz2 / gpkg; `emerge --help` for usage)"
        );
        return ExitCode::from(2);
    }

    // Real `actions.py::adjust_configs` colour gate -- resolved once here
    // so every action path (the standalone cleanup actions below and the
    // ordinary resolve-graph path) shares one `Colorizer`.
    let color = Colorizer::new(color::resolve_havecolor(color_opt));

    // Real `_emerge/actions.py::apply_priorities` (called from
    // `run_action`, before any action): renice / ionice this process so
    // every build subprocess it spawns inherits the policy.
    apply_portage_scheduling_policy();

    // `--resume` / `-r`: a standalone action -- replay the saved
    // `mtimedb["resume"]` mergelist (real `_emerge/actions.py`). No atoms.
    // `--resume --pretend` prints the saved list and stops (see
    // `run_resume`).
    if resume {
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(&config));
        return run_resume(
            &root,
            &config_root,
            &config,
            &portage_tmpdir,
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
        );
    }

    // `--config <atom>`: a standalone action -- run `pkg_config` for one
    // installed package (real `action_config`). Needs nothing from the
    // resolved `config`; ignores `--pretend`.
    if config_action {
        return run_config_action(&atom_args, &root, shell, debug, ask, &color);
    }

    // `--search`/`-s` (`--searchdesc`/`-S` also matches DESCRIPTION): a
    // standalone read-only query action (real `action_search`). Real
    // `action_search` passes `search`'s `verbose` as `"--quiet" not in
    // myopts` -- so the full `Latest version` / `Homepage` /
    // `Description` block shows by default and `-q` makes it terse; `-v`
    // is irrelevant to search.
    if search_action {
        return run_search(
            &atom_args,
            &repos,
            &root,
            searchdesc,
            !quiet,
            fuzzy_search,
            regex_search_auto,
            search_similarity,
            &color,
        );
    }

    // `--check-news`: a standalone read-only query action (real
    // `actions.py`'s `count_unread_news` block). Real gates its output on
    // `"--quiet" not in myopts`.
    if check_news {
        return run_check_news(&repos, &root, &config_root, &config, quiet, &color);
    }

    // `--info`: a standalone read-only query action (real
    // `action_info`), narrowed to its deterministic config block plus
    // the per-atom "Package Settings" block.
    if info_action {
        let info_usepkg = usepkg || usepkgonly || getbinpkg || getbinpkgonly;
        // Real `action_info` consults `bindb` (the local `$PKGDIR` pool)
        // for its `(bindb, "binary")` search half whenever `--usepkg` is
        // on -- so scan `$PKGDIR` here too, exactly as the ordinary
        // `--usepkg` resolve path does further down (that scan runs after
        // this standalone-action dispatch, so `--info` needs its own).
        if info_usepkg {
            match crate::binpkg::populate_local_pkgdir(
                Path::new(&config.pkgdir),
                pkgdir_index_trusted(&config),
            ) {
                Ok(entries) if !entries.is_empty() => config.scanned_binpkgs = Some(entries),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("emerge: scanning {}: {e}", config.pkgdir);
                    return ExitCode::from(1);
                }
            }
        }
        return run_info(
            &config,
            &repos,
            &root,
            &atom_args,
            misspell_suggestions,
            info_usepkg,
            &config_root,
            shell,
            debug,
            &color,
        );
    }

    // `--clean`: a standalone removal action -- keep only the newest
    // version per slot.
    if clean_action {
        return run_clean_pretend(
            &atom_args,
            &root,
            &config_root,
            pretend,
            ask,
            shell,
            debug,
            &color,
            quiet,
            config_features_list(&config).iter().any(|t| t == "noinfo"),
            &repos,
            &config,
        );
    }
    // `--rage-clean`: a fast `--unmerge` (identical `--pretend` display).
    if rage_clean {
        return run_unmerge_pretend(
            &atom_args,
            "rage-clean",
            &root,
            &config_root,
            &config,
            &repos,
            false,
            pretend,
            ask,
            shell,
            debug,
            &color,
            quiet,
            config_features_list(&config).iter().any(|t| t == "noinfo"),
        );
    }

    // `--unmerge`/`-C`: a standalone action -- resolved config in hand
    // (its `@system` target support and system-profile check both need
    // it), dispatch before the ordinary resolve-graph path below.
    if unmerge {
        // `pretend` last: without it, `run_unmerge_pretend` really
        // removes the selected packages after the display.
        return run_unmerge_pretend(
            &atom_args,
            "unmerge",
            &root,
            &config_root,
            &config,
            &repos,
            false,
            pretend,
            ask,
            shell,
            debug,
            &color,
            quiet,
            config_features_list(&config).iter().any(|t| t == "noinfo"),
        );
    }
    if depclean {
        return run_depclean_pretend(
            &atom_args,
            &root,
            &config_root,
            &config,
            &repos,
            verbose,
            lib_check,
            !deselect_n,
            pretend,
            ask,
            shell,
            debug,
            &color,
            quiet,
            config_features_list(&config).iter().any(|t| t == "noinfo"),
        );
    }
    if prune {
        if nodeps {
            return run_prune_nodeps_pretend(
                &atom_args,
                &root,
                &config_root,
                pretend,
                ask,
                shell,
                debug,
                &color,
                quiet,
                config_features_list(&config).iter().any(|t| t == "noinfo"),
                &repos,
                &config,
            );
        }
        return run_prune_pretend(
            &atom_args,
            &root,
            &config_root,
            &config,
            &repos,
            verbose,
            lib_check,
            pretend,
            ask,
            shell,
            debug,
            &color,
            quiet,
            config_features_list(&config).iter().any(|t| t == "noinfo"),
        );
    }

    // Real `actions.py:3772`: `--autounmask-continue` + `--autounmask=n`
    // -> a WARNING that the former "has been disabled by" the latter,
    // printed on the merge/build path before the depgraph. Portuale's
    // only observable effect for `--autounmask-continue` -- its
    // write-and-continue path is real-portage-gated on `"--pretend" not
    // in myopts` (`depgraph.py:5796`), inert here.
    if autounmask_continue.is_some() && autounmask == Some(false) {
        eprintln!(
            " {} --autounmask-continue has been disabled by --autounmask=n",
            color.c("WARN", "*")
        );
    }

    // The built-in set tokens each expand to their own real atom list,
    // in place, at whatever position they appear: `@world`/`@selected`
    // (`expand_selected` -- the world file's atoms + `world_sets`' nested
    // sets), `@system` (the profile chain's `packages` files), and
    // `@installed` (`installed_set_atoms` -- a `cat/pkg:slot` atom per
    // vdb package). Any other `@name` token is a user-defined (file-based)
    // package set
    // -- expanded recursively via `resolve_custom_set` (real
    // `StaticFileSet` / `SetConfig.getSetAtoms`), the same machinery the
    // `--unmerge`/`--depclean`/`--deselect` paths already use; a `@name`
    // with no matching set file is a fatal "set 'name' not found".
    let mut expanded_atoms: Vec<String> = Vec::new();
    // Backlog #286: which expanded atoms rode a `@world`/`@selected` set
    // argument (real `SetArg` named `"world"`/`"selected"`,
    // `depgraph.py:5577-5605` -- a member that selects no package drops
    // into `_missing_args` instead of failing the run). Parallel to
    // `expanded_atoms` through the moves rewrite and dedup below (both
    // index-preserving); `dep_expand_token` mutates atoms in place, so
    // the flag rides renames too.
    let mut expanded_from_set: Vec<bool> = Vec::new();
    // The user-given `@name` set args (leading `@` stripped) that are
    // world candidates -- i.e. resolved via a set FILE, not `@world`/
    // `@system`. Recorded in `world_sets` after a successful
    // non-`--pretend` merge (real `depgraph.saveNomergeFavorites`).
    let mut selected_set_args: Vec<String> = Vec::new();
    for atom_str in &atom_args {
        if *atom_str == "@world" || *atom_str == "@selected" {
            // Real `@world` (`_sets/__init__.py:97-98`) is
            // `@profile ∪ @selected ∪ @system` -- not just the world
            // file. `emerge @world` / `-uD @world` re-emerges / updates
            // every `@profile` and `@system` member too; on an empty
            // world it is still non-empty. `@selected` alone stays the
            // world file.
            if *atom_str == "@world" {
                // Real `_resolve` (`depgraph.py:5500`) processes each
                // `SetArg`'s atom list `sorted(arg.pset.getAtoms(),
                // key=str)`, and the world set lists its nested sets as
                // `@profile @selected @system`.
                let mut atoms = config.profile_packages.clone();
                atoms.sort();
                expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                expanded_atoms.extend(atoms);
            }
            match expand_selected(&root, &config_root) {
                Ok(atoms) => {
                    expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                    expanded_atoms.extend(atoms);
                }
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(1);
                }
            }
            if *atom_str == "@world" {
                // Real `_resolve` (`depgraph.py:5500`) processes each
                // `SetArg`'s atom list `sorted(arg.pset.getAtoms(),
                // key=str)`; the nested `@system` set is one such arg.
                let mut atoms = config.system_packages.clone();
                atoms.sort();
                expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                expanded_atoms.extend(atoms);
            }
        } else if *atom_str == "@profile" {
            // The `@profile` set on its own (real
            // `ProfilePackageSet.load`); same sorted-arg shape as
            // `@system` above.
            let mut atoms = config.profile_packages.clone();
            atoms.sort();
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@system" {
            // Real `_resolve` (`depgraph.py:5500`): `for atom in
            // sorted(arg.pset.getAtoms(), key=str)`. A `>=`-prefixed
            // atom sorts before every plain one (`>` is 0x3E), which is
            // exactly why real's `-pe @system` pre-bias order starts
            // `>=sys-apps/baselayout-2`, `>=sys-apps/findutils-4.4`,
            // `>=sys-devel/patch-2.7`, `app-admin/eselect`,
            // `app-alternatives/awk`, … (F-B3's probe). Portuale used
            // the profile's raw `packages` order, so the resolver walk
            // -- and therefore the pre-bias order the scheduler's stable
            // `_merge_order_bias` preserves -- diverged from index 1.
            let mut atoms = config.system_packages.clone();
            atoms.sort();
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@installed" {
            let atoms = installed_set_atoms(&root);
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@preserved-rebuild" {
            let atoms = preserved_rebuild_atoms(&root);
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if let Some(name) = atom_str.strip_prefix('@') {
            let mut seen = HashSet::new();
            match resolve_custom_set(&config_root, name, &mut seen) {
                Ok(atoms) => {
                    expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
                    expanded_atoms.extend(atoms);
                    selected_set_args.push(name.to_string());
                }
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(1);
                }
            }
        } else {
            expanded_from_set.push(false);
            expanded_atoms.push((*atom_str).to_string());
        }
    }

    // Real `profiles/updates/` package moves (SCOPE_BACKLOG Part 2 Section
    // H item 2): a `move sys-libs/foo sys-libs/bar` / `slotmove` directive
    // rewrites a command-line, `@world`/`@system` or `@<set>`-expanded
    // atom before it's resolved *or* used as a display "requested" key
    // (real global-updates rewrites the world file and the vdb; portuale
    // applies the moves in memory on read -- see
    // `portage_repo::apply_updates_to_atom`).
    for atom in &mut expanded_atoms {
        *atom = portage_repo::apply_updates_to_atom(atom);
    }

    // A real package set is an unordered *set* -- and `@world` now folds
    // `@system` in, which overlaps the world file for anyone who ran
    // `emerge --noreplace @system`. Drop later duplicates, keeping first
    // position (portuale's merge order is position-sensitive).
    {
        // Explicit-wins: an atom given both explicitly and through a
        // set keeps its explicit nature (real holds both a `SetArg`
        // and an `AtomArg` for it: the set member drops but the
        // explicit argument still fails the run).
        let explicit: HashSet<String> = expanded_atoms
            .iter()
            .zip(expanded_from_set.iter())
            .filter(|(_, f)| !**f)
            .map(|(a, _)| (*a).clone())
            .collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut kept_atoms = Vec::with_capacity(expanded_atoms.len());
        let mut kept_flags = Vec::with_capacity(expanded_from_set.len());
        for (a, f) in expanded_atoms.drain(..).zip(expanded_from_set.drain(..)) {
            if seen.insert(a.clone()) {
                kept_flags.push(f && !explicit.contains(a.as_str()));
                kept_atoms.push(a);
            }
        }
        expanded_atoms = kept_atoms;
        expanded_from_set = kept_flags;
    }

    // Real `dep_expand()` / `cpv_expand()` (`lib/portage/dbapi/`): a
    // command-line target with no category (`emerge eix`, `emerge
    // eix-1.2`, `emerge eix:0`) is qualified against the repo tree --
    // scan every `<repo>/<cat>/<pkg>/` for a package matching the bare
    // name. Exactly one -> use it silently; more than one -> the real
    // `ambiguous_package_name` block (its `--quiet` form -- real's
    // default form runs a full `search`, a documented cut), exit 1;
    // none -> `there are no ebuilds to satisfy`, exit 1. A
    // `@set`-expanded atom is already `cat/pkg`, so only a genuine
    // command-line bare name reaches this.
    {
        let mut all_cp: Option<Vec<String>> = None;
        for atom in &mut expanded_atoms {
            let cps = all_cp.get_or_insert_with(|| portage_repo::all_cp(&repos));
            match dep_expand_token(atom, cps) {
                DepExpand::Unchanged => {}
                DepExpand::Qualified(q) => *atom = q,
                DepExpand::Ambiguous(matches) => {
                    if quiet {
                        eprintln!(
                            "\n!!! The short ebuild name \"{atom}\" is ambiguous. Please specify"
                        );
                        eprintln!(
                            "!!! one of the following fully-qualified ebuild names instead:\n"
                        );
                        for cp in &matches {
                            println!("    {}", color.c("INFORM", cp));
                        }
                    } else {
                        render_ambiguous_search_output(atom, &matches, &repos, &root, &color);
                    }
                    return ExitCode::from(1);
                }
                DepExpand::NoMatch => {
                    eprintln!("emerge: there are no ebuilds to satisfy {atom:?}.");
                    return ExitCode::from(1);
                }
            }
        }
    }

    if expanded_atoms.is_empty() {
        eprintln!(
            "emerge: no package atoms to resolve (the target list, after \
             expanding any @world/@selected/@system/@installed/@<set>, is empty)"
        );
        return ExitCode::from(2);
    }
    // Backlog #286: the droppable set members (see `expanded_from_set`).
    // Built after `dep_expand_token` so renames (bare `pkg` -> `cat/pkg`)
    // match the resolver's own atom texts.
    let set_args: HashSet<String> = expanded_atoms
        .iter()
        .zip(expanded_from_set.iter())
        .filter(|(_, f)| **f)
        .map(|(a, _)| (*a).clone())
        .collect();

    for atom_str in &expanded_atoms {
        let Some(atom) = parse_atom(atom_str) else {
            eprintln!("emerge: invalid atom {atom_str:?}");
            return ExitCode::from(1);
        };
        if atom.blocker != Blocker::None {
            eprintln!("emerge: {atom_str:?} is a blocker, not a valid emerge target");
            return ExitCode::from(2);
        }
    }

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
        match crate::binpkg::populate_local_pkgdir(pkgdir_path, pkgdir_index_trusted(&config)) {
            Ok(entries) if !entries.is_empty() => config.scanned_binpkgs = Some(entries),
            Ok(_) => {}
            Err(e) => {
                eprintln!("emerge: scanning {}: {e}", config.pkgdir);
                return ExitCode::from(1);
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
        Some(norm(&src)).filter(|s| *s != norm(&root))
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
        resolve_root_deps_running_root(root_deps, &root, portage_repo::running_root_from_env());

    // Backlog #242 Slice D: the running root's own profile for per-node
    // USE paint (see `load_running_config`). Loaded once per run, from
    // the effective running root — never the target config.
    let running_config = load_running_config(&root, &portage_repo::running_root_from_env(), &repos);

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
        emerge_getbinpkg::refresh_binhost_indexes(&config.binrepos, &root, pretend);
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
            let mut v = expand_selected(&root, &config_root).unwrap_or_default();
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
        let notice_printed = display_news_notice_if_any(&repos, &root, &config, &color);
        // Backlog #231 (c): real `actions.py:4266-4281` offers the
        // `eselect news read` spawn here, once the notice printed. An
        // interrupted news prompt (backlog #234) exits `128 + SIGINT`
        // before `action_build`, like real `UserQuery.query`'s own
        // `sys.exit` (`_emerge/UserQuery.py:74-76`).
        if offer_news_reading(ask, read_news, notice_printed, &color) {
            return ExitCode::from(130);
        }
    }
    let run_resolve = |complete: bool, locked: &[String], with_seeds: bool| {
        let cfg: std::borrow::Cow<portage_profile::Config> =
            if !complete_seed_atoms.is_empty() || !locked.is_empty() {
                let mut c = config.clone();
                // #72 B2c: the blocker classification follows real's
                // complete-mode retry even on a non-complete pass
                // (`blocker_retry_seed_atoms`), while `complete_seed_atoms`
                // stays gated on `with_seeds` -- filling *that* field here
                // would switch the slot-operator-rebuild scan on for every
                // plain resolve (the trap the plan calls out).
                c.blocker_retry_seed_atoms = complete_seed_atoms.clone();
                if with_seeds {
                    c.complete_seed_atoms = complete_seed_atoms.clone();
                    c.complete_locked_merges = locked.to_vec();
                }
                std::borrow::Cow::Owned(c)
            } else {
                std::borrow::Cow::Borrowed(&config)
            };
        // New code builds a `ResolveRequest` and calls the runtime-selected
        // `Resolver` directly (see `resolve_pretend_graph`'s doc comment) --
        // this is the one production call site that carries `--solver=`.
        let req = ResolveRequest {
            config_root: config_root.clone(),
            root: root.to_path_buf(),
            atoms: expanded_atoms.clone(),
            config: cfg.into_owned(),
            newuse,
            changed_use,
            nodeps,
            onlydeps,
            onlydeps_with_rdeps,
            onlydeps_with_ideps,
            update,
            deep,
            excluded: excluded.clone(),
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
            usepkg_exclude: usepkg_exclude.clone(),
            usepkg_include: usepkg_include.clone(),
            rebuilt_binaries,
            rebuilt_binaries_timestamp,
            newrepo,
            buildpkgonly,
            root_deps_running_root: root_deps_running_root.clone(),
            distdir: distdir.clone(),
            empty: emptytree,
            getbinpkg,
            ignore_built_slot_operator_deps,
            backtrack_max,
            reinstall_atoms: reinstall_atoms.clone(),
            rebuild_if_new_slot,
            rebuild_if_unbuilt,
            rebuild_if_new_rev,
            rebuild_if_new_ver,
            rebuild_exclude: rebuild_exclude.clone(),
            rebuild_ignore: rebuild_ignore.clone(),
            dynamic_deps,
            implicit_system_deps,
            complete,
            set_args: set_args.clone(),
            solver,
        };
        active_resolver_for(solver).resolve(&req)
    };
    let handle = |r: Result<portage_repo::GraphResult, crate::error::Error>| match r {
        Ok(result) => Ok(result),
        Err(e) => {
            let msg = e.to_string();
            // Real never prefixes a `!!!`-headed report (masked packages,
            // the REQUIRED_USE "has unmet requirements" block) with
            // `emerge: ` -- it is `writemsg`-emitted verbatim by
            // `_show_unsatisfied_dep`.
            if msg.trim_start().starts_with("!!!") {
                eprint!("{msg}");
            } else {
                eprint!("emerge: {msg}");
            }
            if let Some(extra) = misspell_suggestion_block(&msg, &repos, misspell_suggestions) {
                eprint!("{extra}");
            }
            eprintln!();
            Err(ExitCode::from(1))
        }
    };

    // Phase 1 is **always non-complete** -- the authoritative merge
    // list. Real `_complete_graph` runs *after* the normal graph is
    // built and only extends it (graph-or-installed selection, never a
    // new merge).
    let result = match handle(run_resolve(false, &[], false).map_err(crate::error::Error::from)) {
        Ok(result) => result,
        Err(code) => {
            pretend_end_news_notice(pretend, &repos, &root, &config, &color);
            return code;
        }
    };
    // The `cp`s phase 1 would actually merge -- real's "already in the
    // graph" for `_select_pkg_from_graph`. The complete pass may not
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
    // Phase 2 (`_complete_graph`): run when `--complete-graph` is
    // explicit, or auto-enabled -- real `depgraph.py:8581-8648`, re-walk
    // in complete mode when the first graph changes an already-installed
    // package and `--complete-graph-if-new-use`/`-if-new-ver`/
    // `--rebuild-if-new-slot` is on. Real has no `--deep` condition (it
    // forces deep itself when not already deep); when `--deep` already
    // forced the deep walk, the locked-gate re-resolve is skipped --
    // the deep walk already visited the reachable closure -- and the
    // phase-1 walk is instead re-run below with only the
    // `complete_seed_atoms` fed in (they feed `slot_op_reachable` and
    // nothing else -- see `ResolveCtx::new`). Skipped entirely under
    // `--nodeps`. The completeness re-walk
    // never changes the visible merge list (the locked set gates it) --
    // it only widens `--json` / `--changed-deps-report` / slot-op
    // accounting.
    let auto_enable = !nodeps
        && portage_repo::complete_graph_auto_enable(
            &result.entries,
            complete_if_new_use,
            complete_if_new_ver,
            rebuild_if_new_slot,
            &root,
            &repos,
            &config,
        );
    let want_complete = complete_graph || (deep == portage_repo::Deep::NotRequested && auto_enable);
    let result = if want_complete {
        match handle(run_resolve(true, &locked_merges, true).map_err(crate::error::Error::from)) {
            Ok(result) => result,
            Err(code) => {
                pretend_end_news_notice(pretend, &repos, &root, &config, &color);
                return code;
            }
        }
    } else if auto_enable {
        // `--deep` in force (the only way to reach here with
        // `auto_enable` true): real would still run `_complete_graph`,
        // so re-run the phase-1 walk with the seeds fed in. Deterministic
        // input means an identical walk; the only delta is the
        // slot-operator-rebuild scan now seeing `slot_op_reachable`.
        match handle(run_resolve(false, &[], true).map_err(crate::error::Error::from)) {
            Ok(result) => result,
            Err(code) => {
                pretend_end_news_notice(pretend, &repos, &root, &config, &color);
                return code;
            }
        }
    } else {
        result
    };
    // Backlog #242 Slice D: repaint running-rooted rows from the running
    // root's own profile (S0 §2) — merge entries and abort-partial rows
    // alike (the circular re-display renders the partial list). Runs only
    // on a genuine cross-root run with a loaded running profile;
    // single-root runs (and hermetic trees without one) keep the
    // resolve-time target paint.
    let mut result = result;
    if let (Some(running), Some(running_cfg)) =
        (root_deps_running_root.as_deref(), running_config.as_ref())
    {
        portage_repo::repaint_running_root_entry_displays(
            &mut result.entries,
            &repos,
            running,
            running_cfg,
        );
        if let portage_repo::ResolveOutcome::Aborted { partial, .. } = &mut result.outcome {
            portage_repo::repaint_running_root_entry_displays(
                partial,
                &repos,
                running,
                running_cfg,
            );
        }
    }
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
    // Backlog #185 (fix round 1): real's resolution-phase display for
    // a non-`--pretend` run, printed whether or not the list itself is
    // shown -- and whether or not resolution succeeds. Real `depgraph.py::
    // _start_resolution_display` prints the header (when `--ask`,
    // `--tree` or `--verbose` is given and `--quiet` is absent --
    // `--nodeps` is NOT consulted, the `spinner.mode = QUIET`
    // assignment at `depgraph.py:12129-12130` comes after the header
    // block) and starts the spinner notice before resolving, and
    // `backtrack_depgraph`'s `finally:` completes the notice plus the
    // timing line after (success or failure alike). `--quiet` suppresses
    // everything; `--nodeps` suppresses only the notice and the timing
    // line (QUIET mode: `end_notice()` reports nothing to complete).
    // Provenance: container probe m185 (real 3.0.81.3,
    // `localhost/test-portuale:latest`, `emerge --oneshot --usepkgonly
    // probe/probe-a` in six shapes; the gating is identical in 3.0.82.2
    // by source read) plus the 3.0.82.2 source for exact text.
    if !pretend && !quiet {
        if resolution_header_shown(ask, tree, verbose, quiet) {
            // Real `_start_resolution_display`
            // (`depgraph.py:12087-12127`): a leading blank line, then
            // `These are the packages that would be ...` -- `in reverse
            // order` for `--tree` (a plain `:` under
            // `--unordered-display`), `fetched` under `--fetchonly` and
            // `built` under `--buildpkgonly` (`--fetchonly` is
            // unimplemented here, rc 2, so only the `built` arm below is
            // reachable), else `merged, in order:` -- then a blank line.
            println!();
            let action = if buildpkgonly { "built" } else { "merged" };
            if tree {
                if unordered_display {
                    println!("These are the packages that would be {action}:");
                } else {
                    println!("These are the packages that would be {action}, in reverse order:");
                }
            } else {
                println!("These are the packages that would be {action}, in order:");
            }
            println!();
        }
        if resolution_notice_shown(quiet, nodeps) {
            // Real `stdout_spinner` STATIC mode (piped stdout, real
            // `stdout_spinner.py:105-145`): `begin_notice("Calculating
            // dependencies")` writes the notice plus ` ...`, and
            // `end_notice()` completes it with ` done!`. NOTE: the m185
            // probe (3.0.81.3) shows two spaces before `...`; the 3.0.82.2
            // source (`f"{notice} ..."` + `" done!\n"`) shows one -- ported
            // from the pinned source, pending the coordinator's 3.0.82.2
            // bed confirmation (see the m185 report).
            println!("Calculating dependencies ... done!");
            // Real `_show_resolution_report` (`depgraph.py:12134-12146`):
            // `Dependency resolution took N.NN s (backtrack: B/M).` plus a
            // trailing blank line. The wall-clock seconds are cut exactly
            // like the #135 abort-path timing line just below (repeated-run
            // determinism is a jointly-owned gate; nondeterministic values
            // ride `--json` only) -- the `backtrack: B/M` signal is kept.
            println!(
                "Dependency resolution took (backtrack: {}/{}).\n",
                result.backtrack_restarts, result.backtrack_max,
            );
        }
    }

    // Real `depgraph.py:11192-11235`'s `display_problems()` block for a
    // directly-requested atom that matched `package.provided` -- printed
    // to stderr, before the merge list (matching real portage's own
    // `display_problems()` -> `display()` order). Portuale tracks no
    // `SetArg`, so the "pulled in by" ref is always `'args'` and the real
    // `@world`/`@selected` "A) B) C)" solution text is never reached (a
    // documented divergence -- see `GraphResult::pprovided_atoms`).
    if !result.pprovided_atoms.is_empty() {
        eprint!("{}", color.c("BAD", "\nWARNING: "));
        if result.pprovided_atoms.len() > 1 {
            eprintln!("Requested packages will not be merged because they are listed in");
        } else {
            eprintln!("A requested package will not be merged because it is listed in");
        }
        eprintln!("package.provided:\n");
        for atom in &result.pprovided_atoms {
            eprintln!("  {} pulled in by 'args'", color.c("INFORM", atom));
        }
        eprintln!();
    }

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

    if json {
        print_json(
            display_entries,
            result.backtrack_restarts,
            result.backtrack_max,
            &result.slot_conflicts,
            &result.changed_deps_report,
            &result.autounmask_keyword_changes,
            &result.autounmask_use_changes,
            &result.autounmask_license_changes,
            &result.autounmask_mask_changes,
            &result.abi_rebuilds,
            &result.outcome,
            &top_level_pkgs,
            verbose,
            root_deps_running_root.as_deref(),
        );
        // Same non-zero-on-autounmask-changes contract as the text path
        // below (a `--json` consumer's exit-code check must see the
        // "config still needs editing" signal too). `--autounmask-only`
        // stays exit 0 (its whole point is "just show the changes").
        // (`has_autounmask_changes` is bound once, above, next to the
        // `--autounmask-only` re-show gate -- backlog #271.)
        if has_autounmask_changes
            && !autounmask_only
            && !(!pretend && autounmask_continue == Some(true))
        {
            return ExitCode::from(1);
        }
        // An unsolvable blocker fails the run for a `--json` consumer's
        // `$?` check too (real `actions.py` returns 1 regardless of
        // output format) -- entry-owned rows and #80 orphans alike.
        if result
            .entries
            .iter()
            .any(|e| e.blockers.iter().any(|b| b.unsolvable))
            || !result.orphan_blockers.is_empty()
        {
            return ExitCode::from(1);
        }
        // Backlog #62: same slot-conflict rule as the text path below
        // (real `actions.py:460-462` runs before any format split, and
        // `--json` has no real counterpart -- it follows the same rule
        // for consistency): a recorded conflict fails the run unless an
        // escape holds. `--autounmask-only` is escape 1 (its whole point
        // is "just show the changes", exit 0); backtracking disabled
        // plus `--buildpkgonly`/`--nodeps` is escape 2
        // (`_accept_blocker_conflicts`). See the text-path gate for the
        // full grounding and the `--fetchonly`/`--fetch-all-uri`
        // carve-out.
        if !autounmask_only
            && !result.slot_conflicts.is_empty()
            && !(backtrack_max == 0 && (buildpkgonly || nodeps))
        {
            return ExitCode::from(1);
        }
        // Backlog #19 Slice 4: an aborted resolve exits 1 in every output
        // format (real `actions.py:460-462` runs before any format
        // split). Gated like the text path below; the `Aborted` arm there
        // fires first, so this only matters for `--json`.
        if gated_abort_partial.is_some() {
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
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
    if show_merge_list {
        if disp_tree {
            // #131 S1: the tree-mode serialization order for the display
            // walk (real's reversed tree-mode retlist). Computed on the
            // displayed slice, so abort-partial lists stay
            // self-consistent. One extra schedule+select pass per `--tree`
            // run; the flat list (and everything else) never sees it.
            let tree_order = if circular_forced_display
                && display_entries.iter().any(|e| e.targets_running_root)
            {
                // Backlog #245 (cross-root partial only; the single-root
                // cycle keeps the re-serialization below): real hands `handler.merge_list` (the stuck remainder
                // drained in digraph insertion order) straight to `display`; it is
                // not re-serialized, and `print_tree` walks the reverse of its
                // retlist, so the partial goes in reversed.
                (0..display_entries.len()).rev().collect()
            } else {
                portage_repo::tree_display_order(
                    display_entries,
                    &expanded_atoms,
                    &config,
                    &root,
                    implicit_system_deps,
                    &repos,
                    dynamic_deps,
                    &result.circular_dependency,
                )
            };
            rendered_rows = print_tree(
                display_entries,
                &tree_order,
                &root,
                &top_level_pkgs,
                onlydeps,
                oneshot,
                unordered_display,
                disp_verbose,
                disp_quiet,
                alphabetical,
                root_deps_running_root.as_deref(),
                &color,
                system_atoms,
                &world_atoms,
                &force_reinstall_cps,
                &mut blocker_lines,
                &result.masked_deps,
                &result.use_unsat_deps,
                &result.plain_miss_deps,
                circular_forced_display,
                &result.circular_dependency,
            );
        } else {
            // #68/#72 B2: rows whose replacement waits on its owner print
            // right after that replacement's package line (real appends
            // them to `print_msg` at the solved blocker's merge-list
            // position, `output.py:119-121`).
            let inline_blockers = collect_inline_blocker_lines(
                display_entries,
                &root,
                quiet,
                columns,
                &color,
                &result.circular_dependency,
            );
            for i in 0..display_entries.len() {
                print_entry_line(
                    display_entries,
                    &root,
                    i,
                    "",
                    true,
                    &top_level_pkgs,
                    onlydeps,
                    oneshot,
                    disp_verbose,
                    disp_quiet,
                    alphabetical,
                    columns,
                    columnwidth,
                    root_deps_running_root.as_deref(),
                    &color,
                    system_atoms,
                    &world_atoms,
                    &force_reinstall_cps,
                    &mut blocker_lines,
                    &result.masked_deps,
                    &result.use_unsat_deps,
                    &result.plain_miss_deps,
                    circular_forced_display,
                    &result.circular_dependency,
                );
                for (_, line) in inline_blockers.iter().filter(|(after, _)| *after == i) {
                    println!("{line}");
                }
            }
        }

        // Backlog #19 Slice 4: the aborted partial list carries no
        // `NoVisibleCandidate` entries (masked/unsat partials are empty;
        // the cycle remainder is merge-bound), so the disclosure / bare-
        // unsat lines `print_entry_line` emits for them would go silent
        // with the list switch. Re-emit them here, in full-entries order
        // — the same stderr sequence the full-list walk produced, since
        // these lines already go to stderr while the list goes to stdout.
        // Slice 5 reshapes this into the `AbortReason` error block that
        // follows the partial list; until then the content is unchanged.
        // Non-abort resolves (including `Complete` graphs that still
        // carry NVC entries, e.g. build-root-only misses) render those
        // lines in the loop above exactly as before.
        // Backlog #185: pretend-only since the unconditional
        // resolution-phase block above took over the non-`--pretend`
        // preamble. Real's resolution-phase prints (header, notice,
        // timing) run before success/failure is known, so a failed
        // non-`--pretend` resolve already printed them above; re-printing
        // here would double them, and real never shows the partial list
        // itself on a non-`--pretend` abort (`actions.py:460-462`).
        if pretend
            && gated_abort_partial.is_some()
            && matches!(
                &result.outcome,
                portage_repo::ResolveOutcome::Aborted {
                    reason: portage_repo::AbortReason::MaskedDep { .. }
                        | portage_repo::AbortReason::UnsatisfiedAtom { .. }
                        | portage_repo::AbortReason::VirtualCycle { .. },
                    ..
                }
            )
        {
            // #135 (c) (O7: un-suppress): real prints the merge-list
            // preamble and the timing line and *then* aborts
            // (`These are the packages ...` + `Calculating dependencies
            // ... done!` + `Dependency resolution took N s
            // (backtrack: N/M).` + blank, all on stdout, before the
            // abort block on stderr). The partial list itself is empty
            // for these arms, so this is the preamble only.
            // Determinism cut (lessons-of-backlog-ops: nondeterministic
            // values ride `--json` only, never stdout; the
            // byte-identical repeated-run invariant is a jointly-owned
            // gate): the wall-clock seconds real prints are omitted --
            // the line carries real's `backtrack: N/M` signal only.
            // O7's "timing line" is therefore the signal half, not the
            // seconds; re-open with the owner to print real seconds.
            println!(
                "These are the packages that would be merged, in order:
"
            );
            println!("Calculating dependencies ... done!");
            // Real prints the timing line on every non-quiet run
            // (backlog #59(b)); under `--quiet` it is suppressed.
            if !quiet {
                println!(
                    "Dependency resolution took (backtrack: {}/{}).
",
                    result.backtrack_restarts, result.backtrack_max,
                );
            }
            let mut thrown_away: Vec<String> = Vec::new();
            for (i, entry) in result.entries.iter().enumerate() {
                // Backlog #195 (round 2): a dependency the resolver
                // collected into a parent USE flip (record-and-fail, real
                // `_apply_parent_use_changes` dropping the item from
                // `_unsatisfied_deps_for_display`) renders no row at all
                // -- the USE-changes block below is its whole output, and
                // the `!!! note:` fallback in `print_entry_line` would
                // duplicate it.
                if matches!(
                    entry.outcome,
                    portage_repo::PretendOutcome::NoVisibleCandidate
                ) && entry.parent_use_suggestion.is_none()
                {
                    // Backlog #244: real's `display_problems` prints the
                    // slot-conflict block before the leftover
                    // `_unsatisfied_deps_for_display` item
                    // (`depgraph.py:11113-11280`); with a conflict
                    // recorded, this line waits until after that block.
                    if !result.slot_conflicts.is_empty() {
                        deferred_abort_unsat.push(i);
                        continue;
                    }
                    print_entry_line(
                        &result.entries,
                        &root,
                        i,
                        "",
                        true,
                        &top_level_pkgs,
                        onlydeps,
                        oneshot,
                        verbose,
                        quiet,
                        alphabetical,
                        columns,
                        columnwidth,
                        root_deps_running_root.as_deref(),
                        &color,
                        system_atoms,
                        &world_atoms,
                        &force_reinstall_cps,
                        &mut thrown_away,
                        &result.masked_deps,
                        &result.use_unsat_deps,
                        &result.plain_miss_deps,
                        false,
                        &result.circular_dependency,
                    );
                }
            }
        }

        // Real `Display.print_blockers()`: the collected `[blocks B ...]`
        // lines, printed as one group after every package line and before
        // the counters. Backlog #80: unresolved rows with no owner entry
        // join the same group (flat and tree alike -- real's
        // `Display.blockers` is mode-independent). Unresolved rows are
        // never column-suppressed, so no `columns` gate here.
        for orphan in &result.orphan_blockers {
            blocker_lines.push(format_blocker_row(
                &format!(
                    "{}/{}-{}",
                    orphan.owner_category, orphan.owner_package, orphan.owner_version
                ),
                !quiet,
                "",
                &color,
                &orphan.conflict,
            ));
        }
        for line in &blocker_lines {
            println!("{line}");
        }
    }

    // Real `output.py::display`: `if self.conf.verbosity == 3:
    // self.print_verbose(...)` -- the `Total: …` counters line, printed
    // after every entry (and blocker) line, only under `-v`, for the
    // tree/columns/flat layouts alike. Real emits `f"\n{self.counters}\n"`
    // (a leading blank line). `--quiet` forces verbosity to 1, so `-pvq`
    // suppresses the line that `-pv` would show. Suppressed together with
    // an empty aborted list (real never calls `display()` there, so no
    // counters exist); the cycle partial counts over its own rows only.
    //
    // Backlog #206 S2: the circular display runs forced-verbose (see
    // `circular_forced_display` above), so the gate reads the effective
    // flags and the counters count the rendered rows, not the entries:
    // real's `_PackageCounters` accumulate per displayed merge row, so a
    // repeat-merge row (the cycle closure) counts again while a
    // `[nomerge]` row never counts. The recount input is exactly the
    // rendered merge/uninstall rows (`rendered_rows` from the tree walk
    // above); `nomerge`, already-installed and disclosure rows contribute
    // nothing, matching real's `set_pkg_info` gating.
    let disp_count_entries: Option<Vec<GraphEntry>> = if circular_forced_display {
        let mut counted: Vec<GraphEntry> = Vec::new();
        for (i, ordered) in &rendered_rows {
            if !ordered {
                continue;
            }
            let e = &display_entries[*i];
            match &e.outcome {
                portage_repo::PretendOutcome::New { .. }
                | portage_repo::PretendOutcome::Upgrade { .. }
                | portage_repo::PretendOutcome::Downgrade { .. }
                | portage_repo::PretendOutcome::Reinstall { .. }
                | portage_repo::PretendOutcome::Uninstall { .. } => {
                    counted.push(e.clone());
                }
                portage_repo::PretendOutcome::AlreadyInstalled { .. }
                | portage_repo::PretendOutcome::NoVisibleCandidate => {}
            }
        }
        Some(counted)
    } else {
        None
    };
    if show_merge_list && disp_verbose && !disp_quiet && !display_list_suppressed {
        println!();
        println!(
            "{}",
            package_counters_summary(
                disp_count_entries.as_deref().unwrap_or(display_entries),
                &root,
                &top_level_pkgs,
                onlydeps,
                &color,
                &result.orphan_blockers,
                disp_tree,
                &result.circular_dependency,
            )
        );
    }

    // Backlog #185: real `_emerge/actions.py:526` prints one blank line
    // after the merge list, before the `--ask` prompt (or the scheduler,
    // when nothing prompts). `show_merge_list` (not the bare gate) is
    // the condition: real's `print()` only runs when the list branch ran
    // (an aborted resolve returns before it). `--ask` needs none here:
    // `ask_confirm` already opens with that same single blank line
    // (real's `print()` before `UserQuery.query`), so printing another
    // would double it.
    if !pretend && show_merge_list && !ask {
        println!();
    }

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
    if show_merge_list && !result.cycle_display.is_empty() && !skip_redisplay {
        println!();
        let mut thrown_away: Vec<String> = Vec::new();
        for cpv in &result.cycle_display {
            let Some((idx, _entry)) = display_entries.iter().enumerate().find(|(_, e)| {
                let ver = match &e.outcome {
                    portage_repo::PretendOutcome::New { version }
                    | portage_repo::PretendOutcome::Reinstall { version, .. } => version,
                    portage_repo::PretendOutcome::Upgrade { to, .. }
                    | portage_repo::PretendOutcome::Downgrade { to, .. } => to,
                    _ => return false,
                };
                format!("{}/{}-{ver}", e.category, e.package) == *cpv
            }) else {
                continue;
            };
            print_entry_line(
                display_entries,
                &root,
                idx,
                "",
                true,
                &top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                root_deps_running_root.as_deref(),
                &color,
                system_atoms,
                &world_atoms,
                &force_reinstall_cps,
                &mut thrown_away,
                &result.masked_deps,
                &result.use_unsat_deps,
                &result.plain_miss_deps,
                // The legacy (gate-off) re-display keeps the user-flagged
                // rendering it always had; the forced-verbose display is
                // the gated path's main list above (whose re-display is
                // skipped as duplicative).
                false,
                &result.circular_dependency,
            );
        }
    }

    // Real `depgraph._show_slot_collision_notice` -> `slot_conflict_handler.
    // get_conflict()` (`lib/_emerge/resolver/slot_collision.py`): the
    // `!!! Multiple package instances within a single package slot ...`
    // block, then the advisory paragraph. Ported: the preamble, the
    // per-instance `(<cpv>, ebuild scheduled for merge) USE="" pulled in
    // by` line, real `_prepare_conflict_msg_and_check_for_specificity`'s
    // `collision_reasons` grouping + one-representative-per-reason
    // selection (every parent under `--verbose-conflicts`), the
    // `highlight_violations` `^` marker line (operator, version, slot,
    // and USE-token spans, wrapped in `BAD` red under `--color y` with
    // markers kept aligned on the displayed string -- real marks
    // pre-color indices and drifts, a genuine upstream bug portuale
    // deliberately does not reproduce), the `(and N more with the
    // same problem[s])` tail, the `NOTE: Use the '--verbose-conflicts'
    // option ...` footer, the `need_rebuild` "cannot be rebuilt" trailer,
    // and the advisory (with the `--backtrack=30` hint gated the real
    // way: shown unless `--backtrack` is >=30 or 0). Documented cuts
    // (fixtures don't exercise them): the `soname` reason key (no soname
    // atom can reach this renderer) and an `=*` operator's `("version",
    // None)` key. The one deliberate divergence in this block: real
    // drifts its `^` markers once ANSI codes lengthen the line under
    // `--color y` (genuine upstream bug); portuale wraps the same spans
    // but keeps the markers aligned. The block is display-only: the
    // exit-code consequence lives below in the "display, then fail the
    // action" band next to `unsolvable_blockers` (real `actions.py:
    // 460-462` -- a slot conflict fails the run with rc 1, `--pretend`
    // included; see that gate's own comment for the two escapes).
    if !result.slot_conflicts.is_empty() {
        let mut any_omitted = false;
        // Real `need_rebuild`: installed parents with built slot-operator
        // atoms that cannot be rebuilt, in first-seen order, printed once
        // after all conflicts (real keeps a dict across the whole
        // handler).
        let mut need_rebuild: Vec<(String, String)> = Vec::new();
        let installed_pkgs: std::collections::HashSet<(String, String, String)> =
            all_installed_packages(&root)
                .into_iter()
                .map(|p| (p.category, p.package, p.version))
                .collect();
        println!();
        println!("!!! Multiple package instances within a single package slot have been pulled");
        println!("!!! into the dependency graph, resulting in a slot conflict:");
        for c in &result.slot_conflicts {
            println!();
            println!("{}/{}:{}", c.category, c.package, c.slot);
            for inst in &c.instances {
                println!();
                if inst.installed {
                    // Real's installed nomerge node: `(cpv, installed in
                    // '<root>')` (see `build_residual_slot_conflicts`).
                    println!(
                        "  ({}/{}-{}:{}/{}::{}, installed in '{}') {} pulled in by",
                        c.category,
                        c.package,
                        inst.version,
                        c.slot,
                        inst.sub_slot,
                        inst.repo_name,
                        root.display(),
                        render_pkg_use_display(&inst.use_display)
                    );
                } else {
                    println!(
                        "  ({}/{}-{}:{}/{}::{}, ebuild scheduled for merge) {} pulled in by",
                        c.category,
                        c.package,
                        inst.version,
                        c.slot,
                        inst.sub_slot,
                        inst.repo_name,
                        render_pkg_use_display(&inst.use_display)
                    );
                }
                // Whether a bare `(Argument)` parent shows: real files one
                // under a reason only when the *other* package is already
                // installed -- always the case for a residual record's
                // installed side, never for merge-vs-merge records (whose
                // instances are all scheduled for merge, so such a parent
                // contributes no specific reason and is not shown).
                let show_argument_parents = c
                    .instances
                    .iter()
                    .any(|o| !std::ptr::eq(o, inst) && o.installed);
                // Real compares against every *other* instance in the
                // slot (`slot_collision.py`: `if other_pkg == pkg:
                // continue`) -- identity, not version: a same-version
                // merge/installed twin (a USE-only break, backlog #222)
                // still sees its twin, so the USE parent classifies.
                let others: Vec<ConflictOther> = c
                    .instances
                    .iter()
                    .filter(|o| !std::ptr::eq(*o, inst))
                    .map(|o| {
                        // Flag sets for the `("use", flag)` branch: an
                        // installed other's own vdb-recorded sets, like
                        // the `use_display` on its header line.
                        let (iuse, use_flags) = if o.installed {
                            portage_repo::installed_pkg_iuse_and_use(
                                &root,
                                &c.category,
                                &c.package,
                                &o.version,
                            )
                        } else {
                            slot_conflict_flag_sets(
                                &repos,
                                &config,
                                &c.category,
                                &c.package,
                                &o.version,
                            )
                        };
                        ConflictOther {
                            cpv: format!(
                                "{}/{}-{}:{}/{}::{}",
                                c.category, c.package, o.version, c.slot, o.sub_slot, o.repo_name
                            ),
                            iuse,
                            use_flags,
                            installed: o.installed,
                        }
                    })
                    .collect();
                let mut classified: Vec<ClassifiedParent> = Vec::new();
                // (parent_cpv, atom, display, installed) for the selected
                // line rendering below -- an installed parent renders
                // `required by (<cpv>, installed in '<root>')`.
                let mut classified_installed: Vec<bool> = Vec::new();
                for p in &inst.parents {
                    if p.parent_cpv.is_empty() && !show_argument_parents {
                        continue;
                    }
                    let Some(atom) = parse_atom(&p.atom) else {
                        continue;
                    };
                    let (mut reasons, use_unconditional) = slot_conflict_reasons(&atom, &others);
                    // Real `slot_collision.py`'s `("AtomArg", None)` key
                    // (`elif isinstance(ppkg, AtomArg) and
                    // other_pkg.installed`): a bare command-line parent
                    // satisfied by every other instance is still shown
                    // once some other instance is installed. Like real's
                    // `elif`, it only fires when no version/slot/USE
                    // reason did (so an argument parent that already
                    // classifies, like pg1's `>=cgp1x-2`, renders exactly
                    // once, as today).
                    if p.parent_cpv.is_empty()
                        && reasons.is_empty()
                        && others.iter().any(|o| o.installed)
                    {
                        reasons.push(SlotConflictReason::Argument);
                    }
                    if !reasons.is_empty() {
                        classified.push(ClassifiedParent {
                            parent_cpv: &p.parent_cpv,
                            atom_str: &p.atom,
                            atom,
                            reasons,
                            use_unconditional,
                            use_display: &p.use_display,
                        });
                        classified_installed.push(p.installed);
                    }
                }
                let num_all_specific: usize = classified.iter().map(|p| p.reasons.len()).sum();
                // Group parents by reason, first-seen order preserved.
                let mut groups: Vec<(SlotConflictReason, Vec<usize>)> = Vec::new();
                for (i, p) in classified.iter().enumerate() {
                    for reason in &p.reasons {
                        match groups.iter_mut().find(|(r, _)| r == reason) {
                            Some((_, members)) => members.push(i),
                            None => groups.push((reason.clone(), vec![i])),
                        }
                    }
                }
                // Representative selection: farthest version per `cp` for
                // a `version` group, the first member for a `slot` group,
                // every member of a `use` group (real shows all broken
                // reverse dependencies -- "people can simply use a pager"),
                // and every member under `--verbose-conflicts`.
                let mut selected: Vec<usize> = Vec::new();
                for (reason, members) in &groups {
                    if verbose_conflicts {
                        selected.extend(members.iter().copied());
                        continue;
                    }
                    match reason {
                        SlotConflictReason::Version(sub) => {
                            let mut best: Vec<(String, usize)> = Vec::new();
                            for &m in members {
                                let atom = &classified[m].atom;
                                let cp = format!("{}/{}", atom.category, atom.package);
                                match best.iter_mut().find(|(c, _)| *c == cp) {
                                    None => best.push((cp, m)),
                                    Some((_, cur)) => {
                                        let cur_ver =
                                            classified[*cur].atom.version.as_deref().unwrap_or("");
                                        let new_ver = atom.version.as_deref().unwrap_or("");
                                        let cmp = portage_versions::vercmp(new_ver, cur_ver);
                                        let farther = match *sub {
                                            "ge" | "eq" => cmp == Some(1),
                                            "le" => cmp == Some(-1),
                                            _ => false,
                                        };
                                        if farther {
                                            *cur = m;
                                        }
                                    }
                                }
                            }
                            selected.extend(best.into_iter().map(|(_, m)| m));
                        }
                        SlotConflictReason::Slot(_) => {
                            if let Some(&first) = members.first() {
                                selected.push(first);
                            }
                        }
                        SlotConflictReason::Use(_) => {
                            selected.extend(members.iter().copied());
                        }
                        SlotConflictReason::Argument => {
                            selected.extend(members.iter().copied());
                        }
                    }
                }
                // Real's `need_rebuild` scan runs over every `slot`-group
                // parent (verbose or not): an unrebuildable parent is
                // always shown, and recorded for the once-per-notice
                // trailer below.
                for (reason, members) in &groups {
                    if !matches!(reason, SlotConflictReason::Slot(_)) {
                        continue;
                    }
                    for &m in members {
                        let p = &classified[m];
                        if let Some(why) = slot_conflict_need_rebuild(
                            p.parent_cpv,
                            &p.atom,
                            &excluded,
                            &useoldpkg_atoms_render,
                            usepkgonly,
                            &repos,
                            &config,
                            &installed_pkgs,
                        ) {
                            if !selected.contains(&m) {
                                selected.push(m);
                            }
                            let entry = (format!("({}, installed)", p.parent_cpv), why);
                            if !need_rebuild.contains(&entry) {
                                need_rebuild.push(entry);
                            }
                        }
                    }
                }
                selected.sort_unstable();
                selected.dedup();
                // Real shows `unconditional_use_deps` first (its own set
                // order is nondeterministic; portuale keeps the stable
                // classified order within each half).
                selected.sort_by_key(|m| !classified[*m].use_unconditional);
                for &m in &selected {
                    let p = &classified[m];
                    let version_violated = p
                        .reasons
                        .iter()
                        .any(|r| matches!(r, SlotConflictReason::Version(_)));
                    let slot_violated = p
                        .reasons
                        .iter()
                        .any(|r| matches!(r, SlotConflictReason::Slot(_)));
                    let use_flags: Vec<String> = p
                        .reasons
                        .iter()
                        .filter_map(|r| match r {
                            SlotConflictReason::Use(f) => Some(f.clone()),
                            _ => None,
                        })
                        .collect();
                    let idx = slot_conflict_caret_idx(
                        p.atom_str,
                        &p.atom,
                        version_violated,
                        slot_violated,
                        &use_flags,
                    );
                    // Real `highlight_violations` wraps the violated
                    // spans in `BAD` red here (`--color y` only; plain
                    // otherwise) -- and then marks the PRE-color
                    // indices, so its `^` line drifts once ANSI codes
                    // lengthen the line (genuine upstream bug: the
                    // markers land left of their visible targets, and
                    // real's own red varies by terminal colormap, so
                    // byte-parity with the drift is unachievable
                    // anyway). Portuale wraps the same spans but marks
                    // the DISPLAYED string, so color and carets agree.
                    let (atom_display, shifted) = colorize_marked_spans(p.atom_str, &idx, &color);
                    // A bare command-line parent renders `<atom>
                    // (Argument)` with no marker line (real shows no `^`
                    // under one either); an installed parent names its
                    // position like an installed instance does.
                    if p.parent_cpv.is_empty() {
                        println!("    {atom_display} (Argument)");
                        continue;
                    }
                    let parent_pos = if classified_installed[m] {
                        format!("{}, installed in '{}'", p.parent_cpv, root.display())
                    } else {
                        format!("{}, ebuild scheduled for merge", p.parent_cpv)
                    };
                    let cur_line = format!(
                        "{} required by ({}) {}\n",
                        atom_display,
                        parent_pos,
                        render_pkg_use_display(p.use_display)
                    );
                    let marker: String = (0..cur_line.chars().count())
                        .map(|k| if shifted.contains(&k) { '^' } else { ' ' })
                        .collect();
                    print!("    {cur_line}");
                    println!("    {marker}");
                }
                if selected.is_empty() && !classified.is_empty() {
                    println!(
                        "    (no parents that aren't satisfied by other packages in this slot)"
                    );
                }
                let omitted = num_all_specific - selected.len();
                if omitted > 0 {
                    any_omitted = true;
                    let noun = if selected.len() > 1 {
                        "problems"
                    } else {
                        "problem"
                    };
                    println!("    (and {omitted} more with the same {noun})");
                }
            }
        }
        if any_omitted {
            println!();
            println!(
                "{}",
                color.c(
                    "INFORM",
                    "NOTE: Use the '--verbose-conflicts' option to display parents omitted above"
                )
            );
            println!();
        }
        // Real `slot_collision.py`'s `need_rebuild` trailer: installed
        // parents with built slot-operator atoms that cannot be rebuilt.
        if !need_rebuild.is_empty() {
            println!();
            println!("!!! The slot conflict(s) shown above involve package(s) which may need to");
            println!("!!! be rebuilt in order to solve the conflict(s). However, the following");
            println!("!!! package(s) cannot be rebuilt for the reason(s) shown:");
            println!();
            for (ppkg, reason) in &need_rebuild {
                println!("  {ppkg}: {reason}");
            }
        }
        println!();
        for line in [
            "It may be possible to solve this problem by using package.mask to",
            "prevent one of those packages from being selected. However, it is also",
            "possible that conflicting dependencies exist such that they are",
            "impossible to satisfy simultaneously.  If such a conflict exists in",
            "the dependencies of two different packages, then those packages can",
        ] {
            println!("{line}");
        }
        // Real gates the hint on the reported depgraph's own
        // `not _allow_backtracking` too (`depgraph.py:1750-1759`): a loop
        // ended early by `need_config_change` reports a pass that still
        // allowed backtracking (backlog #244).
        if backtrack_max > 0 && backtrack_max < 30 && !result.settled_with_backtracking {
            println!("not be installed simultaneously. You may want to try a larger value of");
            println!("the --backtrack option, such as --backtrack=30, in order to see if");
            println!("that will solve this conflict automatically.");
        } else {
            println!("not be installed simultaneously.");
        }
        println!();
        println!("For more information, see MASKED PACKAGES section in the emerge man");
        println!("page or refer to the Gentoo Handbook.");
        println!();
    }
    if !deferred_abort_unsat.is_empty() {
        let mut thrown_away: Vec<String> = Vec::new();
        for &i in &deferred_abort_unsat {
            print_entry_line(
                &result.entries,
                &root,
                i,
                "",
                true,
                &top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                quiet,
                alphabetical,
                columns,
                columnwidth,
                root_deps_running_root.as_deref(),
                &color,
                system_atoms,
                &world_atoms,
                &force_reinstall_cps,
                &mut thrown_away,
                &result.masked_deps,
                &result.use_unsat_deps,
                &result.plain_miss_deps,
                false,
                &result.circular_dependency,
            );
        }
    }

    // Backlog #90 (S2) + #92, #129 review round 2: real
    // `_conflict_missed_update["slot conflict"]`
    // (`depgraph.py:2063-2106`) -- `WARNING: One or more
    // updates/rebuilds have been skipped due to a dependency
    // conflict:` plus one `conflicts with` block per missed upgrade,
    // printed after the merge list, rc 0. Real keeps one entry per
    // `(root, slot_atom)` with the whole `(parent, atom)` set inside
    // (`_get_missed_updates`, `:1553-1562`); portuale's rows are one
    // per rejecting parent, so rows naming the same missed pkg group
    // into one block (`portage_repo::group_skipped_updates`). Two
    // producers, one shape: the direct solve's removed instances and
    // the reverse-pin withholds (`GraphResult::skipped_updates`; the
    // renderer cannot tell them apart and real does not distinguish
    // them either). Backlog #230 renders each package with real's full
    // `pkg_use_display` (`skipped_update_use_display_for` /
    // `skipped_update_installed_use_display_for`, reusing the
    // slot-collision notice's `render_pkg_use_display` string form):
    // the whole effective USE masked to the valid-IUSE domain, one
    // `VAR="…"` group per non-hidden `USE_EXPAND` var, `( )`-wrapped
    // force/mask flags. Backlog #242 Slice D: destinations follow real's
    // per-node rule (`skipped_node_suffix` — quoted `to '<root>'` iff
    // the node's OWN root is not `/`), and each side paints from its own
    // root's profile (the running `Config` when the row rides the
    // running root, recomputed here from the shared repos; single-root
    // runs keep the resolve-time target paint). An installed consumer
    // renders `(cpv, installed in '<root>')` with its own root, like the
    // notice's installed instances. Real's own rendering pairs a bare
    // missed line with `to`-suffixed parents exactly when the missed
    // package sits in the running-root (`/`) tree while the parents sit
    // in the target (the bed blk0 shape with the running pin dropped);
    // under `fixture_env`'s single root every node is target-rooted, so
    // the missed line and the parents all carry the fixture `to` suffix
    // with fixture-profile paint — deterministic, and the contract pin
    // records it as such. The `^` marker line
    // mirrors real `format_unmatched_atom`'s operator + version spans
    // plus a mismatched `:slot[/sub-slot]` span (USE-token spans stay a
    // documented cut -- no grounded case carries USE-deps here). A
    // top-level `(Argument)` parent (empty `consumer_cpv`: a CLI atom
    // that accepts only the surviving instance) renders as real's bare
    // indented arg line with no atom and no marker
    // (`depgraph.py:1681-1686`). Suppressed under `--quiet` unless
    // `--debug` -- real `_show_missed_update` drops both notice types
    // then (`depgraph.py:1576-1581`). `--json` never reaches this block
    // (it returns above); `--columns` has no gate (real shows the
    // notices regardless of columns: they are not merge-list rows).
    if !(quiet && !debug) && !result.skipped_updates.is_empty() {
        println!(
            "WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:"
        );
        for group in portage_repo::group_skipped_updates(&result.skipped_updates) {
            let Some(header) = group.first() else {
                continue;
            };
            println!();
            println!("{}/{}:{}", header.category, header.package, header.slot);
            println!();
            // Backlog #242 Slice D: per-node destination (quoted, iff the
            // missed node's own root is not `/`) and own-root USE paint
            // (recomputed from the shared repos under the running profile
            // when the miss rides the running root).
            let missed_suffix = skipped_node_suffix(
                header.missed_targets_running_root,
                &root,
                root_deps_running_root.as_deref(),
            );
            let missed_use = if header.missed_targets_running_root
                && let Some(cfg) = running_config.as_ref()
            {
                portage_repo::skipped_update_use_display_for(
                    &repos,
                    cfg,
                    &header.category,
                    &header.package,
                    &header.skipped_version,
                )
            } else {
                header.skipped_use.clone()
            };
            println!(
                "  ({}/{}-{}:{}/{}::{}, ebuild scheduled for merge{}) {} conflicts with",
                header.category,
                header.package,
                header.skipped_version,
                header.slot,
                header.skipped_sub_slot,
                header.skipped_repo,
                missed_suffix,
                render_pkg_use_display(&missed_use),
            );
            for s in group.iter() {
                if s.consumer_cpv.is_empty() {
                    // Real's `PackageArg`/`AtomArg` arm
                    // (`depgraph.py:1681-1686`): the bare command-line
                    // argument, no atom and no `^` marker. `s.atom`
                    // already IS that argument here: the only producer
                    // that emits empty-`consumer_cpv` rows is the
                    // direct-solve path, and its empty-cpv parents come
                    // solely from top-level argument pullers, whose
                    // recorded atom is the argument text as typed
                    // (unit-pinned by portage-repo's
                    // `direct_solve_reports_an_argument_parent_with_the_cli_text`;
                    // the reverse-pin producer always builds a
                    // non-empty consumer, the backtrack-mask rows skip
                    // empty pullers). No contract or bed cell grounds
                    // this arm end to end (blk0 parents are all
                    // Packages), so beyond that producer pin it stays
                    // unpinned.
                    println!("    {}", s.atom);
                    continue;
                }
                if s.consumer_installed {
                    // Backlog #242 Slice D: the installed root is the
                    // consumer's own (`in '<root>'` with the running path
                    // when the consumer rides it — always the target walk
                    // from every producer today, so the flag is false and
                    // this matches the old rendering exactly).
                    let installed_root: &Path = if s.consumer_targets_running_root {
                        root_deps_running_root.as_deref().unwrap_or(root.as_path())
                    } else {
                        root.as_path()
                    };
                    println!(
                        "    {} required by ({}, installed in '{}') {}",
                        s.atom,
                        s.consumer_cpv,
                        installed_root.display(),
                        render_pkg_use_display(&s.consumer_use),
                    );
                } else {
                    // Backlog #242 Slice D: per-node destination + own-root
                    // USE paint, like the missed line above. The consumer
                    // cpv re-derives `(cat, pkg, ver)` for the recompute;
                    // an unparsable cpv keeps the resolve-time paint.
                    let consumer_suffix = skipped_node_suffix(
                        s.consumer_targets_running_root,
                        &root,
                        root_deps_running_root.as_deref(),
                    );
                    let consumer_use = if s.consumer_targets_running_root
                        && let Some(cfg) = running_config.as_ref()
                        && let Some((cc, cp2, cv)) = s
                            .consumer_cpv
                            .split(':')
                            .next()
                            .and_then(portage_repo::split_cpv)
                    {
                        portage_repo::skipped_update_use_display_for(&repos, cfg, &cc, &cp2, &cv)
                    } else {
                        s.consumer_use.clone()
                    };
                    println!(
                        "    {} required by ({}, ebuild scheduled for merge{}) {}",
                        s.atom,
                        s.consumer_cpv,
                        consumer_suffix,
                        render_pkg_use_display(&consumer_use),
                    );
                }
                // #270: real marks the operator + version only when the
                // missed package does not satisfy the version part
                // (`highlight_version`, `output.py:925-929`: the atom
                // without repo, slot and USE against the package).
                let version_ok = {
                    let cpv = format!(
                        "{}/{}-{}",
                        header.category, header.package, header.skipped_version
                    );
                    let base = portage_dep::without_use(&s.atom);
                    let base = base.split("::").next().unwrap_or(base);
                    let base = base.split(':').next().unwrap_or(base);
                    portage_dep::match_from_list(base, &[cpv.as_str()])
                        .is_some_and(|m| !m.is_empty())
                };
                println!(
                    "    {}",
                    skip_conflict_caret_line(
                        &s.atom,
                        &header.slot,
                        &header.skipped_sub_slot,
                        !version_ok
                    )
                );
            }
        }
        println!();
    }

    // Backlog #129 (S1): real `_show_missed_update_unsatisfied_dep`'s
    // abbreviated tail (`depgraph.py:1638-1649`) -- the slots whose
    // higher masked version lost to an unsatisfied dependency under
    // backtracking (`GraphResult::skipped_missing_deps`, in first-mask
    // order like the `WARNING` rows above). Real prefixes the section
    // with one blank line (its slot section already ends with one);
    // the `for <root>` suffix is omitted like every other portuale
    // notice row. Same `--quiet`-unless-`--debug` gate as the
    // `WARNING` block above (real drops both types together,
    // `depgraph.py:1576-1581`); likewise no `--columns` gate.
    if !(quiet && !debug) && !result.skipped_missing_deps.is_empty() {
        println!();
        println!("!!! The following update(s) have been skipped due to unsatisfied dependencies");
        println!("!!! triggered by backtracking:");
        println!();
        for s in &result.skipped_missing_deps {
            println!("{}/{}:{}", s.category, s.package, s.slot);
        }
    }

    // Backlog #198: real `_show_missed_update_unsatisfied_dep`'s full
    // per-update form (`depgraph.py:1592-1636`) -- one block per masked
    // higher version whose missing atom bites nothing backtrack-masked
    // (`GraphResult::skipped_missing_dep_full`, in the same first-mask
    // order). The `cat/pkg:slot` header, the `selected:` / `skipped:
    // ... (see unsatisfied dependency below)` rows and the missing
    // atom's own disclosure follow real line for line; the `for <root>`
    // suffix is omitted like every other portuale notice row. Same
    // `--quiet`-unless-`--debug` gate as the two blocks above (real
    // drops all missed-update types together, `depgraph.py:1576-1581`);
    // likewise no `--columns` gate, and `--json` returns above.
    if !(quiet && !debug) && !result.skipped_missing_dep_full.is_empty() {
        for f in &result.skipped_missing_dep_full {
            println!();
            println!("!!! The following update has been skipped due to unsatisfied dependencies:");
            println!();
            println!("{}/{}:{}", f.category, f.package, f.slot);
            println!();
            println!("  selected: {}", f.selected_display);
            println!(
                "  skipped: {} (see unsatisfied dependency below)",
                f.skipped_display
            );
            println!();
            match &f.dep {
                portage_repo::SkippedDepDisclosure::Masked(report) => {
                    println!(
                        "!!! All ebuilds that could satisfy {:?} have been masked.",
                        report.atom
                    );
                    println!(
                        "!!! One of the following masked packages is required to complete your request:"
                    );
                    for (cpv, reasons) in &report.masked {
                        println!("- {cpv} (masked by: {})", reasons.join(", "));
                    }
                    println!();
                    for (node, ty) in &report.chain {
                        println!("(dependency required by \"{node}\" [{ty}])");
                    }
                    println!("For more information, see the MASKED PACKAGES section in the emerge");
                    println!("man page or refer to the Gentoo Handbook.");
                    println!();
                }
                portage_repo::SkippedDepDisclosure::Plain(report) => {
                    println!("emerge: there are no ebuilds to satisfy {:?}.", report.atom);
                    for (node, ty) in &report.chain {
                        println!("(dependency required by \"{node}\" [{ty}])");
                    }
                    println!();
                }
            }
        }
    }

    // Backlog #286: real `display_problems()`'s world-file tail
    // (`depgraph.py:11150-11190`) for the dropped set members
    // (`GraphResult::missing_args`) -- what real prints right after the
    // skipped-update block above. The world-file notice fires only for a
    // missing member of the world file itself (real's `atom in world_set`
    // gate: a `@system`/`@profile` member riding `@world` prints only
    // the list); the masked-or-don't-exist list names every missing arg
    // in requested order. Unlike the missed-update blocks above these
    // two carry no `--quiet` gate (real prints them outside
    // `_show_missed_update`); `--json` returns above, and `--columns`
    // has no gate (not merge-list rows).
    if !result.missing_args.is_empty() {
        let world_atoms: HashSet<String> = read_world_atoms(&root)
            .unwrap_or_default()
            .into_iter()
            .map(|a| portage_repo::apply_updates_to_atom(&a))
            .collect();
        if result.missing_args.iter().any(|a| world_atoms.contains(a)) {
            println!();
            println!("!!! Problems have been detected with your world file");
            println!("!!! Please run emaint --check world");
            println!();
        }
        println!();
        println!("!!! Ebuilds for the following packages are either all");
        println!("!!! masked or don't exist:");
        println!("{}", result.missing_args.join(" "));
    }

    // Backlog #19 Slice 5: with the gate on, an aborted cycle prints its
    // circular block HERE -- before the autounmask section -- matching
    // real `display_problems()` order (`_show_circular_deps` at
    // `depgraph.py:11113`, `_display_autounmask` at `:11140`). Flow then
    // continues into the autounmask rendering below (USE block, notice,
    // exit 1), which is exactly the fourth-shape oracle (spec §4d). A
    // masked/unsat abort prints no circular block at all (real abandons
    // the walk before serialization ever runs); gate off keeps the
    // legacy position below.
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted {
            reason: portage_repo::AbortReason::UnserializableCycle { members: _ },
            ..
        } = &result.outcome
        && let Some(cycle) = result.circular_deps.first()
    {
        print_circular_block(
            cycle,
            &portage_repo::cycle_edge_labels(&result.entries, cycle),
            &color,
            &repos,
            &config,
            &result.autounmask_use_changes,
            &result.entries,
            result.large_cycle_count,
        );
    }

    // Backlog #193: real `select_files`' `_virtual_cycle_error` handler
    // (`lib/_emerge/depgraph.py:5002-5013` in 3rdparty/portage 3.0.82.2)
    // prints `\n\n!!! virtual cycle detected:\n\n`, one
    // `  {cpv}::{repo}` line per probe-stack member (sorted), then `\n`
    // -- to stderr (`writemsg`, noiselevel -1), mid-resolution (between
    // real's `Calculating dependencies ...` notice halves; portuale
    // prints that notice whole in the preamble above, like the
    // masked/unsat arm). No merge list, no counters, no trailing error
    // block: real's run ends after the timing line (g193 probe), and the
    // shared abort gate below exits 1. Uncoloured like real's own
    // `writemsg` (no colour markup on that path).
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted {
            reason: portage_repo::AbortReason::VirtualCycle { members },
            ..
        } = &result.outcome
    {
        eprint!("\n\n!!! virtual cycle detected:\n\n");
        for member in members {
            eprintln!("  {member}");
        }
        eprintln!();
    }

    // Real `depgraph.py::_display_autounmask` (`:10625`), the
    // `unstable_keyword_msg` half: `--autounmask` accepted a
    // `KEYWORDS`-alone mask to make the graph resolve, so the implicit
    // `package.accept_keywords` change is reported after the merge list.
    // Real `_writemsg`: `\nThe following <BAD>keyword changes</BAD> are
    // necessary to proceed:\n (see "package.accept_keywords" in the
    // portage(5) man page for more details)\n`; then `format_msg`
    // (`#`-prefixed dep-chain comment lines stay plain, the `=<cpv>
    // <kw>` line is `INFORM`-coloured). One header covers every change.
    // Real portage does NOT print the "Use --autounmask-write" hint
    // under `--pretend` (`:11084` `not pretend`), and `emerge --pretend`
    // still exits 0 (real `actions.py:563` `return os.EX_OK`).
    let print_autounmask_block =
        |reason: &str, file: &str, changes: &[portage_repo::AutounmaskChange]| {
            if changes.is_empty() {
                return;
            }
            eprintln!(
                "\nThe following {} are necessary to proceed:",
                color.c("BAD", reason)
            );
            eprintln!(" (see \"{file}\" in the portage(5) man page for more details)");
            for change in changes {
                for line in &change.dep_chain {
                    eprintln!("# {line}");
                }
                let line = if change.token.is_empty() {
                    change.atom.clone()
                } else {
                    format!("{} {}", change.atom, change.token)
                };
                eprintln!("{}", color.c("INFORM", &line));
            }
        };
    // Real `_display_autounmask` `_writemsg` order: keyword, mask, USE,
    // license. The `atom` field already carries its own op prefix (`=`
    // for keywords/masks, `>=`/`>=…:slot`/`=` for USE/license).
    print_autounmask_block(
        "keyword changes",
        "package.accept_keywords",
        &result.autounmask_keyword_changes,
    );
    print_autounmask_block(
        "mask changes",
        "package.unmask",
        &result.autounmask_mask_changes,
    );
    print_autounmask_block("USE changes", "package.use", &result.autounmask_use_changes);
    print_autounmask_block(
        "license changes",
        "package.license",
        &result.autounmask_license_changes,
    );

    // `--autounmask-only`: real `actions.py:456` returns 0 right after
    // `display_problems()` -- nothing below (the abi-rebuild info, the
    // changed-deps report, and any real merge) runs. Real still calls
    // `post_emerge` after (`run_action`'s unconditional tail), so a
    // `--pretend` run prints the end notice here too.
    if autounmask_only {
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::SUCCESS;
    }

    // Real `action_build`: `backtrack_depgraph` returns `success=False`
    // whenever `_have_autounmask_changes()` (i.e. autounmask had to
    // touch `package.use`/`.accept_keywords`/`.unmask`/`.license` to make
    // the graph resolve), and then `if not success:
    // mydepgraph.display_problems(); return 1` -- BEFORE any merge, and
    // under `--pretend` too (`--pretend` does not exempt it). The merge
    // list and the change blocks just printed ARE `display_problems()`'s
    // output; the run then fails so a caller's `$?` check sees that the
    // config still needs editing. The one exception is a real (non-
    // `--pretend`) `--autounmask-continue`, which writes the changes and
    // proceeds (real `_resolve`'s own `'--pretend' not in myopts` guard).
    // (`has_autounmask_changes` is bound once, above, next to the
    // `--autounmask-only` re-show gate -- backlog #271.)
    let autounmask_continue_active = !pretend && autounmask_continue == Some(true);

    // Real `_display_autounmask`'s tail (`depgraph.py:11093`, gated on
    // `_dynamic_config._autounmask_backtrack_disabled`): with
    // `--autounmask-backtrack` off (the default), the resolver stops
    // after the first autounmask batch instead of re-driving, and says
    // so -- `--pretend` included. `--autounmask-continue` implies
    // backtrack=y, so the notice is suppressed there. Backlog #217:
    // the flag is set at exactly one site (`need_config_change:11752`),
    // reached only when the early return (`:11713-11717`) did NOT fire
    // -- i.e. NOT when `_success_without_autounmask` holds (the
    // `_resolve` tail `:5793`, "reserved for cases where there are
    // *zero* other problems": the resolve succeeds once the autounmask
    // changes are applied, no other failure). A lone autounmask change
    // (oracle: `abort-au-plain`, `fixtures/abort-captures/`) therefore
    // prints the change block with NO notice; the notice stays exactly
    // when another failure coincides (a cycle: `abort-au-cycle`; a
    // restart-worthy flip on an already-graphed package: `aucasctop`;
    // a post-failure parent rescue). `GraphResult::
    // autounmask_backtrack_disabled` ports the flag, including real's
    // `_allow_backtracking` conjunct (`--backtrack=0` never prints it).
    if result.autounmask_backtrack_disabled(config.autounmask_backtrack) {
        eprintln!();
        for line in [
            "In order to avoid wasting time, backtracking has terminated early",
            "due to the above autounmask change(s). The --autounmask-backtrack=y",
            "option can be used to force further backtracking, but there is no",
            "guarantee that it will produce a solution.",
        ] {
            eprintln!(" {} {line}", color.c("WARN", "*"));
        }
    }

    if has_autounmask_changes && !autounmask_continue_active {
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }

    // Real `_serialize_tasks` -> `_show_unsatisfied_blockers` +
    // `show_blocker_docs_link` (`depgraph.py:10496`): a blocked installed
    // package the resolver could neither replace nor unmerge is an
    // unresolved conflict -- real prints the `* Error: The above package
    // list …` block and `actions.py` returns 1. Portuale's per-parent
    // `pulled in by` detail is a documented cut (it needs real's
    // `_parent_atoms` / complete graph); the sentence, the docs link and
    // the exit code are what a `$?` check and the `Conflict: N blocks
    // (M unsatisfied)` line depend on.
    let unsolvable_blockers = result
        .entries
        .iter()
        .any(|e| e.blockers.iter().any(|b| b.unsolvable))
        // Backlog #80: an orphan row is unsolvable by construction
        // (`file_blocker_conflicts` only orphans `unsolvable` rows), so
        // it fails the run exactly like an entry-owned one -- real
        // prints the same `* Error` block for the n1b shape.
        || !result.orphan_blockers.is_empty();
    if unsolvable_blockers && show_merge_list {
        eprintln!();
        for line in [
            "Error: The above package list contains packages which cannot be",
            "installed at the same time on the same system.",
        ] {
            eprintln!(" {} {line}", color.c("BAD", "*"));
        }
        eprintln!(
            "\nFor more information about {}, please refer to the following",
            color.c("BAD", "Blocked Packages")
        );
        eprintln!("section of the Gentoo Linux x86 Handbook (architecture is irrelevant):\n");
        eprintln!("https://wiki.gentoo.org/wiki/Handbook:X86/Working/Portage#Blocked_packages\n");
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }

    // Backlog #62: a recorded slot conflict fails the whole action with
    // rc 1, matching real Portage 3.0.82.2. Real's rule, verified against
    // the source and host-side oracles before coding (the obvious "notice
    // printed => rc 1" formulation is wrong in the escape cases, and the
    // first draft of the plan had it wrong too):
    //
    // - The notice itself is unconditional on the conflict existing
    //   (`display_problems` prints it whenever
    //   `any(_package_tracker.slot_conflicts())`, `depgraph.py:
    //   11117-11120`); it says nothing about the exit code.
    // - The exit code comes from `action_build`'s `if not success:
    //   display_problems(); return 1` (`actions.py:460-462`), and
    //   `success` is False exactly when a conflict is recorded and not
    //   tolerated (`_resolve` tail, `depgraph.py:5685-5691`).
    // - Escape 1 -- `--autounmask-only` returns 0 first (`actions.py:
    //   456-458`), notice printed and all. Preserved for free: this gate
    //   sits after that early return above. Host oracle: the
    //   needer/othermod triangle with `--autounmask-only` prints the
    //   notice and exits 0.
    // - Escape 2 -- `_accept_blocker_conflicts()` (`depgraph.py:
    //   9259-9271`): backtracking disabled plus one of `--buildpkgonly`,
    //   `--fetchonly`, `--fetch-all-uri`, `--nodeps` tolerates the
    //   conflict. Host oracles: `--backtrack=0 --nodeps` and
    //   `--backtrack=0 --buildpkgonly` on the triangle exit 0 (in fact
    //   real records no conflict at all there -- `--nodeps` never walks
    //   the deps that would pull it in, `--buildpkgonly` resolves the
    //   shape without the upgrade -- so no "notice with rc 0" shape was
    //   reproducible; the gate still matters because portuale's resolver
    //   *does* record the conflict there). `--fetchonly`/
    //   `--fetch-all-uri` are still portuale-unimplemented options (rc 2
    //   before reaching here), so they need no parsed flag yet -- extend
    //   this condition when that surface lands. `--fetchonly` shapes
    //   could not be oracled hermetically anyway: real attempts the fetch
    //   phase past resolution and fails on the fixture's unexpanded
    //   `file://${PORTAGE_CONFIGROOT}` binhost URL (rc 1 even with no
    //   conflict anywhere).
    //
    // Gated on the data (`!result.slot_conflicts.is_empty()`), never on
    // "the notice was printed", so `-q`/`--json`/`--columns` cannot change
    // the rc -- `--json` has no real counterpart (real rejects it, rc 2)
    // and follows the same rule for consistency. The blocker path is
    // untouched: its `all satisfied` -> rc 0 / `unsolvable` -> rc 1 split
    // already matches real.
    let backtracking_disabled = backtrack_max == 0;
    let slot_conflict_tolerated = backtracking_disabled && (buildpkgonly || nodeps);
    if !result.slot_conflicts.is_empty() && !slot_conflict_tolerated {
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }
    // Backlog #295: the best-run re-pass could not take the `prune_rebuilds`
    // restart real wants (`depgraph.py:5763-5775`) -- display, then fail.
    if result.prune_unresolved {
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }

    // `emerge --pretend --debug` Stage 5: real
    // `_compute_abi_rebuild_info`'s own DEBUG-level dump (real
    // `depgraph.py:1011-1206`), on stdout, right before the
    // `_show_abi_rebuild_info` "causing rebuilds" block. Real's
    // `_slot_operator_deps` / `_forced_rebuilds` are richer dicts than
    // portuale's flat `abi_rebuilds` `(provider-cpv, consumer-cpv)`
    // list -- portuale emits the same shapes from what it has (a
    // documented fidelity gap: real's `slot operator dependencies:`
    // lists *every* installed `:=` edge, portuale only the ones that
    // actually forced a rebuild this run).
    if portage_repo::resolver_debug() {
        let root_str = root.display().to_string();
        // cpv -> (cp, slot, sub_slot, repo, type_name) for every
        // merge-bound entry (both ends of each abi-rebuild edge are
        // Reinstall/Upgrade). Same shape the "causing rebuilds" block
        // below builds.
        let mut by_cpv: HashMap<String, (String, String, String, String, &str)> = HashMap::new();
        for e in entries {
            let version = match &e.outcome {
                portage_repo::PretendOutcome::New { version }
                | portage_repo::PretendOutcome::Reinstall { version, .. } => version.clone(),
                portage_repo::PretendOutcome::Upgrade { to, .. }
                | portage_repo::PretendOutcome::Downgrade { to, .. } => to.clone(),
                _ => continue,
            };
            let ty = match e.source {
                portage_repo::CandidateSource::Binary => "binary",
                portage_repo::CandidateSource::Ebuild => "ebuild",
            };
            by_cpv.insert(
                format!("{}/{}-{version}", e.category, e.package),
                (
                    format!("{}/{}", e.category, e.package),
                    e.slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.repo_name
                        .clone()
                        .unwrap_or_else(|| "__unknown__".to_string()),
                    ty,
                ),
            );
        }
        let label = |cpv: &str| -> String {
            match by_cpv.get(cpv) {
                Some((_, slot, sub_slot, repo, ty)) => {
                    format!("({cpv}:{slot}/{sub_slot}::{repo}, {ty} scheduled for merge)")
                }
                None => format!("({cpv}, installed)"),
            }
        };

        println!("forced reinstall atoms:");
        if !reinstall_atoms.is_empty() {
            println!("   root: {root_str}");
            for atom in &reinstall_atoms {
                println!("      atom: {atom}");
            }
        }
        print!("\n\n");

        println!("slot operator dependencies:");
        // Group abi-rebuild pairs by the provider's `cp:slot` slot-atom.
        let mut by_slot_atom: std::collections::BTreeMap<String, Vec<&(String, String)>> =
            std::collections::BTreeMap::new();
        for pair in &result.abi_rebuilds {
            let slot_atom = match by_cpv.get(&pair.0) {
                Some((cp, slot, ..)) => format!("{cp}:{slot}"),
                None => pair.0.clone(),
            };
            by_slot_atom.entry(slot_atom).or_default().push(pair);
        }
        for (slot_atom, pairs) in &by_slot_atom {
            println!("   ({root_str}, {slot_atom})");
            for (provider, consumer) in pairs {
                println!("      parent: {}", label(consumer));
                println!("        child: {} (runtime_slot_op)", label(provider));
            }
        }
        print!("\n\n");

        println!("forced rebuilds:");
        let mut by_child: std::collections::BTreeMap<&str, Vec<&str>> =
            std::collections::BTreeMap::new();
        for (provider, consumer) in &result.abi_rebuilds {
            by_child.entry(provider).or_default().push(consumer);
        }
        if !by_child.is_empty() {
            println!("   root: {root_str}");
            for (child, parents) in &by_child {
                println!("      child: {}", label(child));
                for parent in parents {
                    println!("         parent: {}", label(parent));
                }
            }
        }
        print!("\n\n");
    }

    // Real `_show_abi_rebuild_info` (`depgraph.py:1210`), gated on
    // `--verbose-slot-rebuilds != "n"` (default on, NOT `--verbose`) and
    // rendered right after the merge list / autounmask blocks, before the
    // changed-deps report -- `_forced_rebuilds[root][child] = {parents}`,
    // grouped by the provider whose new sub-slot broke each consumer's
    // built `:=` link. `writemsg_stdout`, so stdout. Real prints each
    // package as `str(Package)` (`Package.__str__`):
    // `(cpv:slot/sub_slot::repo, ebuild scheduled for merge[ to '<root>'])`.
    if verbose_slot_rebuilds && !result.abi_rebuilds.is_empty() {
        // cpv -> (slot, sub_slot, repo, type_name) for every merge-bound
        // entry, so both sides of each edge render at their *resolved*
        // slot (the rebuild's new sub-slot, not the vdb's).
        let mut by_cpv: HashMap<String, (String, String, String, &str)> = HashMap::new();
        for e in entries {
            let version = match &e.outcome {
                portage_repo::PretendOutcome::New { version }
                | portage_repo::PretendOutcome::Reinstall { version, .. } => version.clone(),
                portage_repo::PretendOutcome::Upgrade { to, .. }
                | portage_repo::PretendOutcome::Downgrade { to, .. } => to.clone(),
                _ => continue,
            };
            let type_name = match e.source {
                portage_repo::CandidateSource::Binary => "binary",
                portage_repo::CandidateSource::Ebuild => "ebuild",
            };
            by_cpv.insert(
                format!("{}/{}-{version}", e.category, e.package),
                (
                    e.slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                    e.repo_name.clone().unwrap_or_default(),
                    type_name,
                ),
            );
        }
        let root_suffix = if root == Path::new("/") {
            String::new()
        } else {
            format!(" to '{}'", root.display())
        };
        let render = |cpv: &str| -> String {
            match by_cpv.get(cpv) {
                Some((slot, sub_slot, repo, type_name)) => format!(
                    "({cpv}:{slot}/{sub_slot}::{repo}, {type_name} scheduled for merge{root_suffix})"
                ),
                None => cpv.to_string(),
            }
        };
        println!();
        println!("The following packages are causing rebuilds:");
        println!();
        let mut provider: Option<&str> = None;
        for (child, parent) in &result.abi_rebuilds {
            if provider != Some(child.as_str()) {
                println!("  {} causes rebuilds for:", render(child));
                provider = Some(child.as_str());
            }
            println!("    {}", render(parent));
        }
    }

    // `--changed-deps-report`: real `_changed_deps_report`'s own WARN
    // block, ported verbatim (real portage colorizes it when the
    // terminal supports it; portuale, like every other message it
    // prints, stays plain text). Already empty unless `changed_deps_report`
    // was given AND `changed_deps` was NOT (see `resolve_pretend_graph`'s
    // own doc comment for that gating), so no extra condition needed
    // here beyond "is there anything to report at all".
    if !result.changed_deps_report.is_empty() {
        eprintln!();
        eprintln!("!!! Detected ebuild dependency change(s) without revision bump:");
        eprintln!();
        for c in &result.changed_deps_report {
            if root == Path::new("/") {
                eprintln!(
                    "    {}/{}-{}::{}",
                    c.category, c.package, c.version, c.repo_name
                );
            } else {
                eprintln!(
                    "    {}/{}-{}::{} for {}",
                    c.category,
                    c.package,
                    c.version,
                    c.repo_name,
                    root.display()
                );
            }
        }
        eprintln!();
        eprintln!("NOTE: Refer to the following page for more information about dependency");
        eprintln!("      change(s) without revision bump:");
        eprintln!();
        eprintln!("          https://wiki.gentoo.org/wiki/Project:Portage/Changed_dependencies");
        eprintln!();
        eprintln!("      In order to suppress reports about dependency changes, add");
        eprintln!("      --changed-deps-report=n to the EMERGE_DEFAULT_OPTS variable in");
        eprintln!("      '/etc/portage/make.conf'.");
        eprintln!();
        eprintln!("HINT: In order to avoid problems involving changed dependencies, use the");
        eprintln!("      --changed-deps option to automatically trigger rebuilds when changed");
        eprintln!("      dependencies are detected. Refer to the emerge man page for more");
        eprintln!("      information about this option.");
    }

    // Real depgraph.py's own display_problems(): shown *after* the merge
    // list above (real `_show_merge_list()` runs first), then the whole
    // action fails -- see GraphResult::buildpkgonly_deps_unsatisfied's
    // own doc comment for the exact real check this mirrors.
    if result.buildpkgonly_deps_unsatisfied {
        eprintln!("\n!!! --buildpkgonly requires all dependencies to be merged.");
        eprintln!("!!! Cannot merge requested packages. Merge deps and try again.\n");
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }

    // Real `_serialize_tasks` -> `_show_circular_deps` (`depgraph.py:
    // 10425`): an unbreakable dependency cycle among the merge-bound
    // packages -- every edge in it one real portage's `_ignore_runtime`
    // scan can't linearize (an unsatisfied `DEPEND`/`BDEPEND` with no
    // run-time alternative, or since backlog #228 an unsatisfied
    // slot-operator edge). Printed to stderr after the merge list, then
    // the whole action fails (exit 1), pretend or not -- real portage
    // never proceeds to build a cycle.
    //
    // Faithful transcription of `_show_circular_deps`'s `writemsg`
    // sequence: the reduced cycle-only re-display
    // (`self.display(handler.merge_list)`, printed above between the
    // merge list and this block -- flat portuale lines, since portuale's
    // tree model dedups shared nodes and never abandons the list) and
    // the `large_cycle_count` "a lot of cycles" trailer below (shown
    // only with a concrete suggestion, like real). The `Change USE:`
    // suggestion branch itself is shipped
    // (`portage_repo::circular_dep_solutions`, real
    // `circular_dependency_handler._find_suggestions`): fixtures
    // `usecyclea` (bare suggestion), `gpcyclec` (hard grandparent clash
    // disqualifies it), and `fucyclec` (conditional grandparent clash
    // keeps it with `followup_change`) pin all three outcomes.
    // The shortest cycle drives `_prepare_circular_dep_message`; every
    // line after `depends on` carries its own edge's `priorities[-1]`
    // label (backlog #228 -- `(buildtime)` on every line for the
    // all-buildtime cycles, `slopcyca` pins the mixed shape).
    // Slice 5 routing for the legacy site: gate-on aborts never reach
    // here with work left (a cycle abort printed above, before the
    // autounmask section; a masked/unsat abort prints no circular block
    // at all), so this serves gate-off and Complete resolves only — the
    // pre-Slice-5 print-and-exit-1 shape, byte-identical.
    let gated_abort = portage_repo::abort_path_enabled()
        && matches!(
            &result.outcome,
            portage_repo::ResolveOutcome::Aborted { .. }
        );
    if let Some(cycle) = result.circular_deps.first() {
        if !gated_abort {
            print_circular_block(
                cycle,
                &portage_repo::cycle_edge_labels(&result.entries, cycle),
                &color,
                &repos,
                &config,
                &result.autounmask_use_changes,
                &result.entries,
                result.large_cycle_count,
            );
        }
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
    }

    // Backlog #19 abort-path outcome mapping (real `_emerge/actions.py:
    // 460-462`: `not success` -> `display_problems()`, `return 1` — the
    // merge list `display()` is never reached, so an aborted resolve
    // shows at most the partial list and always exits 1). Gated on
    // `portage_repo::abort_path_enabled` (`PORTUALE_ABORT_PATH=0` keeps
    // the legacy "report, don't enforce" list + exit 0 — Gate G0.4).
    // Since Slice 3 the resolver produces `Aborted` (see
    // `portage_repo::abort_outcome`), so this fires for a masked-only or
    // otherwise unsatisfiable dependency of a merge-bound package and for
    // an unserializable cycle; it sits *after* the circular block above
    // because real's `display_problems()` prints `_show_circular_deps`
    // first (`depgraph.py:11113`) and only then returns 1. Slice 4 moves
    // the partial-list rendering here. `mrg` needs no separate wiring:
    // `to_emerge_argv` hands its argv to this same `pretend::run`
    // (`mrg.rs`), so resolution output and exit codes are literally
    // `emerge`'s by construction.
    if portage_repo::abort_path_enabled()
        && let portage_repo::ResolveOutcome::Aborted { .. } = &result.outcome
    {
        pretend_end_news_notice(pretend, &repos, &root, &config, &color);
        return ExitCode::from(1);
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
