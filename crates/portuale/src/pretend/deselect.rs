//! `--deselect`.

use crate::pretend::*;

/// `emerge --deselect <atom-or-bare-name> [...]`: preview (`Would remove`
/// under `--pretend`) or rewrite (`Removing ...`) both `world` and
/// `world_sets` with the discarded entries dropped. Needs no repo/config
/// resolution -- only the world files and the vdb.
/// Portage: actions.py, action_deselect
///
/// Candidates per target: a bare name expands through the world file's own
/// atoms (null-category substitution); an explicit-category atom is added
/// directly, plus a bare `cat/pkg:slot` candidate per installed version; a
/// `@name` target only ever matches a `world_sets` entry by exact name.
/// Every candidate is compared via `atom_intersects` plus the repo check.
/// Always exits 0, even with no matches or no targets.
pub(crate) fn run_deselect(
    targets: &[&str],
    root: &Path,
    pretend: bool,
    ask: bool,
    color: &Colorizer,
) -> ExitCode {
    let world_atoms = match read_world_atoms(root) {
        Ok(atoms) => atoms,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let world_sets = match read_world_sets(root) {
        Ok(sets) => sets,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let mut expanded: Vec<Atom> = Vec::new();
    // A `@name` target never goes through atom expansion below.
    let mut set_targets: HashSet<&str> = HashSet::new();
    for target in targets {
        if let Some(name) = target.strip_prefix('@') {
            set_targets.insert(name);
            continue;
        }
        if let Some(atom) = parse_atom(target).filter(|_| target.contains('/')) {
            // The literal target seeds the set; installed versions add a
            // bare `cat/pkg:slot` candidate each (no version/operator).
            expanded.push(atom.clone());
            for (version, slot, _sub_slot) in
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
            {
                let candidate_str = format!("{}/{}-{version}:{slot}", atom.category, atom.package);
                if match_from_list(target, &[candidate_str.as_str()]).is_some_and(|m| !m.is_empty())
                    && let Some(vardb_atom) =
                        parse_atom(&format!("{}/{}:{slot}", atom.category, atom.package))
                {
                    expanded.push(vardb_atom);
                }
            }
        } else if !target.contains('/') {
            // Bare name: null-category substitution, no installed check.
            for w in world_atoms.iter().filter_map(|w| parse_atom(w)) {
                if w.package != *target {
                    continue;
                }
                if let Some(substituted) = parse_atom(&format!("{}/{}", w.category, target)) {
                    expanded.push(substituted);
                }
            }
        } else {
            eprintln!("emerge: invalid atom {target:?}");
            return ExitCode::from(1);
        }
    }

    // `(entry text, source file)` pairs, sorted together into one list.
    let mut discard: Vec<(String, &'static str)> = world_atoms
        .iter()
        .filter(|world_atom_str| {
            let Some(w) = parse_atom(world_atom_str) else {
                return false;
            };
            expanded.iter().any(|arg_atom| {
                portage_dep::atom_intersects(arg_atom, &w)
                    && !(arg_atom.repo.is_some() && w.repo.is_none())
            })
        })
        .map(|s| (s.clone(), "world"))
        .collect();
    discard.extend(
        world_sets
            .iter()
            .filter(|name| set_targets.contains(name.as_str()))
            .map(|name| (format!("@{name}"), "world_sets")),
    );

    if discard.is_empty() {
        println!(">>> No matching atoms found in \"world\" favorites file...");
        return ExitCode::SUCCESS;
    }
    discard.sort_by(|a, b| a.0.cmp(&b.0));
    let verb = if pretend { "Would remove" } else { "Removing" };
    for (entry, filename) in &discard {
        println!(">>> {verb} {entry} from \"{filename}\" favorites file...");
    }

    // `--ask` prompts once, after the lines, before either file is rewritten.
    if ask
        && !pretend
        && !ask_confirm(
            color,
            "Would you like to remove these packages from your world favorites?",
        )
    {
        return ExitCode::from(130);
    }

    if !pretend {
        // Both files rewritten: sorted, one per line, comments dropped.
        let drop_world: HashSet<&str> = discard
            .iter()
            .filter(|(_, f)| *f == "world")
            .map(|(e, _)| e.as_str())
            .collect();
        let drop_sets: HashSet<&str> = discard
            .iter()
            .filter(|(_, f)| *f == "world_sets")
            .map(|(e, _)| e.as_str())
            .collect();

        let mut remaining_world: Vec<String> = world_atoms
            .into_iter()
            .filter(|a| !drop_world.contains(a.as_str()))
            .collect();
        remaining_world.sort();
        remaining_world.dedup();

        let mut remaining_sets: Vec<String> = world_sets
            .into_iter()
            .map(|n| format!("@{n}"))
            .filter(|s| !drop_sets.contains(s.as_str()))
            .collect();
        remaining_sets.sort();
        remaining_sets.dedup();

        // One transaction: `world`, then `world_sets`.
        let db = portage_vdb::for_root(root);
        let rewritten = db.begin_write().and_then(|mut txn| {
            txn.set_world(&portage_vdb::World {
                atoms: remaining_world,
            })?;
            txn.set_world_sets(&portage_vdb::WorldSets {
                sets: remaining_sets
                    .into_iter()
                    .map(|s| s.strip_prefix('@').unwrap_or(&s).to_string())
                    .collect(),
            })?;
            txn.commit()
        });
        if let Err(e) = rewritten {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}
