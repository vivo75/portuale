//! Preserved libraries during unmerge and prune.

use crate::ebuild_merge::*;

/// Arguments of [`preserve_libs_on_unmerge`].
#[cfg(test)]
pub(crate) struct PreserveLibsOnUnmergeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) contents_text: &'a str,
    pub(crate) is_replacement: bool,
}

/// Unregisters this package's registry entry, then re-registers libraries still needed
/// under the package being removed. Replacement mode only unregisters and returns empty.
/// Returns the preserved paths for the caller to exclude from file removal.
// Portage: vartree.py, dblink._prune_plib_registry
#[cfg(test)]
pub(crate) fn preserve_libs_on_unmerge(
    args: PreserveLibsOnUnmergeArgs<'_>,
) -> Result<BTreeSet<String>, String> {
    let PreserveLibsOnUnmergeArgs {
        root,
        category,
        pn,
        pf,
        slot,
        contents_text,
        is_replacement,
    } = args;
    preserve_libs_on_unmerge_into(PreserveLibsOnUnmergeIntoArgs {
        root,
        category,
        pn,
        pf,
        slot,
        contents_text,
        is_replacement,
        retire: None,
    })
}

/// Arguments of [`preserve_libs_on_unmerge_into`].
pub(crate) struct PreserveLibsOnUnmergeIntoArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) contents_text: &'a str,
    pub(crate) is_replacement: bool,
    pub(crate) retire: Option<&'a mut RetireWrites>,
}

/// [`preserve_libs_on_unmerge`], keeping the registry in `retire` instead of writing it.
pub(crate) fn preserve_libs_on_unmerge_into(
    args: PreserveLibsOnUnmergeIntoArgs<'_>,
) -> Result<BTreeSet<String>, String> {
    let PreserveLibsOnUnmergeIntoArgs {
        root,
        category,
        pn,
        pf,
        slot,
        contents_text,
        is_replacement,
        retire,
    } = args;
    let instance_owns_files = !contents_text.trim().is_empty();

    let counter = read_entry_text(root, category, pf, "COUNTER").unwrap_or_else(|| "0".to_string());
    let cpv = format!("{category}/{pf}");

    let mut registry = read_plib_registry(root);
    // The orphan scan must see the registry before this package's entry is unregistered.
    let preserved = if is_replacement || !instance_owns_files {
        BTreeSet::new()
    } else {
        let old_contents: Vec<String> = portage_vdb::paths(contents_text)
            .map(|(_, path)| path.to_string())
            .collect();

        let owner_entries = owner_entries_with_preserved_orphans(root, &registry.preserved_libs());
        let map = crate::needed_elf::rebuild(root, &owner_entries);
        let defpath =
            crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

        let old_owner_is_owner = |p: &str| owns_path_pf(root, category, pf, p);
        let new_owner_is_owner = |_: &str| false;

        crate::needed_elf::find_libs_to_preserve(crate::needed_elf::FindLibsToPreserveArgs {
            root,
            map: &map,
            defpath: &defpath,
            old_contents: &old_contents,
            old_owner_is_owner: &old_owner_is_owner,
            new_owner_is_owner: &new_owner_is_owner,
        })
    };

    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: &cpv,
        category,
        pn,
        slot,
        counter: &counter,
        paths: &[],
    });
    if !is_replacement && !preserved.is_empty() {
        let paths_vec: Vec<String> = preserved.iter().cloned().collect();
        register_preserved_libs(RegisterPreservedLibsArgs {
            registry: &mut registry,
            cpv: &cpv,
            category,
            pn,
            slot,
            counter: &counter,
            paths: &paths_vec,
        });
    }
    match retire {
        Some(retire) => retire.registry = Some(registry),
        None => write_plib_registry(root, &registry)?,
    }

    Ok(if is_replacement {
        BTreeSet::new()
    } else {
        preserved
    })
}

/// Every registered preserved-library path, keyed by owning cpv.
pub(crate) fn preserved_lib_paths(root: &Path) -> BTreeMap<String, Vec<String>> {
    read_plib_registry(root).preserved_libs()
}

/// Arguments of [`find_unused_preserved_libs`].
#[cfg(test)]
pub(crate) struct FindUnusedPreservedLibsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) unmerge_no_replacement: bool,
    pub(crate) being_unmerged: &'a dyn Fn(&str) -> bool,
    pub(crate) exclude_cpvs: &'a [String],
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
}

/// Registered preserved libraries no installed package links against any more, keyed by
/// owner. Paths whose owner left the vdb fall back to a soname reverse lookup.
// Portage: vartree.py, dblink._find_unused_preserved_libs
#[cfg(test)]
pub(crate) fn find_unused_preserved_libs(
    args: FindUnusedPreservedLibsArgs<'_>,
) -> BTreeMap<String, BTreeSet<String>> {
    let FindUnusedPreservedLibsArgs {
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    } = args;
    find_unused_preserved_libs_in(FindUnusedPreservedLibsInArgs {
        root,
        registry: read_plib_registry(root),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    })
}

/// Arguments of [`find_unused_preserved_libs_in`].
pub(crate) struct FindUnusedPreservedLibsInArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) registry: PlibRegistry,
    pub(crate) unmerge_no_replacement: bool,
    pub(crate) being_unmerged: &'a dyn Fn(&str) -> bool,
    pub(crate) exclude_cpvs: &'a [String],
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
}

/// [`find_unused_preserved_libs`] over an explicit in-memory registry.
pub(crate) fn find_unused_preserved_libs_in(
    args: FindUnusedPreservedLibsInArgs<'_>,
) -> BTreeMap<String, BTreeSet<String>> {
    let FindUnusedPreservedLibsInArgs {
        root,
        registry,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    } = args;
    let plib_dict = registry.preserved_libs();
    if plib_dict.is_empty() {
        return BTreeMap::new();
    }

    let owner_entries = linkage_owner_entries(
        root,
        &plib_dict,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    );
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

    let all_preserved: BTreeSet<String> = plib_dict.values().flatten().cloned().collect();
    let mut path_cpv: BTreeMap<String, String> = BTreeMap::new();
    for (cpv, paths) in &plib_dict {
        for p in paths {
            path_cpv.insert(p.clone(), cpv.clone());
        }
    }

    let exists = |p: &str| std::fs::symlink_metadata(root.join(p.trim_start_matches('/'))).is_ok();

    let mut consumers_by_preserved: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in &all_preserved {
        if !exists(path) {
            continue;
        }
        let indexed = crate::needed_elf::find_consumers(crate::needed_elf::FindConsumersArgs {
            root,
            map: &map,
            defpath: &defpath,
            obj: path,
            exclude_provider: None,
            greedy: true,
        });
        let mut consumers: BTreeSet<String> = match indexed {
            Ok(set) => set,
            // Owning package gone from the vdb: match by soname instead.
            Err(_) => {
                let soname = path.rsplit('/').next().unwrap_or(path);
                crate::needed_elf::soname_consumers(&map, soname)
            }
        };
        if unmerge_no_replacement {
            consumers.retain(|c| !being_unmerged(c));
        }
        consumers_by_preserved.insert(path.clone(), consumers);
    }

    let unneeded =
        crate::needed_elf::find_unneeded_preserved(root, &consumers_by_preserved, &all_preserved);

    let mut cpv_lib_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in unneeded {
        if let Some(cpv) = path_cpv.get(&path) {
            cpv_lib_map.entry(cpv.clone()).or_default().insert(path);
        }
    }
    cpv_lib_map
}

/// Arguments of [`prune_unused_preserved_libs`].
pub(crate) struct PruneUnusedPreservedLibsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) unmerge_no_replacement: bool,
    pub(crate) being_unmerged: &'a dyn Fn(&str) -> bool,
    pub(crate) exclude_cpvs: &'a [String],
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
}

/// Deletes the libraries `find_unused_preserved_libs` reports as unneeded, strips them
/// from owners' `CONTENTS`, and reprunes the registry. Returns the removed paths.
// Portage: vartree.py, dblink._prune_plib_registry
pub(crate) fn prune_unused_preserved_libs(
    args: PruneUnusedPreservedLibsArgs<'_>,
) -> Result<Vec<String>, String> {
    let PruneUnusedPreservedLibsArgs {
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    } = args;
    prune_unused_preserved_libs_into(PruneUnusedPreservedLibsIntoArgs {
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
        retire: None,
    })
}

/// Arguments of [`prune_unused_preserved_libs_into`].
pub(crate) struct PruneUnusedPreservedLibsIntoArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) unmerge_no_replacement: bool,
    pub(crate) being_unmerged: &'a dyn Fn(&str) -> bool,
    pub(crate) exclude_cpvs: &'a [String],
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
    pub(crate) retire: Option<&'a mut RetireWrites>,
}

/// [`prune_unused_preserved_libs`], leaving registry and rewrites in `retire`.
/// The files themselves are removed at once either way.
pub(crate) fn prune_unused_preserved_libs_into(
    args: PruneUnusedPreservedLibsIntoArgs<'_>,
) -> Result<Vec<String>, String> {
    let PruneUnusedPreservedLibsIntoArgs {
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
        mut retire,
    } = args;
    let cpv_lib_map = find_unused_preserved_libs_in(FindUnusedPreservedLibsInArgs {
        root,
        registry: plib_registry_for(root, retire.as_deref()),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    });

    let mut removed: Vec<String> = Vec::new();
    let mut parent_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let all_removed: BTreeSet<String> = cpv_lib_map.values().flatten().cloned().collect();
    for path in &all_removed {
        let abs = root.join(path.trim_start_matches('/'));
        let obj_type = match std::fs::symlink_metadata(&abs) {
            Ok(m) if m.file_type().is_symlink() => "sym",
            _ => "obj",
        };
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", abs.display())),
        }
        if let Some(parent) = abs.parent() {
            parent_dirs.insert(parent.to_path_buf());
        }
        println!("<<< !needed  {obj_type} {}", abs.display());
        removed.push(path.clone());
    }

    // Remove newly-emptied parent directories, stopping at the first non-empty one.
    for mut dir in parent_dirs {
        while dir.starts_with(root) && dir != root {
            if std::fs::remove_dir(&dir).is_err() {
                break;
            }
            match dir.parent() {
                Some(p) => dir = p.to_path_buf(),
                None => break,
            }
        }
    }

    let mut registry = plib_registry_for(root, retire.as_deref());
    for (cpv, paths) in &cpv_lib_map {
        let cat_pf = cpv.split_once('/');
        let still_installed = cat_pf
            .map(|(cat, pf)| {
                portage_vdb::for_root(root)
                    .has_entry(&portage_vdb::EntryKey::new(cat, pf))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if still_installed {
            remove_from_contents_into(root, cpv, paths, retire.as_deref_mut())?;
        }
        for (entry_cpv, _counter, entry_paths) in registry.entries.values_mut() {
            if entry_cpv == cpv {
                entry_paths.retain(|p| !paths.contains(p));
            }
        }
    }

    prune_non_existing(root, &mut registry);
    match retire {
        Some(retire) => retire.registry = Some(registry),
        None => write_plib_registry(root, &registry)?,
    }

    Ok(removed)
}
