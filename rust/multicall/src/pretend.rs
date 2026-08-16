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
// flag character". See `consume_verbose_value` and the bundle-handling
// comment below.
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

/// Reports and returns the exit code for a single option/action token
/// ("-x" or "--long", never a positional atom) that isn't --pretend/-p
/// or --verbose/-v -- shared between a standalone token and one
/// character of a decomposed short-flag bundle, so both produce
/// identical messages for the same underlying flag.
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
             implemented in this pilot (only --pretend/-p and --verbose/-v are \
             implemented so far; see PROMPT.md)",
            found.canonical
        );
    } else {
        eprintln!("emerge: unrecognized option {token:?}");
    }
    ExitCode::from(2)
}

pub fn run(args: &[String]) -> ExitCode {
    let mut atom_args: Vec<&str> = Vec::new();
    let mut pretend = false;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--pretend" || arg == "-p" {
            pretend = true;
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

    // resolve_config needs the main repo's own location for
    // package.mask/.unmask's repo-level source (see its doc comment) --
    // found via the same find_repos repos.conf parsing
    // resolve_pretend_graph uses internally a few lines down; called
    // again here since portage-profile can't depend back on portage-repo
    // (portage-repo already depends on portage-profile).
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

    let config = match portage_profile::resolve_config(&config_root, &main_repo.location) {
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
