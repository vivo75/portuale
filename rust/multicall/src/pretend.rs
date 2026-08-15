// `emerge --pretend <category/package>`: the v1 single-atom, no-recursion
// slice (see PORTING/rust/portage-repo/src/lib.rs for the full scope
// writeup -- hardcoded ACCEPT_KEYWORDS=amd64, no profile/make.conf
// stacking, no dependency recursion, main repo only). Output format is a
// documented, simplified subset of real emerge's --pretend output, not
// byte-identical to it.
//
// Anything outside this narrow slice (no --pretend, more than one atom, a
// versioned/slotted/blocker atom, an unrecognized option) is rejected with
// a clear "not supported in this pilot" message rather than silently doing
// the wrong thing.

use portage_dep::{parse_atom, Operator};
use portage_repo::{config_root_from_env, resolve_pretend, root_from_env, PretendOutcome};
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

    match resolve_pretend(&config_root, &root, &atom.category, &atom.package) {
        Ok(PretendOutcome::New { version }) => {
            println!("[ebuild  N] {}/{}-{version}", atom.category, atom.package);
            ExitCode::SUCCESS
        }
        Ok(PretendOutcome::Upgrade { from, to }) => {
            println!(
                "[ebuild  U] {}/{}-{to} (upgrade from {from})",
                atom.category, atom.package
            );
            ExitCode::SUCCESS
        }
        Ok(PretendOutcome::AlreadyInstalled { version }) => {
            println!(
                "{}/{}-{version} is already installed; nothing to do",
                atom.category, atom.package
            );
            ExitCode::SUCCESS
        }
        Ok(PretendOutcome::NoVisibleCandidate) => {
            eprintln!(
                "!!! no visible ebuild for \"{}/{}\"",
                atom.category, atom.package
            );
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("emerge: {e}");
            ExitCode::from(1)
        }
    }
}
