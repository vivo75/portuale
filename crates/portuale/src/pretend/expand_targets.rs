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
    pub(crate) autounmask_continue: Option<bool>,
    pub(crate) autounmask: Option<bool>,
    pub(crate) color: &'a Colorizer,
    pub(crate) atom_args: &'a [&'a str],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) quiet: bool,
}

/// Expanding the command-line arguments (sets, package moves, bare names) into the atoms to resolve.
pub(crate) fn expand_targets(args: ExpandTargetsArgs<'_>) -> Result<ExpandTargetsOut, ExitCode> {
    let ExpandTargetsArgs {
        autounmask_continue,
        autounmask,
        color,
        atom_args,
        config,
        root,
        config_root,
        repos,
        quiet,
    } = args;

    // Real `actions.py:3772`: `--autounmask-continue` + `--autounmask=n`
    // -> a WARNING that the former "has been disabled by" the latter,
    // printed on the merge/build path before the depgraph. Portuale's
    // only observable effect for `--autounmask-continue` -- its
    // write-and-continue path is real-portage-gated on `"--pretend" not
    // in myopts` (`depgraph.py:5796`), inert here.
    if autounmask_continue.is_some() && autounmask == Some(false) {
        eprintln!(
            " {} --autounmask-continue has been disabled by --autounmask=n",
            color.c("WARN", "*")
        );
    }

    // The built-in set tokens each expand to their own real atom list,
    // in place, at whatever position they appear: `@world`/`@selected`
    // (`expand_selected` -- the world file's atoms + `world_sets`' nested
    // sets), `@system` (the profile chain's `packages` files), and
    // `@installed` (`installed_set_atoms` -- a `cat/pkg:slot` atom per
    // vdb package). Any other `@name` token is a user-defined (file-based)
    // package set
    // -- expanded recursively via `resolve_custom_set` (real
    // `StaticFileSet` / `SetConfig.getSetAtoms`), the same machinery the
    // `--unmerge`/`--depclean`/`--deselect` paths already use; a `@name`
    // with no matching set file is a fatal "set 'name' not found".
    let mut expanded_atoms: Vec<String> = Vec::new();
    // Backlog #286: which expanded atoms rode a `@world`/`@selected` set
    // argument (real `SetArg` named `"world"`/`"selected"`,
    // `depgraph.py:5577-5605` -- a member that selects no package drops
    // into `_missing_args` instead of failing the run). Parallel to
    // `expanded_atoms` through the moves rewrite and dedup below (both
    // index-preserving); `dep_expand_token` mutates atoms in place, so
    // the flag rides renames too.
    let mut expanded_from_set: Vec<bool> = Vec::new();
    // The user-given `@name` set args (leading `@` stripped) that are
    // world candidates -- i.e. resolved via a set FILE, not `@world`/
    // `@system`. Recorded in `world_sets` after a successful
    // non-`--pretend` merge (real `depgraph.saveNomergeFavorites`).
    let mut selected_set_args: Vec<String> = Vec::new();
    for atom_str in atom_args {
        if *atom_str == "@world" || *atom_str == "@selected" {
            // Real `@world` (`_sets/__init__.py:97-98`) is
            // `@profile ∪ @selected ∪ @system` -- not just the world
            // file. `emerge @world` / `-uD @world` re-emerges / updates
            // every `@profile` and `@system` member too; on an empty
            // world it is still non-empty. `@selected` alone stays the
            // world file.
            if *atom_str == "@world" {
                // Real `_resolve` (`depgraph.py:5500`) processes each
                // `SetArg`'s atom list `sorted(arg.pset.getAtoms(),
                // key=str)`, and the world set lists its nested sets as
                // `@profile @selected @system`.
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
                // Real `_resolve` (`depgraph.py:5500`) processes each
                // `SetArg`'s atom list `sorted(arg.pset.getAtoms(),
                // key=str)`; the nested `@system` set is one such arg.
                let mut atoms = config.system_packages.clone();
                atoms.sort();
                expanded_from_set.extend(std::iter::repeat_n(true, atoms.len()));
                expanded_atoms.extend(atoms);
            }
        } else if *atom_str == "@profile" {
            // The `@profile` set on its own (real
            // `ProfilePackageSet.load`); same sorted-arg shape as
            // `@system` above.
            let mut atoms = config.profile_packages.clone();
            atoms.sort();
            expanded_from_set.extend(std::iter::repeat_n(false, atoms.len()));
            expanded_atoms.extend(atoms);
        } else if *atom_str == "@system" {
            // Real `_resolve` (`depgraph.py:5500`): `for atom in
            // sorted(arg.pset.getAtoms(), key=str)`. A `>=`-prefixed
            // atom sorts before every plain one (`>` is 0x3E), which is
            // exactly why real's `-pe @system` pre-bias order starts
            // `>=sys-apps/baselayout-2`, `>=sys-apps/findutils-4.4`,
            // `>=sys-devel/patch-2.7`, `app-admin/eselect`,
            // `app-alternatives/awk`, … (F-B3's probe). Portuale used
            // the profile's raw `packages` order, so the resolver walk
            // -- and therefore the pre-bias order the scheduler's stable
            // `_merge_order_bias` preserves -- diverged from index 1.
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

    // Real `profiles/updates/` package moves (SCOPE_BACKLOG Part 2 Section
    // H item 2): a `move sys-libs/foo sys-libs/bar` / `slotmove` directive
    // rewrites a command-line, `@world`/`@system` or `@<set>`-expanded
    // atom before it's resolved *or* used as a display "requested" key
    // (real global-updates rewrites the world file and the vdb; portuale
    // applies the moves in memory on read -- see
    // `portage_repo::apply_updates_to_atom`).
    for atom in &mut expanded_atoms {
        *atom = portage_repo::apply_updates_to_atom(atom);
    }

    // A real package set is an unordered *set* -- and `@world` now folds
    // `@system` in, which overlaps the world file for anyone who ran
    // `emerge --noreplace @system`. Drop later duplicates, keeping first
    // position (portuale's merge order is position-sensitive).
    {
        // Explicit-wins: an atom given both explicitly and through a
        // set keeps its explicit nature (real holds both a `SetArg`
        // and an `AtomArg` for it: the set member drops but the
        // explicit argument still fails the run).
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

    // Real `dep_expand()` / `cpv_expand()` (`lib/portage/dbapi/`): a
    // command-line target with no category (`emerge eix`, `emerge
    // eix-1.2`, `emerge eix:0`) is qualified against the repo tree --
    // scan every `<repo>/<cat>/<pkg>/` for a package matching the bare
    // name. Exactly one -> use it silently; more than one -> the real
    // `ambiguous_package_name` block (its `--quiet` form -- real's
    // default form runs a full `search`, a documented cut), exit 1;
    // none -> `there are no ebuilds to satisfy`, exit 1. A
    // `@set`-expanded atom is already `cat/pkg`, so only a genuine
    // command-line bare name reaches this.
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
    // Backlog #286: the droppable set members (see `expanded_from_set`).
    // Built after `dep_expand_token` so renames (bare `pkg` -> `cat/pkg`)
    // match the resolver's own atom texts.
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
