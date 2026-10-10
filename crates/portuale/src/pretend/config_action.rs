//! `emerge --config`.

use crate::pretend::*;

/// Real `_emerge/actions.py::action_config` (`emerge --config <atom>`):
/// run `pkg_config` for a single installed package. Exactly one atom
/// (real `if len(myfiles) != 1`), matched against the vdb the same way
/// `--unmerge` matches (bare name → null-category vdb lookup, ambiguous
/// is a hard error; `cat/pkg` via `installed_candidates` +
/// `match_from_list`). Zero matches → `No packages found.` exit 0; more
/// than one → `The following packages available:` list + exit 1; exactly
/// one → `Configuring pkg...` then `pkg_config` from that version's own
/// vdb-stored `environment.bz2` + `<pf>.ebuild`
/// (`ebuild_merge::run_vdb_saved_env_phase`, ungated -- real
/// `doebuild(ebuildpath, "config", ...)` runs it unconditionally), then
/// a best-effort builddir clean on success (real `doebuild(..., "clean")`).
///
/// **v1 cuts:** `--ask` (the interactive package picker / "Ready to
/// configure?" prompt) -- portuale is non-interactive, matching real
/// portage's own non-`--ask` branch; `elog` processing.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn run_config_action(
    atom_args: &[&str],
    root: &Path,
    shell: ebuild_phases::ShellBackend,
    debug: bool,
    ask: bool,
    color: &Colorizer,
) -> ExitCode {
    if atom_args.len() != 1 {
        // Real `action_config`: `print(red("!!! ...\n"))` -- stdout, with
        // the string's own trailing newline plus `print`'s.
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
            // Real `action_config`: without `--ask`, list the matches and
            // bail (`Please use a specific atom or the --ask option.`).
            println!("The following packages available:");
            for (c, p, v) in &sorted_matches {
                println!("* {c}/{p}-{v}");
            }
            println!("\nPlease use a specific atom or the --ask option.");
            return ExitCode::from(1);
        }
        // Real `action_config`'s `--ask` branch: a numbered menu +
        // `X) Cancel`, `uq.query("Selection?", responses=[1..N, X])`.
        // `X` -> exit `128 + SIGINT`.
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
    // Real `action_config`: with `--ask`, prompt `Ready to configure
    // <cpv>?` (No -> exit 130); without it, print `Configuring pkg...`.
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
    let rc = match ebuild_merge::run_vdb_saved_env_phase(
        root,
        category,
        package,
        &pf,
        "config",
        &scratch,
        &portage_tmpdir,
        &options,
    ) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    // Real `action_config`: `doebuild(ebuildpath, "clean", ...)` on
    // success -- drop the per-package builddir.
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

/// Real `depgraph.py:7034-7066` + `dbapi/_similar_name_search.py`
/// (`--misspell-suggestions`, default on): when `resolve_pretend_graph`
/// aborts a top-level atom with `there are no ebuilds to satisfy
/// "<atom>".` **and** that atom's `cat/pkg` genuinely doesn't exist in
/// any repo (real `not cp_exists` -- a `cat/pkg` that exists but is only
/// masked gets the autounmask notes instead, never this), print
/// `difflib.get_close_matches` suggestions over every `cat/pkg` in the
/// tree. Returns the block to append to the already-printed
/// `emerge: <message>` (no trailing newline -- the caller adds one), or
/// `None` when it doesn't apply.
pub(crate) fn misspell_suggestion_block(
    message: &str,
    repos: &[portage_repo::RepoConfig],
    enabled: bool,
) -> Option<String> {
    if !enabled {
        return None;
    }
    // The message's first line is exactly `there are no ebuilds to
    // satisfy "<atom-debug>".` -- pull the quoted atom out.
    let first = message.lines().next()?;
    let rest = first.strip_prefix("there are no ebuilds to satisfy \"")?;
    let atom_str = rest.strip_suffix("\".")?;
    let atom = parse_atom(atom_str)?;
    // Real `not cp_exists`: only when the `cat/pkg` directory has no
    // ebuilds anywhere.
    if !portage_repo::list_candidates(repos, &atom.category, &atom.package)
        .map(|c| c.is_empty())
        .unwrap_or(true)
    {
        return None;
    }
    let target = format!("{}/{}", atom.category, atom.package);
    let all_cp: Vec<String> = portage_repo::all_cp(repos)
        .into_iter()
        .filter(|cp| *cp != target)
        .collect();
    let matches = difflib::get_close_matches(&target, &all_cp, 3, 0.6);
    let tail = match matches.len() {
        0 => " nothing similar found.".to_string(),
        1 => format!("\nemerge: Maybe you meant {}?", matches[0]),
        _ => format!(
            "\nemerge: Maybe you meant any of these: {}?",
            matches.join(", ")
        ),
    };
    Some(format!("\n\nemerge: searching for similar names...{tail}"))
}
