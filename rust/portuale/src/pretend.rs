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

use crate::color::{self, Colorizer};
use crate::ebuild_merge;
use crate::ebuild_package;
use crate::ebuild_phases;
use crate::emerge_build;
use crate::emerge_getbinpkg;
use crate::emerge_options;
use crate::needed_elf;
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
/// `  USE="flag1 -flag2" VIDEO_CARDS="-amdgpu nvidia"` (two leading
/// spaces, matching real `--pretend -v`'s own line format), or an empty
/// string when `--verbose` wasn't given or this entry has no displayable
/// flags at all. The `USE_EXPAND` grouping (plain `USE` group, then one
/// `VAR="…"` per non-hidden `USE_EXPAND` variable, empty groups omitted)
/// is real -- `output.py::_display_use`, computed in
/// `portage_repo::build_use_expand_display` and carried on
/// `GraphEntry::use_expand_display`. Still not shown: real portage's own
/// ANSI colorization and its installed-vs-new `*`/`%` diff markers
/// (a separate documented cut -- this shows the plain enabled/disabled
/// set, `_alnum_sort_key`-ordered within each group).
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
/// color stripped for increment 1 (real's `nc_len`/plain `len()`
/// distinction collapses to just `len()` until increment 2 adds ANSI
/// color). `bracket`/`field` reproduce the exact same `"[{bracket}
/// {field}]"` segment the non-columns format prints -- `field` is the
/// full fixed-width `attr_display_field` -- only what comes after it
/// differs: `category/package` (no version -- that's the whole point of
/// `--columns`) padded out to `columnwidth - 60` (`newlp`), then
/// `[version]` right-padded to `columnwidth - 30` (`oldlp`), then
/// `oldbest` (`"[from]"` for an `Upgrade`/`Downgrade`, empty otherwise --
/// real `pkg_info.oldbest_list`, mirrored here via data this pilot
/// already has rather than a new installed-candidate lookup). Padding is
/// skipped once the line's already past the target width, exactly like
/// real portage's own `if (newlp - nc_len(myprint)) > 0` guard -- never
/// truncates, just doesn't pad further.
#[allow(clippy::too_many_arguments)]
fn columns_line(
    bracket_word: &str,
    field: &str,
    indent: &str,
    category: &str,
    package: &str,
    version: &str,
    oldbest: &str,
    columnwidth: i64,
    color: &Colorizer,
    binary: bool,
    system: bool,
    world: bool,
) -> String {
    let newlp = (columnwidth - 60).max(0) as usize;
    let oldlp = (columnwidth - 30).max(0) as usize;
    // Real `_set_root_columns` merge branch: the type word and `pkg.cp`
    // both go through `pkgprint`; the version column is `green("[ver]")`;
    // `oldbest` arrives already `blue("[from]")` from `convert_myoldbest`.
    // Padding measures visible width (`color::nc_len`), never raw bytes.
    let cp = color.pkgprint(&format!("{category}/{package}"), binary, system, world);
    let bword = color.pkgprint(bracket_word, binary, system, world);
    let mut line = format!("[{bword} {field}] {indent}{cp}");
    let pad_to = |line: &mut String, target: usize| {
        let w = color::nc_len(line);
        if target > w {
            line.push_str(&" ".repeat(target - w));
        }
    };
    pad_to(&mut line, newlp);
    line.push_str(&format!(" {} ", color.c("green", &format!("[{version}]"))));
    pad_to(&mut line, oldlp);
    if !oldbest.is_empty() {
        line.push_str(&color.c("blue", oldbest));
    }
    line
}

/// Real `PkgAttrDisplay.__str__` (`_emerge/resolver/output_helpers.py`):
/// the fixed-width status field rendered inside the `[ebuild …]` bracket,
/// exactly `[{pkg.type_name} {attr_display}]`. One column per attribute,
/// a literal space where the attribute is absent, in this exact order:
///
/// 0. `I` -- `interactive` (`GraphEntry::interactive`).
/// 1. `N` -- `new`; `r` instead when `force_reinstall`. This pilot has no
///    `--emptytree`/`arg.force_reinstall` concept, so always `N` or space
///    here -- a plain reinstall shows `R` at col 2, see real
///    `_get_installed_best`: `replace=True` is set only when the exact cpv
///    is already installed.
/// 2. `S` -- `new_slot`; `R` instead when `replace` (the cpv is already
///    installed -- every one of this pilot's `Reinstall` outcomes).
/// 3. `f`/`F`/`g` -- fetch-restrict satisfied / unsatisfied / remote binary
///    (`g` is out of scope, needs `--getbinpkg`).
/// 4. `U` -- `new_version` (an in-slot version change -- `Upgrade`/`Downgrade`).
/// 5. `D` -- `downgrade`.
/// 6. the mask column -- the `#`/`~`/`*` char from `gen_mask_str`
///    (`GraphEntry::keyword_mask`) or a space. Real `set_pkg_info` fills
///    it in only `if self.include_mask_str()` (`verbosity > 1`), and real
///    default `emerge -p` verbosity is *2* (`_DisplayConfig.__init__`:
///    `--quiet and 1 or --verbose and 3 or 2`) -- so the column is
///    present at plain `-p` *and* `-pv`, and absent only under `--quiet`
///    (verbosity 1), which this pilot doesn't model. Always rendered.
///
/// Each present letter is ANSI-coloured per real `PkgAttrDisplay.__str__`
/// (`green("N")`, `yellow("R")`, `turquoise("U")`, `blue("D")`,
/// `colorize("WARN", "I")`, the `#`/`*`/`~` mask via `BAD`/`WARN`, …)
/// when `color.enabled`; a space is never coloured. When colour is off
/// every call returns the bare char, so the field is exactly 7 visible
/// columns either way (`color::nc_len` recovers that width for
/// `--columns` padding).
#[allow(clippy::too_many_arguments)]
fn attr_display_field(
    interactive: bool,
    new: bool,
    force_reinstall: bool,
    new_slot: bool,
    replace: bool,
    fetch_restrict: bool,
    fetch_restrict_satisfied: bool,
    remote_binary: bool,
    new_version: bool,
    downgrade: bool,
    mask: Option<char>,
    color: &Colorizer,
) -> String {
    let col = |key: &str, ch: char| color.c(key, &ch.to_string());
    let mut f = String::new();
    f.push_str(&if interactive {
        col("WARN", 'I')
    } else {
        " ".to_string()
    });
    f.push_str(&if force_reinstall {
        col("red", 'r')
    } else if new {
        col("green", 'N')
    } else {
        " ".to_string()
    });
    f.push_str(&if replace {
        col("yellow", 'R')
    } else if new_slot {
        col("green", 'S')
    } else {
        " ".to_string()
    });
    f.push_str(&if fetch_restrict_satisfied {
        col("green", 'f')
    } else if fetch_restrict {
        col("red", 'F')
    } else if remote_binary {
        col("fuchsia", 'g')
    } else {
        " ".to_string()
    });
    f.push_str(&if new_version {
        col("turquoise", 'U')
    } else {
        " ".to_string()
    });
    f.push_str(&if downgrade {
        col("blue", 'D')
    } else {
        " ".to_string()
    });
    // Real `__str__` appends `self.mask` only `if self.mask is not None`,
    // and `set_pkg_info` sets it only `if self.include_mask_str()`
    // (`verbosity > 1`) -- true at real portage's default `emerge -p`
    // verbosity of 2, so the column is always present here (this pilot
    // has no `--quiet`/verbosity-1 mode). Real `gen_mask_str`: `#`/`*` ->
    // `BAD` (red), `~` -> `WARN` (yellow), no mark -> a space.
    f.push_str(&match mask {
        Some(c @ ('#' | '*')) => col("BAD", c),
        Some('~') => col("WARN", '~'),
        _ => " ".to_string(),
    });
    f
}

/// The bare flag name inside a rendered `USE=` token, for the
/// `--alphabetical` re-sort: strip a leading `(` / `-` and any trailing
/// `)` / `*` / `%` (`(-maskflag)` -> `maskflag`, `foo%*` -> `foo`).
fn use_flag_sort_key(tok: &str) -> &str {
    tok.trim_start_matches('(')
        .trim_start_matches('-')
        .trim_end_matches([')', '*', '%'])
}

/// Real `_create_use_string`'s per-flag colour (`output_helpers.py:262-334`),
/// re-derived from an already-rendered token's shape -- the marker suffix
/// and sign fully determine it: a plain enabled `flag` is `red`, a plain
/// disabled `-flag` is `blue`, a `%`/`%*` marker means `yellow` (newly in
/// IUSE), a lone `*` means `green` (polarity flipped). Only the
/// `flag`/`-flag` core is coloured -- the `*`/`%` markers and any `( )`
/// forced/removed wrap stay plain, exactly as real (`yellow(flag) + "%*"`
/// etc). Known imperfection (no fixture reaches it, and it matches the
/// pilot's own `render_flag` `%`-collapse for forced flags): a forced
/// *disabled* flag newly added to IUSE on an Upgrade renders `(-flag)`
/// and is coloured `blue` here, where real portage would `yellow` it.
fn colorize_use_token(tok: &str, color: &Colorizer) -> String {
    let (open, inner, close) = match tok.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(i) => ("(", i, ")"),
        None => ("", tok, ""),
    };
    let (core, markers) = if let Some(c) = inner.strip_suffix("%*") {
        (c, "%*")
    } else if let Some(c) = inner.strip_suffix('*') {
        (c, "*")
    } else if let Some(c) = inner.strip_suffix('%') {
        (c, "%")
    } else {
        (inner, "")
    };
    let key = match markers {
        "%*" | "%" => "yellow",
        "*" => "green",
        _ if core.starts_with('-') => "blue",
        _ => "red",
    };
    format!("{open}{}{markers}{close}", color.c(key, core))
}

/// Real `output.py::_append_slot` + `_append_repository`, which decorate
/// the bracket-line version *only at `emerge -pv`* (`verbosity == 3`):
/// `:slot` (plus `/sub_slot` when it differs from `slot`) and `::repo`.
/// `show_slot` carries real `_append_slot`'s own gate -- `new_slot`, or
/// any package involved in this line (the entry or one of its
/// `oldbest` refs) has a slot/sub-slot other than `0/0`. Real portage
/// omits `::repo` only under `--quiet-repo-display` (not modelled here --
/// its default is off, so `::repo` is always shown at `-pv`).
fn decorate_version(
    version: &str,
    slot: &str,
    sub_slot: &str,
    repo: &str,
    show_slot: bool,
) -> String {
    let mut s = String::from(version);
    if show_slot {
        s.push(':');
        s.push_str(slot);
        if slot != sub_slot {
            s.push('/');
            s.push_str(sub_slot);
        }
    }
    s.push_str("::");
    s.push_str(repo);
    s
}

/// Renders the ` USE="…"` (and `VAR="…"` per USE_EXPAND group) suffix.
///
/// Real `_DisplayConfig`: `print_use_string = verbosity != 1`, and real
/// default `emerge -p` verbosity is 2 -- so the USE line is *not*
/// `-v`-gated. What `-v` (verbosity 3) actually changes is `all_flags`,
/// i.e. *which* flags render: `emerge -pv` uses
/// `GraphEntry::use_expand_display` (every flag, unchanged ones plain,
/// plus the `(-flag%)` removed-from-IUSE list); plain `emerge -p` uses
/// `use_expand_display_p`, where `_create_use_string` leaves an
/// *unchanged* flag omitted -- so for a `New` package (`is_new` renders
/// everything) the `-p` list equals the `-pv` list, and for a
/// `Reinstall`/`Upgrade`/`Downgrade` only the changed flags
/// (`flag%*`/`flag*`/`-flag%`/`-flag*`) show, often none.
fn use_suffix(entry: &GraphEntry, verbose: bool, alphabetical: bool, color: &Colorizer) -> String {
    let display = if verbose {
        &entry.use_expand_display
    } else {
        &entry.use_expand_display_p
    };
    if display.is_empty() {
        return String::new();
    }
    // Real `output.py:_display_use`: `USE="…"` first, then one `VAR="…"`
    // per non-hidden USE_EXPAND group, each already rendered and ordered
    // (enabled flags first, then disabled) by
    // `portage_repo::build_use_expand_display`. With `--alphabetical`,
    // real `_create_use_string` instead joins one combined list sorted
    // by bare flag name -- reproduced here by re-sorting each group's
    // already-rendered tokens. Colour (real `_create_use_string`'s own
    // `red`/`green`/`blue`/`yellow`) is applied per token *after* the
    // sort, so the `--alphabetical` sort key still sees plain tokens.
    let groups: Vec<String> = display
        .iter()
        .map(|(name, rendered)| {
            let mut toks: Vec<&str> = rendered.split(' ').collect();
            if alphabetical {
                // Real `_create_use_string`'s combined-list sort is
                // `_alnum_sort_key` (natural), same as the non-alphabetical
                // within-group sort.
                toks.sort_by_key(|t| portage_repo::alnum_sort_key(use_flag_sort_key(t)));
            }
            let body = toks
                .iter()
                .map(|t| colorize_use_token(t, color))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{name}=\"{body}\"")
        })
        .collect();
    // Real `print_messages`: `myprint += " " + self.verboseadd` -- a
    // single space joins the USE display to the line, which already ends
    // with the (possibly empty) `oldbest` slot's own trailing space.
    format!(" {}", groups.join(" "))
}

/// Real `lib/_emerge/resolver/output.py:841-862`'s own `darkgreen("to " +
/// pkg.root)` suffix: an entry that builds against the running root
/// rather than the target `ROOT` (`GraphEntry::targets_running_root`,
/// `--root-deps`'s own real `ESYSROOT`-vs-`ROOT` distinction) is
/// annotated with where it actually installs -- exactly as real portage
/// annotates any entry whose own `pkg.root_config.settings["ROOT"] !=
/// "/"`. Deliberately narrower than that real gate, though: this pilot
/// annotates *only* the running-root build entries, never every entry
/// merged under a non-`/` `ROOT`. Porting the real gate literally would
/// make every fixture test emit its own non-deterministic `mktemp -d`
/// `ROOT` path, breaking the shared contract suite's determinism -- the
/// same tension the parent `--root-deps` slice resolved by scoping its
/// behavior as strictly opt-in machinery. Empty for every ordinary
/// `ROOT`-targeted entry, and empty (defensively) if the caller somehow
/// has a `targets_running_root` entry but no running-root path in hand.
/// Returned bare (`"to /"`, no leading space) -- real `output.py:856-861`
/// places it right after the always-present space that follows the
/// package string, with `oldbest` (when non-empty) getting its own
/// trailing space before it; `print_entry_line`'s own `emit` reproduces
/// that spacing.
fn root_suffix(entry: &GraphEntry, running_root: Option<&Path>) -> String {
    match (entry.targets_running_root, running_root) {
        (true, Some(root)) => format!("to {}", root.display()),
        _ => String::new(),
    }
}

/// Real `math.ceil(num_bytes / 1024)` KiB (`portage.localization.
/// localized_size`) -- "always round up, so that small files don't end
/// up as '0 KiB'". Real portage additionally applies `LC_NUMERIC`
/// thousands grouping to the KiB count; this pilot doesn't -- only
/// observable above 999 KiB of downloads, which no fixture reaches, and
/// a locale-dependent separator would break the contract suite's
/// byte-exact determinism. Always `KiB`, never `MiB`/`GiB` (real
/// `localized_size` is the same -- its docstring: "The output will be in
/// kibibytes").
fn localized_size(bytes: u64) -> String {
    format!("{} KiB", bytes.div_ceil(1024))
}

/// Real `_PackageCounters.__str__` (`output_helpers.py`), the trailing
/// `Total: …` summary line real `output.py::print_verbose` emits via
/// `writemsg_stdout(f"\n{self.counters}\n")` -- gated, in real portage
/// too, on `verbosity == 3` (i.e. `-v`), never plain `-p`. Now includes
/// `, Size of downloads: …` (real `_calc_size`/`counters.totalsize`, via
/// `GraphEntry::download_files`, deduped by filename across the graph
/// like real `myfetchlist`) and the `\nFetch Restriction: N package[s][
/// (M unsatisfied)]` line (from `GraphEntry::fetch_restrict` /
/// `fetch_restrict_satisfied`). The `Conflict:` line's own `(N
/// unsatisfied)`/`(all satisfied)` suffix is still dropped -- this pilot
/// resolves no blocker (its whole blocker story is "report, don't
/// enforce", see `resolve_pretend_graph`'s doc comment, portage-repo),
/// so it can't honestly classify one. A top-level package suppressed by
/// `--onlydeps` isn't in real's merge list at all (`pkg_info.ordered`),
/// so it isn't counted here either.
fn package_counters_summary(
    entries: &[GraphEntry],
    top_level_pkgs: &HashSet<(String, String)>,
    onlydeps: bool,
    color: &Colorizer,
) -> String {
    let plural = |n: u64| if n > 1 { "s" } else { "" };
    let (mut upgrades, mut downgrades, mut new, mut newslot, mut reinst) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut binary, mut interactive, mut blocks) = (0u64, 0u64, 0u64);
    let (mut restrict_fetch, mut restrict_fetch_satisfied) = (0u64, 0u64);
    let mut totalsize: u64 = 0;
    let mut fetched: HashSet<&str> = HashSet::new();
    for entry in entries {
        blocks += entry.blockers.len() as u64;
        let suppressed =
            onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
        if suppressed {
            continue;
        }
        let merge_bound = match &entry.outcome {
            PretendOutcome::New { .. } => {
                if entry.new_slot {
                    newslot += 1;
                } else {
                    new += 1;
                }
                true
            }
            PretendOutcome::Upgrade { .. } => {
                upgrades += 1;
                true
            }
            PretendOutcome::Downgrade { .. } => {
                downgrades += 1;
                true
            }
            PretendOutcome::Reinstall { .. } => {
                reinst += 1;
                true
            }
            PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate => false,
        };
        if merge_bound {
            if entry.source == portage_repo::CandidateSource::Binary {
                binary += 1;
            }
            if entry.interactive {
                interactive += 1;
            }
            if entry.fetch_restrict {
                restrict_fetch += 1;
            }
            if entry.fetch_restrict_satisfied {
                restrict_fetch_satisfied += 1;
            }
            // Real `_calc_size`: sum the bytes still to fetch, counting
            // a shared distfile once (real `myfetchlist`).
            for (name, size) in &entry.download_files {
                if fetched.insert(name.as_str()) {
                    totalsize += size;
                }
            }
        }
    }

    // Real `total_installs = upgrades + downgrades + newslot + new + reinst`.
    let total = upgrades + downgrades + newslot + new + reinst;
    let mut out = format!(
        "Total: {total} package{}",
        if total != 1 { "s" } else { "" }
    );
    let mut details: Vec<String> = Vec::new();
    if upgrades > 0 {
        details.push(format!("{upgrades} upgrade{}", plural(upgrades)));
    }
    if downgrades > 0 {
        details.push(format!("{downgrades} downgrade{}", plural(downgrades)));
    }
    if new > 0 {
        details.push(format!("{new} new"));
    }
    if newslot > 0 {
        details.push(format!("{newslot} in new slot{}", plural(newslot)));
    }
    if reinst > 0 {
        details.push(format!("{reinst} reinstall{}", plural(reinst)));
    }
    if binary > 0 {
        details.push(format!(
            "{binary} {}",
            if binary > 1 { "binaries" } else { "binary" }
        ));
    }
    if interactive > 0 {
        // Real `_PackageCounters.__str__`: `colorize("WARN", "interactive")`
        // -- only the word, not the count.
        details.push(format!("{interactive} {}", color.c("WARN", "interactive")));
    }
    if total != 0 {
        out.push_str(&format!(" ({})", details.join(", ")));
    }
    // Real `__str__`: `f", Size of downloads: {localized_size(self.totalsize)}"`
    // -- appended to the `Total:` line unconditionally.
    out.push_str(&format!(
        ", Size of downloads: {}",
        localized_size(totalsize)
    ));
    if restrict_fetch > 0 {
        out.push_str(&format!(
            "\nFetch Restriction: {restrict_fetch} package{}",
            plural(restrict_fetch)
        ));
        if restrict_fetch_satisfied < restrict_fetch {
            // Real `_PackageCounters.__str__`: `bad(f" (N unsatisfied)")`
            // -- the whole parenthetical is red (`bad` = `BAD` = red).
            out.push_str(&color.c(
                "BAD",
                &format!(
                    " ({} unsatisfied)",
                    restrict_fetch - restrict_fetch_satisfied
                ),
            ));
        }
    }
    if blocks > 0 {
        out.push_str(&format!("\nConflict: {blocks} block{}", plural(blocks)));
    }
    out
}

/// Real `ResolverOutput._blockers` (`output.py:75-123`): one
/// `[blocks B     ] <resolved> ("<atom>" is {hard,soft} blocking
/// <parents>)` line per blocker on `entry`. Purely informational (see
/// `resolve_pretend_graph`'s doc comment) -- v1 neither refuses nor
/// changes the exit code for a blocker match. Collected into a `Vec`
/// rather than printed inline: real `Display` gathers blocker lines
/// while walking the entries and prints them as one group *after* every
/// package line (real `output.py::display` -> `print_messages()` then
/// `print_blockers()`).
///
/// This pilot only ever reports an *unsatisfied* blocker (it never
/// resolves one away), so real `blocker.satisfied` is always `false`
/// here: the bracket letter is always the red `B` / style `PKG_BLOCKER`,
/// never the teal `b` / `PKG_BLOCKER_SATISFIED` branch. `resolved` is
/// real `dep_expand(str(atom).lstrip("!"))` -- a category-qualification
/// only, and every pilot blocker atom is already `cat/pkg[...]`, so it
/// reduces to stripping the leading `!`/`!!`. Real's `(is <desc>
/// <parents>)` alternative (`self.resolved == blocker.atom`) is
/// unreachable -- `resolved` drops the `!` while `blocker.atom` keeps
/// it. Real `_blockers` appends `empty_space_in_brackets()` after the
/// five-space `B     ` pad, and that adds the mask column's own space
/// whenever `verbosity > 1` -- true at real portage's default `emerge
/// -p` verbosity of 2, so it's always present here (this pilot has no
/// `--quiet`).
fn format_blocker_lines(entry: &GraphEntry, owner_version: &str, color: &Colorizer) -> Vec<String> {
    let style = "PKG_BLOCKER";
    let pad = "      ";
    entry
        .blockers
        .iter()
        .map(|b| {
            let resolved = b.atom_str.trim_start_matches('!');
            let desc = if b.strong {
                "hard blocking"
            } else {
                "soft blocking"
            };
            let parents = format!("{}/{}-{owner_version}", entry.category, entry.package);
            format!(
                "[{} {}{pad}] {}{}",
                color.c(style, "blocks"),
                color.c(style, "B"),
                color.c(style, resolved),
                color.c(style, &format!(" (\"{resolved}\" is {desc} {parents})")),
            )
        })
        .collect()
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
    oneshot: bool,
    verbose: bool,
    alphabetical: bool,
    columns: bool,
    columnwidth: i64,
    running_root: Option<&Path>,
    color: &Colorizer,
    system_atoms: &[String],
    world_atoms: &[String],
    blocker_lines: &mut Vec<String>,
) {
    let onlydeps_suppressed =
        onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    // Real `Display.check_system_world` (`output.py`): `world` when this
    // package already matches an atom in `var/lib/portage/world`, OR
    // when it's a directly-requested target (a "favorite") that
    // `create_world_atom` would actually add -- i.e. NOT `--oneshot`/
    // `--onlydeps` (real `_DisplayConfig.oneshot`), and not an unslotted
    // `@system` member (real "unslotted system packages will not be
    // stored in world"). `system` = matches a `@system` atom (slot-
    // qualified `@system` atoms are matched version-only -- a cosmetic-
    // only miss, colour only). The full `create_world_atom` slot/repo/
    // virtual logic is a documented cut (see `update_world_file`).
    let binary = entry.source == portage_repo::CandidateSource::Binary;
    let is_favorite = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
    let classify = |version: &str| -> (bool, bool) {
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let matches_any = |atoms: &[String]| {
            atoms
                .iter()
                .any(|a| match_from_list(a, &[cpv.as_str()]).is_some_and(|m| !m.is_empty()))
        };
        let system = matches_any(system_atoms);
        let would_add_to_world = is_favorite && !(oneshot || onlydeps) && !(system && unslotted);
        let world = would_add_to_world || matches_any(world_atoms);
        (system, world)
    };
    // Real `output.py:841-862`'s own `to <root>` annotation for a
    // running-root build entry -- empty for every ordinary entry (see
    // `root_suffix`'s own doc comment). Placed right before `use_suffix`
    // in each arm below, matching real portage's own ordering
    // (`pkg_str + " " + oldbest + "to " + pkg.root`, with the USE display
    // coming later on the same line) -- though in practice a
    // `targets_running_root` entry always has an empty `use_flags_display`
    // anyway (a documented cut, see `GraphEntry::targets_running_root`).
    let root = root_suffix(entry, running_root);
    // Real --pretend's own bracket word: literally `pkg.type_name`
    // (`lib/_emerge/RootConfig.py`'s own `pkg_tree_map`, the exact
    // two strings `"ebuild"`/`"binary"` this pilot's own
    // `CandidateSource` mirrors) -- a binary merge prints
    // `"[binary"`, never `"[ebuild"`, regardless of outcome.
    let bracket = match entry.source {
        portage_repo::CandidateSource::Binary => "binary",
        portage_repo::CandidateSource::Ebuild => "ebuild",
    };
    // The fixed-width `attr_display` field flags this entry contributes,
    // shared by every merge outcome below (see `attr_display_field`).
    // `force_reinstall` is always `false` here -- this pilot has no
    // `arg.force_reinstall` concept. `remote_binary` (the `g` column) is
    // `entry.remote_binary` -- real `attr_display.remote_binary =
    // pkg.remote` for a `--getbinpkg` binary not yet in `$PKGDIR`.
    let field = |new: bool, new_slot: bool, replace: bool, new_version: bool, downgrade: bool| {
        attr_display_field(
            entry.interactive,
            new,
            false,
            new_slot,
            replace,
            entry.fetch_restrict && !entry.fetch_restrict_satisfied,
            entry.fetch_restrict_satisfied,
            entry.remote_binary,
            new_version,
            downgrade,
            entry.keyword_mask,
            color,
        )
    };
    // `emerge -pv` (verbosity 3) decorates the bracket cpv *and* every
    // `[old-ver]` with `:slot`/`::repo` (real `_append_slot` /
    // `_append_repository` / `convert_myoldbest`). `show_slot` is real
    // `_append_slot`'s own gate, computed once for the whole line.
    let entry_slot = entry.slot.as_deref().unwrap_or("0");
    let entry_sub = entry.sub_slot.as_deref().unwrap_or("0");
    let entry_repo = entry.repo_name.as_deref().unwrap_or("");
    let is_non00 = |s: &str, ss: &str| format!("{s}/{ss}") != "0/0";
    let show_slot = entry.new_slot
        || is_non00(entry_slot, entry_sub)
        || entry.oldbest.iter().any(|r| is_non00(&r.slot, &r.sub_slot));
    // The version as displayed in the bracket line: bare at `-p`,
    // `:slot::repo`-decorated at `-pv`.
    let disp_version = |v: &str| -> String {
        if verbose {
            decorate_version(v, entry_slot, entry_sub, entry_repo, show_slot)
        } else {
            v.to_string()
        }
    };
    // Real `convert_myoldbest`: `blue("[" + ", ".join(versions) + "]")`,
    // each version `-r0`-stripped and (at `-pv`) `:slot::repo`-decorated
    // with *its own* slot/sub_slot/repo. Empty string when there's no
    // `oldbest` (a brand-new `New`, a `Reinstall`).
    let oldbest_str = || -> String {
        if entry.oldbest.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = entry
            .oldbest
            .iter()
            .map(|r| {
                let v = r.version.strip_suffix("-r0").unwrap_or(&r.version);
                if verbose {
                    decorate_version(v, &r.slot, &r.sub_slot, &r.repo, show_slot)
                } else {
                    v.to_string()
                }
            })
            .collect();
        format!("[{}]", parts.join(", "))
    };
    // One merge line, shared by `New`/`Upgrade`/`Downgrade`/`Reinstall`.
    // Real `_set_no_columns`: `f"[{type} {attr}] {indent}{pkg_str}
    // {oldbest}"` -- the space before `oldbest` is always there even when
    // `oldbest` is empty. The running-root `to <root>` suffix (real
    // `output.py:856-861`) and the `USE="…"` display (real
    // `print_messages`' own `" " + verboseadd`) follow, each already
    // carrying its own leading space via `root_suffix`/`use_suffix`.
    let emit = |f: &str, version: &str| {
        if onlydeps_suppressed {
            return;
        }
        let (system, world) = classify(version);
        let disp_ver = disp_version(version);
        let oldbest = oldbest_str();
        let use_str = use_suffix(entry, verbose, alphabetical, color);
        // Real `output.py::verbose_size` (`conf.verbosity == 3` only):
        // `verboseadd += localized_size(mysize)` -- the bytes still to
        // fetch, appended after the USE string. This pilot renders it
        // only for a `--getbinpkg` remote binary (the one case that's
        // ever non-zero here -- an ebuild's distfiles / a local `$PKGDIR`
        // binary are already present, so real would show a bare ` 0 KiB`
        // that this pilot's `-pv` lines have always omitted; closing that
        // wider gap would re-pin every `-pv` assertion and is left out).
        let size_suffix = if verbose && entry.remote_binary {
            let bytes: u64 = entry.download_files.iter().map(|(_, s)| s).sum();
            format!(" {}", localized_size(bytes))
        } else {
            String::new()
        };
        // Real `output.py:856-861`: the running-root suffix is
        // `darkgreen("to " + pkg.root)`.
        let root_col = |r: &str| {
            if r.is_empty() {
                String::new()
            } else {
                color.c("darkgreen", r)
            }
        };
        if columns {
            let root_str = if root.is_empty() {
                String::new()
            } else {
                format!(" {}", root_col(&root))
            };
            println!(
                "{}{root_str}{use_str}{size_suffix}",
                columns_line(
                    bracket,
                    f,
                    indent,
                    &entry.category,
                    &entry.package,
                    &disp_ver,
                    &oldbest,
                    columnwidth,
                    color,
                    binary,
                    system,
                    world,
                )
            );
            return;
        }
        // Real `_set_no_columns`: `f"[{pkgprint(type)} {attr}]
        // {indent}{pkgprint(pkg_str)} {oldbest}"`.
        let bword = color.pkgprint(bracket, binary, system, world);
        let pkg_str = color.pkgprint(
            &format!("{}/{}-{disp_ver}", entry.category, entry.package),
            binary,
            system,
            world,
        );
        let mut tail = String::from(" ");
        if !oldbest.is_empty() {
            tail.push_str(&color.c("blue", &oldbest));
        }
        if !root.is_empty() {
            if !oldbest.is_empty() {
                tail.push(' ');
            }
            tail.push_str(&root_col(&root));
        }
        tail.push_str(&use_str);
        tail.push_str(&size_suffix);
        println!("[{bword} {f}] {indent}{pkg_str}{tail}");
    };
    match &entry.outcome {
        PretendOutcome::New { version } => {
            // Real `_get_installed_best`: brand-new -> `attr.new`; into a
            // fresh slot while another slot is installed -> `attr.new`
            // *and* `attr.new_slot` (`GraphEntry::new_slot`). No oldbest
            // for a brand-new package; the other-slot version list real
            // portage shows for a new-slot install (`myoldbest =
            // installed_versions`) is deferred to a follow-up increment
            // (this pilot doesn't carry the other-slot versions on the
            // entry yet).
            emit(&field(true, entry.new_slot, false, false, false), version);
            blocker_lines.extend(format_blocker_lines(entry, version, color));
        }
        PretendOutcome::Upgrade { from: _, to } => {
            // Real: an in-slot version bump -> `attr.new_version` only
            // (the exact new cpv isn't installed, so `attr.replace`
            // stays clear -> `U`, no `R`). oldbest = the in-slot
            // installed version(s) (`myinslotlist`), from `entry.oldbest`.
            emit(&field(false, false, false, true, false), to);
            blocker_lines.extend(format_blocker_lines(entry, to, color));
        }
        PretendOutcome::Downgrade { from: _, to } => {
            // Real: in-slot downgrade -> `attr.new_version` *and*
            // `attr.downgrade` (`U` and `D`). oldbest as for `Upgrade`.
            emit(&field(false, false, false, true, true), to);
            blocker_lines.extend(format_blocker_lines(entry, to, color));
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags: _,
            deps_changed: _,
            slot_changed: _,
            rebuilt_binary: _,
            new_repo: _,
        } => {
            // Real `_get_installed_best`: the exact cpv is already
            // installed -> `attr.replace` (the yellow `R` at column 2),
            // and `myoldbest` stays empty for a same-slot/same-repo
            // reinstall -> no `[from]`. Real portage's `-pv` shows no
            // inline "why" for a reinstall at all -- the pilot's former
            // `(reinstall for changed …)` prose is dropped here (the
            // USE diff still shows in the `USE="…"` section for
            // `--changed-use`; `--changed-deps`/`--changed-slot` reasons
            // are genuinely invisible in real `-pv` too).
            emit(&field(false, false, true, false, false), version);
            blocker_lines.extend(format_blocker_lines(entry, version, color));
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
            // `--autounmask-use`'s own opt=-aware *parent* flip -- see
            // GraphEntry::parent_use_suggestion's own doc comment. When
            // that flip resolves the dep, `resolve_pretend_graph` applies
            // it and this entry is no longer NoVisibleCandidate, so this
            // arm is a fallback hint for the (currently unreachable) case
            // where the suggestion exists but wasn't applied.
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
/// (now real portage's dependency-first merge order, per
/// `topological_merge_order` -- so top-level roots that depend on each
/// other appear dep-first here too; real portage feeds `_tree_display`
/// `reversed(mylist)`, an implementation detail this top-down walk from
/// roots doesn't need). A node already rendered once (anywhere in the tree,
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
/// each level: `entries`' own order when true (now merge order rather
/// than raw BFS discovery -- still "not sorted" per se, just whatever
/// `topological_merge_order` produced) versus
/// alphabetical-by-`(category, package)` when false, this pilot's own
/// deterministic default. Any entry never reached from a root at all
/// (shouldn't normally happen -- every non-root entry's own
/// `required_by` should trace back to one) is still printed, unindented,
/// after the tree itself, rather than silently dropped -- this pilot's
/// own "never silently lose information" invariant, seen already for
/// slot conflicts and unresolvable dependencies.
#[allow(clippy::too_many_arguments)]
fn print_tree(
    entries: &[GraphEntry],
    top_level_pkgs: &HashSet<(String, String)>,
    onlydeps: bool,
    oneshot: bool,
    unordered_display: bool,
    verbose: bool,
    alphabetical: bool,
    running_root: Option<&Path>,
    color: &Colorizer,
    system_atoms: &[String],
    world_atoms: &[String],
    blocker_lines: &mut Vec<String>,
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
        oneshot: bool,
        verbose: bool,
        alphabetical: bool,
        running_root: Option<&'a Path>,
        color: &'a Colorizer,
        system_atoms: &'a [String],
        world_atoms: &'a [String],
    }

    fn render(
        i: usize,
        depth: u32,
        ctx: &TreeCtx,
        rendered: &mut HashSet<usize>,
        blocker_lines: &mut Vec<String>,
    ) {
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
            ctx.oneshot,
            ctx.verbose,
            ctx.alphabetical,
            false,
            130,
            ctx.running_root,
            ctx.color,
            ctx.system_atoms,
            ctx.world_atoms,
            blocker_lines,
        );
        let key = (
            ctx.entries[i].category.clone(),
            ctx.entries[i].package.clone(),
        );
        if let Some(kids) = ctx.children.get(&key) {
            for &child in kids {
                render(child, depth + 1, ctx, rendered, blocker_lines);
            }
        }
    }

    let ctx = TreeCtx {
        entries,
        children: &children,
        top_level_pkgs,
        onlydeps,
        oneshot,
        verbose,
        alphabetical,
        running_root,
        color,
        system_atoms,
        world_atoms,
    };
    let mut rendered: HashSet<usize> = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        if top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone())) {
            render(i, 0, &ctx, &mut rendered, blocker_lines);
        }
    }

    // Safety net, not expected to ever trigger in practice (see this
    // function's own doc comment) -- prints anything the tree walk
    // somehow never reached, flat, rather than silently dropping it.
    for (i, entry) in entries.iter().enumerate() {
        if !rendered.contains(&i) {
            print_entry_line(
                entry,
                "",
                top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                alphabetical,
                false,
                130,
                running_root,
                color,
                system_atoms,
                world_atoms,
                blocker_lines,
            );
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
    merge_order: usize,
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
    running_root: Option<&Path>,
) -> String {
    let requested = top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
    let mut fields: Vec<String> = vec![
        format!("\"category\":{}", json_string(&entry.category)),
        format!("\"package\":{}", json_string(&entry.package)),
        // The entry's 0-based position in real portage's dependency-first
        // merge order (`portage_repo::topological_merge_order`). The
        // `entries` array is already emitted in this order -- the field
        // is here so a consumer that re-sorts or filters the array keeps
        // the schedule.
        format!("\"merge_order\":{merge_order}"),
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
    // Real `output.py`'s own `S` bracket column, exposed unconditionally
    // (like every other `--json` field, see this module's own doc
    // comment): `true` for a `New` into a slot the package isn't
    // installed in while another slot of it is (`GraphEntry::new_slot`).
    if let PretendOutcome::New { .. } = &entry.outcome {
        fields.push(format!("\"new_slot\":{}", entry.new_slot));
    }
    // Real `output.py:833`'s own `I` bracket column, exposed
    // unconditionally: `true` for a merge-bound entry whose evaluated
    // `PROPERTIES` contains `interactive` (`GraphEntry::interactive`).
    if matches!(
        entry.outcome,
        PretendOutcome::New { .. }
            | PretendOutcome::Upgrade { .. }
            | PretendOutcome::Downgrade { .. }
            | PretendOutcome::Reinstall { .. }
    ) {
        fields.push(format!("\"interactive\":{}", entry.interactive));
        // Real `output.py:633`'s own `f`/`F` fetch-restrict column
        // (`GraphEntry::fetch_restrict` / `fetch_restrict_satisfied`).
        fields.push(format!("\"fetch_restrict\":{}", entry.fetch_restrict));
        fields.push(format!(
            "\"fetch_restrict_satisfied\":{}",
            entry.fetch_restrict_satisfied
        ));
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
    // `--root-deps`'s own running-root build entries (see `root_suffix`'s
    // own doc comment and `GraphEntry::targets_running_root`): the same
    // `to <root>` distinction the plain-text output carries, as an
    // explicit field -- the running-root path string for such an entry,
    // `null` for every ordinary `ROOT`-targeted one. `null` (rather than
    // absent) universally, same shape as `slot` above.
    fields.push(format!(
        "\"builds_against_running_root\":{}",
        if entry.targets_running_root {
            running_root
                .map(|r| json_string(&r.display().to_string()))
                .unwrap_or_else(|| "null".to_string())
        } else {
            "null".to_string()
        }
    ));
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
fn autounmask_change_to_json(change: &portage_repo::AutounmaskChange) -> String {
    let chain: Vec<String> = change.dep_chain.iter().map(|l| json_string(l)).collect();
    format!(
        "{{\"atom\":{},\"token\":{},\"dep_chain\":[{}]}}",
        json_string(&change.atom),
        json_string(&change.token),
        chain.join(",")
    )
}

#[allow(clippy::too_many_arguments)]
fn print_json(
    entries: &[GraphEntry],
    slot_conflicts: &[SlotConflict],
    changed_deps_report: &[ChangedDepsReportEntry],
    autounmask_keyword_changes: &[portage_repo::AutounmaskChange],
    autounmask_use_changes: &[portage_repo::AutounmaskChange],
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
    running_root: Option<&Path>,
) {
    let entries_json: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| entry_to_json(e, i, top_level_pkgs, verbose, running_root))
        .collect();
    let conflicts_json: Vec<String> = slot_conflicts.iter().map(slot_conflict_to_json).collect();
    let changed_deps_report_json: Vec<String> = changed_deps_report
        .iter()
        .map(changed_deps_report_entry_to_json)
        .collect();
    let autounmask_kw_json: Vec<String> = autounmask_keyword_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    let autounmask_use_json: Vec<String> = autounmask_use_changes
        .iter()
        .map(autounmask_change_to_json)
        .collect();
    println!(
        "{{\"entries\":[{}],\"slot_conflicts\":[{}],\"changed_deps_report\":[{}],\"autounmask_keyword_changes\":[{}],\"autounmask_use_changes\":[{}]}}",
        entries_json.join(","),
        conflicts_json.join(","),
        changed_deps_report_json.join(","),
        autounmask_kw_json.join(","),
        autounmask_use_json.join(",")
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
             --unmerge/-C, --depclean/-c, --prune/-P, \
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

/// Real `Scheduler._world_atom` + `depgraph.saveNomergeFavorites`: after
/// a successful non-`--pretend` `emerge <atom>`, each directly-requested
/// **plain** target atom (not a dependency, not a `@set`) is recorded in
/// `<root>/var/lib/portage/world` -- whether it merged or was already
/// installed. `--oneshot`/`--onlydeps` suppress this entirely (real
/// `_world_atom`'s own early-return set). The recorded atom is the
/// argument's own `cat/pkg` (plus `::repo` when the arg carried one) --
/// real `create_world_atom`'s full slot-atom / system-virtual logic is a
/// documented v1 cut, so the pilot's world file is `cat/pkg`-granular.
/// An unslotted `@system` member is not recorded (real "unslotted system
/// packages will not be stored in world"). Already-present atoms are
/// left alone; when anything is added the file is rewritten sorted +
/// deduplicated (real `WorldSelectedPackagesSet.write`). Prints the real
/// `>>> Recording <atom> in "world" favorites file...` line per addition.
fn update_world_file(
    root: &Path,
    target_atoms: &[&str],
    entries: &[GraphEntry],
    system_atoms: &[String],
    oneshot: bool,
    onlydeps: bool,
) -> Result<(), String> {
    if oneshot || onlydeps {
        return Ok(());
    }
    let mut current = read_world_atoms(root)?;
    let before: std::collections::HashSet<String> = current.iter().cloned().collect();
    let mut added = false;

    for raw in target_atoms {
        if raw.starts_with('@') {
            // A `@set` target belongs in `world_sets`, not `world` --
            // a v1 cut (see `read_world_sets`).
            continue;
        }
        let Some(atom) = parse_atom(raw) else {
            continue;
        };
        // The resolved entry for this cp (a top-level `NoVisibleCandidate`
        // aborts the whole resolve before we get here, so it's present).
        let Some(entry) = entries
            .iter()
            .find(|e| e.category == atom.category && e.package == atom.package)
        else {
            continue;
        };
        // Real "unslotted system packages will not be stored in world".
        let cpv_any = format!("{}/{}-0", atom.category, atom.package);
        let in_system = system_atoms
            .iter()
            .any(|a| match_from_list(a, &[cpv_any.as_str()]).is_some_and(|m| !m.is_empty()));
        let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
        if in_system && unslotted && atom.repo.is_none() {
            continue;
        }

        let mut world_atom = format!("{}/{}", atom.category, atom.package);
        if let Some(repo) = &atom.repo {
            world_atom.push_str("::");
            world_atom.push_str(repo);
        }
        if before.contains(&world_atom) {
            continue;
        }
        println!(">>> Recording {world_atom} in \"world\" favorites file...");
        current.push(world_atom);
        added = true;
    }

    if added {
        current.sort();
        current.dedup();
        let path = root.join("var/lib/portage/world");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut body = current.join("\n");
        body.push('\n');
        std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
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

/// Real `_unmerge_display`'s own `installed_sets` -- every custom set
/// directly or indirectly selected via `world_sets` (real
/// `WorldSelectedSetsSet`), paired with its *direct* atoms only (not
/// recursively flattened, unlike `resolve_custom_set` -- the "still
/// listed in the following package sets" warning names the set that
/// *directly* contains the package). BFS over the `@`-references, cycle-
/// guarded. A referenced-but-missing set is dropped silently here (real
/// portage `eerror`s "Unknown set" and moves on) -- a documented
/// narrowing.
fn collect_installed_sets(
    config_root: &Path,
    root: &Path,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = read_world_sets(root)?;
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let path = config_root.join("etc/portage/sets").join(&name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut direct: Vec<String> = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if let Some(nested) = line.strip_prefix('@') {
                queue.push(nested.to_string());
            } else {
                direct.push(line.to_string());
            }
        }
        out.push((name, direct));
    }
    Ok(out)
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

/// `emerge --unmerge` / `-C <atoms>` (with or without `--pretend`): real
/// `_emerge/unmerge.py::_unmerge_display` for `unmerge_action ==
/// "unmerge"`. With `pretend` this is a preview only; **without** it,
/// after the display, `execute_unmerge` really removes each `selected`
/// package (real `unmerge()`'s own removal loop -- see its own doc
/// comment). The `>>> These are the packages that would be unmerged:`
/// header is `--pretend`/`--ask`-gated in real portage
/// (`unmerge.py:195`); everything else in the block prints either way.
/// `--depclean`/`--prune` reuse this for their own preview and always
/// pass `pretend = true` (their real removal is a separate, larger
/// slice). Each target atom is
/// matched against the vdb (`installed_candidates` + `match_from_list`,
/// exactly real `vartree.dbapi.match`); every match goes into
/// `selected`, and every *other* installed version of the same
/// `category/package` becomes `omitted` (real `vartree.dep_match(cp)`
/// minus the selected/protected ones). `sys-apps/portage` itself is
/// force-`protected` with real portage's own "no valid reason for
/// Portage to unmerge itself" note (real `PORTAGE_PACKAGE_ATOM`). A
/// `@world`/`@system`/`@customset` target expands to its own atom list
/// first (the same machinery `run()` and `run_deselect` already use).
///
/// The "still listed in the following package sets" set-protection
/// warning (real `unmerge.py:355-447`, `EditablePackageSet` members
/// reached via `world_sets`) and the "is part of your system profile"
/// warning (real `cp in syslist`) are both real -- including the
/// higher-slot refinement (`unmerge.py:421-441`: an installed
/// higher-versioned instance of the same cp *in a different slot* that
/// also matches the set atom suppresses the warning for that set).
///
/// **Documented cuts** (real `_unmerge_display` still does these): the
/// "currently used Python interpreter" self-skip (real
/// `_dblink(cpv).isowner(portage._python_interpreter)`) -- a non-gap
/// for this pilot, whose `emerge` is a Rust binary with no Python
/// interpreter of its own to protect. The `--prune`/`--depclean`
/// variants have their own dedicated `run_prune_pretend`/
/// `run_depclean_pretend` and never really remove.
/// Real `unmerge.py:137-182`'s own installed-ebuild-path handling: an
/// `--unmerge`/`-C` argument that starts with `.` or `/`, or ends with
/// `.ebuild`, is a path into the vdb, not an atom. Returns `Ok(None)` if
/// `arg` isn't path-shaped, `Ok(Some("=cat/pkg-ver"))` for a valid vdb
/// entry (real portage also echoes that `=atom` to stdout, reproduced
/// here), or `Err(code)` after printing the matching diagnostic for a
/// bad path.
///
/// The path is resolved with `canonicalize` (real portage uses
/// `os.path.abspath`, which doesn't follow symlinks -- `canonicalize`
/// does, but it resolves the vdb root the same way, so `strip_prefix`
/// still works, and it is what actually keeps a symlinked test `ROOT`
/// working). Real portage's own stray `print(sp_absx)` / `print(absx)`
/// debug lines before the "not inside …; aborting" message (a raw
/// list repr -- clearly unintended output) are deliberately omitted.
fn resolve_vdb_path_arg(arg: &str, root: &Path) -> Result<Option<String>, ExitCode> {
    let path_shaped = arg.starts_with('.') || arg.starts_with('/') || arg.ends_with(".ebuild");
    if !path_shaped {
        return Ok(None);
    }
    let Ok(mut absx) = std::fs::canonicalize(arg) else {
        println!("\n!!! The path '{arg}' doesn't exist.\n");
        return Err(ExitCode::from(1));
    };
    // Real: `if sp_absx[-1][-7:] == ".ebuild": del sp_absx[-1]`.
    if absx
        .file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".ebuild"))
    {
        absx.pop();
    }
    if !absx.join("CONTENTS").exists() {
        println!("!!! Not a valid db dir: {}", absx.display());
        return Err(ExitCode::from(1));
    }
    let vdb =
        std::fs::canonicalize(root.join("var/db/pkg")).unwrap_or_else(|_| root.join("var/db/pkg"));
    let Ok(rel) = absx.strip_prefix(&vdb) else {
        println!("\n!!! {arg} is not inside {}; aborting.\n", vdb.display());
        return Err(ExitCode::from(1));
    };
    let rel = rel.to_string_lossy();
    // Real: `sp_absx_len <= sp_vdb_len` -> "cannot be inside …".
    if rel.is_empty() || !rel.contains('/') {
        println!(
            "\n!!! {arg} cannot be inside {}; aborting.\n",
            vdb.display()
        );
        return Err(ExitCode::from(1));
    }
    let atom = format!("={rel}");
    println!("{atom}");
    Ok(Some(atom))
}

/// Real `unmerge.py:355-447`'s "still listed in the following package
/// sets" check for one selected `category/package-version`: the names of
/// the user-editable sets (`installed_sets` -- `EditablePackageSet`s
/// reached via `world_sets`) that still directly list a matching atom,
/// *minus* any set whose matching atom is also satisfied by an installed
/// newer version of the same cp in a different slot (real
/// `unmerge.py:421-441`'s `higher_slot`: `pkg.slot_atom != inst_pkg.
/// slot_atom` after the descending-order `pkg >= inst_pkg` break --
/// removing this version leaves that set satisfied). Shared by
/// `run_unmerge_pretend` and `run_prune_pretend`, matching real
/// portage's own single `_unmerge_display` handling every
/// `unmerge_action`.
fn still_listed_parents<'a>(
    root: &Path,
    installed_sets: &'a [(String, Vec<String>)],
    cat: &str,
    pkg: &str,
    version: &str,
) -> Vec<&'a str> {
    let installed = portage_repo::installed_candidates(root, cat, pkg);
    let selected_slot = installed
        .iter()
        .find(|(v, _, _)| v == version)
        .map(|(_, s, _)| s.as_str());
    let covered_by_higher_slot = |atom_str: &str| -> bool {
        installed.iter().any(|(v, s, ss)| {
            portage_versions::vercmp(v, version).is_some_and(|c| c > 0)
                && Some(s.as_str()) != selected_slot
                && match_from_list(atom_str, &[format!("{cat}/{pkg}-{v}:{s}/{ss}").as_str()])
                    .is_some_and(|m| !m.is_empty())
        })
    };
    let candidate = format!("{cat}/{pkg}-{version}");
    let mut parents: Vec<&str> = Vec::new();
    for (set_name, atoms) in installed_sets {
        let listed = atoms.iter().any(|atom_str| {
            parse_atom(atom_str).is_some_and(|a| a.category == cat && a.package == pkg)
                && match_from_list(atom_str, &[candidate.as_str()]).is_some_and(|m| !m.is_empty())
                && !covered_by_higher_slot(atom_str)
        });
        if listed {
            parents.push(set_name);
        }
    }
    parents
}

fn run_unmerge_pretend(
    targets: &[&str],
    root: &Path,
    config_root: &Path,
    config: &portage_profile::Config,
    // Real `_unmerge_display`'s `ordered` flag (`unmerge.py:459`): when
    // `true` the per-package blocks are rendered in `targets` order and
    // *not* regrouped/re-sorted by `cat/pn`. Only `--depclean`'s own
    // topologically-sorted cleanlist sets this (`run_depclean_pretend`);
    // a plain `--unmerge`/`-C` from the CLI is always unordered.
    preserve_order: bool,
    // `false` for a real `emerge -C <atom>` (no `--pretend`): the
    // `>>> These are the packages that would be unmerged:` header is
    // suppressed and `execute_unmerge` runs after the display. `true`
    // for `-pC` and for the `--depclean`/`--prune` preview reuse.
    pretend: bool,
    color: &Colorizer,
) -> ExitCode {
    if targets.is_empty() {
        eprintln!("emerge: no package atoms given to --unmerge");
        return ExitCode::from(1);
    }

    // Expand every `@set` target into its member atoms first (real
    // `root_config.sets[s].getAtoms()` / `_iter_atoms_for_pkg`), so the
    // vdb-matching loop below only ever sees ordinary atoms. `@world` and
    // `@system` are the two built-in sets; anything else `@name` is a
    // custom set file (recursively expanded, cycle-guarded).
    let mut expanded: Vec<String> = Vec::new();
    // Real `root_config.setconfig.active` -- the sets the user passed as
    // `-C` targets. Excluded from the "still listed in package sets"
    // check below (their members are being removed from the set anyway).
    let mut active_sets: HashSet<String> = HashSet::new();
    for target in targets {
        if let Some(name) = target.strip_prefix('@') {
            active_sets.insert(name.to_string());
        }
        match *target {
            "@world" => match read_world_atoms(root) {
                Ok(atoms) => {
                    expanded.extend(atoms);
                    match read_world_sets(root) {
                        Ok(names) => {
                            for name in names {
                                let mut seen = HashSet::new();
                                match resolve_custom_set(config_root, &name, &mut seen) {
                                    Ok(atoms) => expanded.extend(atoms),
                                    Err(e) => {
                                        eprintln!("{e}");
                                        return ExitCode::from(1);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("emerge: {e}");
                            return ExitCode::from(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("emerge: {e}");
                    return ExitCode::from(1);
                }
            },
            "@system" => expanded.extend(config.system_packages.iter().cloned()),
            other if other.starts_with('@') => {
                let mut seen = HashSet::new();
                match resolve_custom_set(config_root, &other[1..], &mut seen) {
                    Ok(atoms) => expanded.extend(atoms),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                }
            }
            other => match resolve_vdb_path_arg(other, root) {
                Ok(Some(atom)) => expanded.push(atom),
                Ok(None) => expanded.push(other.to_string()),
                Err(code) => return code,
            },
        }
    }

    // Real `_unmerge_display` (`unmerge.py:195`) prints this header only
    // under `--pretend`/`--ask`, before the per-atom matching loop -- so
    // it shows even when nothing ends up selected, but a real `emerge -C`
    // removal run doesn't print it at all.
    if pretend {
        println!(
            "{}",
            color.c(
                "darkgreen",
                ">>> These are the packages that would be unmerged:"
            )
        );
    }

    // Real `PORTAGE_PACKAGE_ATOM` -- the one package `unmerge` always
    // refuses to select, moving it to `protected` with an eerror note.
    let portage_self = ("sys-apps".to_string(), "portage".to_string());

    // Per-`(category, package)` accumulators, real portage's own `pkgmap`
    // entry with its `selected`/`protected`/`omitted` sets, keyed by cp
    // so multiple atoms hitting the same cp merge (real `unordered`
    // dedup). `all_selected` dedups a version picked by two atoms.
    let mut per_cp: HashMap<(String, String), (Vec<String>, Vec<String>)> = HashMap::new();
    let mut all_selected: HashSet<(String, String, String)> = HashSet::new();
    let mut order: Vec<(String, String)> = Vec::new();

    for atom_str in &expanded {
        // A bare name (no `/`) borrows its category from the vdb: real
        // `vartree.dep_match`'s own "null category" lookup. Ambiguous
        // (installed under >1 category) is a hard error, real
        // `AmbiguousPackageName`.
        let matches: Vec<(String, String, String, String)> = if !atom_str.contains('/') {
            let mut found: Vec<(String, String, String, String)> = Vec::new();
            for (cat, pkg, version, slot) in installed_cp_versions(root) {
                if pkg == *atom_str {
                    found.push((cat, pkg, version, slot));
                }
            }
            let cats: HashSet<&String> = found.iter().map(|(c, _, _, _)| c).collect();
            if cats.len() > 1 {
                eprintln!(
                    "\n!!! The short package name \"{atom_str}\" is ambiguous. Please specify"
                );
                eprintln!("!!! one of the following fully-qualified package names instead:\n");
                let mut names: Vec<String> = cats
                    .into_iter()
                    .map(|c| format!("    {c}/{atom_str}"))
                    .collect();
                names.sort();
                for n in names {
                    println!("{n}");
                }
                return ExitCode::from(1);
            }
            found
        } else {
            let Some(atom) = parse_atom(atom_str) else {
                eprintln!("emerge: invalid atom {atom_str:?}");
                return ExitCode::from(1);
            };
            portage_repo::installed_candidates(root, &atom.category, &atom.package)
                .into_iter()
                .filter(|(version, slot, sub_slot)| {
                    let cs = format!(
                        "{}/{}-{version}:{slot}/{sub_slot}",
                        atom.category, atom.package
                    );
                    match_from_list(atom_str, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                })
                .map(|(version, slot, _sub)| {
                    (atom.category.clone(), atom.package.clone(), version, slot)
                })
                .collect()
        };

        if matches.is_empty() {
            println!("\n--- Couldn't find '{atom_str}' to unmerge.");
            continue;
        }

        for (cat, pkg, version, _slot) in matches {
            let cp = (cat.clone(), pkg.clone());
            let entry = per_cp.entry(cp.clone()).or_insert_with(|| {
                order.push(cp.clone());
                (Vec::new(), Vec::new())
            });
            let key = (cat, pkg, version.clone());
            if all_selected.insert(key) {
                entry.0.push(version);
            }
        }
    }

    if all_selected.is_empty() {
        println!("\n>>> No packages selected for removal by unmerge");
        return ExitCode::from(1);
    }

    // `sys-apps/portage` self-protection: real portage moves it out of
    // `selected` into `protected` and prints the note.
    if let Some((selected, protected)) = per_cp.get_mut(&portage_self) {
        if !selected.is_empty() {
            for v in selected.drain(..) {
                eprintln!(
                    "!!! Not unmerging package sys-apps/portage-{v} since there is no valid \
                     reason for Portage to unmerge itself."
                );
                all_selected.remove(&(portage_self.0.clone(), portage_self.1.clone(), v.clone()));
                protected.push(v);
            }
        }
    }
    // Recheck: the self-skip may have emptied the only selection.
    if all_selected.is_empty() {
        println!("\n>>> No packages selected for removal by unmerge");
        return ExitCode::from(1);
    }

    // Real `syslist = root_config.sets["system"].getAtoms()` -> the
    // `@system` cps, for the "is part of your system profile" warning.
    let syslist: HashSet<(String, String)> = config
        .system_packages
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    // Real `_unmerge_display`'s "still listed in the following package
    // sets" warning: a `selected` package that a user-editable set
    // (reached via `world_sets`) still lists would be re-pulled on the
    // next `@world` update, so it's flagged. Real `unmerge.py:421-441`
    // suppresses the flag for a set when an installed *higher-versioned*
    // instance of the same cp *in a different slot* also matches the set
    // atom (`pkg.slot_atom != inst_pkg.slot_atom` after the `pkg >=
    // inst_pkg` break -> "a newer version, other slot") -- removing this
    // version leaves the set satisfied by that one.
    let installed_sets: Vec<(String, Vec<String>)> = match collect_installed_sets(config_root, root)
    {
        Ok(sets) => sets
            .into_iter()
            .filter(|(name, _)| !active_sets.contains(name))
            .collect(),
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let mut selected_sorted: Vec<&(String, String, String)> = all_selected.iter().collect();
    selected_sorted.sort();
    for (cat, pkg, version) in selected_sorted {
        let mut parents = still_listed_parents(root, &installed_sets, cat, pkg, version);
        if !parents.is_empty() {
            parents.sort_unstable();
            println!(
                "{}",
                color.c(
                    "WARN",
                    &format!("Package {cat}/{pkg}-{version} is going to be unmerged,")
                )
            );
            println!(
                "{}",
                color.c("WARN", "but still listed in the following package sets:")
            );
            println!("    {}\n", parents.join(", "));
        }
    }

    let vercmp_key = |a: &String, b: &String| {
        portage_versions::vercmp(a, b)
            .map(|c| c.cmp(&0))
            .unwrap_or_else(|| a.cmp(b))
    };
    if !preserve_order {
        order.sort();
    }
    let mut all_selected_display: Vec<String> = Vec::new();
    // `(category, package, version)` of every `selected` version, in the
    // same order the preview blocks render -- real `unmerge()`'s own
    // removal loop walks `pkgmap` in exactly this order.
    let mut removal_list: Vec<(String, String, String)> = Vec::new();
    for cp in &order {
        let (selected, protected) = per_cp.get_mut(cp).unwrap();
        if selected.is_empty() {
            continue;
        }
        selected.sort_by(vercmp_key);
        protected.sort_by(vercmp_key);
        // `omitted` = every other installed version of this cp, real
        // `vartree.dep_match(cp)` minus selected/protected.
        let mut omitted: Vec<String> = portage_repo::installed_candidates(root, &cp.0, &cp.1)
            .into_iter()
            .map(|(v, _, _)| v)
            .filter(|v| !selected.contains(v) && !protected.contains(v))
            .collect();
        omitted.sort_by(vercmp_key);

        // Real `_unmerge_display`: `if not (protected or omitted) and cp
        // in syslist` -- a cp that would be *fully* removed and is a
        // `@system` member. To stderr (real `writemsg_level(...,
        // level=logging.WARNING)`).
        if protected.is_empty() && omitted.is_empty() && syslist.contains(cp) {
            eprintln!(
                "{}",
                color.c(
                    "BAD",
                    &format!(
                        "\n\n!!! '{}/{}' is part of your system profile.",
                        cp.0, cp.1
                    )
                )
            );
            eprintln!(
                "{}",
                color.c("WARN", "!!! Unmerging it may be damaging to your system.\n")
            );
        }

        println!("\n {}/{}", cp.0, cp.1);
        print_unmerge_row("selected", selected, color);
        print_unmerge_row("protected", protected, color);
        print_unmerge_row("omitted", &omitted, color);

        for v in selected.iter() {
            all_selected_display.push(format!("={}/{}-{v}", cp.0, cp.1));
            removal_list.push((cp.0.clone(), cp.1.clone(), v.clone()));
        }
    }

    all_selected_display.sort();
    println!(
        "\nAll selected packages: {}",
        all_selected_display.join(" ")
    );
    println!(
        "\n>>> {} packages are slated for removal.",
        color.c("UNMERGE_WARN", "'Selected'")
    );
    println!(
        ">>> {} and {} packages will not be removed.",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );

    // Real `unmerge()` (`unmerge.py:637-699`): once `_unmerge_display`
    // returns EX_OK and we're not in `--pretend`, the packages are
    // actually removed.
    if !pretend {
        return execute_unmerge(&removal_list, root, color);
    }
    ExitCode::SUCCESS
}

/// Real `_emerge/unmerge.py::unmerge`'s own removal loop, reached when
/// `emerge -C`/`--unmerge <atoms>` runs **without** `--pretend`: after
/// `_unmerge_display` (the preview above) returns, each `selected`
/// package is really removed -- `>>> Unmerging (N of M) <cpv>...`, then
/// `dblink.unmerge()` (`pkg_prerm` from that version's own vdb-saved
/// env -> delete its files -> `pkg_postrm`) + `dblink.delete()` (drop
/// the vdb dir), via `ebuild_merge::unmerge_one_installed`. On success
/// each removed package is deselected from the world file (real
/// `WorldSelectedPackagesSet.cleanPackage`, called per-package right
/// after its removal).
///
/// **v1 cuts:** no `CLEAN_DELAY` countdown (real `countdown(5,
/// ">>> Unmerging")`); no `--ask` prompt; `FEATURES=unmerge-backup` not
/// honored; the trailing `for s in setconfig.active: selected.remove(@s)`
/// pass is a no-op here (this pilot's world writer already drops `@set`
/// lines on any rewrite). A `pkg_prerm`/`pkg_postrm` failure is logged
/// and removal continues (`unmerge_one_installed`); real `unmerge()`
/// `sys.exit`s on a `portage.unmerge()` non-zero, but that return only
/// tracks the file-removal core, which the pilot still surfaces as a
/// hard `Err`.
fn execute_unmerge(
    removal_list: &[(String, String, String)],
    root: &Path,
    color: &Colorizer,
) -> ExitCode {
    let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));
    let options =
        ebuild_merge::MergeOptions::from_env(ebuild_phases::ShellBackend::default(), false);
    let scratch = portage_tmpdir.join("portage").join("_unmerge_src");
    let total = removal_list.len();
    for (idx, (category, package, version)) in removal_list.iter().enumerate() {
        let pf = format!("{package}-{version}");
        println!(
            ">>> Unmerging ({} of {}) {}/{}...",
            color.c("MERGE_LIST_PROGRESS", &(idx + 1).to_string()),
            color.c("MERGE_LIST_PROGRESS", &total.to_string()),
            category,
            pf
        );
        if let Err(e) = ebuild_merge::unmerge_one_installed(
            root,
            category,
            package,
            &pf,
            &[],
            &scratch,
            &portage_tmpdir,
            &options,
        ) {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
        if let Err(e) = deselect_from_world(root, category, package) {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

/// Real `WorldSelectedPackagesSet.cleanPackage` (`_sets/files.py:336`),
/// called by real `unmerge()` right after each package is removed: every
/// world atom whose `cp` equals the removed package's `category/package`
/// is dropped **unless** some installed version still satisfies it; all
/// other atoms are kept untouched. The file is then rewritten sorted
/// (real `.write`), with comment and `@set` lines not carried forward --
/// matching real portage and `update_world_file`. A world file that ends
/// up empty is written empty (real `replace([])`).
fn deselect_from_world(root: &Path, category: &str, package: &str) -> Result<(), String> {
    let mykey = format!("{category}/{package}");
    let mut current = read_world_atoms(root)?;
    let before = current.len();
    current.retain(|raw| {
        let Some(atom) = parse_atom(raw) else {
            return true;
        };
        if format!("{}/{}", atom.category, atom.package) != mykey {
            return true;
        }
        portage_repo::installed_candidates(root, &atom.category, &atom.package)
            .into_iter()
            .any(|(v, slot, sub_slot)| {
                let cs = format!("{}/{}-{v}:{slot}/{sub_slot}", atom.category, atom.package);
                match_from_list(raw, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
            })
    });
    if current.len() == before {
        return Ok(());
    }
    current.sort();
    current.dedup();
    let path = root.join("var/lib/portage/world");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let body = if current.is_empty() {
        String::new()
    } else {
        format!("{}\n", current.join("\n"))
    };
    std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// One `    selected: 1.0 ` / `   protected: none ` / `     omitted: ...`
/// row of `_unmerge_display`'s per-package block: the label
/// right-justified into 14 columns (real `(mytype + ": ").rjust(14)`),
/// then each version followed by a trailing space, or the literal
/// `none ` when empty -- reproduced faithfully, trailing spaces and all.
/// Real `_unmerge_display`: each `selected` version is
/// `colorize("UNMERGE_WARN", v + " ")` (red), each `protected`/`omitted`
/// version `colorize("GOOD", v + " ")` (green) -- the label and `none`
/// stay plain.
fn print_unmerge_row(label: &str, versions: &[String], color: &Colorizer) {
    let head = format!("{label}: ");
    let padded = format!("{head:>14}");
    if versions.is_empty() {
        println!("{padded}none ");
    } else {
        let key = if label == "selected" {
            "UNMERGE_WARN"
        } else {
            "GOOD"
        };
        let mut line = padded;
        for v in versions {
            line.push_str(&color.c(key, &format!("{v} ")));
        }
        println!("{line}");
    }
}

/// Every installed `(category, package, version, slot)` in the vdb under
/// `root` -- real `vartree.dbapi.cpv_all()` split out, used only for
/// `--unmerge`/`-C`'s own bare-name ("null category") resolution.
fn installed_cp_versions(root: &Path) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    let vdb = root.join("var/db/pkg");
    let Ok(cats) = std::fs::read_dir(&vdb) else {
        return out;
    };
    for cat in cats.filter_map(Result::ok).filter(|e| e.path().is_dir()) {
        let category = cat.file_name().to_string_lossy().to_string();
        let Ok(pkgs) = std::fs::read_dir(cat.path()) else {
            continue;
        };
        for pkg in pkgs.filter_map(Result::ok).filter(|e| e.path().is_dir()) {
            let dirname = pkg.file_name().to_string_lossy().to_string();
            // Split `name-version` on the version boundary via the atom
            // parser's own knowledge -- reuse `installed_candidates`
            // once the package name is known. Cheaper: try each `-`
            // split point and keep the one whose right half version-
            // parses. `strip_version_prefix` (portage-repo) already does
            // this, but isn't public; approximate with a scan.
            if let Some((name, version)) = split_pf(&dirname) {
                let slot = std::fs::read_to_string(pkg.path().join("SLOT"))
                    .unwrap_or_default()
                    .trim()
                    .split('/')
                    .next()
                    .unwrap_or("0")
                    .to_string();
                out.push((category.clone(), name, version, slot));
            }
        }
    }
    out
}

/// Splits a vdb directory name (`foo-bar-1.2.3-r1`) into `(package,
/// version)` -- the last `-`-separated run that `portage_versions::
/// ververify` accepts as a version (optionally with an `-r<n>` revision)
/// is the version, everything before it the package name.
fn split_pf(dirname: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = dirname.split('-').collect();
    for i in 1..parts.len() {
        let candidate = parts[i..].join("-");
        if portage_versions::ververify(&candidate) {
            return Some((parts[..i].join("-"), candidate));
        }
    }
    None
}

/// Real `action_depclean`'s own argument handling (`actions.py:848-863`),
/// shared by `--depclean` and `--prune`: resolve each bare name against
/// the vdb null-category scan (ambiguous -> `!!! ... ambiguous` +
/// `Err(1)`), then check every resulting atom -- one matching nothing
/// prints `--- Couldn't find 'X' to <action>.` (stderr), and if *none*
/// match, print `>>> No packages selected for removal by <action>` and
/// return `Err(1)`. An empty `targets` is `Ok(vec![])` (the full,
/// no-args form).
fn resolve_cleanup_args(
    targets: &[&str],
    root: &Path,
    action: &str,
) -> Result<Vec<String>, ExitCode> {
    let mut args: Vec<String> = Vec::new();
    let installed_scan = installed_cp_versions(root);
    for t in targets {
        if t.contains('/') {
            args.push((*t).to_string());
        } else {
            let cats: HashSet<&String> = installed_scan
                .iter()
                .filter(|(_, p, _, _)| p == *t)
                .map(|(c, _, _, _)| c)
                .collect();
            match cats.len() {
                0 => args.push((*t).to_string()), // let the "Couldn't find" path report it
                1 => args.push(format!("{}/{t}", cats.into_iter().next().unwrap())),
                _ => {
                    eprintln!("\n!!! The short package name \"{t}\" is ambiguous. Please specify");
                    eprintln!("!!! one of the following fully-qualified package names instead:\n");
                    let mut names: Vec<String> =
                        cats.into_iter().map(|c| format!("    {c}/{t}")).collect();
                    names.sort();
                    for n in names {
                        println!("{n}");
                    }
                    return Err(ExitCode::from(1));
                }
            }
        }
    }

    if !args.is_empty() {
        let mut any_matched = false;
        for a in &args {
            let matched = portage_dep::parse_atom(a).is_some_and(|atom| {
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
                    .iter()
                    .any(|(v, s, sub)| {
                        let cs = format!("{}/{}-{v}:{s}/{sub}", atom.category, atom.package);
                        match_from_list(a, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                    })
            });
            if matched {
                any_matched = true;
            } else {
                eprintln!(
                    "--- Couldn't find '{}' to {action}.",
                    a.strip_prefix("null/").unwrap_or(a)
                );
            }
        }
        if !any_matched {
            println!(">>> No packages selected for removal by {action}");
            return Err(ExitCode::from(1));
        }
    }
    Ok(args)
}

/// Real `emerge --pretend --prune` / `-pP` (real `action_depclean` with
/// `action="prune"`). Unlike `--depclean`, real `action_depclean`
/// returns right after the `unmerge()` preview (`actions.py:888`), so
/// there is **no** `* ` advisory block (only `action == "depclean"`
/// prints it, `:840`) and **no** `Packages installed:` / `Required
/// packages:` / `Number to remove:` stats block. The empty-cleanlist
/// message gains a `>>> To ignore dependencies, use --nodeps` line
/// (`create_cleanlist`, `:1348`). See `portage_repo::prune_cleanlist`'s
/// own doc comment for the removal-set semantics.
fn run_prune_pretend(
    targets: &[&str],
    root: &Path,
    config_root: &Path,
    config: &portage_profile::Config,
    verbose: bool,
    lib_check: bool,
    color: &Colorizer,
) -> ExitCode {
    let args = match resolve_cleanup_args(targets, root, "prune") {
        Ok(a) => a,
        Err(code) => return code,
    };

    let result = portage_repo::prune_cleanlist(root, &args, &[]);
    // Real `_calc_depclean`'s `unresolved_deps()` safety halt -- serves
    // `action in ("depclean", "prune")`, so it applies here too (with the
    // prune-only `use --nodeps` trailer).
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, true, color) {
        return code;
    }
    // Real `_calc_depclean` serves `action in ("depclean", "prune")`, so
    // `--depclean-lib-check` applies to `--prune` too.
    let result = apply_depclean_lib_check(root, result, lib_check, color, |providers| {
        portage_repo::prune_cleanlist(root, &args, providers)
    });

    // Real `create_cleanlist`'s prune branch prints `show_parents(pkg)`
    // inline while it builds the removal list -- before the removal-order
    // line / empty message.
    if verbose {
        for (pkg, lines) in &result.kept_parents {
            println!("  {} pulled in by:", pkg.cpv());
            for line in lines {
                println!("    {line}");
            }
            println!();
        }
    }

    if result.cleanlist.is_empty() {
        println!(">>> No packages selected for removal by prune");
        // Real `create_cleanlist`: `if "--verbose" not in myopts`.
        if !verbose {
            println!(">>> To see reverse dependencies, use --verbose");
        }
        println!(">>> To ignore dependencies, use --nodeps");
        return ExitCode::SUCCESS;
    }

    println!(">>> Calculating removal order...");
    let cpv_atoms: Vec<String> = result
        .cleanlist
        .iter()
        .map(|p| format!("={}", p.cpv()))
        .collect();
    let cpv_refs: Vec<&str> = cpv_atoms.iter().map(String::as_str).collect();
    // `--prune --nodeps` is preview-only in this pilot (its real removal
    // is a separate slice), so always `pretend = true`.
    run_unmerge_pretend(
        &cpv_refs,
        root,
        config_root,
        config,
        result.ordered,
        true,
        color,
    )
}

/// Real `emerge --pretend --prune --nodeps` (`actions.py:2684-2697`):
/// `--nodeps` routes prune to `unmerge()`'s own `_unmerge_display` prune
/// branch (`unmerge.py:245-272`) *instead of* `_calc_depclean` -- so
/// there is NO dependency/reachability check at all, no `>>>
/// Calculating removal order...`, and no `show_parents` (`--verbose` is
/// inert here). For every cp with more than one version installed the
/// best (highest) version is `protected` and every other version is
/// `selected` for removal (see `portage_repo::prune_nodeps_selection`).
/// `_unmerge_display` renders it unordered: the header, per-cp
/// `selected:`/`protected:`/`omitted:` blocks (`omitted` is always
/// `none` here -- every version is either selected or protected), the
/// `sys-apps/portage` self-skip, the "still listed in package sets"
/// warning, and the footer. Empty result: `>>> No outdated packages
/// were found on your system.` with no args (real `global_unmerge`),
/// else `>>> No packages selected for removal by prune` -- both exit 1
/// (real `_unmerge_display` returns `(1, {})`, unlike plain `--prune`'s
/// exit 0).
fn run_prune_nodeps_pretend(
    targets: &[&str],
    root: &Path,
    config_root: &Path,
    color: &Colorizer,
) -> ExitCode {
    let args = match resolve_cleanup_args(targets, root, "prune") {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut selection = portage_repo::prune_nodeps_selection(root, &args);

    println!(
        "{}",
        color.c(
            "darkgreen",
            ">>> These are the packages that would be unmerged:"
        )
    );

    // Real `sys-apps/portage` self-skip (`unmerge.py:368-391`): move any
    // selected `sys-apps/portage` version into `protected` with the
    // eerror. Realistically dead code -- `sys-apps/portage` is never
    // installed at more than one version -- kept for fidelity.
    for cp in &mut selection {
        if (cp.category.as_str(), cp.package.as_str()) == ("sys-apps", "portage") {
            for v in std::mem::take(&mut cp.other_versions) {
                eprintln!(
                    "!!! Not unmerging package sys-apps/portage-{v} since there is no valid \
                     reason for Portage to prune itself."
                );
            }
        }
    }

    let total_selected: usize = selection.iter().map(|cp| cp.other_versions.len()).sum();
    if total_selected == 0 {
        if args.is_empty() {
            println!("\n>>> No outdated packages were found on your system.");
        } else {
            println!("\n>>> No packages selected for removal by prune");
        }
        return ExitCode::from(1);
    }

    // "still listed in package sets" warning (real `_unmerge_display`,
    // `unmerge.py:393-447`) -- same machinery `run_unmerge_pretend` uses.
    let installed_sets: Vec<(String, Vec<String>)> = match collect_installed_sets(config_root, root)
    {
        Ok(sets) => sets,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let mut selected_flat: Vec<(String, String, String)> = Vec::new();
    for cp in &selection {
        for v in &cp.other_versions {
            selected_flat.push((cp.category.clone(), cp.package.clone(), v.clone()));
        }
    }
    selected_flat.sort();
    for (cat, pkg, version) in &selected_flat {
        let mut parents = still_listed_parents(root, &installed_sets, cat, pkg, version);
        if !parents.is_empty() {
            parents.sort_unstable();
            println!(
                "{}",
                color.c(
                    "WARN",
                    &format!("Package {cat}/{pkg}-{version} is going to be unmerged,")
                )
            );
            println!(
                "{}",
                color.c("WARN", "but still listed in the following package sets:")
            );
            println!("    {}\n", parents.join(", "));
        }
    }

    // Per-cp blocks (already cp-sorted by `prune_nodeps_selection`).
    let mut all_selected_display: Vec<String> = Vec::new();
    for cp in &selection {
        if cp.other_versions.is_empty() {
            continue;
        }
        println!("\n {}/{}", cp.category, cp.package);
        print_unmerge_row("selected", &cp.other_versions, color);
        print_unmerge_row("protected", std::slice::from_ref(&cp.best_version), color);
        print_unmerge_row("omitted", &[], color);
        for v in &cp.other_versions {
            all_selected_display.push(format!("={}/{}-{v}", cp.category, cp.package));
        }
    }

    all_selected_display.sort();
    println!(
        "\nAll selected packages: {}",
        all_selected_display.join(" ")
    );
    println!(
        "\n>>> {} packages are slated for removal.",
        color.c("UNMERGE_WARN", "'Selected'")
    );
    println!(
        ">>> {} and {} packages will not be removed.",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );
    ExitCode::SUCCESS
}

/// One cleanlist package `--depclean-lib-check` keeps installed: it
/// solely provides a library still needed at link level (`NEEDED.ELF.2`
/// soname) by a package that is *not* itself being removed.
struct LibConsumerProtection {
    provider: portage_repo::InstalledPackage,
    /// Surviving consumer cpv -> the sonames it needs from `provider`,
    /// sorted (real `sorted(libs)` / `sorted(consumer.mycpv ...)`).
    consumers: Vec<(String, Vec<String>)>,
}

/// Real `_calc_depclean`'s `--depclean-lib-check` scan (`actions.py:
/// 1381-1546`), narrowed to its pure computation. For every cleanlist
/// package, take the `NEEDED.ELF.2`-indexed objects it owns that carry a
/// `DT_SONAME`, and ask `needed_elf::find_consumers` (non-greedy, so a
/// consumer already satisfied by *another* provider of the same soname
/// is excluded -- real `actions.py:1454-1504`'s own multi-provider
/// filter) which surviving objects still link against them. A consumer
/// whose owning package is itself in the cleanlist is dropped (real
/// `lib_consumer in clean_set`).
///
/// **Documented narrowing**: real's multi-provider filter only lets an
/// *alternative* provider satisfy a consumer when that alternative's own
/// package is not also being removed; `find_consumers` is not clean-set
/// aware, so the pilot can under-report in the (rare) case where the
/// only surviving provider of a soname is itself another cleanlist
/// member. Also real's intermediate `>>> Assigning files to packages...`
/// progress line can appear with no WARNING following (all consumers
/// satisfied elsewhere); the pilot prints it only alongside the WARNING.
fn lib_consumer_scan(
    root: &Path,
    cleanlist: &[portage_repo::InstalledPackage],
) -> Vec<LibConsumerProtection> {
    let owner_entries = needed_elf::read_all_needed_entries(root);
    let map = needed_elf::rebuild(root, &owner_entries);
    let defpath = needed_elf::getlibpaths(root, None);
    let clean_cpvs: HashSet<String> = cleanlist.iter().map(|p| p.cpv()).collect();

    let mut out: Vec<LibConsumerProtection> = Vec::new();
    for pkg in cleanlist {
        let pkg_cpv = pkg.cpv();
        // The objects this package owns that provide a soname (real
        // `pkg_dblink.getcontents()` ∩ the linkmap ∩ non-empty soname).
        let mut provided: Vec<(&str, &str)> = map
            .obj_properties
            .values()
            .filter(|props| props.owner == pkg_cpv && !props.soname.is_empty())
            .map(|props| (props.alt_paths[0].as_str(), props.soname.as_str()))
            .collect();
        provided.sort();

        let mut per_consumer: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        for (lib_path, soname) in provided {
            let Ok(consumers) =
                needed_elf::find_consumers(root, &map, &defpath, lib_path, None, false)
            else {
                continue;
            };
            for consumer_path in consumers {
                let ckey = needed_elf::obj_key(root, &consumer_path);
                let Some(cprops) = map.obj_properties.get(&ckey) else {
                    continue;
                };
                if cprops.owner.is_empty() || cprops.owner == pkg_cpv {
                    continue;
                }
                if clean_cpvs.contains(&cprops.owner) {
                    continue;
                }
                per_consumer
                    .entry(cprops.owner.clone())
                    .or_default()
                    .insert(soname.to_string());
            }
        }

        if !per_consumer.is_empty() {
            out.push(LibConsumerProtection {
                provider: pkg.clone(),
                consumers: per_consumer
                    .into_iter()
                    .map(|(c, s)| (c, s.into_iter().collect()))
                    .collect(),
            });
        }
    }
    // Real `sorted(consumer_map, key=cmp_sort_key(cmp_pkg_cpv))`.
    out.sort_by_key(|p| p.provider.cpv());
    out
}

/// Real `_calc_depclean`'s `--depclean-lib-check` phase (`actions.py:
/// 1356-1590`): run `lib_consumer_scan` on the tentative `result`, print
/// the `>>> Checking for lib consumers...` progress and, when any
/// provider must be kept, the `* ...one or more packages will not be
/// removed` WARNING (real `bad(" * ")` prefix, `logging.WARNING` ->
/// stderr) plus the per-provider `pulled in by:` / `needs <soname>`
/// detail, then hand the protected providers to `recompute` (a second
/// `depclean_cleanlist` / `prune_cleanlist` pass that seeds them as
/// roots so their own deps also leave the cleanlist). A no-op when
/// `lib_check` is off (`--depclean-lib-check=n`) or the cleanlist is
/// already empty.
fn apply_depclean_lib_check(
    root: &Path,
    result: portage_repo::DepcleanResult,
    lib_check: bool,
    color: &Colorizer,
    recompute: impl FnOnce(&[portage_repo::InstalledPackage]) -> portage_repo::DepcleanResult,
) -> portage_repo::DepcleanResult {
    if !lib_check || result.cleanlist.is_empty() {
        return result;
    }
    eprintln!(">>> Checking for lib consumers...");
    let protections = lib_consumer_scan(root, &result.cleanlist);
    if protections.is_empty() {
        return result;
    }
    eprintln!(">>> Assigning files to packages...");

    // Real: `"".join(bad(" * ") + f"{line}\n" for line in textwrap.wrap(
    // msg, 70))` -- the wrap is pinned here since it never changes.
    let star = color.c("BAD", " * ");
    for line in [
        "In order to avoid breakage of link level dependencies, one or more",
        "packages will not be removed. This can be solved by rebuilding the",
        "packages that pulled them in.",
    ] {
        eprintln!("{star}{line}");
    }
    // Real's second `msg` list: a blank ` * ` line, then `  <cpv> pulled
    // in by:` and `    <consumer> needs <sonames>` per provider, then a
    // trailing blank ` * ` line.
    for prot in &protections {
        eprintln!("{star}");
        eprintln!("{star}  {} pulled in by:", prot.provider.cpv());
        for (consumer, sonames) in &prot.consumers {
            eprintln!("{star}    {consumer} needs {}", sonames.join(", "));
        }
    }
    eprintln!("{star}");

    eprintln!(">>> Adding lib providers to graph...");
    let providers: Vec<portage_repo::InstalledPackage> =
        protections.iter().map(|p| p.provider.clone()).collect();
    recompute(&providers)
}

/// Real `_calc_depclean`'s `unresolved_deps()` halt (`actions.py:1177-1248`):
/// when a kept installed package has a hard runtime dependency no
/// installed package satisfies, depclean/prune print the `bad(" * ")`-
/// prefixed `Dependencies could not be completely resolved ...` block
/// (`logging.ERROR` -> stderr) and exit 1 without removing anything.
/// Returns `Some(exit 1)` when it halted, `None` to carry on. `is_prune`
/// adds the real prune-only `use --nodeps` trailer.
fn depclean_unresolved_halt(
    unresolved: &[(String, String)],
    is_prune: bool,
    color: &Colorizer,
) -> Option<ExitCode> {
    if unresolved.is_empty() {
        return None;
    }
    let star = color.c("BAD", " * ");
    eprintln!("{star}Dependencies could not be completely resolved due to");
    eprintln!("{star}the following required packages not being installed:");
    for (atom, parent) in unresolved {
        eprintln!("{star}");
        eprintln!("{star}  {atom} pulled in by:");
        eprintln!("{star}    {parent}");
    }
    eprintln!("{star}");
    // Real `textwrap.wrap(..., 65)` -- pinned, it never changes.
    eprintln!("{star}Have you forgotten to do a complete update prior to depclean? The");
    eprintln!("{star}most comprehensive command for this purpose is as follows:");
    eprintln!("{star}");
    eprintln!(
        "{star}  {}",
        color.c(
            "GOOD",
            "emerge --update --newuse --deep --with-bdeps=y @world"
        )
    );
    eprintln!("{star}");
    eprintln!("{star}Note that the --with-bdeps=y option is not required in many");
    eprintln!("{star}situations. Refer to the emerge manual page (run `man emerge`)");
    eprintln!("{star}for more information about --with-bdeps.");
    eprintln!("{star}");
    eprintln!("{star}Also, note that it may be necessary to manually uninstall");
    eprintln!("{star}packages that no longer exist in the repository, since it may not");
    eprintln!("{star}be possible to satisfy their dependencies.");
    if is_prune {
        eprintln!("{star}");
        eprintln!(
            "{star}If you would like to ignore dependencies then use {}.",
            color.c("GOOD", "--nodeps")
        );
    }
    Some(ExitCode::from(1))
}

/// Real `emerge --pretend --depclean` / `-pc` (real `action_depclean` +
/// `_calc_depclean`, no package arguments): the packages nothing in
/// `@world` ∪ `@system` needs, at runtime, are the cleanlist -- reported
/// (never removed; `--depclean`'s own real removal is a separate slice,
/// unlike `--unmerge`/`-C` which does remove now). Real
/// `action_depclean` literally feeds its cleanlist to `unmerge(...,
/// "unmerge", cleanlist)`, so the per-package block here is exactly
/// `run_unmerge_pretend`'s (each cleanlist cpv passed as an `=cat/pkg-ver`
/// atom). See `portage_repo::depclean_cleanlist`'s own doc comment for
/// the graph and its documented narrowings.
///
/// `emerge -pc <atoms>` (the `--depclean <atoms>` narrowing): the world
/// "selected" plain atoms are dropped (the named packages get deselected
/// *and* removed), every other installed package becomes a protected
/// root, and the cleanlist is just the `args`-matched packages nothing
/// else needs -- see `depclean_cleanlist`'s own doc comment. Real
/// `action_depclean` only shows the `* ` advisory block with no args, so
/// this doesn't either.
#[allow(clippy::too_many_arguments)]
fn run_depclean_pretend(
    targets: &[&str],
    root: &Path,
    config_root: &Path,
    config: &portage_profile::Config,
    verbose: bool,
    lib_check: bool,
    // Real `action_depclean`'s `deselect = myopts.get("--deselect") !=
    // "n"` -- `--depclean <atoms> --deselect=n` keeps the `world` set as
    // a protection root (a world member named as an arg is kept). See
    // `depclean_cleanlist`'s own `deselect` param.
    deselect: bool,
    color: &Colorizer,
) -> ExitCode {
    // Bare-name targets get their category from the vdb, then each atom
    // is checked against the vdb (real `action_depclean`, `:848-863`) --
    // shared with `--prune`.
    let args = match resolve_cleanup_args(targets, root, "depclean") {
        Ok(a) => a,
        Err(code) => return code,
    };

    // Real `action_depclean`'s own advisory block -- *only* with no
    // package arguments (`not myfiles`), non-quiet. The "Depclean may
    // break link level dependencies" first paragraph is real's own
    // `if "preserve-libs" not in features and --depclean-lib-check == "n"`
    // -- the pilot has no preserve-libs feature, so it hinges purely on
    // `--depclean-lib-check=n` (`!lib_check`).
    if args.is_empty() {
        // Real `action_depclean`: each line is `colorize("WARN", " * ")`
        // (yellow) + text, and each backtick-wrapped command inside the
        // text is `good("`…`")` (green).
        let star = color.c("WARN", " * ");
        let green_ticks = |text: &str| -> String {
            let mut out = String::new();
            let mut rest = text;
            while let Some(open) = rest.find('`') {
                out.push_str(&rest[..open]);
                let after = &rest[open + 1..];
                if let Some(close) = after.find('`') {
                    out.push_str(&color.c("GOOD", &format!("`{}`", &after[..close])));
                    rest = &after[close + 1..];
                } else {
                    out.push('`');
                    rest = after;
                }
            }
            out.push_str(rest);
            out
        };
        // `None` = real's leading bare `writemsg_stdout("\n")`; `Some("")`
        // = real's `msg.append("\n")` (still `WARN(" * ")`-prefixed);
        // `Some(text)` = a ` * `-prefixed advisory line. The first
        // paragraph (`Depclean may break link level dependencies...`)
        // only when `--depclean-lib-check=n`.
        let libcheck_off_paragraph = [
            Some("Depclean may break link level dependencies. Thus, it is"),
            Some("recommended to use a tool such as `revdep-rebuild` (from"),
            Some("app-portage/gentoolkit) in order to detect such breakage."),
            Some(""),
        ];
        for line in [None]
            .into_iter()
            .chain(libcheck_off_paragraph.into_iter().filter(|_| !lib_check))
            .chain([
                Some("Always study the list of packages to be cleaned for any obvious"),
                Some("mistakes. Packages that are part of the world set will always"),
                Some("be kept.  They can be manually added to this set with"),
                Some("`emerge --noreplace <atom>`.  Packages that are listed in"),
                Some("package.provided (see portage(5)) will be removed by"),
                Some("depclean, even if they are part of the world set."),
                Some(""),
                Some("As a safety measure, depclean will not remove any packages"),
                Some("unless *all* required dependencies have been resolved.  As a"),
                Some("consequence of this, it often becomes necessary to run "),
                Some("`emerge --update --newuse --deep @world` prior to depclean."),
            ])
        {
            match line {
                None => println!(),
                Some(text) => println!("{star}{}", green_ticks(text)),
            }
        }
    }

    // @world = the `world` file (parent `@selected`, real `show_parents`)
    // plus each `world_sets` nested set's own atoms (parent `@<name>`) --
    // kept as `(atom, label)` pairs for the `--verbose` reverse-dep
    // display. `depclean_cleanlist` drops these seeds in `args` mode.
    let mut world_seeds: Vec<(String, String)> = Vec::new();
    match read_world_atoms(root) {
        Ok(atoms) => world_seeds.extend(atoms.into_iter().map(|a| (a, "@selected".to_string()))),
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    match read_world_sets(root) {
        Ok(names) => {
            for name in names {
                let mut seen = HashSet::new();
                match resolve_custom_set(config_root, &name, &mut seen) {
                    Ok(atoms) => {
                        let label = format!("@{name}");
                        world_seeds.extend(atoms.into_iter().map(|a| (a, label.clone())));
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    let world_atom_count = world_seeds
        .iter()
        .map(|(a, _)| a.clone())
        .collect::<HashSet<_>>()
        .len();

    let result = portage_repo::depclean_cleanlist(
        root,
        &world_seeds,
        &config.system_packages,
        &args,
        deselect,
        &[],
    );
    // Real `_calc_depclean`'s `unresolved_deps()` safety halt
    // (`actions.py:1247`) -- checked before the lib scan.
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, false, color) {
        return code;
    }
    // Real `_calc_depclean`'s `--depclean-lib-check` phase: a cleanlist
    // package still needed at link level by a survivor is kept (and its
    // own deps with it, via a second `depclean_cleanlist` pass seeding
    // the protected providers as roots).
    let result = apply_depclean_lib_check(root, result, lib_check, color, |providers| {
        portage_repo::depclean_cleanlist(
            root,
            &world_seeds,
            &config.system_packages,
            &args,
            deselect,
            providers,
        )
    });

    // Real `create_cleanlist`'s own `elif "--verbose": show_parents(pkg)`
    // -- the reverse-dep blocks come right after the `* ` advisory and
    // before `>>> Calculating removal order...` / the empty-cleanlist
    // message.
    if verbose {
        for (pkg, lines) in &result.kept_parents {
            println!("  {} pulled in by:", pkg.cpv());
            for line in lines {
                println!("    {line}");
            }
            println!();
        }
    }

    let installed_total = portage_repo::all_installed_packages(root).len();
    let stats = || {
        println!("Packages installed:   {installed_total}");
        println!("Packages in world:    {world_atom_count}");
        println!("Packages in system:   {}", config.system_packages.len());
        println!("Required packages:    {}", result.required_count);
        println!("Number to remove:     {}", result.cleanlist.len());
    };

    if result.cleanlist.is_empty() {
        println!(">>> No packages selected for removal by depclean");
        // Real `create_cleanlist`: `if "--verbose" not in myopts`.
        if !verbose {
            println!(">>> To see reverse dependencies, use --verbose");
        }
        stats();
        return ExitCode::SUCCESS;
    }

    // Real `_calc_depclean`: this line only when there's something to
    // remove; then `unmerge(..., "unmerge", cleanlist)` renders the
    // per-package block.
    println!(">>> Calculating removal order...");
    let cpv_atoms: Vec<String> = result
        .cleanlist
        .iter()
        .map(|p| format!("={}", p.cpv()))
        .collect();
    let cpv_refs: Vec<&str> = cpv_atoms.iter().map(String::as_str).collect();
    // `--depclean` is preview-only in this pilot (its real removal is a
    // separate slice), so always `pretend = true`.
    let unmerge_rc = run_unmerge_pretend(
        &cpv_refs,
        root,
        config_root,
        config,
        result.ordered,
        true,
        color,
    );
    stats();
    unmerge_rc
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
    // --oneshot/-1: don't record the target in the world file on a real
    // merge, and (at --pretend) don't colour it as a would-be world
    // member -- real `Scheduler._world_atom` / `_DisplayConfig.oneshot`.
    let mut oneshot = false;
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
    // --getbinpkg/-g, --getbinpkgonly/-G (real `main.py`, `y_or_n`).
    // Folded into `usepkg`/`usepkgonly` below (real depgraph treats a
    // binrepo package the same as a $PKGDIR one for pool eligibility);
    // `getbinpkg` additionally turns on *remote* candidate loading.
    let mut getbinpkg = false;
    let mut getbinpkgonly = false;
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
                    deselect_n = true;
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
        } else if arg == "--emptytree" || arg == "-e" {
            emptytree = true;
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
        } else if arg == "--getbinpkg" || arg == "-g" {
            // Real `main.py`: `--getbinpkg`/`-g` (`y_or_n`) -- distinct
            // from `--usepkg`, but real depgraph makes a `--getbinpkg`
            // binrepo's packages eligible the same way `--usepkg` does
            // for `$PKGDIR`, so this pilot folds it into `usepkg` and
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
                    '1' => oneshot = true,
                    't' => tree = true,
                    'u' => update = true,
                    'n' => noreplace = true,
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

    // `--unmerge`/`-C` WITHOUT `--pretend` is a real removal now (see
    // `run_unmerge_pretend`'s own `pretend` param + `execute_unmerge`),
    // so -- unlike `--deselect`/`--depclean`/`--prune` -- it has no
    // `--pretend`-only gate.

    // `--depclean`/`-c`: real `emerge -c` removes; this pilot only
    // previews it, same `--pretend`-only gate.
    if depclean && !pretend {
        eprintln!("emerge (pilot v1): --depclean/-c requires --pretend (see PROMPT.md)");
        return ExitCode::from(2);
    }

    // `--prune`/`-P`: real `emerge -P` removes; this pilot only previews
    // it, same `--pretend`-only gate `--depclean` has.
    if prune && !pretend {
        eprintln!("emerge (pilot v1): --prune/-P requires --pretend (see PROMPT.md)");
        return ExitCode::from(2);
    }

    // Real non-dry-run execution paths this pilot implements for `emerge`
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
        return run_deselect(&atom_args, &root_from_env());
    }

    if atom_args.is_empty() && !unmerge && !depclean && !prune {
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

    // Every repo's own `aliases` (`repos.conf`/`layout.conf`), each
    // paired with that repo's location -- real
    // `repositories.get_location_for_name` resolves an aliased
    // `reponame:path` profile `parent` (see `resolve_config`'s own doc
    // comment).
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();

    let mut config = match portage_profile::resolve_config(
        &config_root,
        &main_repo.location,
        &overlay_repos,
        &repo_aliases,
        &main_repo.name,
        &repo_masters,
    ) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    // Real `actions.py::adjust_configs` colour gate -- resolved once here
    // so every action path (the standalone cleanup actions below and the
    // ordinary resolve-graph path) shares one `Colorizer`.
    let color = Colorizer::new(color::resolve_havecolor(color_opt));

    // `--unmerge`/`-C`: a standalone action -- resolved config in hand
    // (its `@system` target support and system-profile check both need
    // it), dispatch before the ordinary resolve-graph path below.
    if unmerge {
        // `pretend` last: without it, `run_unmerge_pretend` really
        // removes the selected packages after the display.
        return run_unmerge_pretend(
            &atom_args,
            &root,
            &config_root,
            &config,
            false,
            pretend,
            &color,
        );
    }
    if depclean {
        return run_depclean_pretend(
            &atom_args,
            &root,
            &config_root,
            &config,
            verbose,
            lib_check,
            !deselect_n,
            &color,
        );
    }
    if prune {
        if nodeps {
            return run_prune_nodeps_pretend(&atom_args, &root, &config_root, &color);
        }
        return run_prune_pretend(
            &atom_args,
            &root,
            &config_root,
            &config,
            verbose,
            lib_check,
            &color,
        );
    }

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

    // Fold the --getbinpkg family into the --usepkg family (see their
    // parsing above): `--getbinpkgonly` implies binary-only; either
    // getbinpkg flag makes binary candidates eligible; `getbinpkg`
    // additionally turns on *remote* binrepo candidate loading.
    let usepkgonly = usepkgonly || getbinpkgonly;
    let usepkg = usepkg || getbinpkg || getbinpkgonly;
    let getbinpkg = getbinpkg || getbinpkgonly;

    // Real `bintree._populate_local`'s own "no trusted index" branch: when
    // `--usepkg`/`--usepkgonly` makes local binary candidates eligible
    // but `<PKGDIR>/Packages` is absent, walk `$PKGDIR` for binpkg files
    // and synthesize the index from each file's own embedded metadata
    // (`binpkg::scan_pkgdir` -- real `xpak`/`gpkg`). Unlike real portage
    // this is NOT written back to `Packages` (see
    // `Config::scanned_binpkgs`). A present `Packages` is always used as
    // is (no mtime-staleness revalidation -- real
    // `FEATURES=pkgdir-index-trusted` behavior, this pilot's own
    // long-standing stance for the index).
    if usepkg || usepkgonly {
        let pkgdir_path = Path::new(&config.pkgdir);
        if !pkgdir_path.join("Packages").is_file() {
            match crate::binpkg::scan_pkgdir(pkgdir_path) {
                Ok(entries) if !entries.is_empty() => config.scanned_binpkgs = Some(entries),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("emerge: scanning {}: {e}", config.pkgdir);
                    return ExitCode::from(1);
                }
            }
        }
    }

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

    // Real `make.globals`'s own `DISTDIR="/var/cache/distfiles"` --
    // env-var-sourced at this CLI boundary, the same "env var / hardcoded
    // default" shortcut `fetch.rs`'s own `FetchOptions::distdir` already
    // uses. Consulted only for the `f`/`F` fetch-restrict bracket column
    // (see `GraphEntry::fetch_restrict`, portage-repo).
    let distdir = std::env::var_os("DISTDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/distfiles"));

    // Real `bintree._populate_remote`: a non-`--pretend` `--getbinpkg`
    // run refreshes each `http(s)` binhost's `Packages` index into the
    // local edb cache *before* resolution, so the resolver picks up the
    // live pool. (`--pretend` deliberately never touches the network --
    // it resolves against whatever is already cached.)
    if !pretend && getbinpkg {
        if let Err(e) = emerge_getbinpkg::refresh_binhost_indexes(&config.binrepos, &root) {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }

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
        &distdir,
        emptytree,
        getbinpkg,
    ) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let entries = &result.entries;

    // Real `depgraph.py:11192-11235`'s `display_problems()` block for a
    // directly-requested atom that matched `package.provided` -- printed
    // to stderr, before the merge list (matching real portage's own
    // `display_problems()` -> `display()` order). This pilot tracks no
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
            entries,
            &result.slot_conflicts,
            &result.changed_deps_report,
            &result.autounmask_keyword_changes,
            &result.autounmask_use_changes,
            &top_level_pkgs,
            verbose,
            root_deps_running_root.as_deref(),
        );
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
    let world_atoms = read_world_atoms(&root).unwrap_or_default();
    let system_atoms = &config.system_packages;
    // Real `Display.blockers`: blocker lines are collected while walking
    // the entries and printed as one group after every package line (see
    // `format_blocker_lines`).
    let mut blocker_lines: Vec<String> = Vec::new();
    if tree {
        print_tree(
            entries,
            &top_level_pkgs,
            onlydeps,
            oneshot,
            unordered_display,
            verbose,
            alphabetical,
            root_deps_running_root.as_deref(),
            &color,
            system_atoms,
            &world_atoms,
            &mut blocker_lines,
        );
    } else {
        for entry in entries {
            print_entry_line(
                entry,
                "",
                &top_level_pkgs,
                onlydeps,
                oneshot,
                verbose,
                alphabetical,
                columns,
                columnwidth,
                root_deps_running_root.as_deref(),
                &color,
                system_atoms,
                &world_atoms,
                &mut blocker_lines,
            );
        }
    }

    // Real `Display.print_blockers()`: the collected `[blocks B ...]`
    // lines, printed as one group after every package line and before
    // the counters.
    for line in &blocker_lines {
        println!("{line}");
    }

    // Real `output.py::display`: `if self.conf.verbosity == 3:
    // self.print_verbose(...)` -- the `Total: …` counters line, printed
    // after every entry (and blocker) line, only under `-v`, for the
    // tree/columns/flat layouts alike. Real emits `f"\n{self.counters}\n"`
    // (a leading blank line).
    if verbose {
        println!();
        println!(
            "{}",
            package_counters_summary(entries, &top_level_pkgs, onlydeps, &color)
        );
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
                eprintln!(
                    "{}",
                    color.c("INFORM", &format!("{} {}", change.atom, change.token))
                );
            }
        };
    // Real `_display_autounmask` order: keyword block, then (mask,) then
    // USE. The `atom` field already carries its own op prefix (`=` for
    // keywords, `>=`/`>=…:slot`/`=` for USE).
    print_autounmask_block(
        "keyword changes",
        "package.accept_keywords",
        &result.autounmask_keyword_changes,
    );
    print_autounmask_block("USE changes", "package.use", &result.autounmask_use_changes);

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
            binpkg_format: std::env::var("BINPKG_FORMAT")
                .unwrap_or(default_package_options.binpkg_format),
            config_root: portage_repo::config_root_from_env(),
        };
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));
        let merge_options =
            ebuild_merge::MergeOptions::from_env(ebuild_phases::ShellBackend::default(), false);
        if buildpkgonly {
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
        } else if getbinpkg {
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
            ) {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        } else {
            // Plain `emerge <atom>`: real source build + merge (see
            // `emerge_build::run_source_merge`).
            if let Err(e) = emerge_build::run_source_merge(
                entries,
                &repos,
                &root,
                &portage_tmpdir,
                &merge_options,
                keep_going,
            ) {
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
            if let Err(e) =
                update_world_file(&root, &atom_args, entries, system_atoms, oneshot, onlydeps)
            {
                eprintln!("emerge: {e}");
                return ExitCode::from(1);
            }
        }
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn world_entry(category: &str, package: &str, slot: &str) -> GraphEntry {
        let mut e = entry_with_use(
            PretendOutcome::New {
                version: "1.0".into(),
            },
            "",
            "",
        );
        e.category = category.into();
        e.package = package.into();
        e.slot = Some(slot.into());
        e
    }

    #[test]
    fn update_world_file_records_a_new_target_and_skips_deps_and_oneshot() {
        let tmp = std::env::temp_dir().join(format!(
            "portuale-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("var/lib/portage")).unwrap();
        std::fs::write(root.join("var/lib/portage/world"), "dev-libs/existing\n").unwrap();

        let entries = vec![
            world_entry("dev-libs", "wanted", "0"),
            world_entry("dev-libs", "adep", "0"),
        ];
        // `adep` isn't a requested atom -> not recorded; `wanted` is.
        update_world_file(&root, &["dev-libs/wanted"], &entries, &[], false, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
            "dev-libs/existing\ndev-libs/wanted\n"
        );

        // A second run is idempotent, and `--oneshot` writes nothing.
        update_world_file(&root, &["dev-libs/wanted"], &entries, &[], false, false).unwrap();
        update_world_file(&root, &["dev-libs/wanted2"], &entries, &[], true, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
            "dev-libs/existing\ndev-libs/wanted\n"
        );

        // An unslotted `@system` member is never recorded.
        std::fs::write(root.join("var/lib/portage/world"), "").unwrap();
        update_world_file(
            &root,
            &["dev-libs/wanted"],
            &entries,
            &["dev-libs/wanted".to_string()],
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
            ""
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn still_listed_parents_applies_the_higher_slot_refinement() {
        // dev-libs/dualslotpkg is installed in slot 1 (1.0) and slot 2
        // (2.0); `dualslotset` lists the bare `dev-libs/dualslotpkg`.
        let root = fixtures_root();
        let sets = vec![(
            "dualslotset".to_string(),
            vec!["dev-libs/dualslotpkg".to_string()],
        )];

        // Removing the slot-1 version: slot 2 (higher, different slot)
        // still matches the bare set atom -> not a parent.
        assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

        // Removing the slot-2 version: nothing higher -> the set is a
        // parent.
        assert_eq!(
            still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "2.0"),
            vec!["dualslotset"]
        );
    }

    #[test]
    fn still_listed_parents_is_empty_when_no_set_lists_the_cp() {
        let root = fixtures_root();
        let sets = vec![("someset".to_string(), vec!["dev-libs/other".to_string()])];
        assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());
    }

    #[test]
    fn attr_display_field_always_carries_the_seventh_mask_column() {
        // Real `set_pkg_info` fills the mask column `if
        // self.include_mask_str()` (`verbosity > 1`), and real default
        // `emerge -p` verbosity is 2 -- so the field is 7 columns even
        // without `-v`; this pilot has no `--quiet`/verbosity-1 mode
        // that would drop it. A plain reinstall with no keyword/hard
        // mask -> `I N S f U D` = `  R   ` then a bare space.
        let nc = Colorizer::new(false);
        let plain = attr_display_field(
            false, false, false, false, true, false, false, false, false, false, None, &nc,
        );
        assert_eq!(plain, "  R    ");
        assert_eq!(plain.chars().count(), 7);

        // A `~arch`-only-visible New -> the 7th column is `~`
        // (`gen_mask_str` "unstable"), not a space.
        let masked = attr_display_field(
            false,
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Some('~'),
            &nc,
        );
        assert_eq!(masked, " N    ~");
        assert_eq!(masked.chars().count(), 7);
    }

    fn entry_with_use(outcome: PretendOutcome, pv_use: &str, p_use: &str) -> GraphEntry {
        let grp = |s: &str| {
            if s.is_empty() {
                vec![]
            } else {
                vec![("USE".to_string(), s.to_string())]
            }
        };
        GraphEntry {
            category: "dev-libs".into(),
            package: "foo".into(),
            outcome,
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![("bar".into(), true), ("baz".into(), false)],
            use_expand_display: grp(pv_use),
            use_expand_display_p: grp(p_use),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source: portage_repo::CandidateSource::Ebuild,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
        }
    }

    #[test]
    fn use_suffix_picks_the_p_or_pv_use_rendering_by_verbosity() {
        let nc = Colorizer::new(false);
        // A `New` entry: the resolver renders the same full list into
        // both fields (`is_new` renders every flag regardless of
        // `all_flags`), so `-p` and `-pv` show the same USE line.
        let new = entry_with_use(
            PretendOutcome::New {
                version: "1.0".into(),
            },
            "bar -baz",
            "bar -baz",
        );
        assert_eq!(use_suffix(&new, false, false, &nc), " USE=\"bar -baz\"");
        assert_eq!(use_suffix(&new, true, false, &nc), " USE=\"bar -baz\"");

        // A `Reinstall` with one flipped flag: `-pv` shows the full list,
        // plain `-p` shows only the change.
        let reinstall = entry_with_use(
            PretendOutcome::Reinstall {
                version: "1.0".into(),
                changed_flags: vec![],
                deps_changed: false,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
            },
            "bar* -baz",
            "bar*",
        );
        assert_eq!(use_suffix(&reinstall, false, false, &nc), " USE=\"bar*\"");
        assert_eq!(
            use_suffix(&reinstall, true, false, &nc),
            " USE=\"bar* -baz\""
        );

        // A `Reinstall` with nothing changed: no USE line at `-p`.
        let unchanged = entry_with_use(
            PretendOutcome::Reinstall {
                version: "1.0".into(),
                changed_flags: vec![],
                deps_changed: true,
                slot_changed: false,
                rebuilt_binary: false,
                new_repo: false,
            },
            "bar -baz",
            "",
        );
        assert_eq!(use_suffix(&unchanged, false, false, &nc), "");
        assert_eq!(
            use_suffix(&unchanged, true, false, &nc),
            " USE=\"bar -baz\""
        );
    }
}
