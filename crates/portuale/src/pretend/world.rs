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
            } else {
                direct.push(line.to_string());
            }
        }
        out.push((name, direct));
    }
    Ok(out)
}
