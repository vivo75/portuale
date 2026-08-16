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
use portage_dep::{parse_atom, Blocker};
use portage_repo::{
    config_root_from_env, resolve_pretend_graph, root_from_env, GraphEntry, PretendOutcome,
};
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
             --update/-u, --deep/-D, --exclude/-X, and --help/-h are \
             implemented so far; see PROMPT.md)",
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
    println!("   -h, --help      show this message and exit");
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
/// KNOWN, DOCUMENTED SCOPE CUT: only plain atom lines are read, via a
/// leading `@` check. Real portage's own world file may also contain
/// `@some-set` lines (added by a prior `emerge --noreplace @some-set`),
/// and real `@world` is itself defined as the *union* of this file's own
/// atoms with any such referenced sets (see `WorldSelectedSet` in
/// `lib/portage/_sets/files.py`) -- resolving those recursively would
/// need general set-recursion machinery this pilot doesn't have, so a
/// `@`-prefixed line here is simply skipped rather than expanded.
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
                    'N' => newuse = true,
                    'U' => changed_use = true,
                    'O' => nodeps = true,
                    'o' => onlydeps = true,
                    'u' => update = true,
                    'D' => deep = portage_repo::Deep::Unlimited,
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

    let config = match portage_profile::resolve_config(&config_root, &main_repo.location) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    // "@world"/"@system" each expand to their own real atom list, in
    // place, at whichever position they appear -- see read_world_atoms's
    // doc comment for @world's exact scope (plain atoms only; nested
    // "@set" references stay unimplemented), and portage-profile's
    // `system_packages` doc comment for @system's. Only these two
    // literal tokens trigger expansion -- any other "@"-prefixed token
    // falls through to the ordinary atom-parsing path below and gets a
    // clear "invalid atom" error, not a silent no-op.
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
    let top_level_pkgs: std::collections::HashSet<(String, String)> = expanded_atoms
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

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
            } => {
                if !onlydeps_suppressed {
                    println!(
                        "[ebuild  r] {}/{}-{version} (reinstall for changed USE: {}){}",
                        entry.category,
                        entry.package,
                        changed_flags.join(", "),
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
