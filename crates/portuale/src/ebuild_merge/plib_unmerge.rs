//! Preserved libraries during unmerge and prune.

use crate::ebuild_merge::*;

/// Real `dblink._prune_plib_registry`, called from real `unmerge()`
/// with `unmerge=True` right before real `_unmerge_pkgfiles()` runs.
/// Real `preserve_paths` (a `_prune_plib_registry` parameter, not to be
/// confused with this function's own *return* value) is only ever
/// non-`None` when a real depgraph-driven upgrade transaction already
/// computed it via a companion `merge()` call in the *same* transaction:
/// `None` on the standalone path (portuale's own `merge`/`unmerge` are
/// always separate, independent CLI invocations), the merge-side set on
/// the replace loop. An instance that owns no files still unregisters
/// (real `instance_owns_files` gates only the rebuild and the scans).
///
/// Real order: rebuild the system-wide `LinkageMap` from every real
/// installed package's own vdb-stored `NEEDED.ELF.2`
/// (`linkage_owner_entries` + `rebuild` -- real `exclude_pkgs=None` on
/// the standalone path, since the package being unmerged hasn't left
/// the vdb yet, so its own data is still really part of the map,
/// matching real behavior exactly). Compute `needed_elf::find_
/// libs_to_preserve` with `new_owner_is_owner` always `false` (matching
/// what real `not unmerge and self.isowner(f)` collapses to when
/// `unmerge` is `true`) and `old_owner_is_owner` real `self.isowner`
/// (`owns_path_pf`, this exact package's own real `CONTENTS`).
/// Unconditionally unregister this package's own prior registry entry
/// first (real `plib_registry.unregister`); if anything is actually
/// preserved, register this package -- the one being removed -- as the
/// new keeper of those paths (real `plib_registry.register`).
///
/// `is_replacement` is real `unmerge_with_replacement`
/// (`preserve_paths is not None`, set exactly when `treewalk()`'s own
/// replace loop drives this unmerge): real then skips the
/// `_find_libs_to_preserve(unmerge=True)` re-registration entirely and
/// only unregisters the old entry -- the preserved set was already
/// computed merge-side (`find_preserve_paths_for_merge`) and injected
/// into the replacing package's own `CONTENTS`, so the files survive
/// removal through same-slot ownership (`remove_contents`' own
/// `is_owned` skip) and the record lands under the merging package via
/// `register_merge_preserved_libs`. Returns an empty set in that mode;
/// the standalone (`emerge -C`) mode returns the preserved paths for
/// the caller to exclude from its own real file-removal loop.
///
/// Returns the set of preserved paths (already `ROOT`-relative absolute
/// paths, portuale's own `CONTENTS` convention) -- the caller is
/// responsible for excluding them from its own real file-removal loop
/// (real "remove the preserved files from our contents so that they
/// won't be unmerged"; portuale's own vdb entry directory gets deleted
/// wholesale moments later regardless, so there's no separate real
/// `CONTENTS`-file rewrite to also perform here).
#[cfg(test)]
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn preserve_libs_on_unmerge(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    slot: &str,
    contents_text: &str,
    is_replacement: bool,
) -> Result<BTreeSet<String>, String> {
    preserve_libs_on_unmerge_into(
        root,
        category,
        pn,
        pf,
        slot,
        contents_text,
        is_replacement,
        None,
    )
}

/// [`preserve_libs_on_unmerge`], optionally keeping the registry in
/// `retire` instead of writing it (feat#157 S4.2).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn preserve_libs_on_unmerge_into(
    root: &Path,
    category: &str,
    pn: &str,
    pf: &str,
    slot: &str,
    contents_text: &str,
    is_replacement: bool,
    retire: Option<&mut RetireWrites>,
) -> Result<BTreeSet<String>, String> {
    // Real `_prune_plib_registry` still runs `unregister()` for an
    // instance that owns no files (`instance_owns_files` gates only the
    // linkmap rebuild and the preserve/prune scans below); only the
    // preserve-set computation is skipped.
    let instance_owns_files = !contents_text.trim().is_empty();

    let counter = read_entry_text(root, category, pf, "COUNTER").unwrap_or_else(|| "0".to_string());
    let cpv = format!("{category}/{pf}");

    let mut registry = read_plib_registry(root);
    // The orphan scan below must see the registry *before* this
    // package's own entry is unregistered: real `_prune_plib_registry`
    // rebuilds the `LinkageMap` (whose orphan `scanelf` branch reads the
    // registry) before `unregister()` runs, so a previously-preserved
    // library this instance no longer ships in its own `NEEDED.ELF.2`
    // is still indexed for the computation.
    let preserved = if is_replacement || !instance_owns_files {
        BTreeSet::new()
    } else {
        let old_contents: Vec<String> = contents_text
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
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

/// Every currently-registered preserved-library path, keyed by the cpv
/// that owns it (real `PreservedLibsRegistry.getPreservedLibs()`). A thin
/// `pub(crate)` reader for the resolver-side `@preserved-rebuild` set and
/// the `display_preserved_libs` advisory, both of which live outside this
/// module.
pub(crate) fn preserved_lib_paths(root: &Path) -> BTreeMap<String, Vec<String>> {
    read_plib_registry(root).preserved_libs()
}

/// Real `dblink._find_unused_preserved_libs`: the registered
/// preserved libraries that no installed package links against any
/// more, keyed by the cpv that owns them (so the caller can prune each
/// owner's `CONTENTS`). Rebuilds the system-wide `LinkageMap` from
/// every installed `NEEDED.ELF.2` **plus** the registry orphans no
/// `NEEDED.ELF.2` indexes (`linkage_owner_entries`, real
/// `LinkageMap.rebuild`'s own preserved-libs branch) -- without the
/// orphans a just-preserved library is invisible here -- and, for each
/// registered path that still exists on disk, collects its consumers
/// -- from the linkage map when the path is still an indexed object,
/// otherwise via a basename==soname reverse lookup
/// (`needed_elf::soname_consumers`) for a library whose owning package
/// has already left the vdb. The consumer/preserved graph is then
/// reduced by `needed_elf::find_unneeded_preserved` (real
/// `_find_unneeded_preserved_nodes`, cycle-aware).
///
/// `unmerge_no_replacement` + `being_unmerged`: real "also eliminate
/// consumers that are going to be unmerged if unmerge_no_replacement is
/// True" (real `unmerge_no_replacement = unmerge and not
/// unmerge_with_replacement`): on a plain unmerge with no replacement,
/// a consumer that is itself entirely owned by the package now being
/// removed does not keep a library alive; in replacement mode those
/// consumers survive through the replacing package, so they still
/// count. `being_unmerged` returns true for such a path.
///
/// `exclude_cpvs` + `replacement_preserved`: real `exclude_pkgs` +
/// `preserve_paths` (see `linkage_owner_entries`): in replacement mode
/// the unmerged instance's own linkmap data is excluded while the
/// merge-side just-preserved set is scanned in, so a preserved library
/// whose only linkmap consumer is a just-preserved file is kept. Both
/// are empty/`None` on every other path.
///
/// `replacement_needed`: real `include_file` (see
/// `linkage_owner_entries`): the replacing package's own
/// `NEEDED.ELF.2` lines, so a preserved library whose only remaining
/// consumer is the replacing package itself survives the
/// replace-loop prune. Empty on every other path (standalone
/// `emerge -C` has no replacing package; the merge-end prune runs
/// after the rename into place, when the new entry is enumerated).
///
/// Deliberate narrowing vs. real: the per-consumer "an alternative,
/// non-preserved provider of the same soname is installed" edge removal
/// (real "erroneously preserved due to a move from one directory to
/// another") is not reproduced -- portuale has never had a preserved-lib
/// directory-move path, and the collision-protect takeover
/// (`unregister_preserved_libs`) already covers the same-path case.
#[cfg(test)]
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn find_unused_preserved_libs(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> BTreeMap<String, BTreeSet<String>> {
    find_unused_preserved_libs_in(
        root,
        read_plib_registry(root),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    )
}

/// [`find_unused_preserved_libs`] over an explicit registry (the one an
/// unmerge in progress holds in memory, feat#157 S4.2).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn find_unused_preserved_libs_in(
    root: &Path,
    registry: PlibRegistry,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> BTreeMap<String, BTreeSet<String>> {
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
            // Not an indexed object (owning package gone) -- fall back to
            // "does any installed object still need this soname".
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

/// Real `dblink._prune_plib_registry`'s own tail
/// (`_remove_preserved_libs` + the `removeFromContents` loop +
/// `pruneNonExisting`), which real portage runs at the end of **both** a
/// merge (`treewalk` -- "For gcc upgrades, preserved libs have to be
/// removed after the library path has been updated") and an unmerge.
///
/// Deletes every preserved-library file `find_unused_preserved_libs`
/// reports as unneeded (tolerating an already-gone file), removes empty
/// parent directories, strips the paths from each still-installed
/// owner's `CONTENTS`/`NEEDED.ELF.2` (`remove_from_contents`), rewrites
/// the registry with those paths gone (and drops any entry whose paths
/// no longer exist on disk -- real `pruneNonExisting`). Prints real
/// `<<< !needed  {obj|sym} <path>` per removed file. Returns the removed
/// `ROOT`-relative paths.
///
/// `unmerge_no_replacement` / `being_unmerged` / `exclude_cpvs` /
/// `replacement_preserved` / `replacement_needed` are passed straight
/// through to `find_unused_preserved_libs` (see its own doc comment
/// for the real grounding); every caller except the replace-loop
/// unmerge passes `exclude_cpvs=&[]` and empty feeds.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn prune_unused_preserved_libs(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
) -> Result<Vec<String>, String> {
    prune_unused_preserved_libs_into(
        root,
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
        None,
    )
}

/// [`prune_unused_preserved_libs`], optionally reading the registry from
/// `retire` and leaving its writes (the registry, the W4 rewrites) there
/// instead of committing them (feat#157 S4.2). The files it removes are
/// removed at once either way.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn prune_unused_preserved_libs_into(
    root: &Path,
    unmerge_no_replacement: bool,
    being_unmerged: &dyn Fn(&str) -> bool,
    exclude_cpvs: &[String],
    replacement_preserved: &BTreeMap<String, Vec<String>>,
    replacement_needed: &[(String, Vec<crate::needed_elf::NeededEntry>)],
    mut retire: Option<&mut RetireWrites>,
) -> Result<Vec<String>, String> {
    let cpv_lib_map = find_unused_preserved_libs_in(
        root,
        plib_registry_for(root, retire.as_deref()),
        unmerge_no_replacement,
        being_unmerged,
        exclude_cpvs,
        replacement_preserved,
        replacement_needed,
    );

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

    // Real "Remove empty parent directories if possible" -- walk upward
    // from each, stopping at the first non-empty one.
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

    // Strip the removed paths from every still-installed owner's vdb
    // CONTENTS/NEEDED.ELF.2, then rewrite the registry.
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

    // Real `_remove_preserved_libs`'s own tail (`vartree.py:3995`):
    // `self.vartree.dbapi._plib_registry.pruneNonExisting()` -- drop a
    // registry entry once none of its recorded paths exist on disk any
    // more (rebuilding survivors file-then-symlink, not just retaining).
    prune_non_existing(root, &mut registry);
    match retire {
        Some(retire) => retire.registry = Some(registry),
        None => write_plib_registry(root, &registry)?,
    }

    Ok(removed)
}
