// `emerge --pretend <category/package>`: the v1 slice (see
// PORTING/rust/portage-repo/src/lib.rs for the full scope writeup -- main
// repo only, no package.mask/.use/.accept_keywords, no slot conflicts, no
// blockers enforced). USE/ACCEPT_KEYWORDS come from the real profile
// chain + make.conf (see portage-profile's doc comment for what that
// does and doesn't implement), not a hardcoded stand-in. Recursively
// resolves DEPEND+RDEPEND (see resolve_pretend_graph's doc comment for
// the recursion's own scope cuts: DEPEND+RDEPEND only, || resolves every
// alternative, blockers skipped, cycle/dup-safe). Output format is a
// documented, simplified subset of real emerge's --pretend output, not
// byte-identical to it.
//
// Anything outside the top-level atom's narrow slice (no --pretend, more
// than one atom, a versioned/slotted/blocker top-level atom, an
// unrecognized option) is rejected with a clear "not supported in this
// pilot" message rather than silently doing the wrong thing. Dependency
// atoms extracted from DEPEND/RDEPEND are NOT restricted this way --
// real dependency strings need the full atom grammar (operators, slots).

use portage_dep::{parse_atom, Operator};
use portage_repo::{config_root_from_env, resolve_pretend_graph, root_from_env, PretendOutcome};
use std::process::ExitCode;

pub fn run(args: &[String]) -> ExitCode {
    let mut atom_arg: Option<&str> = None;
    let mut pretend = false;

    for arg in args {
        match arg.as_str() {
            "--pretend" | "-p" => pretend = true,
            other if !other.starts_with('-') => {
                if atom_arg.is_some() {
                    eprintln!("emerge (pilot v1): only a single package atom is supported");
                    return ExitCode::from(2);
                }
                atom_arg = Some(other);
            }
            other => {
                eprintln!(
                    "emerge (pilot v1): unsupported option {other:?} \
                     (only --pretend/-p is implemented)"
                );
                return ExitCode::from(2);
            }
        }
    }

    if !pretend {
        eprintln!(
            "emerge (pilot v1): only --pretend is implemented \
             (no real merges yet, see PROMPT.md)"
        );
        return ExitCode::from(2);
    }

    let Some(atom_str) = atom_arg else {
        eprintln!("emerge (pilot v1): expected a package atom, e.g. `emerge --pretend cat/pkg`");
        return ExitCode::from(2);
    };

    let Some(atom) = parse_atom(atom_str) else {
        eprintln!("emerge: invalid atom {atom_str:?}");
        return ExitCode::from(1);
    };
    if atom.operator != Operator::None || atom.slot.is_some() || atom.version.is_some() {
        eprintln!(
            "emerge (pilot v1): only a bare category/package atom is supported, got {atom_str:?}"
        );
        return ExitCode::from(2);
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

    let entries = match resolve_pretend_graph(&config_root, &root, atom_str, &config) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    // The BFS in resolve_pretend_graph always visits the requested atom
    // first, so entries[0] is the top-level package; its outcome keeps
    // the exact messages/exit codes the single-atom (no-deps) case always
    // had, for backward compatibility with that simpler behavior.
    let top = &entries[0];
    match &top.outcome {
        PretendOutcome::NoVisibleCandidate => {
            eprintln!(
                "!!! no visible ebuild for \"{}/{}\"",
                top.category, top.package
            );
            return ExitCode::from(1);
        }
        PretendOutcome::AlreadyInstalled { version } if entries.len() == 1 => {
            println!(
                "{}/{}-{version} is already installed; nothing to do",
                top.category, top.package
            );
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    for entry in &entries {
        match &entry.outcome {
            PretendOutcome::New { version } => {
                println!("[ebuild  N] {}/{}-{version}", entry.category, entry.package);
            }
            PretendOutcome::Upgrade { from, to } => {
                println!(
                    "[ebuild  U] {}/{}-{to} (upgrade from {from})",
                    entry.category, entry.package
                );
            }
            // Already-satisfied dependencies aren't shown, matching real
            // emerge's usual "don't clutter the list with what's already
            // there" behavior -- the top-level already-installed,
            // nothing-to-recurse-into case is handled above instead.
            PretendOutcome::AlreadyInstalled { .. } => {}
            PretendOutcome::NoVisibleCandidate => {
                eprintln!(
                    "!!! no visible ebuild for dependency \"{}/{}\"",
                    entry.category, entry.package
                );
            }
        }
    }
    ExitCode::SUCCESS
}
