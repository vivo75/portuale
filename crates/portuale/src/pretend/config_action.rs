//! `emerge --config`.

use crate::pretend::*;

/// Arguments of [`run_config_action`].
pub(crate) struct RunConfigActionArgs<'a> {
    pub(crate) atom_args: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) ask: bool,
    pub(crate) color: &'a Colorizer,
}

/// Portage's `_emerge/actions.py::action_config` (`emerge --config <atom>`):
/// run `pkg_config` for a single installed package. Exactly one atom,
/// matched against the vdb the same way `--unmerge` matches. Zero matches
/// print `No packages found.` (exit 0); more than one lists them (exit 1);
/// exactly one runs `pkg_config` from that version's own vdb-stored
/// environment, then a best-effort builddir clean on success.
/// Portage: actions.py, action_config
///
/// `--ask` shows the numbered menu and the `Ready to configure?` confirm.
/// Cuts: `elog` processing.
pub(crate) fn run_config_action(args: RunConfigActionArgs<'_>) -> ExitCode {
    let RunConfigActionArgs {
        atom_args,
        root,
        shell,
        debug,
        ask,
        color,
    } = args;
    if atom_args.len() != 1 {
        // Stdout, with the string's own trailing newline plus `print`'s.
        // Portage: actions.py, action_config
        println!(
            "{}",
            color.c(
                "red",
                "!!! config can only take a single package atom at this time\n"
            )
        );
        return ExitCode::from(1);
    }
    let raw = atom_args[0];

    let matches: Vec<(String, String, String)> = if !raw.contains('/') {
        let mut found: Vec<(String, String, String)> = Vec::new();
        for (cat, pkg, version, _slot) in installed_cp_versions(root) {
            if pkg == *raw {
                found.push((cat, pkg, version));
            }
        }
        let cats: HashSet<&String> = found.iter().map(|(c, _, _)| c).collect();
        if cats.len() > 1 {
            eprintln!("\n!!! The short package name \"{raw}\" is ambiguous. Please specify");
            eprintln!("!!! one of the following fully-qualified package names instead:\n");
            let mut names: Vec<String> =
                cats.into_iter().map(|c| format!("    {c}/{raw}")).collect();
            names.sort();
            for n in names {
                println!("{n}");
            }
            return ExitCode::from(1);
        }
        found
    } else {
        let Some(atom) = parse_atom(raw) else {
            eprintln!("!!! '{raw}' is not a valid package atom.");
            eprintln!("!!! Please check ebuild(5) for full details.");
            eprintln!("!!! (Did you specify a version but forget to prefix with '='?)");
            return ExitCode::from(1);
        };
        portage_repo::installed_candidates(root, &atom.category, &atom.package)
            .into_iter()
            .filter(|(version, slot, sub_slot)| {
                let cs = format!(
                    "{}/{}-{version}:{slot}/{sub_slot}",
                    atom.category, atom.package
                );
                match_from_list(raw, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
            })
            .map(|(version, _slot, _sub)| (atom.category.clone(), atom.package.clone(), version))
            .collect()
    };

    println!();
    if matches.is_empty() {
        println!("No packages found.\n");
        return ExitCode::SUCCESS;
    }
    let mut sorted_matches = matches.clone();
    sorted_matches.sort_by(|a, b| {
        format!("{}/{}-{}", a.0, a.1, a.2).cmp(&format!("{}/{}-{}", b.0, b.1, b.2))
    });
    let chosen: &(String, String, String) = if sorted_matches.len() > 1 {
        if !ask {
            // Without `--ask`, list the matches and bail.
            println!("The following packages available:");
            for (c, p, v) in &sorted_matches {
                println!("* {c}/{p}-{v}");
            }
            println!("\nPlease use a specific atom or the --ask option.");
            return ExitCode::from(1);
        }
        // Numbered menu + `X) Cancel` (`X` -> exit `128 + SIGINT`).
        println!("Please select a package to configure:");
        for (idx, (c, p, v)) in sorted_matches.iter().enumerate() {
            println!("{}) {c}/{p}-{v}", idx + 1);
        }
        println!("X) Cancel");
        match ask_select(sorted_matches.len()) {
            Some(i) => &sorted_matches[i],
            None => return ExitCode::from(130),
        }
    } else {
        &sorted_matches[0]
    };

    let (category, package, version) = chosen;
    let pf = format!("{package}-{version}");
    println!();
    if ask {
        if !ask_confirm(color, &format!("Ready to configure {category}/{pf}?")) {
            return ExitCode::from(130);
        }
    } else {
        println!("Configuring pkg...");
    }
    println!();

    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
    let options = ebuild_merge::MergeOptions::from_env(shell, debug);
    let scratch = portage_tmpdir.join("portage").join("_config_src");
    let rc = match ebuild_merge::run_vdb_saved_env_phase(ebuild_merge::RunVdbSavedEnvPhaseArgs {
        root,
        category,
        package,
        pf: &pf,
        phase: "config",
        scratch_dir: &scratch,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
    }) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    // Drop the per-package builddir on success.
    if rc == 0 {
        let _ = std::fs::remove_dir_all(portage_tmpdir.join("portage").join(category).join(&pf));
    }
    println!();
    if rc == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(rc.clamp(0, 255) as u8)
    }
}
