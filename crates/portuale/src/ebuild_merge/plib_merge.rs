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
    let mut scan_input = preserved.clone();
    for (owner, paths) in replacement_preserved {
        scan_input
            .entry(owner.clone())
            .or_default()
            .extend(paths.iter().cloned());
    }
    for excluded in exclude_cpvs {
        scan_input.remove(excluded);
    }
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
