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
                    // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
        replaced_by_version: None,
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

/// The databases Portage's similar-name search and `cp_exists` check read besides the
/// installed one: the ebuild db unless `--usepkgonly` (any value) drops it, and the binary
/// db under `--usepkg`.
// Portage: depgraph.py, _show_unsatisfied_dep (`dbs`)
#[derive(Clone, Copy)]
pub(crate) struct SimilarNameDbs<'a> {
    pub(crate) ebuild: bool,
    pub(crate) pkgdir: Option<&'a Path>,
    /// The binrepos read under `--getbinpkg`; empty otherwise.
    pub(crate) binrepos: &'a [portage_profile::BinRepo],
}

/// `--misspell-suggestions` (default on): when `resolve_pretend_graph`
/// aborts a top-level atom whose `cat/pkg` genuinely doesn't exist in any
/// repo, print close `cat/pkg` matches. Returns the block to append to the
/// already-printed `emerge: <message>`, or `None` when it doesn't apply.
/// Portage: depgraph.py, _similar_name_search
pub(crate) fn misspell_suggestion_block(
    message: &str,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    enabled: bool,
    dbs: SimilarNameDbs<'_>,
) -> Option<String> {
    if !enabled {
        return None;
    }
    // The message's first line quotes the atom; pull it back out.
    let first = message.lines().next()?;
    let rest = first
        .strip_prefix("there are no ebuilds to satisfy \"")
        .or_else(|| first.strip_prefix("there are no binary packages to satisfy \""))?;
    // The line may carry ` for <root>/` after the quoted atom (a `ROOT`
    // other than the running root), so cut at the closing quote.
    let atom_str = rest.split_once('"')?.0;
    if !atom_str.contains('/') && !atom_str.is_empty() {
        return bare_name_suggestion_block(atom_str, repos, root, dbs);
    }
    let atom = parse_atom(atom_str)?;
    let binary = dbs.pkgdir.map(|pkgdir| {
        let mut cps = portage_repo::BinaryIndex::from_pkgdir(pkgdir).all_cp();
        cps.extend(portage_repo::remote_binary_all_cp(dbs.binrepos, root));
        cps
    });
    let target = format!("{}/{}", atom.category, atom.package);
    // Only when no database that is read has the `cat/pkg` (`cp_exists`): an installed
    // one reports no similar-names block (the S0-backed `360-gone-pv` capture).
    // Portage: depgraph.py, _show_unsatisfied_dep.
    let ebuild_has = dbs.ebuild
        && !portage_repo::list_candidates(repos, &atom.category, &atom.package)
            .map(|c| c.is_empty())
            .unwrap_or(true);
    let installed_has =
        !portage_repo::installed_candidates(root, &atom.category, &atom.package).is_empty();
    let binary_has = binary.as_ref().is_some_and(|cps| cps.contains(&target));
    if ebuild_has || installed_has || binary_has {
        return None;
    }
    let mut all_cp: std::collections::BTreeSet<String> = portage_repo::all_installed_packages(root)
        .iter()
        .map(|p| format!("{}/{}", p.category, p.package))
        .collect();
    if dbs.ebuild {
        all_cp.extend(portage_repo::all_cp(repos));
    }
    all_cp.extend(binary.into_iter().flatten());
    all_cp.remove(&target);
    let all_cp: Vec<String> = all_cp.into_iter().collect();
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

/// The bare-name branch of `similar_name_search` (`cat == "null"`): the
/// close matches are package names, each expanded to every `cat/pkg` that
/// carries it. Names equal to the typed one are dropped.
// Portage: dbapi/_similar_name_search.py
fn bare_name_suggestion_block(
    name: &str,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    dbs: SimilarNameDbs<'_>,
) -> Option<String> {
    let mut all_cp: std::collections::BTreeSet<String> = portage_repo::all_installed_packages(root)
        .iter()
        .map(|p| format!("{}/{}", p.category, p.package))
        .collect();
    if dbs.ebuild {
        all_cp.extend(portage_repo::all_cp(repos));
    }
    let pkg = name.to_lowercase();
    let mut pkg_to_cp: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for cp in &all_cp {
        let other = cp
            .split_once('/')
            .map_or(cp.as_str(), |(_, p)| p)
            .to_lowercase();
        if other == pkg {
            continue;
        }
        pkg_to_cp.entry(other).or_default().push(cp.clone());
    }
    let names: Vec<String> = pkg_to_cp.keys().cloned().collect();
    let matches: Vec<String> = difflib::get_close_matches(&pkg, &names, 3, 0.6)
        .iter()
        .flat_map(|m| pkg_to_cp[m].clone())
        .collect();
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
