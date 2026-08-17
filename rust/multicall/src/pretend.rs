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

use crate::emerge_options;
use portage_dep::{match_from_list, parse_atom, Blocker};
use portage_repo::{
    config_root_from_env, resolve_pretend_graph, root_from_env, GraphEntry, PretendOutcome,
    SlotConflict,
};
use std::collections::HashSet;
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
/// treating `--newuse`/`--changed-use` and `--changed-deps` as
/// independent, freely-combinable triggers (see `PretendOutcome::
/// Reinstall`'s own doc comment, portage-repo) -- `changed_flags` is
/// only ever empty when `deps_changed` alone triggered this outcome
/// (`resolve_pretend`'s own construction guarantees at least one is
/// non-trivial). Pilot-invented wording either way, same as the
/// pre-existing "changed USE: ..." text -- real portage's own default
/// `--pretend` output shows no such itemized reason at all.
fn reinstall_reason(changed_flags: &[String], deps_changed: bool, slot_changed: bool) -> String {
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
    assert!(
        !reasons.is_empty(),
        "Reinstall is only ever constructed with a non-empty changed_flags, deps_changed=true, or slot_changed=true"
    );
    reasons.join("; ")
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
/// text `[ebuild ...]`/"already installed"/blocker lines above, plus
/// two fields no plain-text line carries at all: `requested` (was this
/// exact category/package one of `atoms` directly, as opposed to reached
/// only via a dependency string) and `required_by` (which package(s), if
/// any, pulled it in that way -- see `GraphEntry::required_by`'s own doc
/// comment, portage-repo). `source` is always `"ebuild"`: this pilot has
/// no binary-package support anywhere (no `--usepkg`/`--getbinpkg`, no
/// binpkg reading in `portage-repo` at all), so nothing else is ever
/// possible -- included so a consumer doesn't have to assume it, not
/// because this pilot actually distinguishes binary from source.
/// Deliberately NOT affected by `--onlydeps`'s own suppression (a
/// display-only concern for the plain-text loop below): `--json` always
/// dumps the whole resolved graph, letting a consumer filter on
/// `requested` itself if they want the `--onlydeps` view.
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
        PretendOutcome::Reinstall { .. } => "reinstall",
        PretendOutcome::AlreadyInstalled { .. } => "already_installed",
        PretendOutcome::NoVisibleCandidate => "no_visible_candidate",
    };
    fields.push(format!("\"outcome\":{}", json_string(outcome_tag)));
    match &entry.outcome {
        PretendOutcome::New { version } | PretendOutcome::AlreadyInstalled { version } => {
            fields.push(format!("\"version\":{}", json_string(version)));
        }
        PretendOutcome::Upgrade { from, to } => {
            fields.push(format!("\"version\":{}", json_string(to)));
            fields.push(format!("\"from_version\":{}", json_string(from)));
        }
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            deps_changed,
            slot_changed,
        } => {
            fields.push(format!("\"version\":{}", json_string(version)));
            let changed_use: Vec<String> = changed_flags.iter().map(|f| json_string(f)).collect();
            fields.push(format!("\"changed_use\":[{}]", changed_use.join(",")));
            fields.push(format!("\"changed_deps\":{deps_changed}"));
            fields.push(format!("\"changed_slot\":{slot_changed}"));
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
        fields.push("\"source\":\"ebuild\"".to_string());
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

/// The whole `--json` output: `{"entries": [...], "slot_conflicts": [...]}`,
/// one line, no pretty-printing (a pilot-specific convenience format, not
/// a stable schema -- see the module doc comment).
fn print_json(
    entries: &[GraphEntry],
    slot_conflicts: &[SlotConflict],
    top_level_pkgs: &HashSet<(String, String)>,
    verbose: bool,
) {
    let entries_json: Vec<String> = entries
        .iter()
        .map(|e| entry_to_json(e, top_level_pkgs, verbose))
        .collect();
    let conflicts_json: Vec<String> = slot_conflicts.iter().map(slot_conflict_to_json).collect();
    println!(
        "{{\"entries\":[{}],\"slot_conflicts\":[{}]}}",
        entries_json.join(","),
        conflicts_json.join(",")
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
             --with-bdeps, --changed-deps, --changed-slot, and --help/-h \
             are implemented so far; see PROMPT.md)",
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
        "       --changed-deps[=y|n]  reinstall an already-installed package whose own vdb-recorded dependencies differ from the current ebuild's"
    );
    println!(
        "       --changed-slot[=y|n]  reinstall an already-installed package whose own vdb-recorded SLOT differs from the current ebuild's"
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
/// expanded via real portage's own "null category" mechanism -- scan
/// the world file's own atoms for one sharing that package name, and
/// substitute in its category (real `Atom(..., category="null")`
/// handling; this pilot's own atom parser has no equivalent, so this is
/// a dedicated lookup instead). Every resulting atom (bare-name-expanded
/// or given with an explicit category already) is then matched against
/// every installed version of that category/package
/// (`portage_repo::installed_candidates`, this pilot's own vdb scan) via
/// `match_from_list`, mirroring real `vardb.match(atom)` -- only a
/// target that's *actually installed* can ever match anything in the
/// world file at all, matching real portage's own behavior exactly (the
/// world file's own text alone is never enough; an unresolvable bare
/// name simply contributes nothing, not an error, same as real
/// portage's own empty `vardb.match()` result).
///
/// A world-file entry is discarded once it shares category/package with
/// one of these installed matches, and (if the world entry itself
/// carries an explicit slot) that slot matches too -- a deliberate,
/// documented simplification of real `Atom.intersects()`'s own full
/// version/slot/USE-dep compatibility algebra: this pilot's own atom
/// grammar has no `intersects()` equivalent, and the dominant real-world
/// `--deselect` usage (a plain, unversioned target against a plain,
/// unversioned or slot-qualified world entry) is fully captured by this
/// narrower category/package(+slot) check. Deliberately, still
/// out-of-scope here (unlike `@world`'s own expansion, which now does
/// resolve `world_sets`/nested custom sets -- see `read_world_sets`'s
/// own doc comment): a `--deselect @some-set` target, or a world-set
/// member being considered for removal, is never expanded against
/// `world_sets`/custom sets at all -- `read_world_atoms` above only
/// ever returns the plain world *file*'s own atoms. Real
/// `action_deselect` operates against the same combined `world_set`
/// (`WorldSelectedSet`, atoms + sets together) `@world` itself uses, so
/// this is a real, narrower scope than real portage's own -- confirmed
/// deliberate rather than fixed alongside `@world`'s own expansion,
/// since deselect's own removal semantics (matching installed
/// candidates, discarding matched world *entries*) are a genuinely
/// separate mechanism from simply resolving `@world` for a dependency
/// walk, not a trivial extension of the same code.
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

    let mut expanded: HashSet<(String, String, String)> = HashSet::new();
    for target in targets {
        let candidate_atom_strs: Vec<String> = if target.contains('/') {
            vec![(*target).to_string()]
        } else {
            world_atoms
                .iter()
                .filter_map(|w| parse_atom(w))
                .filter(|a| a.package == *target)
                .map(|a| format!("{}/{}", a.category, target))
                .collect()
        };
        for atom_str in candidate_atom_strs {
            let Some(atom) = parse_atom(&atom_str) else {
                eprintln!("emerge: invalid atom {atom_str:?}");
                return ExitCode::from(1);
            };
            for (version, slot) in
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
            {
                let candidate_str = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if match_from_list(&atom_str, &[candidate_str.as_str()])
                    .is_some_and(|m| !m.is_empty())
                {
                    expanded.insert((atom.category.clone(), atom.package.clone(), slot));
                }
            }
        }
    }

    let mut discard: Vec<&String> = world_atoms
        .iter()
        .filter(|world_atom_str| {
            let Some(w) = parse_atom(world_atom_str) else {
                return false;
            };
            expanded.iter().any(|(cat, pkg, slot)| {
                w.category == *cat
                    && w.package == *pkg
                    && w.slot.as_deref().is_none_or(|ws| ws == slot)
            })
        })
        .collect();

    if discard.is_empty() {
        println!(">>> No matching atoms found in \"world\" favorites file...");
    } else {
        discard.sort();
        for atom in discard {
            println!(">>> Would remove {atom} from \"world\" favorites file...");
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
    let mut update = false;
    let mut deep = portage_repo::Deep::NotRequested;
    let mut excluded: Vec<String> = Vec::new();
    let mut json = false;
    let mut deselect = false;
    let mut with_bdeps = true;
    let mut changed_deps = false;
    let mut changed_slot = false;

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
        } else if arg == "--update" || arg == "-u" {
            update = true;
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
                    i += 2;
                }
                "n" => {
                    with_bdeps = false;
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
                    i += 1;
                }
                "n" => {
                    with_bdeps = false;
                    i += 1;
                }
                _ => {
                    eprintln!("emerge: option \"--with-bdeps\": invalid choice: {value:?} (choose from \"y\", \"n\")");
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
                    'u' => update = true,
                    'D' => deep = portage_repo::Deep::Unlimited,
                    'W' => deselect = true,
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

    if !pretend {
        eprintln!(
            "emerge (pilot v1): only --pretend is implemented \
             (no real merges yet, see PROMPT.md)"
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

    let config = match portage_profile::resolve_config(
        &config_root,
        &main_repo.location,
        &overlay_repos,
        &main_repo.name,
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
        print_json(entries, &result.slot_conflicts, &top_level_pkgs, verbose);
        return ExitCode::SUCCESS;
    }

    for entry in entries {
        // --onlydeps (man/emerge.1: "Only merge (or pretend to merge) the
        // dependencies of the packages specified, not the packages
        // themselves"): a directly-requested (top-level) atom's own line
        // is suppressed -- whatever its outcome -- while its dependencies
        // (reached the same as always, since resolve_pretend_graph's own
        // recursion is entirely unaffected by this flag) print normally.
        // A dependency entry is never a top-level atom, so this is a
        // no-op for it either way.
        let onlydeps_suppressed =
            onlydeps && top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone()));
        match &entry.outcome {
            PretendOutcome::New { version } => {
                if !onlydeps_suppressed {
                    println!(
                        "[ebuild  N] {}/{}-{version}{}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
                }
                print_blockers(entry, version);
            }
            PretendOutcome::Upgrade { from, to } => {
                if !onlydeps_suppressed {
                    println!(
                        "[ebuild  U] {}/{}-{to} (upgrade from {from}){}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
                }
                print_blockers(entry, to);
            }
            PretendOutcome::Reinstall {
                version,
                changed_flags,
                deps_changed,
                slot_changed,
            } => {
                if !onlydeps_suppressed {
                    let reason = reinstall_reason(changed_flags, *deps_changed, *slot_changed);
                    println!(
                        "[ebuild  r] {}/{}-{version} (reinstall for {reason}){}",
                        entry.category,
                        entry.package,
                        use_suffix(entry, verbose)
                    );
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
                        "{}/{}-{version} is already installed; nothing to do",
                        entry.category, entry.package
                    );
                }
            }
            PretendOutcome::NoVisibleCandidate => {
                eprintln!(
                    "!!! no visible ebuild for dependency \"{}/{}\"",
                    entry.category, entry.package
                );
            }
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
    ExitCode::SUCCESS
}
