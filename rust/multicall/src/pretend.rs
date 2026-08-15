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
// for the (deliberately unfaithful) value-consumption and no-bundling
// scope cuts.

use crate::emerge_options;
use portage_dep::{parse_atom, Blocker};
use portage_repo::{
    config_root_from_env, resolve_pretend_graph, root_from_env, GraphEntry, PretendOutcome,
};
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

pub fn run(args: &[String]) -> ExitCode {
    let mut atom_args: Vec<&str> = Vec::new();
    let mut pretend = false;
    let mut verbose = false;

    for arg in args {
        let arg = arg.as_str();
        if arg == "--pretend" || arg == "-p" {
            pretend = true;
        } else if arg == "--verbose" || arg == "-v" {
            verbose = true;
        } else if !arg.starts_with('-') {
            atom_args.push(arg);
        } else if let Some(found) = emerge_options::lookup(arg) {
            // Reports and exits immediately, matching every other
            // out-of-scope-input case in this pilot -- so there's no
            // need to correctly skip over this option's own value token
            // (see emerge_options.rs's doc comment): nothing after this
            // point is ever looked at.
            let kind = if found.category == emerge_options::Category::Action {
                "action"
            } else {
                "option"
            };
            eprintln!(
                "emerge (pilot v1): {kind} {:?} is a real emerge {kind}, but is not \
                 implemented in this pilot (only --pretend/-p and --verbose/-v are \
                 implemented so far; see PROMPT.md)",
                found.canonical
            );
            return ExitCode::from(2);
        } else {
            eprintln!("emerge: unrecognized option {arg:?}");
            return ExitCode::from(2);
        }
    }

    if !pretend {
        eprintln!(
            "emerge (pilot v1): only --pretend is implemented \
             (no real merges yet, see PROMPT.md)"
        );
        return ExitCode::from(2);
    }

    if atom_args.is_empty() {
        eprintln!("emerge (pilot v1): expected a package atom, e.g. `emerge --pretend cat/pkg`");
        return ExitCode::from(2);
    }

    for atom_str in &atom_args {
        let Some(atom) = parse_atom(atom_str) else {
            eprintln!("emerge: invalid atom {atom_str:?}");
            return ExitCode::from(1);
        };
        if atom.blocker != Blocker::None {
            eprintln!("emerge (pilot v1): {atom_str:?} is a blocker, not a valid emerge target");
            return ExitCode::from(2);
        }
    }

    let config_root = config_root_from_env();
    let root = root_from_env();

    let config = match portage_profile::resolve_config(&config_root) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let atoms: Vec<String> = atom_args.iter().map(|s| s.to_string()).collect();
    let result = match resolve_pretend_graph(&config_root, &root, &atoms, &config) {
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
    let top_level_pkgs: std::collections::HashSet<(String, String)> = atom_args
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    for entry in entries {
        match &entry.outcome {
            PretendOutcome::New { version } => {
                println!(
                    "[ebuild  N] {}/{}-{version}{}",
                    entry.category,
                    entry.package,
                    use_suffix(entry, verbose)
                );
                print_blockers(entry, version);
            }
            PretendOutcome::Upgrade { from, to } => {
                println!(
                    "[ebuild  U] {}/{}-{to} (upgrade from {from}){}",
                    entry.category,
                    entry.package,
                    use_suffix(entry, verbose)
                );
                print_blockers(entry, to);
            }
            PretendOutcome::AlreadyInstalled { version } => {
                // Already-satisfied dependencies aren't shown, matching
                // real emerge's usual "don't clutter the list with what's
                // already there" behavior -- only a directly-requested
                // (top-level) atom gets its own "nothing to do" line.
                if top_level_pkgs.contains(&(entry.category.clone(), entry.package.clone())) {
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
