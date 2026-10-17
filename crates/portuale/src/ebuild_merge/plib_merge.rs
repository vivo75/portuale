//! Preserved libraries during a merge.

use crate::ebuild_merge::*;

/// Drops paths this merge took over from the registry and the previous
/// owner's vdb `CONTENTS`; skipped when the previous owner is the merged package.
// Portage: vartree.py, dblink.merge
pub(crate) fn unregister_preserved_libs(
    root: &Path,
    merging_cpv: &str,
    mut registry: PlibRegistry,
    plib_collisions: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), String> {
    for (cpv, paths) in plib_collisions {
        let mut empty_key = None;
        for (key, (entry_cpv, _counter, entry_paths)) in registry.entries.iter_mut() {
            if entry_cpv == cpv {
                entry_paths.retain(|p| !paths.contains(p));
                if entry_paths.is_empty() {
                    empty_key = Some(key.clone());
                }
                break;
            }
        }
        if let Some(key) = empty_key {
            registry.entries.remove(&key);
        }

        if cpv != merging_cpv {
            remove_from_contents(root, cpv, paths)?;
        }
    }
    write_plib_registry(root, &registry)
}

/// Strips `paths` from `cpv`'s vdb `CONTENTS` plus its stale `NEEDED.ELF.2`
/// entries; a missing vdb entry is silently a no-op.
// Portage: vartree.py, vardbapi.removeFromContents
pub(crate) fn remove_from_contents(
    root: &Path,
    cpv: &str,
    paths: &BTreeSet<String>,
) -> Result<(), String> {
    remove_from_contents_into(root, cpv, paths, None)
}

/// [`remove_from_contents`], collecting the rewrites into `retire` instead of writing.
pub(crate) fn remove_from_contents_into(
    root: &Path,
    cpv: &str,
    paths: &BTreeSet<String>,
    mut retire: Option<&mut RetireWrites>,
) -> Result<(), String> {
    let Some((category, pf)) = cpv.split_once('/') else {
        return Ok(());
    };
    let key = portage_vdb::EntryKey::new(category, pf);
    let Some(text) = read_entry_text(root, category, pf, "CONTENTS") else {
        return Ok(());
    };
    let db = portage_vdb::for_root(root);
    let mut removed = false;
    let mut surviving_paths: BTreeSet<String> = BTreeSet::new();
    let new_text: String = text
        .lines()
        .filter(|line| {
            let mut parts = line.split_whitespace();
            parts.next();
            let abs_path = parts.next();
            if matches!(abs_path, Some(p) if paths.contains(p)) {
                removed = true;
                false
            } else {
                if let Some(p) = abs_path {
                    surviving_paths.insert(p.to_string());
                }
                true
            }
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let mut replace = |name: &str, data: String| -> Result<(), String> {
        if let Some(retire) = retire.as_deref_mut() {
            retire
                .files
                .push((key.clone(), name.to_string(), data.into_bytes()));
            return Ok(());
        }
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.replace_file(&key, name, data.as_bytes())
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())
    };
    replace("CONTENTS", new_text)?;

    if removed && let Some(needed_text) = read_entry_text(root, category, pf, "NEEDED.ELF.2") {
        let new_needed: String = crate::needed_elf::NeededEntry::parse_file(&needed_text)
            .into_iter()
            .filter(|entry| surviving_paths.contains(&entry.filename))
            .map(|entry| entry.to_needed_line())
            .collect();
        replace("NEEDED.ELF.2", new_needed)?;
    }
    Ok(())
}

/// Arguments of [`register_preserved_libs`].
pub(crate) struct RegisterPreservedLibsArgs<'a> {
    pub(crate) registry: &'a mut PlibRegistry,
    pub(crate) cpv: &'a str,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) counter: &'a str,
    pub(crate) paths: &'a [String],
}

/// Registers (`paths`) or unregisters (empty `paths`, only on the same `cpv` + `counter`)
/// one `cp:slot` record; the key is `category/package:slot`, without the version.
// Portage: PreservedLibsRegistry.py, PreservedLibsRegistry.register
pub(crate) fn register_preserved_libs(args: RegisterPreservedLibsArgs<'_>) {
    let RegisterPreservedLibsArgs {
        registry,
        cpv,
        category,
        pn,
        slot,
        counter,
        paths,
    } = args;
    let cps = format!("{category}/{pn}:{slot}");
    let counter = counter.trim();
    if paths.is_empty() {
        if let Some((entry_cpv, entry_counter, _)) = registry.entries.get(&cps)
            && entry_cpv == cpv
            && entry_counter.trim() == counter
        {
            registry.entries.remove(&cps);
        }
    } else {
        registry
            .entries
            .insert(cps, (cpv.to_string(), counter.to_string(), paths.to_vec()));
    }
}

/// The registry-orphan scan input [`linkage_owner_entries`] scans: the
/// `exclude_cpvs` removal applies to the registry map first, then the
/// just-preserved feed merges in untouched, so a feed key equal to an
/// excluded cpv (a same-version reinstall) survives.
// Portage: LinkageMapELF.py, LinkageMapELF.rebuild
pub(crate) fn preserved_scan_input(
    preserved: &BTreeMap<String, Vec<String>>,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut scan_input = preserved.clone();
    for excluded in exclude_cpvs {
        scan_input.remove(excluded);
    }
    for (owner, paths) in replacement_preserved {
        scan_input
            .entry(owner.clone())
            .or_default()
            .extend(paths.iter().cloned());
    }
    scan_input
}

/// Shared `LinkageMap` input for the preserve-libs computations: vdb entries plus
/// registry-orphan scan results. `exclude_cpvs` drops unmerging instances; the
/// `replacement_*` feeds cover data that enumeration skips.
// Portage: vartree.py, LinkageMap.rebuild
pub(crate) fn linkage_owner_entries(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    let mut owner_entries: Vec<(String, Vec<crate::needed_elf::NeededEntry>)> =
        crate::needed_elf::read_all_needed_entries(root)
            .into_iter()
            .filter(|(cpv, _)| !exclude_cpvs.contains(cpv))
            .collect();
    for (owner, entries) in replacement_needed {
        if let Some(slot) = owner_entries.iter_mut().find(|(o, _)| o == owner) {
            slot.1.extend(entries.iter().cloned());
        } else {
            owner_entries.push((owner.clone(), entries.clone()));
        }
    }
    let scan_input = preserved_scan_input(preserved, exclude_cpvs, replacement_preserved);
    if scan_input.is_empty() {
        return owner_entries;
    }
    for (owner, entries) in crate::needed_elf::scan_preserved_lib_entries(root, &scan_input) {
        if let Some(slot) = owner_entries.iter_mut().find(|(o, _)| o == &owner) {
            slot.1.extend(entries);
        } else {
            owner_entries.push((owner, entries));
        }
    }
    owner_entries
}

pub(crate) fn owner_entries_with_preserved_orphans(
    root: &Path,
    preserved: &BTreeMap<String, Vec<String>>,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    linkage_owner_entries(root, preserved, &[], &BTreeMap::new(), &[])
}

/// The replacing package's `NEEDED.ELF.2` lines from its `-MERGING-<pf>` temporary
/// entry, which enumeration skips; empty when there is no replacing package.
// Portage: vartree.py, dblink.treewalk
pub(crate) fn replacement_needed_entries(
    root: &Path,
    category: &str,
    new_pf: &str,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    let Some(text) = portage_vdb::for_root(root)
        .read_pending_file(
            &portage_vdb::EntryKey::new(category, new_pf),
            "NEEDED.ELF.2",
        )
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return Vec::new();
    };
    let entries = crate::needed_elf::NeededEntry::parse_file(&text);
    if entries.is_empty() {
        return Vec::new();
    }
    vec![(format!("{category}/{new_pf}"), entries)]
}

/// Arguments of [`find_preserve_paths_for_merge`].
pub(crate) struct FindPreservePathsForMergeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) new_image_paths: &'a BTreeSet<String>,
}

/// Selects the installed same-slot instance's still-needed libraries before the replace
/// loop; `None` on a first-ever install or when it owns no files. Runs after `merge_tree`
/// and `populate_vdb_tmp`, before `write_vdb_tmp_contents`.
// Portage: vartree.py, dblink.treewalk
pub(crate) fn find_preserve_paths_for_merge(
    args: FindPreservePathsForMergeArgs<'_>,
) -> Option<(BTreeSet<String>, String)> {
    let FindPreservePathsForMergeArgs {
        root,
        category,
        package,
        new_pf,
        main_slot,
        new_image_paths,
    } = args;
    let old_pf = installed_instance_pf(root, category, package, main_slot)?;
    let old_contents_text =
        read_entry_text(root, category, &old_pf, "CONTENTS").unwrap_or_default();
    let old_contents: Vec<String> = old_contents_text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    if old_contents.is_empty() {
        return None;
    }

    let preserved = read_plib_registry(root).preserved_libs();
    let replacement_needed = replacement_needed_entries(root, category, new_pf);
    let owner_entries =
        linkage_owner_entries(root, &preserved, &[], &BTreeMap::new(), &replacement_needed);
    let map = crate::needed_elf::rebuild(root, &owner_entries);
    let defpath =
        crate::needed_elf::getlibpaths(root, std::env::var("LD_LIBRARY_PATH").ok().as_deref());

    let old_owner_is_owner = |p: &str| owns_path_pf(root, category, &old_pf, p);
    let new_owner_is_owner = |p: &str| new_image_paths.contains(p);

    Some((
        crate::needed_elf::find_libs_to_preserve(crate::needed_elf::FindLibsToPreserveArgs {
            root,
            map: &map,
            defpath: &defpath,
            old_contents: &old_contents,
            old_owner_is_owner: &old_owner_is_owner,
            new_owner_is_owner: &new_owner_is_owner,
        }),
        old_contents_text,
    ))
}

/// Copies preserved paths' entries from the replaced instance's `CONTENTS` into the
/// merging package's; a path missing there is dropped with a warning.
// Portage: vartree.py, dblink._add_preserve_libs_to_contents
pub(crate) fn inject_preserved_libs_into_contents(
    old_contents_text: &str,
    preserve_paths: &BTreeSet<String>,
    new_image_paths: &BTreeSet<String>,
) -> (String, BTreeSet<String>) {
    let mut lines = String::new();
    let mut surviving = BTreeSet::new();
    for f in preserve_paths {
        let Some(entry) = old_contents_text.lines().find(|line| {
            let mut parts = line.split_whitespace();
            parts.next();
            parts.next() == Some(f.as_str())
        }) else {
            eprintln!("!!! File '{f}' will not be preserved due to missing contents entry");
            continue;
        };
        surviving.insert(f.clone());
        let obj_type = entry.split_whitespace().next().unwrap_or("obj");
        println!(">>> needed    {obj_type} {f}");
        if !new_image_paths.contains(f) {
            lines.push_str(entry);
            lines.push('\n');
        }
    }
    (lines, surviving)
}

/// Arguments of [`register_merge_preserved_libs`].
pub(crate) struct RegisterMergePreservedLibsArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) preserve_paths: &'a BTreeSet<String>,
}

/// Records the merge-side preserve set under the merging package; a no-op when empty,
/// leaving the replace loop's unregistration as the final state.
// Portage: vartree.py, dblink.treewalk
pub(crate) fn register_merge_preserved_libs(
    args: RegisterMergePreservedLibsArgs<'_>,
) -> Result<(), String> {
    let RegisterMergePreservedLibsArgs {
        root,
        category,
        pn,
        new_pf,
        main_slot,
        preserve_paths,
    } = args;
    if preserve_paths.is_empty() {
        return Ok(());
    }
    let new_counter =
        read_entry_text(root, category, new_pf, "COUNTER").unwrap_or_else(|| "0".to_string());
    let registry = merge_plib_registration(MergePlibRegistrationArgs {
        root,
        category,
        pn,
        new_pf,
        main_slot,
        preserve_paths,
        new_counter: &new_counter,
    });
    write_plib_registry(root, &registry)
}

/// Arguments of [`merge_plib_registration`].
pub(crate) struct MergePlibRegistrationArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) new_pf: &'a str,
    pub(crate) main_slot: &'a str,
    pub(crate) preserve_paths: &'a BTreeSet<String>,
    pub(crate) new_counter: &'a str,
}

/// The registry [`register_merge_preserved_libs`] stores, split out for database backends.
pub(crate) fn merge_plib_registration(args: MergePlibRegistrationArgs<'_>) -> PlibRegistry {
    let MergePlibRegistrationArgs {
        root,
        category,
        pn,
        new_pf,
        main_slot,
        preserve_paths,
        new_counter,
    } = args;
    let new_cpv = format!("{category}/{new_pf}");
    let mut registry = read_plib_registry(root);
    let paths_vec: Vec<String> = preserve_paths.iter().cloned().collect();
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: &new_cpv,
        category,
        pn,
        slot: main_slot,
        counter: new_counter,
        paths: &paths_vec,
    });
    registry
}
