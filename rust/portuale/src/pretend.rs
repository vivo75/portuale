// `emerge --pretend <category/package>`: the v1 slice (see
// PORTING/rust/portage-repo/src/lib.rs for the full scope writeup --
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
// one -- this is real emerge's own default, not something this pilot
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
// machinery even runs. It reports (never writes, requires --pretend,
// same "never merges" invariant as everything else in this pilot) which
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
// `argument_options` with an optional y/n value, the same shape
// `--verbose`/`-v` already has: a bare `--deselect`/`-W` or `--deselect
// y` enables it, `--deselect n` explicitly disables it (falling through
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
// this file, there's no real behavior to port. Built as a pilot-specific
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
// see `json_escape`'s own doc comment for why. The Python reference
// mirrors this output byte-for-byte (verified directly, not just
// structurally-equal-as-JSON), via its own hand-rolled
// `_json_escape`/`_entry_to_json`/`_print_json`, the same "two
// independent implementations building the identical string via the
// identical algorithm" approach this pilot uses everywhere else, rather
// than two different JSON libraries that merely happen to agree.
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
// implemented in this pilot" message, distinct from a genuinely unknown
// flag ("unrecognized option"). This lets real-world invocations that
// happen to include options this pilot doesn't implement (e.g. from
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
// recycling stack, regardless of position), this pilot processes
// strictly left to right and reports on the first character that's
// either unimplemented-but-recognized or genuinely unrecognized, exiting
// immediately -- exactly the same two outcomes (and same messages) a
// standalone occurrence of that character would produce. This is an
// intentional simplification of the *processing order*, not the
// *outcome*: since this pilot exits at the first out-of-scope input
// either way, which internal algorithm finds it first is unobservable
// except in the rare case of two DIFFERENT unimplemented flags bundled
// together, where this pilot always reports the leftmost one.
//
// --help/-h is real and implemented, checked unconditionally before
// anything else in `args` -- matching real emerge's own behavior
// (`main.py`'s `parse_opts` maps `-h`/`--help` to the "help" action,
// which `main()` special-cases: `if myaction == "help": emerge_help();
// return os.EX_OK` -- checked once, after the *whole* line has already
// parsed successfully, so it wins regardless of where in argv it
// appears or what other real-but-unimplemented flags accompany it).
// This pilot's own scan is a documented simplification of that: it
// checks every token (including each character of a short-flag bundle)
// for a literal `--help`/`-h`/`h` match unconditionally, rather than
// first confirming the rest of the line would even parse -- so
// `emerge --help --this-is-not-a-real-flag-at-all` prints help here,
// where real emerge would report a parse error instead, since
// `--this-is-not-a-real-flag-at-all` would never successfully reach
// argparse's post-parse action dispatch at all. The help text itself is
// NOT a port of real emerge's own `_emerge/help.py` (157 lines of
// colorized usage syntax for its full ~130-flag surface, most of which
// this pilot doesn't implement -- reproducing it here would be actively
// misleading) -- it's a short, honest, pilot-specific summary of what
// this pilot actually supports, ending with a pointer to
// PORTING/README.md and PORTING/PROMPT.md for the rest.

use crate::ebuild_package;
use crate::emerge_build;
use crate::emerge_options;
use portage_dep::{match_from_list, parse_atom, Atom, Blocker};
use portage_repo::{
    config_root_from_env, resolve_pretend_graph, root_from_env, ChangedDepsReportEntry, GraphEntry,
    PretendOutcome, SlotConflict,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

/// Prints one `[blocks]` line per conflict recorded on `entry` (see
/// `portage_repo::BlockerConflict`), right after its own `[ebuild ...]`
/// line -- purely informational, matching real `--pretend`'s "show what
/// would happen" spirit: v1 neither refuses nor changes the exit code for
/// a blocker match, strong or weak (see resolve_pretend_graph's doc
/// comment).
/// `  USE="flag1 -flag2"` (two leading spaces, matching real `--pretend
/// -v`'s own line format), or an empty string when `--verbose` wasn't
/// given or this entry has no IUSE-declared flags at all -- see
/// `GraphEntry::use_flags_display`'s doc comment. Real portage's own USE
/// display additionally colorizes and diffs against the previously
/// installed version's IUSE (`*`/`%` markers) and groups by USE_EXPAND;
/// this pilot shows none of that, just the plain enabled/disabled set,
/// alphabetically sorted.
/// Real `output_helpers.py`'s own `columnwidth` resolution
/// (`MergeListItem.__init__`): 130 by default, overridden by a
/// `COLUMNWIDTH` setting -- this pilot only ever reads it as a plain
/// environment variable (real portage's own `frozen_config.settings` is
/// env + `make.conf` + profile merged together; parsing `COLUMNWIDTH`
/// out of `make.conf` too would need a new generic scalar-lookup path
/// through `portage_profile::Config`, which nothing else in this pilot
/// needs yet -- a deliberate v1 narrowing, same spirit as every other
/// scope cut in this codebase). An unparsable value warns and falls back
/// to the default, exactly like real portage's own `except ValueError`
/// branch, rather than treating it as a hard error. Real portage's own
/// warning has a first line echoing the raw exception text
/// (`f"!!! {e!s}\n"`) -- omitted here, same as every other parse-error
/// message in this pilot (see `--deep`'s own invalid-value handling):
/// Rust's `ParseIntError` and Python's `ValueError` never stringify
/// identically, so echoing either verbatim would make this the one
/// message the two implementations could never agree on byte-for-byte.
fn columnwidth_from_env() -> i64 {
    match std::env::var("COLUMNWIDTH") {
        Ok(value) => match value.parse::<i64>() {
            Ok(width) => width,
            Err(_) => {
                eprintln!("!!! Unable to parse COLUMNWIDTH={value:?}");
                130
            }
        },
        Err(_) => 130,
    }
}

/// One `--columns` line: real `_set_root_columns`'s own layout algorithm
/// (the `pkg_info.merge == True` branch only -- see this function's own
/// call sites' doc comments for why the "not merging" branch never
/// applies to any outcome this pilot prints in brackets at all), with
/// color stripped (this pilot has no ANSI color output anywhere, so
/// real's `nc_len`/plain `len()` distinction collapses to just `len()`).
/// `bracket`/`code` reproduce the exact same `"[{bracket}  {code}]"`
/// segment the non-columns format already prints unchanged -- only what
/// comes after it differs: `category/package` (no version -- that's the
/// whole point of `--columns`) padded out to `columnwidth - 60`
/// (`newlp`), then `[version]` right-padded to `columnwidth - 30`
/// (`oldlp`), then `oldbest` (`"[from]"` for an `Upgrade`/`Downgrade`,
/// empty otherwise -- real `pkg_info.oldbest_list`, mirrored here via
/// data this pilot already has rather than a new installed-candidate
/// lookup). Padding is skipped once the line's already past the target
/// width, exactly like real portage's own `if (newlp - nc_len(myprint))
/// > 0` guard -- never truncates, just doesn't pad further.
#[allow(clippy::too_many_arguments)]
fn columns_line(
    bracket: &str,
    code: &str,
    indent: &str,
    category: &str,
    package: &str,
    version: &str,
    oldbest: &str,
    columnwidth: i64,
) -> String {
    let newlp = (columnwidth - 60).max(0) as usize;
    let oldlp = (columnwidth - 30).max(0) as usize;
    let mut line = format!("[{bracket}  {code}] {indent}{category}/{package}");
    if newlp > line.len() {
        line.push_str(&" ".repeat(newlp - line.len()));
    }
    line.push_str(&format!(" [{version}] "));
    if oldlp > line.len() {
        line.push_str(&" ".repeat(oldlp - line.len()));
    }
    line.push_str(oldbest);
    line
}

fn use_suffix(entry: &GraphEntry, verbose: bool) -> String {
    if !verbose || entry.use_flags_display.is_empty() {
        return String::new();
    }
    let flags: Vec<String> = entry
        .use_flags_display
        .iter()
        .map(|(flag, enabled)| {
            if *enabled {
                flag.clone()
            } else {
                format!("-{flag}")
            }
        })
        .collect();
    format!("  USE=\"{}\"", flags.join(" "))
}

/// The `(reinstall for ...)` note's own reason text, real portage
/// treating `--newuse`/`--changed-use`, `--changed-deps`,
/// `--changed-slot`, `--rebuilt-binaries`, and `--newrepo` as
/// independent, freely-combinable triggers (see
/// `PretendOutcome::Reinstall`'s own doc comment, portage-repo).
/// Pilot-invented wording, same as the pre-existing "changed USE: ..."
/// text -- real portage's own default `--pretend` output shows no such
/// itemized reason at all. Returns `None` when all five fields are
/// empty/false -- real portage's own bare, reasonless `[ebuild R]` (see
/// `resolve_pretend`'s own `selective`/`is_top_level` doc comment
/// paragraph, portage-repo): unlike every other `Reinstall`, this one
/// genuinely has no tracked reason to report at all, so the caller omits
/// the whole `(reinstall for ...)` parenthetical rather than printing an
/// empty one.
fn reinstall_reason(
    changed_flags: &[String],
    deps_changed: bool,
    slot_changed: bool,
    rebuilt_binary: bool,
    new_repo: bool,
) -> Option<String> {
    let mut reasons = Vec::new();
    if !changed_flags.is_empty() {
        reasons.push(format!("changed USE: {}", changed_flags.join(", ")));
    }
    if deps_changed {
        reasons.push("changed dependencies".to_string());
    }
    if slot_changed {
        reasons.push("changed slot".to_string());
    }
    if rebuilt_binary {
        reasons.push("rebuilt binary".to_string());
    }
    if new_repo {
        reasons.push("new repository".to_string());
    }
    if reasons.is_empty() {
        return None;
    }
    Some(reasons.join("; "))
}

fn print_blockers(entry: &GraphEntry, owner_version: &str) {
    for b in &entry.blockers {
        let strength = if b.strong { "hard" } else { "soft" };
        println!(
            "[blocks] {}/{}-{owner_version} {strength} blocks {}/{}-{} (\"{}\")",
            entry.category,
            entry.package,
            b.matched_category,
            b.matched_package,
            b.matched_version,
            b.atom_str
        );
    }
}

/// One `GraphEntry`'s own display line, `indent` prepended right before
/// the category/package text (empty for flat mode, `print_tree`'s own
/// growing prefix for `--tree`) -- the exact same per-outcome
/// bracket/reason logic the flat loop always had, just factored out so
/// both display modes share one implementation rather than drifting
/// apart. `onlydeps`/`top_level_pkgs` decide suppression exactly as
/// before: a directly-requested top-level atom's own line (not its
/// dependencies) is hidden under `--onlydeps`, whatever its outcome.
/// `columns`/`columnwidth` switch the New/Upgrade/Downgrade/Reinstall
/// arms below to `columns_line`'s own layout instead of the default
/// inline `"...-version (upgrade from X)"` format -- see its own doc
/// comment. Never both `columns` and a non-empty `indent` at once: the
/// CLI layer refuses `--tree`+`--columns` together (the only source of a
/// non-empty `indent`), so `columns_line`'s own `indent` parameter is
/// always `""` in practice here, still threaded through for symmetry
/// with the non-columns arms.
#[allow(clippy::too_many_arguments)]
fn print_entry_line(
    entry: &GraphEntry,
    indent: &str,
    top_level_pkgs: &HashSet<(String, String)>,
    onlydeps: bool,
    verbose: bool,
    columns: bool,
    columnwidth: i64,
) {
    let onlydeps_suppressed =
        onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    // Real --pretend's own bracket word: literally `pkg.type_name`
    // (`lib/_emerge/RootConfig.py`'s own `pkg_tree_map`, the exact
    // two strings `"ebuild"`/`"binary"` this pilot's own
    // `CandidateSource` mirrors) -- a binary merge prints
    // `"[binary"`, never `"[ebuild"`, regardless of outcome.
    let bracket = match entry.source {
        portage_repo::CandidateSource::Binary => "binary",
        portage_repo::CandidateSource::Ebuild => "ebuild",
    };
    match &entry.outcome {
        PretendOutcome::New { version } => {
            if !onlydeps_suppressed {
                if columns {
                    println!(
                        "{}{}",
                        columns_line(
                            bracket,
                            "N",
                            indent,
                            &entry.category,
                            &entry.package,
                            version,
                            "",
                            columnwidth
                        ),
                        use_suffix(entry, verbose)
                    );
                } else {
                    println!(
                        "[{bracket}  N] {indent}{}/{}-{version}{}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
                }
            }
            print_blockers(entry, version);
        }
        PretendOutcome::Upgrade { from, to } => {
            if !onlydeps_suppressed {
                if columns {
                    println!(
                        "{}{}",
                        columns_line(
                            bracket,
                            "U",
                            indent,
                            &entry.category,
                            &entry.package,
                            to,
                            &format!("[{from}]"),
                            columnwidth
                        ),
                        use_suffix(entry, verbose)
                    );
                } else {
                    println!(
                        "[{bracket}  U] {indent}{}/{}-{to} (upgrade from {from}){}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
                }
            }
            print_blockers(entry, to);
        }
        PretendOutcome::Downgrade { from, to } => {
            if !onlydeps_suppressed {
                if columns {
                    println!(
                        "{}{}",
                        columns_line(
                            bracket,
                            "D",
                            indent,
                            &entry.category,
                            &entry.package,
                            to,
                            &format!("[{from}]"),
                            columnwidth
                        ),
                        use_suffix(entry, verbose)
                    );
                } else {
                    println!(
                        "[{bracket}  D] {indent}{}/{}-{to} (downgrade from {from}){}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
                }
            }
            print_blockers(entry, to);
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            deps_changed,
            slot_changed,
            rebuilt_binary,
            new_repo,
        } => {
            if !onlydeps_suppressed {
                if columns {
                    println!(
                        "{}{}",
                        columns_line(
                            bracket,
                            "r",
                            indent,
                            &entry.category,
                            &entry.package,
                            version,
                            "",
                            columnwidth
                        ),
                        use_suffix(entry, verbose)
                    );
                } else {
                    match reinstall_reason(
                        changed_flags,
                        *deps_changed,
                        *slot_changed,
                        *rebuilt_binary,
                        *new_repo,
                    ) {
                        Some(reason) => println!(
                            "[{bracket}  r] {indent}{}/{}-{version} (reinstall for {reason}){}",
                            entry.category,
                            entry.package,
                            use_suffix(entry, verbose)
                        ),
                        None => println!(
                            "[{bracket}  r] {indent}{}/{}-{version}{}",
                            entry.category,
                            entry.package,
                            use_suffix(entry, verbose)
                        ),
                    }
                }
            }
            print_blockers(entry, version);
        }
        PretendOutcome::AlreadyInstalled { version } => {
            // Already-satisfied dependencies aren't shown, matching
            // real emerge's usual "don't clutter the list with what's
            // already there" behavior -- only a directly-requested
            // (top-level) atom gets its own "nothing to do" line, and
            // --onlydeps suppresses that too, same as every other
            // outcome above.
            if top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()))
                && !onlydeps_suppressed
            {
                println!(
                    "{indent}{}/{}-{version} is already installed; nothing to do",
                    entry.category, entry.package
                );
            }
        }
        PretendOutcome::NoVisibleCandidate => {
            eprintln!(
                "!!! no visible ebuild for dependency \"{}/{}\"",
                entry.category, entry.package
            );
            // `--autounmask`'s own keyword-suggestion sub-feature,
            // extended to a dependency's own NoVisibleCandidate -- see
            // GraphEntry::keyword_suggestion's own doc comment.
            // Previously only a top-level atom's own fatal
            // NoVisibleCandidate got this note (as part of the Err
            // message that aborts the whole call).
            if let Some((version, keyword)) = &entry.keyword_suggestion {
                eprintln!(
                    "!!! note: {}/{}-{version} exists but is masked by KEYWORDS; \
                     --autounmask-keep-keywords=n suggests adding \"{}/{} {keyword}\" \
                     to package.accept_keywords",
                    entry.category, entry.package, entry.category, entry.package,
                );
            }
            // `--autounmask-use`'s own suggestion sub-feature -- see
            // GraphEntry::use_suggestion's own doc comment.
            if let Some((version, flip)) = &entry.use_suggestion {
                let adjustments: Vec<String> = flip
                    .iter()
                    .map(|(flag, enabled)| {
                        if *enabled {
                            flag.clone()
                        } else {
                            format!("-{flag}")
                        }
                    })
                    .collect();
                eprintln!(
                    "!!! note: {}/{}-{version} exists but its USE flags don't satisfy \
                     this atom; --autounmask-use suggests adding \"={}/{}-{version} {}\" \
                     to package.use",
                    entry.category,
                    entry.package,
                    entry.category,
                    entry.package,
                    adjustments.join(" "),
                );
            }
            // `--autounmask-use`'s own second, architecturally distinct
            // suggestion sub-feature -- see
            // GraphEntry::parent_use_suggestion's own doc comment: flips
            // the *requesting parent's* own flag, not the candidate's.
            if let Some((parent_category, parent_package, parent_version, flip)) =
                &entry.parent_use_suggestion
            {
                let adjustments: Vec<String> = flip
                    .iter()
                    .map(|(flag, enabled)| {
                        if *enabled {
                            flag.clone()
                        } else {
                            format!("-{flag}")
                        }
                    })
                    .collect();
                eprintln!(
                    "!!! note: {parent_category}/{parent_package}-{parent_version}'s own USE \
                     flags need to change to satisfy this dependency; --autounmask-use \
                     suggests adding \"={parent_category}/{parent_package}-{parent_version} {}\" \
                     to package.use",
                    adjustments.join(" "),
                );
            }
        }
    }
}

/// `--tree`/`-t`: indents each entry under whichever other entry's own
/// dependency string reached it, real `output_helpers.py`'s own
/// `_tree_display` -- but not a faithful port of it. Real
/// `_ordered_tree_display` walks a genuine topologically-*scheduled*
/// merge order (`mylist`) and a real bidirectional digraph
/// (`parent_nodes`/`child_nodes`) to decide, for each node, exactly
/// which already-placed node to nest it under (including cycle-avoiding
/// parent-chasing when a fresh top-level branch needs to attach
/// somewhere) -- machinery this pilot has no equivalent of at all (no
/// merge scheduler exists, see task #55's own "real merge/install"
/// scope boundary), so this is a deliberate, pilot-specific
/// simplification instead, confirmed acceptable in place of a faithful
/// port given that boundary.
///
/// The only edges this pilot has are `GraphEntry::required_by` (already
/// "every distinct owner, sorted" -- see its own doc comment,
/// portage-repo); this function inverts that into a `children` map
/// (owner key -> the entries it pulled in) and walks it from the
/// top-level/requested entries as roots, in their own `entries` order
/// (already argv order, per `resolve_pretend_graph`'s "level-order
/// guarantee"). A node already rendered once (anywhere in the tree,
/// diamond dependencies included) is never rendered or recursed into
/// again -- real `_unordered_tree_display`'s own `seen_nodes` behavior,
/// ported exactly (and, as a side effect, what keeps this recursion
/// from looping forever on a genuine dependency cycle). Since
/// `required_by` only ever tracks `(category, package)`, not slot, a
/// multi-slot package's own dependents can't be disambiguated between
/// its slot-entries any more precisely than `required_by`/`--json`
/// already can't -- an existing imprecision, not a new one.
///
/// `unordered_display` (`--unordered-display`, only ever meaningful
/// together with `--tree` -- real portage's own `_tree_display` is
/// never even called otherwise, and this pilot mirrors that: given
/// alone it's accepted but does nothing) chooses the child order at
/// each level: `entries`' own natural (BFS discovery) order when true
/// -- genuinely "not sorted", using real already-existing data, no
/// invented bookkeeping -- versus alphabetical-by-`(category, package)`
/// when false, this pilot's own deterministic stand-in for real
/// portage's genuine merge-order sort (which would need the scheduler
/// this pilot doesn't have). Any entry never reached from a root at all
/// (shouldn't normally happen -- every non-root entry's own
/// `required_by` should trace back to one) is still printed, unindented,
/// after the tree itself, rather than silently dropped -- this pilot's
/// own "never silently lose information" invariant, seen already for
/// slot conflicts and unresolvable dependencies.
fn print_tree(
    entries: &[GraphEntry],
    top_level_pkgs: &HashSet<(String, String)>,
    onlydeps: bool,
    unordered_display: bool,
    verbose: bool,
) {
    let mut children: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for owner in &entry.required_by {
            children.entry(owner.clone()).or_default().push(i);
        }
    }
    if !unordered_display {
        for kids in children.values_mut() {
            kids.sort_by(|&a, &b| {
                (&entries[a].category, &entries[a].package)
                    .cmp(&(&entries[b].category, &entries[b].package))
            });
        }
    }

    // Bundles print_tree's own mostly-invariant parameters together
    // purely to keep render's own recursive calls readable (7+
    // positional args tripped clippy::too_many_arguments) -- not a
    // reusable abstraction, just this one function's own recursion
    // state.
    struct TreeCtx<'a> {
        entries: &'a [GraphEntry],
        children: &'a HashMap<(String, String), Vec<usize>>,
        top_level_pkgs: &'a HashSet<(String, String)>,
        onlydeps: bool,
        verbose: bool,
    }

    fn render(i: usize, depth: u32, ctx: &TreeCtx, rendered: &mut HashSet<usize>) {
        if !rendered.insert(i) {
            return;
        }
        let indent = "  ".repeat(depth as usize);
        // `columns` is always false here -- the CLI layer refuses
        // --tree+--columns together, so print_tree only ever runs with
        // --columns off; `columnwidth` is a dummy value, unused whenever
        // `columns` is false.
        print_entry_line(
            &ctx.entries[i],
            &indent,
            ctx.top_level_pkgs,
            ctx.onlydeps,
            ctx.verbose,
            false,
            130,
        );
        let key = (
            ctx.entries[i].category.clone(),
            ctx.entries[i].package.clone(),
        );
        if let Some(kids) = ctx.children.get(&key) {
            for &child in kids {
                render(child, depth + 1, ctx, rendered);
            }
        }
    }

    let ctx = TreeCtx {
        entries,
        children: &children,
        top_level_pkgs,
        onlydeps,
        verbose,
    };
    let mut rendered: HashSet<usize> = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        if top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone())) {
            render(i, 0, &ctx, &mut rendered);
        }
    }

    // Safety net, not expected to ever trigger in practice (see this
    // function's own doc comment) -- prints anything the tree walk
    // somehow never reached, flat, rather than silently dropping it.
    for (i, entry) in entries.iter().enumerate() {
        if !rendered.contains(&i) {
            print_entry_line(entry, "", top_level_pkgs, onlydeps, verbose, false, 130);
        }
    }
}

/// Escapes `s` for embedding in a JSON string literal (quote, backslash,
/// and control characters -- category/package/version/atom text from
/// this pilot's own inputs never needs anything fancier). Hand-rolled
/// rather than pulling in a JSON crate: `--json`'s own output is a
/// small, flat shape, and this pilot has no other dependency beyond
/// `regex` anywhere in the workspace -- see the module doc comment for
/// why `--json` exists at all (it's NOT a port of any real emerge
/// behavior).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_string(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

/// One JSON object per `GraphEntry` -- a structured mirror of the plain-
/// text `[ebuild ...]`/`[binary ...]`/"already installed"/blocker lines
/// above, plus two fields no plain-text line carries at all: `requested`
/// (was this exact category/package one of `atoms` directly, as opposed
/// to reached only via a dependency string) and `required_by` (which
/// package(s), if any, pulled it in that way -- see
/// `GraphEntry::required_by`'s own doc comment, portage-repo). `source`
/// mirrors `entry.source`/the plain-text loop's own `bracket` variable
/// below (`"binary"`/`"ebuild"`, real `RootConfig.py`'s own
/// `pkg_tree_map`-driven `type_name`) -- until the binary-package slice
/// (`--usepkg`/`--usepkgonly`, `portage-repo`) this was always
/// `"ebuild"` unconditionally; it no longer is.
/// Deliberately NOT affected by `--onlydeps`'s own suppression (a
/// display-only concern for the plain-text loop below): `--json` always
/// dumps the whole resolved graph, letting a consumer filter on
/// `requested` itself if they want the `--onlydeps` view. `provenance`
/// (alongside `source`, so also absent for `NoVisibleCandidate`) mirrors
/// `entry.provenance`/`VisibilityProvenance` (portage-repo) directly --
/// this pilot's own state-change trace: which `package.mask`/`.unmask`/
/// `package.accept_keywords` entries, if any, were actually load-bearing
/// for this candidate to be visible at all. Each of its three fields is
/// `null` rather than omitted when not applicable, unlike `use_flags`
/// above -- there's no `verbose` gate here (this is exactly the kind of
/// detail `--json` exists to expose unconditionally, see this module's
/// own doc comment) and a consumer scripting against this output
/// shouldn't have to branch on whether the key is even present.
/// `keyword_suggestion` is `provenance`'s own mirror image -- present
/// (as `{"version", "keyword"}` or `null`) only for `NoVisibleCandidate`
/// entries, since that's the one outcome with nothing visible to trace
/// provenance for and something to suggest instead. Mirrors
/// `entry.keyword_suggestion` (portage-repo) -- see its own doc comment.
fn entry_to_json(
    entry: &GraphEntry,
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
) -> String {
    let requested = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let mut fields: Vec<String> = vec![
        format!("\"category\":{}", json_string(&entry.category)),
        format!("\"package\":{}", json_string(&entry.package)),
    ];
    let outcome_tag = match &entry.outcome {
        PretendOutcome::New { .. } => "new",
        PretendOutcome::Upgrade { .. } => "upgrade",
        PretendOutcome::Downgrade { .. } => "downgrade",
        PretendOutcome::Reinstall { .. } => "reinstall",
        PretendOutcome::AlreadyInstalled { .. } => "already_installed",
        PretendOutcome::NoVisibleCandidate => "no_visible_candidate",
    };
    fields.push(format!("\"outcome\":{}", json_string(outcome_tag)));
    match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::AlreadyInstalled { version } => {
            fields.push(format!("\"version\":{}", json_string(version)));
        }
        PretendOutcome::Upgrade { from, to } | PretendOutcome::Downgrade { from, to } => {
            fields.push(format!("\"version\":{}", json_string(to)));
            fields.push(format!("\"from_version\":{}", json_string(from)));
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            deps_changed,
            slot_changed,
            rebuilt_binary,
            new_repo,
        } => {
            fields.push(format!("\"version\":{}", json_string(version)));
            let changed_use: Vec<String> = changed_flags.iter().map(|f| json_string(f)).collect();
            fields.push(format!("\"changed_use\":[{}]", changed_use.join(",")));
            fields.push(format!("\"changed_deps\":{deps_changed}"));
            fields.push(format!("\"changed_slot\":{slot_changed}"));
            fields.push(format!("\"rebuilt_binary\":{rebuilt_binary}"));
            fields.push(format!("\"new_repo\":{new_repo}"));
        }
        PretendOutcome::NoVisibleCandidate => {}
    }
    fields.push(format!(
        "\"slot\":{}",
        entry
            .slot
            .as_deref()
            .map(json_string)
            .unwrap_or_else(|| "null".to_string())
    ));
    if !matches!(entry.outcome, PretendOutcome::NoVisibleCandidate) {
        let source_tag = match entry.source {
            portage_repo::CandidateSource::Binary => "binary",
            portage_repo::CandidateSource::Ebuild => "ebuild",
        };
        fields.push(format!("\"source\":{}", json_string(source_tag)));
        let opt_str = |v: &Option<String>| {
            v.as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_string())
        };
        fields.push(format!(
            "\"provenance\":{{\"mask_entry\":{},\"unmask_entry\":{},\"keyword_entry\":{}}}",
            opt_str(&entry.provenance.mask_entry),
            opt_str(&entry.provenance.unmask_entry),
            opt_str(&entry.provenance.keyword_entry),
        ));
    } else {
        fields.push(format!(
            "\"keyword_suggestion\":{}",
            entry
                .keyword_suggestion
                .as_ref()
                .map(|(version, keyword)| format!(
                    "{{\"version\":{},\"keyword\":{}}}",
                    json_string(version),
                    json_string(keyword)
                ))
                .unwrap_or_else(|| "null".to_string())
        ));
        fields.push(format!(
            "\"use_suggestion\":{}",
            entry
                .use_suggestion
                .as_ref()
                .map(|(version, flip)| {
                    let flags: Vec<String> = flip
                        .iter()
                        .map(|(flag, enabled)| {
                            format!("{{\"flag\":{},\"enabled\":{enabled}}}", json_string(flag))
                        })
                        .collect();
                    format!(
                        "{{\"version\":{},\"flags\":[{}]}}",
                        json_string(version),
                        flags.join(",")
                    )
                })
                .unwrap_or_else(|| "null".to_string())
        ));
        fields.push(format!(
            "\"parent_use_suggestion\":{}",
            entry
                .parent_use_suggestion
                .as_ref()
                .map(|(parent_category, parent_package, parent_version, flip)| {
                    let flags: Vec<String> = flip
                        .iter()
                        .map(|(flag, enabled)| {
                            format!("{{\"flag\":{},\"enabled\":{enabled}}}", json_string(flag))
                        })
                        .collect();
                    format!(
                        "{{\"category\":{},\"package\":{},\"version\":{},\"flags\":[{}]}}",
                        json_string(parent_category),
                        json_string(parent_package),
                        json_string(parent_version),
                        flags.join(",")
                    )
                })
                .unwrap_or_else(|| "null".to_string())
        ));
    }
    fields.push(format!("\"requested\":{requested}"));
    let required_by: Vec<String> = entry
        .required_by
        .iter()
        .map(|(category, package)| {
            format!(
                "{{\"category\":{},\"package\":{}}}",
                json_string(category),
                json_string(package)
            )
        })
        .collect();
    fields.push(format!("\"required_by\":[{}]", required_by.join(",")));
    if verbose && !entry.use_flags_display.is_empty() {
        let use_flags: Vec<String> = entry
            .use_flags_display
            .iter()
            .map(|(flag, enabled)| format!("{}:{enabled}", json_string(flag)))
            .collect();
        fields.push(format!("\"use_flags\":{{{}}}", use_flags.join(",")));
    }
    let blockers: Vec<String> = entry
        .blockers
        .iter()
        .map(|b| {
            format!(
                "{{\"atom\":{},\"strong\":{},\"matched_category\":{},\"matched_package\":{},\"matched_version\":{}}}",
                json_string(&b.atom_str),
                b.strong,
                json_string(&b.matched_category),
                json_string(&b.matched_package),
                json_string(&b.matched_version)
            )
        })
        .collect();
    fields.push(format!("\"blockers\":[{}]", blockers.join(",")));
    format!("{{{}}}", fields.join(","))
}

fn slot_conflict_to_json(c: &SlotConflict) -> String {
    format!(
        "{{\"category\":{},\"package\":{},\"slot\":{},\"resolved_version\":{},\"conflicting_atom\":{}}}",
        json_string(&c.category),
        json_string(&c.package),
        json_string(&c.slot),
        json_string(&c.resolved_version),
        json_string(&c.conflicting_atom)
    )
}

fn changed_deps_report_entry_to_json(c: &ChangedDepsReportEntry) -> String {
    format!(
        "{{\"category\":{},\"package\":{},\"version\":{},\"repo_name\":{}}}",
        json_string(&c.category),
        json_string(&c.package),
        json_string(&c.version),
        json_string(&c.repo_name)
    )
}

/// The whole `--json` output: `{"entries": [...], "slot_conflicts": [...]}`,
/// one line, no pretty-printing (a pilot-specific convenience format, not
/// a stable schema -- see the module doc comment).
fn print_json(
    entries: &[GraphEntry],
    slot_conflicts: &[SlotConflict],
    changed_deps_report: &[ChangedDepsReportEntry],
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
) {
    let entries_json: Vec<String> = entries
        .iter()
        .map(|e| entry_to_json(e, top_level_pkgs, verbose))
        .collect();
    let conflicts_json: Vec<String> = slot_conflicts.iter().map(slot_conflict_to_json).collect();
    let changed_deps_report_json: Vec<String> = changed_deps_report
        .iter()
        .map(changed_deps_report_entry_to_json)
        .collect();
    println!(
        "{{\"entries\":[{}],\"slot_conflicts\":[{}],\"changed_deps_report\":[{}]}}",
        entries_json.join(","),
        conflicts_json.join(","),
        changed_deps_report_json.join(",")
    );
}

/// Reports and returns the exit code for a single option/action token
/// ("-x" or "--long", never a positional atom) that isn't --pretend/-p,
/// --verbose/-v, --newuse/-N, --changed-use/-U, --nodeps/-O, or
/// --onlydeps/-o -- shared between a standalone token and one character
/// of a decomposed short-flag bundle, so both produce identical
/// messages for the same underlying flag.
fn report_option(token: &str) -> ExitCode {
    if let Some(found) = emerge_options::lookup(token) {
        // Reports and exits immediately, matching every other
        // out-of-scope-input case in this pilot -- so there's no need to
        // correctly skip over this option's own value token (see
        // emerge_options.rs's doc comment): nothing after this point is
        // ever looked at.
        let kind = if found.category == emerge_options::Category::Action {
            "action"
        } else {
            "option"
        };
        eprintln!(
            "emerge (pilot v1): {kind} {:?} is a real emerge {kind}, but is not \
             implemented in this pilot (only --pretend/-p, --verbose/-v, \
             --newuse/-N, --changed-use/-U, --nodeps/-O, --onlydeps/-o, \
             --update/-u, --deep/-D, --exclude/-X, --deselect/-W, \
             --with-bdeps, --with-bdeps-auto, --changed-deps, \
             --changed-deps-report, --changed-slot, --with-test-deps, \
             --noreplace/-n, --selective, and --help/-h are implemented \
             so far; see PROMPT.md)",
            found.canonical
        );
    } else {
        eprintln!("emerge: unrecognized option {token:?}");
    }
    ExitCode::from(2)
}

/// Whether `--help`/`-h` appears anywhere in `args`, including as one
/// character of a short-flag bundle -- see the module doc comment on why
/// this wins unconditionally, checked before anything else.
fn wants_help(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--help"
            || arg == "-h"
            || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('h'))
    })
}

/// A short, honest, pilot-specific summary -- not a port of real
/// emerge's own `_emerge/help.py` (see the module doc comment for why).
fn print_help() {
    println!("emerge (pilot v1): command-line interface to the Rust porting pilot");
    println!();
    println!("Usage:");
    println!("   emerge --pretend [--verbose] <atom> [<atom> ...]");
    println!("   emerge --help");
    println!();
    println!("Options:");
    println!("   -p, --pretend   required: the only real merge calculation this pilot implements");
    println!("   -v, --verbose   show USE=\"...\" on each [ebuild ...] line (optionally: -v y|n)");
    println!("   -N, --newuse    reinstall an already-installed package if its USE has changed");
    println!("   -U, --changed-use  like -N, but ignores newly added/removed IUSE flags entirely");
    println!("   -O, --nodeps    do not resolve or show any dependency, only the given atoms");
    println!(
        "   -o, --onlydeps  show only the given atoms' dependencies, not the atoms themselves"
    );
    println!(
        "   -u, --update    upgrade to a newer visible version even if the installed one satisfies the atom"
    );
    println!(
        "   -D, --deep[=N]  also recurse into an already-installed package's own dependencies (optionally, only N levels deep)"
    );
    println!(
        "   -X, --exclude ATOMS  leave any matching already-installed package as-is, and never install a matching new one (repeatable, space-separated)"
    );
    println!(
        "   -W, --deselect  a standalone action: report which world favorites ATOMS would remove (never writes; requires --pretend)"
    );
    println!(
        "       --with-bdeps y|n  include (y, the default) or skip (n) DEPEND/BDEPEND when --deep walks an already-installed package's own dependencies"
    );
    println!(
        "       --with-bdeps-auto y|n  changes the *default* --with-bdeps value (only when --with-bdeps itself isn't given) -- n makes it default to n instead of the real \"auto\" (y here)"
    );
    println!(
        "       --changed-deps[=y|n]  reinstall an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's"
    );
    println!(
        "       --changed-deps-report[=y|n]  report (without reinstalling) an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's; silent if --changed-deps is also given"
    );
    println!(
        "       --changed-slot[=y|n]  reinstall an already-installed package whose own vdb-recorded SLOT differs from the current ebuild's"
    );
    println!(
        "       --with-test-deps[=y|n]  also pull in a top-level atom's own test?-gated dependencies, if it has a \"test\" USE flag not already enabled"
    );
    println!(
        "   -n, --noreplace  a directly-named, already-installed, still-satisfying atom is left as-is (real portage's own default without this needs --update/--newuse/--changed-use/--changed-deps/--changed-slot/--selective to get the same result)"
    );
    println!(
        "       --selective[=y|n]  identical to --noreplace; \"n\" explicitly cancels it even if another flag above would otherwise set it"
    );
    println!("   -h, --help      show this message and exit");
    println!(
        "       --json      dump the whole resolved graph as one line of JSON instead \
         of the lines above (pilot-specific, not a real emerge option)"
    );
    println!();
    println!(
        "Every other real emerge option/action is recognized by name (see \
         lib/_emerge/main.py) but not implemented -- using one reports which \
         option or action it is, instead of a generic error."
    );
    println!("See PORTING/README.md and PORTING/PROMPT.md for this pilot's current scope.");
}

/// Reads `<root>/var/lib/portage/world` (real portage's own `WORLD_FILE`
/// -- `lib/portage/const.py`) into a list of atom strings, one per line,
/// with the same comment/blank-line handling every other config file
/// this pilot already reads uses. A missing file is not an error -- an
/// empty, or never-yet-created, world is a real, valid state (e.g. a
/// fresh `ROOT`), not a mistake.
///
/// Only plain atom lines are read, via a leading `@` check -- this is
/// real, not a simplification: real `WorldSelectedPackagesSet`'s own
/// `ItemFileLoader` validates each line with a plain `isvalidatom`
/// (`lib/portage/env/validators.py`'s own `ValidAtomValidator`, no `@`
/// bypass), so a `@`-prefixed line in *this* file specifically really
/// would just fail validation and be dropped in real portage too. A
/// nested `@some-set` reference lives in a genuinely separate file --
/// see `read_world_sets`'s own doc comment for the other half of real
/// `@world`'s union (`WorldSelectedSet` in `lib/portage/_sets/files.py`).
/// `@system` (the profile's own `packages` file -- see
/// `portage_profile::Config::system_packages`) is a separate, different
/// mechanism with its own expansion in `run` below, not handled by this
/// function at all. Only the literal token `@world` triggers *this*
/// expansion -- `@some-set`, `@another-random-name`, etc. as a top-level
/// target fall through to the normal atom-parsing path and get a clear
/// "invalid atom" error, not a silent no-op.
fn read_world_atoms(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join("var/lib/portage/world");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .map(String::from)
        .collect())
}

/// Reads `<root>/var/lib/portage/world_sets` (real portage's own
/// `WORLD_SETS_FILE` -- `lib/portage/const.py`), a file genuinely
/// SEPARATE from the world file above, listing every `@name` set
/// reference the user has directly selected (e.g. via a prior `emerge
/// --noreplace @some-set`) -- real `WorldSelectedSetsSet`, whose own
/// validator (`lib/portage/_sets/files.py`) just checks each line
/// starts with `@`. Real `@world` is the union of `WorldSelectedSetsSet`
/// (this) with `WorldSelectedPackagesSet` (`read_world_atoms` above) --
/// see `WorldSelectedSet.load`'s own `chain(self._pkgset, self._setset)`.
/// A missing file is not an error, same "absence is a real, valid
/// state" precedent the world file itself already established. Returns
/// each name with its own leading `@` stripped, ready for
/// `resolve_custom_set`.
fn read_world_sets(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join("var/lib/portage/world_sets");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && l.starts_with('@'))
        .map(|l| l.trim_start_matches('@').to_string())
        .collect())
}

/// Resolves one custom, file-based package set by `name` (no leading
/// `@`), real portage's own default `usersets` source
/// (`lib/portage/_sets/__init__.py`'s own `_create_default_config`:
/// `class = StaticFileSet`, `directory =
/// <config_root>/etc/portage/sets`, one file per set, the file's own
/// path relative to that directory becoming the set's name) -- reads
/// `<config_root>/etc/portage/sets/<name>`, same line format as the
/// world file itself (one atom per line, `#`-comment/blank-line
/// handling identical), *except* a line starting with `@` is itself
/// another nested set reference here, resolved recursively -- real
/// `StaticFileSet`'s own validator (unlike `WorldSelectedPackagesSet`'s
/// stricter one) explicitly accepts a `@`-prefixed line too, and real
/// `SetConfig.getSetAtoms` walks every such non-atom entry, recursing
/// into any that start with `@` (`lib/portage/_sets/__init__.py`).
/// `seen` is that same recursion's own `ignorelist` -- a name already
/// being expanded on the current path contributes nothing further
/// (silently, not an error) rather than looping forever; a *fresh*
/// `seen` set is used for each top-level name in `read_world_sets`'s
/// own list, matching real `getSetAtoms(setname, ignorelist=None)`'s
/// own per-top-level-call default.
///
/// A `name` with no matching file is a real, immediate error (real
/// `PackageSetNotFound`, eventually surfaced and fatal at every real
/// call site in `lib/_emerge/actions.py`/`depgraph.py`) -- deliberately
/// NOT the same "absence is valid" tolerance `read_world_atoms`/
/// `read_world_sets` give their own *files*: those are optional,
/// implicitly-checked-for state (a fresh `ROOT` may simply never have
/// either), but a name explicitly listed in `world_sets` (or referenced
/// by another set) pointing at nothing is a real configuration error,
/// not an absence to tolerate.
fn resolve_custom_set(
    config_root: &Path,
    name: &str,
    seen: &mut HashSet<String>,
) -> Result<Vec<String>, String> {
    if !seen.insert(name.to_string()) {
        return Ok(Vec::new());
    }
    let path = config_root.join("etc/portage/sets").join(name);
    let text =
        std::fs::read_to_string(&path).map_err(|_| format!("emerge: set '{name}' not found"))?;
    let mut atoms = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some(nested_name) = line.strip_prefix('@') {
            atoms.extend(resolve_custom_set(config_root, nested_name, seen)?);
        } else {
            atoms.push(line.to_string());
        }
    }
    Ok(atoms)
}

/// `emerge --deselect <atom-or-bare-name> [...]`: real `action_deselect`
/// (`lib/_emerge/actions.py`), ported for `--pretend` mode only -- this
/// pilot's whole CLI requires `--pretend` (see the "only --pretend is
/// implemented" check in `run`, checked *before* this is ever reached),
/// so real `action_deselect`'s own non-pretend branch (which actually
/// writes `var/lib/portage/world`) is never reachable here; only its
/// "Would remove ..." reporting path is ported. Needs no repo/config
/// resolution at all -- only the world file and the vdb -- so this
/// doesn't call `find_repos`/`resolve_config` either, unlike every
/// other real feature in this pilot.
///
/// For each target in `targets`: a bare package name (no `/` at all) is
/// expanded via real portage's own "null category" mechanism -- scan the
/// world file's own atoms for one sharing that package name, and
/// substitute in its category (real `Atom(..., category="null")`
/// handling; this pilot's own atom parser has no equivalent, so this is
/// a dedicated lookup instead), added to the candidate set directly,
/// **unconditionally, no installed check at all**: confirmed by reading
/// `action_deselect`'s own null-category-substitution loop, which adds
/// the substituted atom to `expanded_atoms` before ever touching the
/// vardb. Real `action_deselect` *does* separately call
/// `vardb.match(atom)` for this same original (still-null-category)
/// atom, but that call can never match a real vardb entry -- no package
/// is ever catalogued under category "null" -- so it's dead code for
/// this branch specifically, and correctly contributes nothing here.
///
/// An *explicit*-category target (already has a `/`) is likewise added
/// to the candidate set directly, with **no installed check at all** --
/// confirmed by reading real portage's own call chain feeding
/// `action_deselect`'s own `atoms` parameter: `action_uninstall`'s own
/// `dep_expand(x, mydb=vardb, ...)` (`lib/portage/dbapi/dep_expand.py`)
/// returns an explicit-category atom completely unchanged, `if
/// mydep.category != "virtual": return mydep`, *before* it ever reaches
/// `cpv_expand` (the vardb-dependent part, only reached for a bare
/// name); `action_deselect` itself then seeds `expanded_atoms =
/// set(atoms)` with that same atom, unconditionally. So `--deselect
/// cat/pkg` (or a bare `pkg` resolvable via the world file) genuinely
/// discards a matching world entry even if never installed -- this
/// pilot's own earlier doc comment (and test) claimed installation was
/// always required, an incorrect generalization: real portage's own
/// vardb-derived narrowing (`vardb.match`) is a *separate, additional*
/// contribution on top of the unconditional substitution/literal-target
/// candidate, for BOTH the bare-name and explicit-category cases -- not
/// a gate on it. For an explicit-category target specifically, that
/// separate vardb contribution (`portage_repo::installed_candidates`,
/// this pilot's own vdb scan, via `match_from_list`) still runs and adds
/// a further bare `category/package:slot` candidate (real
/// `Atom(f"{pkg.cp}:{pkg.slot}")`, no version/operator at all) for
/// whatever version(s) are actually installed; for a bare name it's
/// correctly omitted, per the dead-code reasoning above.
///
/// Every candidate atom collected this way -- installed-derived (bare
/// `category/package:slot`) or the literal target/substituted atom
/// (version/operator intact, if given) -- is compared against every
/// world-file entry via real `Atom.intersects()` (`portage_dep::
/// atom_intersects`, see its own doc comment) plus real
/// `action_deselect`'s own separate repo check (`not (arg_atom.repo and
/// not atom.repo)`), replacing this pilot's own previous narrower
/// category/package(+slot)-only equality check.
///
/// `--deselect @some-set`: real `action_deselect`'s own combined
/// `world_set` (`WorldSelectedSet`) iterates BOTH `world`'s own plain
/// atoms AND `world_sets`'s own literal `@name` reference *strings* --
/// confirmed by reading `WorldSelectedSet.load`'s own `self._setAtoms(
/// chain(self._pkgset, self._setset))`: a `@name` string fails real
/// `Atom(...)` parsing and lands in `_nonatoms`, so it's carried through
/// *unexpanded*, never resolved into its own member atoms at all.
/// `action_deselect`'s own matching loop confirms this: a `@`-prefixed
/// CLI target can only ever discard a `@`-prefixed `world_set` entry via
/// *exact string equality* (`arg_atom == atom`) -- there is no
/// installed-candidate matching, no member-atom expansion, for either
/// side. So despite `resolve_custom_set`'s own real, working nested-set
/// expansion (built for -- and still only used by -- `@world`'s own
/// dependency-resolution walk, a genuinely different real mechanism,
/// `SetConfig.getSetAtoms`), it has no role here at all: this pilot's
/// own equivalent is a plain membership check against `read_world_sets`
/// (`@name` stripped of its own leading `@` for the comparison), nothing
/// more. Each discarded entry is reported against its own real source
/// file (`"world"` for a plain atom, `"world_sets"` for a `@name`
/// reference), matching real `action_deselect`'s own `filename =
/// "world_sets" if str(atom).startswith(SETPREFIX) else "world"` --
/// sorted together into one combined list, not two separate blocks,
/// mirroring real `sorted(discard_atoms, key=str)` exactly.
///
/// Real `action_deselect` always returns `os.EX_OK` on every reachable
/// path here (found matches, no matches, even no targets at all) --
/// ported the same way, unconditionally `ExitCode::SUCCESS`.
fn run_deselect(targets: &[&str], root: &Path) -> ExitCode {
    let world_atoms = match read_world_atoms(root) {
        Ok(atoms) => atoms,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let world_sets = match read_world_sets(root) {
        Ok(sets) => sets,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let mut expanded: Vec<Atom> = Vec::new();
    // A `@name` target only ever matches a `world_sets` entry by exact
    // name -- see this function's own doc comment -- so it's collected
    // separately, never fed through the atom-expansion/vardb-matching
    // path below at all.
    let mut set_targets: HashSet<&str> = HashSet::new();
    for target in targets {
        if let Some(name) = target.strip_prefix('@') {
            set_targets.insert(name);
            continue;
        }
        if let Some(atom) = parse_atom(target).filter(|_| target.contains('/')) {
            // Explicit-category target: real `expanded_atoms =
            // set(atoms)` seeds with the literal target atom itself,
            // version/operator intact, no installed check at all -- see
            // this function's own doc comment. `vardb.match(atom)`
            // (real `installed_candidates` + `match_from_list` here)
            // separately contributes a bare `category/package:slot`
            // candidate for whatever version(s) are *actually*
            // installed, real `Atom(f"{pkg.cp}:{pkg.slot}")` -- no
            // version/operator at all, regardless of the real installed
            // one.
            expanded.push(atom.clone());
            for (version, slot, _sub_slot) in
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
            {
                let candidate_str = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if match_from_list(target, &[candidate_str.as_str()]).is_some_and(|m| !m.is_empty())
                {
                    if let Some(vardb_atom) =
                        parse_atom(&format!("{}/{}:{slot}", atom.category, atom.package))
                    {
                        expanded.push(vardb_atom);
                    }
                }
            }
        } else if !target.contains('/') {
            // Bare name: null-category substitution, unconditional, no
            // installed check -- see this function's own doc comment.
            for w in world_atoms.iter().filter_map(|w| parse_atom(w)) {
                if w.package != *target {
                    continue;
                }
                if let Some(substituted) = parse_atom(&format!("{}/{}", w.category, target)) {
                    expanded.push(substituted);
                }
            }
        } else {
            eprintln!("emerge: invalid atom {target:?}");
            return ExitCode::from(1);
        }
    }

    // (entry text, source file) pairs -- combined and sorted together as
    // one list, matching real `sorted(discard_atoms, key=str)` (not two
    // separate "world" then "world_sets" blocks).
    let mut discard: Vec<(String, &'static str)> = world_atoms
        .iter()
        .filter(|world_atom_str| {
            let Some(w) = parse_atom(world_atom_str) else {
                return false;
            };
            expanded.iter().any(|arg_atom| {
                portage_dep::atom_intersects(arg_atom, &w)
                    && !(arg_atom.repo.is_some() && w.repo.is_none())
            })
        })
        .map(|s| (s.clone(), "world"))
        .collect();
    discard.extend(
        world_sets
            .iter()
            .filter(|name| set_targets.contains(name.as_str()))
            .map(|name| (format!("@{name}"), "world_sets")),
    );

    if discard.is_empty() {
        println!(">>> No matching atoms found in \"world\" favorites file...");
    } else {
        discard.sort_by(|a, b| a.0.cmp(&b.0));
        for (entry, filename) in discard {
            println!(">>> Would remove {entry} from \"{filename}\" favorites file...");
        }
    }
    ExitCode::SUCCESS
}

pub fn run(args: &[String]) -> ExitCode {
    if wants_help(args) {
        print_help();
        return ExitCode::SUCCESS;
    }

    let mut atom_args: Vec<&str> = Vec::new();
    let mut pretend = false;
    let mut verbose = false;
    let mut newuse = false;
    let mut changed_use = false;
    let mut nodeps = false;
    let mut onlydeps = false;
    // --tree/-t and --unordered-display: display-only, entirely
    // independent of resolution itself (real portage's own equivalent,
    // output_helpers.py's _tree_display, lives in the display layer too,
    // never depgraph.py's core resolution) -- see print_tree's own doc
    // comment for the full pilot-specific design this needed.
    let mut tree = false;
    let mut unordered_display = false;
    // --columns: display-only, same "entirely independent of resolution"
    // shape as --tree above (real output_helpers.py's own
    // MergeListItem.conf.columns is a display-layer flag, never consulted
    // anywhere in depgraph.py) -- mutually exclusive with --tree, checked
    // once parsing finishes (see the "can't specify both" check below).
    let mut columns = false;
    let mut update = false;
    let mut deep = portage_repo::Deep::NotRequested;
    let mut excluded: Vec<String> = Vec::new();
    // --usepkg-exclude/--usepkg-include: same "action": "append",
    // space-separated-per-occurrence shape as --exclude/-X above (real
    // main.py: "A space separated list of package names or slot atoms"),
    // but scoped to binary-candidate eligibility specifically -- see
    // `filter_usepkg_exclude_include`'s own doc comment, portage-repo.
    let mut usepkg_exclude: Vec<String> = Vec::new();
    let mut usepkg_include: Vec<String> = Vec::new();
    let mut json = false;
    let mut deselect = false;
    let mut with_bdeps = true;
    let mut with_bdeps_given = false;
    let mut with_bdeps_auto = true;
    let mut changed_deps = false;
    let mut changed_slot = false;
    // --newrepo: real main.py's own plain boolean "options" list, no
    // value at all (same shape as --changed-use/-U above) -- unlike
    // --changed-slot/--rebuilt-binaries, which are real "true_y_or_n".
    let mut newrepo = false;
    // --buildpkgonly/-B: same plain-boolean shape as --newrepo above.
    let mut buildpkgonly = false;
    // --keep-going: real main.py's own `y_or_n` validator, but this
    // pilot's own transcription (`emerge_options::BOOLEAN_OPTIONS`)
    // already narrows it to the bare/`y` form only, the same shape
    // `--newrepo`/`--buildpkgonly` have -- only meaningful alongside
    // `--buildpkgonly` (without `--pretend`), see `emerge_build::
    // run_buildpkgonly`'s own doc comment for why this pilot's own
    // simplified, no-cross-entry-ordering context makes the real
    // semantics much narrower than real portage's own general
    // mergelist-recalculation/resume-state machinery.
    let mut keep_going = false;
    // --root-deps: real main.py's own `choices: ("True", "rdeps")`, plus
    // a bare form (no `=value` at all). This pilot's own v1 doesn't
    // distinguish "True" (fold DEPEND/BDEPEND/IDEPEND into RDEPEND) from
    // "rdeps" (additionally ignore DEPEND for non-BDEPEND-EAPI packages)
    // -- see `root_deps_satisfied_atoms`'s own doc comment for why
    // neither is observable in this pilot's own single-root graph model
    // anyway -- so every accepted real form just enables the one real
    // behavior this pilot does implement: real running-root (`ESYSROOT`)
    // satisfiability for DEPEND/BDEPEND atoms.
    let mut root_deps = false;
    let mut with_test_deps = false;
    let mut changed_deps_report = false;
    // --autounmask/--autounmask-keep-keywords: real "true_y_or_n"
    // (bare flag, "=y", or "=n") for the first, plain required "y"/"n"
    // (no bare form) for the second -- see the on/off default-
    // resolution logic just below where these are actually consumed,
    // grounded against real create_depgraph_params.py's own
    // autounmask/autounmask_keep_keywords computation.
    let mut autounmask: Option<bool> = None;
    let mut autounmask_keep_keywords: Option<bool> = None;
    let mut autounmask_use: Option<bool> = None;
    // --usepkg/-k, --usepkgonly/-K, --binpkg-respect-use: all three real
    // "true_y_or_n" (bare flag, "=y", or "=n"), same shape --autounmask
    // already has. --binpkg-respect-use's own real default ("auto",
    // effectively on, whenever --usepkgonly is NOT given -- see
    // create_depgraph_params.py:47-55) is resolved below, once usepkgonly
    // itself is known.
    let mut usepkg = false;
    let mut usepkgonly = false;
    let mut binpkg_respect_use: Option<bool> = None;
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

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--pretend" || arg == "-p" {
            pretend = true;
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
        } else if arg == "--tree" || arg == "-t" {
            tree = true;
            i += 1;
        } else if arg == "--unordered-display" {
            unordered_display = true;
            i += 1;
        } else if arg == "--columns" {
            columns = true;
            i += 1;
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
        } else if arg == "--json" {
            // NOT a real emerge option at all -- real portage has no
            // structured-output mode for --pretend. Pilot-specific, so
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
        } else if arg == "--verbose=y" {
            verbose = true;
            i += 1;
        } else if arg == "--verbose=n" {
            verbose = false;
            i += 1;
        } else if arg == "--deselect" || arg == "-W" {
            // Real "--deselect": y_or_n (argument_options), the same
            // optional-value shape "--verbose"/"-v" already has -- see
            // that branch's own comment. Unlike "--verbose", a bare
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
                    i += 2;
                }
                _ => {
                    deselect = true;
                    i += 1;
                }
            }
        } else if arg == "--deselect=y" {
            deselect = true;
            i += 1;
        } else if arg == "--deselect=n" {
            deselect = false;
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
                    eprintln!("emerge: option \"--with-bdeps\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    eprintln!("emerge: option \"--with-bdeps\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    eprintln!("emerge: option \"--with-bdeps-auto\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    eprintln!("emerge: option \"--with-bdeps-auto\": invalid choice: {value:?} (choose from \"y\", \"n\")");
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--changed-deps" {
            // Real "--changed-deps": y_or_n (default_arg_opts), the same
            // optional-value shape "--verbose"/"-v" and "--deselect"/"-W"
            // already have -- no short alias, though (real main.py
            // declares none). Unlike --deselect, this stays an ordinary
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
        } else if arg == "--changed-deps=y" {
            changed_deps = true;
            i += 1;
        } else if arg == "--changed-deps=n" {
            changed_deps = false;
            i += 1;
        } else if arg == "--changed-deps-report" {
            // Real "--changed-deps-report": y_or_n (default_arg_opts),
            // the identical optional-value shape "--changed-deps"
            // already has -- no short alias (real main.py declares
            // none). Unlike --changed-deps, this never changes what
            // gets reinstalled -- see resolve_pretend_graph's own doc
            // comment.
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
        } else if arg == "--changed-deps-report=y" {
            changed_deps_report = true;
            i += 1;
        } else if arg == "--changed-deps-report=n" {
            changed_deps_report = false;
            i += 1;
        } else if arg == "--selective" {
            // Real "--selective": y_or_n (default_arg_opts), the same
            // optional-value shape "--changed-deps" already has -- no
            // short alias for this exact spelling (real main.py declares
            // none; "-n" is "--noreplace" above, real portage's own
            // separate, bare-boolean spelling of the identical meaning).
            // "n" here explicitly CANCELS `selective` even if some other
            // flag already set it -- see `resolve_pretend`'s own doc
            // comment (portage-repo) and this override's own application
            // just before the `resolve_pretend_graph` call below.
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
        } else if arg == "--selective=y" {
            selective_flag = Some(true);
            i += 1;
        } else if arg == "--selective=n" {
            selective_flag = Some(false);
            i += 1;
        } else if arg == "--changed-slot" {
            // Real "--changed-slot": y_or_n (default_arg_opts), the
            // identical optional-value shape "--changed-deps" already
            // has -- no short alias (real main.py declares none).
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
        } else if arg == "--changed-slot=y" {
            changed_slot = true;
            i += 1;
        } else if arg == "--changed-slot=n" {
            changed_slot = false;
            i += 1;
        } else if arg == "--newrepo" {
            newrepo = true;
            i += 1;
        } else if arg == "--buildpkgonly" || arg == "-B" {
            buildpkgonly = true;
            i += 1;
        } else if arg == "--keep-going" {
            keep_going = true;
            i += 1;
        } else if arg == "--root-deps" || arg == "--root-deps=True" || arg == "--root-deps=rdeps" {
            root_deps = true;
            i += 1;
        } else if arg == "--with-test-deps" {
            // Real "--with-test-deps": y_or_n (default_arg_opts), the
            // identical optional-value shape "--changed-deps"/
            // "--changed-slot" already have -- no short alias (real
            // main.py declares none).
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
        } else if arg == "--with-test-deps=y" {
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
        } else if arg == "--autounmask-keep-keywords" {
            // Real "--autounmask-keep-keywords": plain y_or_n, a
            // REQUIRED value -- no bare/optional form real
            // "--autounmask" itself has, the same required shape
            // "--with-bdeps" already has.
            let Some(value) = args.get(i + 1) else {
                eprintln!("emerge: option \"--autounmask-keep-keywords\" requires an argument");
                return ExitCode::from(2);
            };
            match value.as_str() {
                "y" => {
                    autounmask_keep_keywords = Some(true);
                    i += 2;
                }
                "n" => {
                    autounmask_keep_keywords = Some(false);
                    i += 2;
                }
                _ => {
                    eprintln!("emerge: option \"--autounmask-keep-keywords\": invalid choice: {value:?} (choose from \"y\", \"n\")");
                    return ExitCode::from(2);
                }
            }
        } else if let Some(value) = arg.strip_prefix("--autounmask-keep-keywords=") {
            match value {
                "y" => {
                    autounmask_keep_keywords = Some(true);
                    i += 1;
                }
                "n" => {
                    autounmask_keep_keywords = Some(false);
                    i += 1;
                }
                _ => {
                    eprintln!("emerge: option \"--autounmask-keep-keywords\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    eprintln!("emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    eprintln!("emerge: option \"--autounmask-use\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    'v' => verbose = true,
                    'N' => newuse = true,
                    'U' => changed_use = true,
                    'O' => nodeps = true,
                    'o' => onlydeps = true,
                    't' => tree = true,
                    'u' => update = true,
                    'n' => noreplace = true,
                    'D' => deep = portage_repo::Deep::Unlimited,
                    'k' => usepkg = true,
                    'K' => usepkgonly = true,
                    'W' => deselect = true,
                    'B' => buildpkgonly = true,
                    'X' => {
                        // Unlike every other bundle-compatible short flag
                        // here, -X's own value is *required*, not
                        // optional -- there's no sensible "just default
                        // it" behavior the way a bundled -v/-D has, so
                        // this pilot deliberately doesn't support
                        // bundling -X at all, with a specific message
                        // instead of a misleading generic one.
                        eprintln!(
                            "emerge: -X (--exclude) requires an argument and can't be \
                             bundled with other short flags in this pilot"
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

    // Real actions.py: "if '--tree' in emerge_config.opts and '--columns'
    // in emerge_config.opts: print(...); return 1" -- checked once
    // parsing finishes (order-independent: works whichever flag came
    // first in argv), right after option parsing and before any other
    // validation, matching real portage's own placement. This pilot's
    // own CLI-usage-error convention (see the contract suite's own doc
    // comment: exit 2, stderr) differs deliberately from real portage's
    // literal `return 1`/stdout here, matching every other CLI-usage
    // error this pilot already reports (`--exclude` requires an
    // argument, an invalid `--deep` value, etc.) rather than real
    // portage's own inconsistent mix of exit codes for different
    // usage errors.
    if tree && columns {
        eprintln!("emerge: can't specify both of \"--tree\" and \"--columns\".");
        return ExitCode::from(2);
    }

    // `--deselect` is checked first, before the general gate below: it's
    // a real action in its own right that always requires `--pretend`
    // (real `action_deselect`'s own file-writing branch is unreachable
    // here), regardless of whether `--buildpkgonly` also happens to be
    // given -- `--buildpkgonly` unlocking real building is not the same
    // thing as unlocking `--deselect`.
    if deselect && !pretend {
        eprintln!("emerge (pilot v1): --deselect requires --pretend (see PROMPT.md)");
        return ExitCode::from(2);
    }

    // `--buildpkgonly` without `--pretend` is the one real, non-dry-run
    // execution path this pilot implements for `emerge` itself (see
    // `emerge_build.rs`'s own module doc comment): it only ever builds a
    // binary package, never merges anything, so it's safe to let through
    // here even though every other real action still isn't implemented.
    if !pretend && !buildpkgonly {
        eprintln!(
            "emerge (pilot v1): no real merges implemented yet -- only \
             --pretend (dry-run) or --buildpkgonly without --pretend (real \
             binary-package building only, still never merges) are \
             supported (see PROMPT.md)"
        );
        return ExitCode::from(2);
    }

    if deselect {
        return run_deselect(&atom_args, &root_from_env());
    }

    if atom_args.is_empty() {
        eprintln!("emerge (pilot v1): expected a package atom, e.g. `emerge --pretend cat/pkg`");
        return ExitCode::from(2);
    }

    let root = root_from_env();
    let config_root = config_root_from_env();

    // resolve_config needs the main repo's own location for
    // package.mask/.unmask's repo-level source (see its doc comment) --
    // found via the same find_repos repos.conf parsing
    // resolve_pretend_graph uses internally a few lines down; called
    // again here since portage-profile can't depend back on portage-repo
    // (portage-repo already depends on portage-profile). Resolved before
    // @world/@system expansion below: @system's own atom list lives in
    // `config` (see portage-profile's `system_packages`), so the config
    // must already exist by the time a "@system" token is seen.
    let repos = match portage_repo::find_repos(&config_root) {
        Ok(repos) => repos,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let Some(main_repo) = repos.iter().find(|r| r.is_main) else {
        eprintln!("emerge: no main repo found in repos.conf");
        return ExitCode::from(1);
    };

    // Every non-main repo's own (name, location) -- portage-profile's
    // own package.mask/.unmask reading needs each overlay's own name to
    // scope its repo-level entries via "::name" (see resolve_config's
    // own doc comment); ascending-priority order, same as find_repos'
    // own order, which only matters if two overlays' own entries could
    // otherwise interfere, and the "::name" scoping already rules that
    // out regardless. The same list, plus the main repo's own name
    // below, also lets resolve_config follow a profile's own cross-repo
    // "parent" entries (reponame:path syntax).
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();

    // Real `masters` (see `portage_repo::RepoConfig::masters`'s own doc
    // comment): each repo's own already-resolved masters chain, keyed by
    // name, for `resolve_config`'s own package.mask stacking.
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();

    let config = match portage_profile::resolve_config(
        &config_root,
        &main_repo.location,
        &overlay_repos,
        &main_repo.name,
        &repo_masters,
    ) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    // "@world"/"@system" each expand to their own real atom list, in
    // place, at whichever position they appear -- see read_world_atoms's
    // doc comment for the world file's own scope, read_world_sets's for
    // the world_sets file's own nested-@set half of real @world's union,
    // and portage-profile's `system_packages` doc comment for @system's.
    // Only these two literal tokens trigger expansion -- any other
    // "@"-prefixed token falls through to the ordinary atom-parsing path
    // below and gets a clear "invalid atom" error, not a silent no-op.
    let mut expanded_atoms: Vec<String> = Vec::new();
    for atom_str in &atom_args {
        if *atom_str == "@world" {
            match read_world_atoms(&root) {
                Ok(world_atoms) => expanded_atoms.extend(world_atoms),
                Err(e) => {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(1);
                }
            }
            let set_names = match read_world_sets(&root) {
                Ok(names) => names,
                Err(e) => {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(1);
                }
            };
            for name in set_names {
                let mut seen = HashSet::new();
                match resolve_custom_set(&config_root, &name, &mut seen) {
                    Ok(atoms) => expanded_atoms.extend(atoms),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                }
            }
        } else if *atom_str == "@system" {
            expanded_atoms.extend(config.system_packages.iter().cloned());
        } else {
            expanded_atoms.push((*atom_str).to_string());
        }
    }

    if expanded_atoms.is_empty() {
        eprintln!(
            "emerge (pilot v1): no package atoms to resolve (the target list, after \
             expanding any @world/@system, is empty)"
        );
        return ExitCode::from(2);
    }

    for atom_str in &expanded_atoms {
        let Some(atom) = parse_atom(atom_str) else {
            eprintln!("emerge: invalid atom {atom_str:?}");
            return ExitCode::from(1);
        };
        if atom.blocker != Blocker::None {
            eprintln!("emerge (pilot v1): {atom_str:?} is a blocker, not a valid emerge target");
            return ExitCode::from(2);
        }
    }

    // Real create_depgraph_params.py's own precedence: an explicit
    // --with-bdeps always wins; only when it's absent does
    // --with-bdeps-auto=n override the real default ("auto", this
    // pilot's own pre-existing `with_bdeps = true`) down to "n" instead.
    if !with_bdeps_given {
        with_bdeps = with_bdeps_auto;
    }

    // Real create_depgraph_params.py's own `selective` condition,
    // computed from whichever of its real trigger flags this pilot
    // implements -- see `resolve_pretend`'s own doc comment
    // (portage-repo) for the full grounding, including why
    // `--changed-use` alone covers this pilot's whole share of real
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

    // --autounmask/--autounmask-keep-keywords/--autounmask-use: real
    // create_depgraph_params.py's own default-resolution logic,
    // simplified for this pilot's own v1 scope (--autounmask-license/
    // -masks still aren't read at all, matching every real fixture/user
    // who never touches them getting the exact same outcome this
    // simplification produces). Real logic: `autounmask` itself defaults
    // to enabled (only `--autounmask=n` turns the whole feature off --
    // with `--autounmask-use` now read but `--autounmask-license`
    // still not, the "is autounmask_use/license itself what makes
    // autounmask default true" branch in real create_depgraph_params.py
    // takes the "yes" arm whenever `--autounmask-use` isn't explicitly
    // "n", which this pilot's own `autounmask_enabled` below still
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
    // `--binpkg-respect-use=y`, not the "auto" default) -- this pilot's
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

    // --root-deps: real running root (see `running_root_from_env`'s own
    // doc comment for why real "/" is the correct default here, and
    // `PORTAGE_RUNNING_ROOT`'s own pilot-specific, test-only override).
    let root_deps_running_root = root_deps.then(portage_repo::running_root_from_env);

    let result = match resolve_pretend_graph(
        &config_root,
        &root,
        &expanded_atoms,
        &config,
        newuse,
        changed_use,
        nodeps,
        update,
        deep,
        &excluded,
        with_bdeps,
        changed_deps,
        changed_slot,
        with_test_deps,
        changed_deps_report,
        selective,
        autounmask_suggest_keywords,
        autounmask_suggest_use,
        usepkg,
        usepkgonly,
        binpkg_respect_use,
        &usepkg_exclude,
        &usepkg_include,
        rebuilt_binaries,
        rebuilt_binaries_timestamp,
        newrepo,
        buildpkgonly,
        root_deps_running_root.as_deref(),
    ) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let entries = &result.entries;

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
            entries,
            &result.slot_conflicts,
            &result.changed_deps_report,
            &top_level_pkgs,
            verbose,
        );
        return ExitCode::SUCCESS;
    }

    // Real portage resolves COLUMNWIDTH (and warns on an unparsable
    // value) as part of general display setup, unconditionally --
    // never gated on --columns itself actually being given. Mirrored
    // here the same way, even though the value only ever affects
    // anything below when `columns` is true.
    let columnwidth = columnwidth_from_env();
    if tree {
        print_tree(
            entries,
            &top_level_pkgs,
            onlydeps,
            unordered_display,
            verbose,
        );
    } else {
        for entry in entries {
            print_entry_line(
                entry,
                "",
                &top_level_pkgs,
                onlydeps,
                verbose,
                columns,
                columnwidth,
            );
        }
    }

    // Purely informational, same as blockers -- see resolve_pretend_graph's
    // doc comment: v1 neither refuses nor changes the exit code for a slot
    // conflict.
    for c in &result.slot_conflicts {
        println!(
            "[slot conflict] {}/{}:{} resolved to {}/{}-{}, which does not satisfy \"{}\"",
            c.category,
            c.package,
            c.slot,
            c.category,
            c.package,
            c.resolved_version,
            c.conflicting_atom
        );
    }

    // `--changed-deps-report`: real `_changed_deps_report`'s own WARN
    // block, ported verbatim (real portage colorizes it when the
    // terminal supports it; this pilot, like every other message it
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
        return ExitCode::from(1);
    }

    // Real execution: only reachable when `!pretend`, which the gate at
    // the top of this function only ever lets through when `buildpkgonly`
    // is also `true` -- see `emerge_build.rs`'s own module doc comment
    // for what this actually does (and doesn't) build.
    if !pretend {
        // Real BINPKG_COMPRESS/BINPKG_COMPRESS_FLAGS[_<NAME>]/
        // PORTAGE_BZIP2_COMMAND resolution -- same env-var-sourced CLI
        // boundary as `ebuild.rs`'s own real `merge`/`qmerge`/`package`
        // construction (see `ebuild_package::PackageOptions::
        // binpkg_compress_flags`'s own doc comment for why the
        // per-compressor override is resolved here, once).
        let default_package_options = ebuild_package::PackageOptions::default();
        let binpkg_compress = std::env::var("BINPKG_COMPRESS")
            .unwrap_or_else(|_| default_package_options.binpkg_compress.clone());
        let binpkg_compress_flags_name =
            format!("BINPKG_COMPRESS_FLAGS_{}", binpkg_compress.to_uppercase());
        let binpkg_compress_flags = std::env::var(&binpkg_compress_flags_name)
            .or_else(|_| std::env::var("BINPKG_COMPRESS_FLAGS"))
            .unwrap_or_else(|_| default_package_options.binpkg_compress_flags.clone());
        let package_options = ebuild_package::PackageOptions {
            debug: false,
            pkgdir: std::env::var_os("PKGDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| default_package_options.pkgdir.clone()),
            distdir: std::env::var_os("DISTDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| default_package_options.distdir.clone()),
            shell: default_package_options.shell,
            binpkg_compress,
            binpkg_compress_flags,
            portage_bzip2_command: std::env::var("PORTAGE_BZIP2_COMMAND")
                .unwrap_or(default_package_options.portage_bzip2_command),
            config_root: portage_repo::config_root_from_env(),
        };
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));
        if let Err(e) = emerge_build::run_buildpkgonly(
            entries,
            &repos,
            &root,
            &portage_tmpdir,
            &package_options,
            keep_going,
        ) {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}
