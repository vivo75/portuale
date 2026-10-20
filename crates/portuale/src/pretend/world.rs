//! The world and world_sets files.

use crate::pretend::*;

/// Reads `<root>/var/lib/portage/world`, one atom per line. A missing file is
/// empty, not an error. `@`-prefixed lines are dropped here; nested sets live
/// in `world_sets` (see `read_world_sets`).
/// Portage: files.py, WorldSelectedPackagesSet.
pub(crate) fn read_world_atoms(root: &Path) -> Result<Vec<String>, String> {
    portage_vdb::for_root(root)
        .world()
        .map(|w| w.atoms)
        .map_err(world_read_error)
}

/// The text the world readers always gave: `reading <path>: <io error>`
/// for a store that could not be read.
pub(crate) fn world_read_error(e: portage_vdb::Error) -> String {
    match e {
        portage_vdb::Error::Io { path, source } => {
            format!("reading {}: {source}", path.display())
        }
        e => e.to_string(),
    }
}

/// The world file's atoms plus every set named in `world_sets`. Callers add the
/// `@profile`/`@system` halves themselves. Errors carry an `emerge: ` prefix.
/// Portage: files.py, WorldSelectedSet.
pub(crate) fn expand_selected(root: &Path, config_root: &Path) -> Result<Vec<String>, String> {
    let mut atoms: Vec<String> = read_world_atoms(root)
        .map_err(|e| format!("emerge: {e}"))?
        // Portage `WorldSelectedPackagesSet` validates every line with the
        // same `ValidAtomValidator` as named sets, but its errors stay on
        // the inner `_pkgset` and are never printed (`_sets/files.py`);
        // bad world lines drop silently (oracle Q2g).
        .into_iter()
        .filter(|a| is_valid_set_atom(a))
        .collect();
    for name in read_world_sets(root).map_err(|e| format!("emerge: {e}"))? {
        let mut seen = HashSet::new();
        let out = resolve_custom_set(config_root, &name, &mut seen)?;
        atoms.extend(out.atoms);
    }
    Ok(atoms)
}

/// A `cat/pkg:slot` atom per installed package, sorted and deduplicated. Always
/// slot-qualified, even a lone slot.
/// Portage: dbapi.py, EverythingSet.
pub(crate) fn installed_set_atoms(root: &Path) -> Vec<String> {
    let mut atoms: Vec<String> = portage_repo::all_installed_packages(root)
        .into_iter()
        .map(|p| format!("{}/{}:{}", p.category, p.package, p.slot))
        .collect();
    atoms.sort();
    atoms.dedup();
    atoms
}

/// Installed packages still linking against a preserved library, as
/// `cat/pkg:slot`, sorted and deduplicated. An empty registry gives empty.
/// Portage: libs.py, PreservedLibraryConsumerSet.
pub(crate) fn preserved_rebuild_atoms(root: &Path) -> Vec<String> {
    let plib_dict = ebuild_merge::preserved_lib_paths(root);
    if plib_dict.is_empty() {
        return Vec::new();
    }
    let all_preserved: HashSet<String> = plib_dict.values().flatten().cloned().collect();

    let owner_entries = needed_elf::read_all_needed_entries(root);
    let map = needed_elf::rebuild(root, &owner_entries);
    let defpath = needed_elf::getlibpaths(root, None);

    let mut atoms: Vec<String> = Vec::new();
    for lib in &all_preserved {
        let consumers = match needed_elf::find_consumers(needed_elf::FindConsumersArgs {
            root,
            map: &map,
            defpath: &defpath,
            obj: lib,
            exclude_provider: None,
            greedy: false,
        }) {
            Ok(set) => set,
            Err(_) => {
                let soname = lib.rsplit('/').next().unwrap_or(lib);
                needed_elf::soname_consumers(&map, soname)
            }
        };
        for consumer in consumers {
            if all_preserved.contains(&consumer) {
                continue;
            }
            let ckey = needed_elf::obj_key(root, &consumer);
            let Some(owner) = map.obj_properties.get(&ckey).map(|p| p.owner.as_str()) else {
                continue;
            };
            let Some((cat, pn, version)) = portage_repo::split_cpv(owner) else {
                continue;
            };
            let slot = ebuild_merge::read_installed_slot(root, &cat, &pn, &version)
                .unwrap_or_else(|| "0".to_string());
            atoms.push(format!("{cat}/{pn}:{slot}"));
        }
    }
    atoms.sort();
    atoms.dedup();
    atoms
}

/// The new world atoms in argument order, for the pre-record `--ask` prompt.
/// `@set` targets belong in `world_sets`, not here.
/// Portage: depgraph.py, saveNomergeFavorites.
pub(crate) fn world_file_additions(
    current: &[String],
    target_atoms: &[&str],
    entries: &[GraphEntry],
    system_atoms: &[String],
    repos: &[portage_repo::RepoConfig],
) -> Vec<String> {
    let before: std::collections::HashSet<String> = current.iter().cloned().collect();
    let mut added = Vec::new();

    for raw in target_atoms {
        if raw.starts_with('@') {
            // A `@set` target belongs in `world_sets`, not `world`.
            continue;
        }
        let Some(atom) = parse_atom(raw) else {
            continue;
        };
        let Some(entry) = entries
            .iter()
            .find(|e| e.category == atom.category && e.package == atom.package)
        else {
            continue;
        };
        // Unslotted `@system` members are never recorded.
        let cpv_any = format!("{}/{}-0", atom.category, atom.package);
        let in_system = system_atoms
            .iter()
            .any(|a| match_from_list(a, &[cpv_any.as_str()]).is_some_and(|m| !m.is_empty()));
        let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
        if in_system && unslotted && atom.repo.is_none() {
            continue;
        }

        // The arg's `:slot` is kept only when it matches the resolved slot and the
        // cp is slotted (several SLOTs, or one non-`"0"` SLOT).
        let slot_suffix = atom.slot.as_deref().filter(|s| {
            entry.slot.as_deref() == Some(s)
                && portage_repo::list_candidates(repos, &atom.category, &atom.package)
                    .map(|cands| {
                        let slots: std::collections::HashSet<&str> =
                            cands.iter().map(|c| c.slot.as_str()).collect();
                        slots.len() > 1 || (slots.len() == 1 && !slots.contains("0"))
                    })
                    .unwrap_or(false)
        });

        let mut world_atom = format!("{}/{}", atom.category, atom.package);
        if let Some(slot) = slot_suffix {
            world_atom.push(':');
            world_atom.push_str(slot);
        }
        if let Some(repo) = &atom.repo {
            world_atom.push_str("::");
            world_atom.push_str(repo);
        }
        if before.contains(&world_atom) {
            continue;
        }
        added.push(world_atom);
    }
    added
}

/// Records one merged entry in `world` right after its install and returns the
/// `>>> Recording ...` status lines, which the scheduler prints between
/// `Installing` and `Completed` like any other status message.
pub type WorldRecorder = std::sync::Arc<dyn Fn(&GraphEntry) -> Vec<String> + Send + Sync>;

/// Builds the per-entry recorder for an `emerge` run; `None` when the run
/// records nothing (`--oneshot`, `--onlydeps`, `--buildpkgonly`).
// Portage: Scheduler.py, _world_atom
pub(crate) fn world_recorder(
    root: &Path,
    opts: &EmergeOpts<'_>,
    system_atoms: &[String],
    repos: &[portage_repo::RepoConfig],
) -> Option<WorldRecorder> {
    if opts.oneshot || opts.onlydeps || opts.buildpkgonly {
        return None;
    }
    let root = root.to_path_buf();
    let target_atoms: Vec<String> = opts.atom_args.iter().map(|a| a.to_string()).collect();
    let system_atoms = system_atoms.to_vec();
    let repos = repos.to_vec();
    Some(std::sync::Arc::new(move |entry: &GraphEntry| {
        let targets: Vec<&str> = target_atoms.iter().map(String::as_str).collect();
        match record_world_atoms(UpdateWorldFileArgs {
            root: &root,
            target_atoms: &targets,
            entries: std::slice::from_ref(entry),
            system_atoms: &system_atoms,
            repos: &repos,
            oneshot: false,
            onlydeps: false,
        }) {
            Ok(added) => added.iter().map(|a| recording_line(a)).collect(),
            Err(e) => {
                eprintln!("emerge: {e}");
                Vec::new()
            }
        }
    }))
}

/// Arguments of [`update_world_file`].
pub(crate) struct UpdateWorldFileArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) target_atoms: &'a [&'a str],
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) system_atoms: &'a [String],
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) oneshot: bool,
    pub(crate) onlydeps: bool,
}

/// Record directly-requested plain targets in the world file after a successful
/// run. `--oneshot`/`--onlydeps` suppress it. Prints `>>> Recording ...` per addition.
/// Portage: depgraph.py, saveNomergeFavorites.
pub(crate) fn update_world_file(args: UpdateWorldFileArgs<'_>) -> Result<(), String> {
    for world_atom in record_world_atoms(args)? {
        println!("{}", recording_line(&world_atom));
    }
    Ok(())
}

/// The `>>> Recording <atom> in "world" favorites file...` status line.
pub(crate) fn recording_line(world_atom: &str) -> String {
    format!(">>> Recording {world_atom} in \"world\" favorites file...")
}

/// Adds the new atoms to the world file and returns them, sorted; prints nothing.
pub(crate) fn record_world_atoms(args: UpdateWorldFileArgs<'_>) -> Result<Vec<String>, String> {
    let UpdateWorldFileArgs {
        root,
        target_atoms,
        entries,
        system_atoms,
        repos,
        oneshot,
        onlydeps,
    } = args;
    if oneshot || onlydeps {
        return Ok(Vec::new());
    }
    let mut current = read_world_atoms(root)?;
    let mut added = world_file_additions(&current, target_atoms, entries, system_atoms, repos);
    // Sorted before printing, like Portage.
    added.sort();
    current.extend(added.iter().cloned());

    if !added.is_empty() {
        current.sort();
        current.dedup();
        let db = portage_vdb::for_root(root);
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.set_world(&portage_vdb::World { atoms: current })
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
    }
    Ok(added)
}

/// The new `@set` lines in argument order, for the same pre-record `--ask`
/// prompt as `world_file_additions`.
/// Portage: depgraph.py, saveNomergeFavorites.
pub(crate) fn world_sets_additions(current: &[String], set_names: &[String]) -> Vec<String> {
    let before: std::collections::HashSet<String> = current.iter().cloned().collect();
    let mut added = Vec::new();
    for name in set_names {
        let line = format!("@{name}");
        if before.contains(&line) {
            continue;
        }
        added.push(line);
    }
    added
}

/// Whether file sets are world candidates: true unless `sets.conf` says
/// otherwise. Unparsable values are ignored (a leniency: Portage aborts).
/// Portage: _sets/__init__.py.
pub(crate) fn usersets_world_candidate(config_root: &Path) -> bool {
    fn files_in(dir: &Path) -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<std::path::PathBuf> =
            entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        out.sort();
        out
    }
    let base = config_root.join("etc/portage/sets.conf");
    let paths: Vec<std::path::PathBuf> = if base.is_dir() {
        files_in(&base)
    } else if base.is_file() {
        vec![base]
    } else {
        return true;
    };
    // Unknown spellings leave the running value alone.
    fn as_bool(value: &str) -> Option<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "yes" | "true" | "on" => Some(true),
            "0" | "no" | "false" | "off" => Some(false),
            _ => None,
        }
    }
    let mut candidate = true;
    // Section state carries across files (a leniency: Portage resets it per file).
    let mut in_usersets = false;
    for path in &paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_usersets = line[1..line.len() - 1].trim() == "usersets";
                continue;
            }
            if !in_usersets {
                continue;
            }
            let split = line.find('=').or_else(|| line.find(':'));
            let Some(i) = split else { continue };
            if line[..i].trim().eq_ignore_ascii_case("world-candidate")
                && let Some(v) = as_bool(&line[i + 1..])
            {
                candidate = v;
            }
        }
    }
    candidate
}

/// Record directly-given set args in `world_sets` after a successful run.
/// Prints `>>> Recording ...` per addition.
/// Portage: depgraph.py, saveNomergeFavorites.
pub(crate) fn update_world_sets_file(
    root: &Path,
    set_names: &[String],
    oneshot: bool,
    onlydeps: bool,
) -> Result<(), String> {
    if oneshot || onlydeps || set_names.is_empty() {
        return Ok(());
    }
    let mut current: Vec<String> = read_world_sets(root)?
        .into_iter()
        .map(|n| format!("@{n}"))
        .collect();
    let mut added = world_sets_additions(&current, set_names);
    // Sorted before printing, like Portage.
    added.sort();
    for line in &added {
        println!(">>> Recording {line} in \"world_sets\" favorites file...");
        current.push(line.clone());
    }
    if !added.is_empty() {
        current.sort();
        current.dedup();
        let db = portage_vdb::for_root(root);
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.set_world_sets(&portage_vdb::WorldSets {
            sets: current
                .into_iter()
                .map(|l| l.strip_prefix('@').unwrap_or(&l).to_string())
                .collect(),
        })
        .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The `@name` set references in `world_sets`, with the `@` stripped. A missing
/// file is empty, not an error.
/// Portage: files.py, WorldSelectedSetsSet.
pub(crate) fn read_world_sets(root: &Path) -> Result<Vec<String>, String> {
    portage_vdb::for_root(root)
        .world_sets()
        .map(|w| w.sets)
        .map_err(world_read_error)
}

/// One file-based set's valid atoms plus its dropped lines, already
/// rendered as Portage prints them after the merge list.
/// Portage: _sets/__init__.py, StaticFileSet; depgraph.py:11142-11145.
pub(crate) struct CustomSetOut {
    pub(crate) atoms: Vec<String>,
    pub(crate) errors: Vec<String>,
}

/// Expand one file-based set, following nested `@` references with a cycle guard.
/// A missing file is an error, not an absence.
/// Every non-`@` line is validated with the set-file `ValidAtomValidator`
/// (see [`is_valid_set_atom`]); bad lines drop and are reported as
/// `<path>: Validation failed at line: N, data X` with the file's physical
/// line number (Portage `ItemFileLoader`, `portage/env/loaders.py:203-221`).
/// Only the depgraph path prints them (after the merge list); `@selected`
/// and the removal paths drop them silently (oracle Q2g).
/// Portage: _sets/__init__.py, StaticFileSet.
pub(crate) fn resolve_custom_set(
    config_root: &Path,
    name: &str,
    seen: &mut HashSet<String>,
) -> Result<CustomSetOut, String> {
    if !seen.insert(name.to_string()) {
        return Ok(CustomSetOut {
            atoms: Vec::new(),
            errors: Vec::new(),
        });
    }
    let path = config_root.join("etc/portage/sets").join(name);
    let text =
        std::fs::read_to_string(&path).map_err(|_| format!("emerge: set '{name}' not found"))?;
    let mut atoms = Vec::new();
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(nested_name) = line.strip_prefix('@') {
            let nested = resolve_custom_set(config_root, nested_name, seen)?;
            atoms.extend(nested.atoms);
            errors.extend(nested.errors);
        } else if is_valid_set_atom(line) {
            atoms.push(line.to_string());
        } else {
            errors.push(format!(
                "{}: Validation failed at line: {}, data {line}",
                path.display(),
                n + 1
            ));
        }
    }
    Ok(CustomSetOut { atoms, errors })
}

/// Every selected custom set with its direct atoms, for the unmerge
/// "still listed" warning. Missing sets are dropped.
/// Portage: unmerge.py, _unmerge_display.
pub(crate) fn collect_installed_sets(
    config_root: &Path,
    root: &Path,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = read_world_sets(root)?;
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let path = config_root.join("etc/portage/sets").join(&name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut direct: Vec<String> = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            if let Some(nested) = line.strip_prefix('@') {
                queue.push(nested.to_string());
            } else if is_valid_set_atom(line) {
                direct.push(line.to_string());
            }
        }
        out.push((name, direct));
    }
    Ok(out)
}
