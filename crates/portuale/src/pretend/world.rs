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
    let mut atoms = read_world_atoms(root).map_err(|e| format!("emerge: {e}"))?;
    for name in read_world_sets(root).map_err(|e| format!("emerge: {e}"))? {
        let mut seen = HashSet::new();
        atoms.extend(resolve_custom_set(config_root, &name, &mut seen)?);
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

/// The `@name` set references in `world_sets`, with the `@` stripped. A missing
/// file is empty, not an error.
/// Portage: files.py, WorldSelectedSetsSet.
pub(crate) fn read_world_sets(root: &Path) -> Result<Vec<String>, String> {
    portage_vdb::for_root(root)
        .world_sets()
        .map(|w| w.sets)
        .map_err(world_read_error)
}

/// Expand one file-based set, following nested `@` references with a cycle guard.
/// A missing file is an error, not an absence.
/// Portage: _sets/__init__.py, StaticFileSet.
pub(crate) fn resolve_custom_set(
    config_root: &Path,
    name: &str,
    seen: &mut HashSet<String>,
) -> Result<Vec<String>, String> {
    if !seen.insert(name.to_string()) {
        return Ok(Vec::new());
    }
    let path = config_root.join("etc/portage/sets").join(name);
    let text =
        std::fs::read_to_string(&path).map_err(|_| format!("emerge: set '{name}' not found"))?;
    let mut atoms = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some(nested_name) = line.strip_prefix('@') {
            atoms.extend(resolve_custom_set(config_root, nested_name, seen)?);
        } else {
            atoms.push(line.to_string());
        }
    }
    Ok(atoms)
}
