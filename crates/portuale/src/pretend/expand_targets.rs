//! Expanding the command-line arguments (sets, package moves, bare names) into the atoms to resolve.

use crate::pretend::*;
use portage_profile::Config;
use portage_repo::RepoConfig;

/// What [`expand_targets`] hands back to its caller.
pub(crate) struct ExpandTargetsOut {
    pub(crate) expanded_atoms: Vec<String>,
    pub(crate) selected_set_args: Vec<String>,
    pub(crate) set_args: HashSet<String>,
}

/// The values [`expand_targets`] reads from its caller.
pub(crate) struct ExpandTargetsArgs<'a> {
    pub(crate) opts: &'a EmergeOpts<'a>,
    pub(crate) color: &'a Colorizer,
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
}

/// Expanding the command-line arguments (sets, package moves, bare names) into the atoms to resolve.
pub(crate) fn expand_targets(args: ExpandTargetsArgs<'_>) -> Result<ExpandTargetsOut, ExitCode> {
    let ExpandTargetsArgs {
        opts,
        color,
        config,
        root,
        config_root,
        repos,
    } = args;
    let &EmergeOpts {
        autounmask_continue,
        autounmask,
        ref atom_args,
        quiet,
        ..
    } = opts;

    // `--autounmask-continue` + `--autounmask=n`: warn that the former is
    // disabled. Portuale's only observable effect for the flag.
    // Portage: actions.py, depgraph.py
    if autounmask_continue.is_some() && autounmask == Some(false) {
        eprintln!(
            " {} --autounmask-continue has been disabled by --autounmask=n",
            color.c("WARN", "*")
        );
    }

    // Built-in sets expand in place: `@world`/`@selected` (world file plus
    // nested sets), `@system` (profile `packages`), `@installed` (one
    // `cat/pkg:slot` atom per vdb package). Any other `@name` is a
    // file-based user set; unknown names are fatal.
    // Portage: _sets/__init__.py, depgraph.py
    let mut expanded_atoms: Vec<String> = Vec::new();
    // Which expanded atoms rode a `@world`/`@selected` argument (a member that
    // selects no package drops into `_missing_args` instead of failing the run).
    // Parallel to `expanded_atoms` below, index-preserving.
    let mut expanded_from_set: Vec<bool> = Vec::new();
    // The `@name` set args (leading `@` stripped) resolved via a set file,
    // recorded in `world_sets` after a successful non-`--pretend` merge.
    // Portage: depgraph.py, saveNomergeFavorites
    let mut selected_set_args: Vec<String> = Vec::new();
    // Every non-set argument must be a valid package atom before anything
    // else runs (Portage `run_action`, `_emerge/actions.py:4244-4262`):
    // short names get `cat/` spliced in, repos and build-ids are allowed,
    // blockers are not. A bad argument prints two `!!!` lines and exits 1.
    // Existing filesystem paths pass through untouched (Portage's
    // `os.sep`-prefix / `os.lstat` skips); `@sets` validate on their own
    // path (S8). Checked before set expansion, like Portage checks before
    // the depgraph touches any argument.
    for &raw in atom_args.iter() {
        if raw.starts_with('@') {
            continue;
        }
        if std::path::Path::new(raw).is_absolute() || std::fs::metadata(raw).is_ok() {
            continue;
        }
        if !is_valid_package_atom(raw) {
            eprintln!("!!! '{raw}' is not a valid package atom.");
            eprintln!("!!! Please check ebuild(5) for full details.");
            return Err(ExitCode::from(1));
        }
    }

    for atom_str in atom_args {
        if *atom_str == "@world" || *atom_str == "@selected" {
            // `@world` is `@profile` + `@selected` + `@system`, not just the
            // world file. Portage sorts each nested set's atoms.
            // Portage: _sets/__init__.py, depgraph.py
            if *atom_str == "@world" {
                let mut atoms = config.profile_packages.clone();
                atoms.sort();
                expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                expanded_atoms.extend(atoms);
            }
            match expand_selected(root, config_root) {
                Ok(atoms) => {
                    expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                    expanded_atoms.extend(atoms);
                }
                Err(e) => {
                    eprintln!("{e}");
                    return Err(ExitCode::from(1));
                }
            }
            if *atom_str == "@world" {
                let mut atoms = config.system_packages.clone();
                atoms.sort();
                expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                expanded_atoms.extend(atoms);
            }
        } else if *atom_str == "@profile" {
            let mut atoms = config.profile_packages.clone();
            atoms.sort();
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@system" {
            // Sorted: a `>=`-prefixed atom sorts before every plain one, which
            // is what makes the pre-bias order match Portage's from index 1.
            let mut atoms = config.system_packages.clone();
            atoms.sort();
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@installed" {
            let atoms = installed_set_atoms(root);
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@preserved-rebuild" {
            let atoms = preserved_rebuild_atoms(root);
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if let Some(name) = atom_str.strip_prefix('@') {
            let mut seen = HashSet::new();
            match resolve_custom_set(config_root, name, &mut seen) {
                Ok(atoms) => {
                    expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
                    expanded_atoms.extend(atoms);
                    selected_set_args.push(name.to_string());
                }
                Err(e) => {
                    eprintln!("{e}");
                    return Err(ExitCode::from(1));
                }
            }
        } else {
            expanded_from_set.push(false);
            expanded_atoms.push((*atom_str).to_string());
        }
    }

    // `profiles/updates` package moves rewrite each expanded atom before it is
    // resolved or used as a display key (applied in memory on read).
    // Portage: profiles/updates, global-updates
    for atom in &mut expanded_atoms {
        *atom = portage_repo::apply_updates_to_atom(atom);
    }

    // A package set is an unordered set, and `@world` now folds `@system` in:
    // drop later duplicates, keeping first position (merge order is
    // position-sensitive). An atom given both explicitly and through a set
    // keeps its explicit nature.
    {
        let explicit: HashSet<String> = expanded_atoms
            .iter()
            .zip(expanded_from_set.iter())
            .filter(|(_, f)| !**f)
            .map(|(a, _)| (*a).clone())
            .collect();
        let mut seen: HashSet<String> = HashSet::new();
        let mut kept_atoms = Vec::with_capacity(expanded_atoms.len());
        let mut kept_flags = Vec::with_capacity(expanded_from_set.len());
        for (a, f) in expanded_atoms.drain(..).zip(expanded_from_set.drain(..)) {
            if seen.insert(a.clone()) {
                kept_flags.push(f && !explicit.contains(a.as_str()));
                kept_atoms.push(a);
            }
        }
        expanded_atoms = kept_atoms;
        expanded_from_set = kept_flags;
    }

    // A bare name (`emerge eix`) is qualified against the repo tree: exactly
    // one match is used silently; zero or several are fatal (exit 1).
    // Portage: lib/portage/dbapi, dep_expand
    {
        let mut all_cp: Option<Vec<String>> = None;
        for atom in &mut expanded_atoms {
            let cps = all_cp.get_or_insert_with(|| portage_repo::all_cp(repos));
            match dep_expand_token(atom, cps) {
                DepExpand::Unchanged => {}
                DepExpand::Qualified(q) => *atom = q,
                DepExpand::Ambiguous(matches) => {
                    if quiet {
                        eprintln!(
                            "\n!!! The short ebuild name \"{atom}\" is ambiguous. Please specify"
                        );
                        eprintln!(
                            "!!! one of the following fully-qualified ebuild names instead:\n"
                        );
                        for cp in &matches {
                            println!("    {}", color.c("INFORM", cp));
                        }
                    } else {
                        render_ambiguous_search_output(atom, &matches, repos, root, color);
                    }
                    return Err(ExitCode::from(1));
                }
                DepExpand::NoMatch => {
                    eprintln!("emerge: there are no ebuilds to satisfy {atom:?}.");
                    return Err(ExitCode::from(1));
                }
            }
        }
    }

    if expanded_atoms.is_empty() {
        eprintln!(
            "emerge: no package atoms to resolve (the target list, after \
             expanding any @world/@selected/@system/@installed/@<set>, is empty)"
        );
        return Err(ExitCode::from(2));
    }
    // The droppable set members, built after expansion so renames match the
    // resolver's own atom texts.
    let set_args: HashSet<String> = expanded_atoms
        .iter()
        .zip(expanded_from_set.iter())
        .filter(|(_, f)| **f)
        .map(|(a, _)| (*a).clone())
        .collect();

    for atom_str in &expanded_atoms {
        let Some(atom) = parse_atom(atom_str) else {
            eprintln!("emerge: invalid atom {atom_str:?}");
            return Err(ExitCode::from(1));
        };
        if atom.blocker != Blocker::None {
            eprintln!("emerge: {atom_str:?} is a blocker, not a valid emerge target");
            return Err(ExitCode::from(2));
        }
    }
    Ok(ExpandTargetsOut {
        expanded_atoms,
        selected_set_args,
        set_args,
    })
}
