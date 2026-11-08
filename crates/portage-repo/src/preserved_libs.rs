//! The preserved-libs registry, read-only: which libraries a package left
//! behind, and which installed packages still link against them
//! (`@preserved-rebuild`).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::{split_cpv, vdb_entry_slot};

fn norm_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for comp in path.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// The absolute path of a symlink's target: the target itself when absolute, else
/// resolved against the symlink's directory.
// Portage: lib/portage/__init__.py, portage.abssymlink
fn abssymlink(path: &str, target: &str) -> String {
    if target.starts_with('/') {
        norm_posix_path(target)
    } else {
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        norm_posix_path(&format!("{dir}/{target}"))
    }
}

/// Drops recorded paths no longer on disk, rebuilding survivors regular-files-first and
/// keeping only symlinks pointing at a surviving file; drops entries left with nothing.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.pruneNonExisting
pub fn prune_preserved_paths(
    root: &Path,
    entries: &mut BTreeMap<String, (String, String, Vec<String>)>,
) {
    let mut dead_keys = Vec::new();
    for (cps, (_cpv, _counter, paths)) in entries.iter_mut() {
        let mut ordered: Vec<String> = Vec::new();
        let mut hardlinks: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut symlinks: Vec<(String, String)> = Vec::new();
        for f in paths.iter() {
            let full = root.join(f.trim_start_matches('/'));
            let meta = match std::fs::symlink_metadata(&full) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                match std::fs::read_link(&full) {
                    Ok(target) => symlinks.push((f.clone(), target.to_string_lossy().into_owned())),
                    Err(_) => continue,
                }
            } else if meta.file_type().is_file() {
                hardlinks.insert(f.clone());
                ordered.push(f.clone());
            }
        }
        for (f, target) in &symlinks {
            if hardlinks.contains(&abssymlink(f, target)) {
                ordered.push(f.clone());
            }
        }
        if ordered.is_empty() {
            dead_keys.push(cps.clone());
        } else {
            *paths = ordered;
        }
    }
    for key in dead_keys {
        entries.remove(&key);
    }
}

/// The stored registry as `owner cpv -> paths`, pruned of vanished paths; a
/// missing or unparseable registry gives an empty map.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.load, getPreservedLibs
pub fn preserved_lib_paths(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut entries: BTreeMap<String, (String, String, Vec<String>)> = portage_vdb::for_root(root)
        .preserved_libs()
        .map(|libs| libs.entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(key, e)| (key, (e.cpv, e.counter, e.paths)))
        .collect();
    prune_preserved_paths(root, &mut entries);
    entries
        .into_values()
        .map(|(cpv, _counter, paths)| (cpv, paths))
        .collect()
}

/// Installed packages still linking against a preserved library, as
/// `cat/pkg:slot`, sorted and deduplicated. An empty registry gives empty.
/// Portage: libs.py, PreservedLibraryConsumerSet.
pub fn preserved_rebuild_atoms(root: &Path) -> Vec<String> {
    let plib_dict = preserved_lib_paths(root);
    if plib_dict.is_empty() {
        return Vec::new();
    }
    let all_preserved: HashSet<String> = plib_dict.values().flatten().cloned().collect();

    let owner_entries = crate::needed_elf::read_all_needed_entries(root);
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath = crate::needed_elf::getlibpaths(root, None);

    let mut atoms: Vec<String> = Vec::new();
    for lib in &all_preserved {
        let consumers =
            match crate::needed_elf::find_consumers(crate::needed_elf::FindConsumersArgs {
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
                    crate::needed_elf::soname_consumers(&map, soname)
                }
            };
        for consumer in consumers {
            if all_preserved.contains(&consumer) {
                continue;
            }
            let ckey = crate::needed_elf::obj_key(root, &consumer);
            let Some(owner) = map.obj_properties.get(&ckey).map(|p| p.owner.as_str()) else {
                continue;
            };
            let Some((cat, pn, version)) = split_cpv(owner) else {
                continue;
            };
            let raw = vdb_entry_slot(root, &cat, &format!("{pn}-{version}"));
            let slot = match raw.split('/').next() {
                Some(main) if !main.is_empty() => main,
                _ => "0",
            };
            atoms.push(format!("{cat}/{pn}:{slot}"));
        }
    }
    atoms.sort();
    atoms.dedup();
    atoms
}
