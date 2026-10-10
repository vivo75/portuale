//! Preserved libraries during a merge.

use crate::ebuild_merge::*;

/// Real `dblink.merge()`'s own post-copy step (`lib/portage/dbapi/
/// vartree.py:5095-5159`): any path this merge's own `find_collisions`
/// matched against a currently-registered preserved lib is now
/// legitimately owned by the just-merged package instead of its
/// previous, registered owner. Drops the taken-over paths from the
/// registry (removing the owning `cp:slot` entry entirely once its own
/// path list empties) and from the previous owner's own real vdb
/// `CONTENTS` (real `removeFromContents`) -- skipped when the previous
/// owner *is* the package that was just merged (real `if cpv !=
/// self.mycpv`: re-merging the exact same cpv already replaces its own
/// vdb entry wholesale, so there's nothing stale left to strip).
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

/// Real `vardbapi.removeFromContents` (`vartree.py:1244-1310`). A
/// missing vdb entry for `cpv` (already unmerged some other way) is
/// silently a no-op, matching real `removeFromContents`'s own tolerance
/// of a stale registry entry.
///
/// Real `NEEDED`-line stripping ("Also remove corresponding NEEDED
/// lines, so that they do no corrupt LinkageMap data for preserve-libs",
/// `vartree.py:1279-1310`) is real now too, closing a gap this module's
/// own doc comment used to document as moot ("without the registration
/// side above ever writing NEEDED data in the first place") -- moot no
/// longer, since real `NEEDED.ELF.2` generation and the full real
/// `LinkageMap`/preserve-libs computation are both real now (see
/// `needed_elf.rs`). Real `removed` (whether any `CONTENTS` line was
/// actually dropped) gates the whole thing, matching real `if removed:`
/// exactly -- when this package's own `NEEDED.ELF.2` doesn't exist at
/// all (real `except OSError: ... new_needed` stays `None`), nothing is
/// written, matching real `if new_needed is not None:` in
/// `writeContentsToContentsFile`. When it does exist, every entry whose
/// own `filename` (already `ROOT`-relative, portuale's own `CONTENTS`/
/// `NEEDED.ELF.2` convention -- no `os.path.join(root, ...)` needed the
/// way real Python does, since both sides already agree on the same
/// convention here) still appears among the *surviving* `CONTENTS`
/// paths is kept; every other entry (now pointing at a file this package
/// no longer owns) is dropped -- stale linkage data that would otherwise
/// corrupt a *later* `LinkageMap.rebuild()`'s own preserve-libs decision
/// for some *other* package's own future unmerge.
pub(crate) fn remove_from_contents(
    root: &Path,
    cpv: &str,
    paths: &BTreeSet<String>,
) -> Result<(), String> {
    remove_from_contents_into(root, cpv, paths, None)
}

/// [`remove_from_contents`], optionally collecting the rewrites into
/// `retire` (feat#157 S4.2) instead of committing each at once.
pub(crate) fn remove_from_contents_into(
    root: &Path,
    cpv: &str,
    paths: &BTreeSet<String>,
    mut retire: Option<&mut RetireWrites>,
) -> Result<(), String> {
    let Some((category, pf)) = cpv.split_once('/') else {
        return Ok(());
    };
    // W4 through the backend: `read_entry_text` is today's `read_to_string(..)
    // else return Ok(())` (missing, unreadable or non-UTF-8 all stop here),
    // `replace_file` its two in-place `std::fs::write`s.
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

/// Real `PreservedLibsRegistry.register`/`.unregister`
/// (`lib/portage/util/_dyn_libs/PreservedLibsRegistry.py:142-176`): real
/// `unregister(cpv, slot, counter) = register(cpv, slot, counter, [])`,
/// so this one function covers both real calls, matching real `register`
/// exactly. Real `cps = cpv_getkey(cpv) + ":" + slot` (the real registry
/// key: `category/package:slot`, no version) -- `category`/`pn` are
/// passed in already split, rather than re-deriving them from a version
/// string the way real `cpv_getkey` does, since every real caller here
/// already has them split (`ebuild_phases::Environment::split`).
///
/// Empty `paths` (real `unregister`): removes the `cps` entry, but only
/// if it currently records the *same* `cpv` and `counter` -- never
/// blindly erasing a different package's own entry that happens to
/// share this exact slot's own key. Non-empty `paths`: unconditionally
/// overwrites the `cps` entry (real `_normalize_counter` is just a
/// whitespace-trim, not integer parsing, so a plain trimmed-string
/// comparison already matches real behavior exactly).
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

/// `read_all_needed_entries` plus real `LinkageMap.rebuild()`'s own
/// preserved-libs branch (`needed_elf::scan_preserved_lib_entries`):
/// the shared linkage-map input of the preserve-libs computations
/// (`find_preserve_paths_for_merge`, `preserve_libs_on_unmerge`, and
/// the post-unmerge prune `find_unused_preserved_libs` below).
/// Scanned entries join their owner's already-present group when one
/// exists (real groups everything by owner for the bundled-library
/// runpath inference), else form a new one; vdb entries come first, so
/// on a same-inode conflict the vdb data wins (real indexes the
/// `scanelf` line first instead -- the two describe the same live file,
/// so they agree in practice). Skipped entirely when there is nothing
/// to scan (backlog #178's second bump is the first merge that ever
/// has anything to scan).
///
/// `exclude_cpvs` is real `LinkageMap.rebuild()`'s own `exclude_pkgs` (a
/// package being unmerged contributes neither `NEEDED.ELF.2` lines nor
/// registry orphans -- its data "would only serve to corrupt the
/// `LinkageMap`"): the unmerged instance in replacement mode (plus, on a
/// database backend, the earlier-unmerged same-slot instances, #310),
/// empty everywhere else. `replacement_preserved` is real `preserve_paths`
/// (the merge-side just-preserved set, owner -> paths, not yet
/// registered at prune time, so it must be passed explicitly): scanned
/// under the replacing cpv -- real indexes it with owner `None` (and
/// skips the bundled-library runpath inference for `None`); grouping
/// it under the replacing cpv is the closest this `String`-keyed input
/// gets, and the owner only feeds that inference, never the keep/drop
/// verdict.
///
/// `replacement_needed` is real `LinkageMap.rebuild()`'s own
/// `include_file` (backlog #224): the replacing package's
/// `NEEDED.ELF.2` lines, fed explicitly because its vdb entry still
/// sits in its `-MERGING-<pf>` temporary at prune time and enumeration
/// (`read_all_needed_entries`, real `cpv_all()`) skips it -- without
/// the feed a preserved library whose only remaining consumer is the
/// replacing package looks orphaned and is pruned. Real indexes those
/// lines with owner `None` (`LinkageMapELF.rebuild` processes the
/// include file first); grouped here under the replacing cpv for the
/// same `String`-keyed reason as `replacement_preserved` above, with
/// the same verdict-neutrality (the owner only feeds the same-owner
/// bundled-library runpath inference, never the keep/drop verdict --
/// and these lines genuinely share one owner, so the inference is, if
/// anything, more faithful). Empty on every path but the replace-loop
/// prune and the merge-side preserve computation
/// (`find_preserve_paths_for_merge`, backlog #229): standalone
/// `emerge -C` has no replacing package, and the
/// merge-end prune runs after the rename into place, when the new
/// entry's own lines are already enumerated.
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

/// Real `dblink.treewalk()`'s own `needed = os.path.join(inforoot,
/// LinkageMap._needed_aux_key)` (backlogs #224/#229): the replacing
/// package's `NEEDED.ELF.2` lines, read from its `-MERGING-<new_pf>`
/// temporary vdb entry, which holds the build-info copy from
/// `populate_vdb_tmp` (every build-info file is copied there, the
/// `scanelf` QA step's `NEEDED.ELF.2` included). Real's replace loop
/// passes that path as `dblink.unmerge(needed=...)` into
/// `_prune_plib_registry`, whose `_linkmap_rebuild(include_file=needed,
/// ...)` indexes the lines explicitly -- because the entry is still
/// `-MERGING-`, `cpv_all()` (portuale's own `read_all_needed_entries`,
/// which skips `-MERGING-` names) never yields them. Grouped under the
/// replacing cpv (see `linkage_owner_entries` for why that owner
/// choice is verdict-neutral). Empty when the temporary entry carries
/// no `NEEDED.ELF.2` at all (a package with no ELF content -- real
/// `grabfile` on a missing file degrades the same way), so callers on
/// paths with no replacing package simply never call this.
pub(crate) fn replacement_needed_entries(
    root: &Path,
    category: &str,
    new_pf: &str,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    // The pending entry's file (`files`: `-MERGING-<new_pf>/NEEDED.ELF.2`,
    // one `open`); unreadable or non-UTF-8 is "no lines", as before.
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

/// Real `dblink.treewalk`'s own pre-replace-loop preserve-libs
/// computation (the `_linkmap_rebuild(include_file=needed)` +
/// `_find_libs_to_preserve()` block): rebuild the system-wide
/// `LinkageMap` and select the installed same-slot instance's libraries
/// that are still needed (`_find_libs_to_preserve()`, `unmerge=False`).
/// `new_image_paths` is the just-merged image's own path set (parsed
/// from `merge_tree`'s `CONTENTS` text) -- real `self.isowner(f)` on the
/// merging instance, whose `CONTENTS` is already written at this point,
/// so a library the new version itself ships is never preserved.
/// Returns `None` when no same-slot instance is installed (a first-ever
/// install preserves nothing) or it owns no files (real
/// `installed_instance.getcontents()` falsy skips the rebuild); otherwise
/// the preserve set plus the old instance's own raw `CONTENTS` text (for
/// entry injection below).
///
/// Must run after `merge_tree` (the files are on disk for `lstat`) and
/// after `populate_vdb_tmp` (backlog #183: the new vdb entry *is*
/// written by then, into its `-MERGING-<pf>` temporary -- the doc used
/// to say it was not). Enumeration (`read_all_needed_entries`, real
/// `cpv_all()`) still skips `-MERGING-` names, so the new package
/// reaches the linkage map only through the explicit include feed below
/// (real `include_file=needed`, backlog #229): without it a library
/// owned by the replaced instance whose only consumer is the replacing
/// package would not be preserved. Runs before `write_vdb_tmp_contents`.
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

    // Real `LinkageMap.rebuild()` sees the registry's own preserved
    // libs too (`_linkmap_rebuild` loads them via the orphan `scanelf`
    // branch): without them a previously-preserved library the replaced
    // instance no longer ships in its own `NEEDED.ELF.2` is invisible
    // here, and a second consecutive soname bump would preserve nothing
    // (backlog #178).
    let preserved = read_plib_registry(root).preserved_libs();
    // Real `LinkageMap.rebuild(include_file=needed)` (backlog #229):
    // the replacing package's `-MERGING-<new_pf>/NEEDED.ELF.2` lines
    // (live since `populate_vdb_tmp`, skipped by enumeration) join the
    // linkage input first, owner verdict-neutral -- see
    // `linkage_owner_entries`. A library owned by the replaced
    // instance whose only consumer is the replacing package is
    // preserved only because of this feed.
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

/// Real `dblink._add_preserve_libs_to_contents` (`vartree.py:3775-3826`):
/// copy the preserved paths' own entries from the replaced instance's
/// `CONTENTS` into the merging package's (digest/mtime carried over
/// verbatim -- the files are *not* reinstalled), printing real
/// `>>> needed    {obj|sym} <path>` per path in `sorted()` order.
/// `new_image_paths` is the just-merged image's own path set: a
/// preserved path already there keeps its own fresh entry (real assigns
/// into a dict, so re-adding is a no-op, never a duplicate line).
/// Returns the `CONTENTS` lines to append plus the surviving preserve set:
/// a path with no entry in the old `CONTENTS` cannot be preserved (real
/// `!!! File ... will not be preserved due to missing contents entry`)
/// and is dropped, exactly like real dropping it from `preserve_paths`.
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

/// Real `dblink.treewalk()`'s own post-replace-loop registration
/// (`vartree.py:5266-5272`): `register(self.mycpv, slot, counter,
/// sorted(preserve_paths))` -- the one `cp:slot` record is **replaced**
/// by the merging package's `(cpv, counter, paths)`, never merged with
/// or re-attributed from the old entry (real `register()`'s own
/// unconditional overwrite, `PreservedLibsRegistry.py:165-169`). Reads
/// the counter back from the just-written new vdb entry (real
/// `counter_tick()`'s own value, the same one `COUNTER` carries).
/// No-op when nothing was preserved (real `if preserve_paths:`), leaving
/// the replace loop's unregistration of the old record as the final
/// state. Keep #167's serialisation and write-only-on-change rules
/// intact: `register_preserved_libs` + `write_plib_registry` do that.
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

/// The registry [`register_merge_preserved_libs`] stores, with the
/// counter given: the freshly loaded (and pruned) registry plus the
/// merging package's `register(...)`. Split out so a database backend can
/// store it in the publishing transaction (feat#157 S4.1), reading the
/// counter from the still-pending entry.
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
