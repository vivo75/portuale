//! The world and world_sets files.

use crate::pretend::*;

/// Reads `<root>/var/lib/portage/world` (real portage's own `WORLD_FILE`
/// -- `lib/portage/const.py`) into a list of atom strings, one per line,
/// with the same comment/blank-line handling every other config file
/// portuale already reads uses. A missing file is not an error -- an
/// empty, or never-yet-created, world is a real, valid state (e.g. a
/// fresh `ROOT`), not a mistake.
///
/// Only plain atom lines are read, via a leading `@` check -- this is
/// real, not a simplification: real `WorldSelectedPackagesSet`'s own
/// `ItemFileLoader` validates each line with a plain `isvalidatom`
/// (`lib/portage/env/validators.py`'s own `ValidAtomValidator`, no `@`
/// bypass), so a `@`-prefixed line in *this* file specifically really
/// would just fail validation and be dropped in real portage too. A
/// nested `@some-set` reference lives in a genuinely separate file --
/// see `read_world_sets`'s own doc comment for the other half of real
/// `@world`'s union (`WorldSelectedSet` in `lib/portage/_sets/files.py`).
/// `@system` (the profile's own `packages` file -- see
/// `portage_profile::Config::system_packages`) is a separate, different
/// mechanism with its own expansion in `run` below, not handled by this
/// function at all. Only the literal token `@world` triggers *this*
/// expansion -- `@some-set`, `@another-random-name`, etc. as a top-level
/// target fall through to the normal atom-parsing path and get a clear
/// "invalid atom" error, not a silent no-op.
///
/// Read through the root's installed-database backend
/// ([`portage_vdb::InstalledDb::world`], feat#157 S1.6; on `files` the
/// same one `read_to_string`).
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

/// Real `@selected` (`WorldSelectedSet` -- `cnf/sets/portage.conf`): the
/// world file's own package atoms unioned with every nested set named in
/// `world_sets` (real `chain(WorldSelectedPackagesSet,
/// WorldSelectedSetsSet)`). Real `@world` is `@profile ∪ @selected ∪
/// @system` (`_sets/__init__.py:97-98`); the callers that expand `@world`
/// add the `@profile` (`Config::profile_packages`) and `@system`
/// (`Config::system_packages`) halves themselves, sorted per real's
/// `_resolve` (`depgraph.py:5500`). Errors carry their own `emerge: `
/// prefix so every caller can just `eprintln!("{e}")`.
pub(crate) fn expand_selected(root: &Path, config_root: &Path) -> Result<Vec<String>, String> {
    let mut atoms = read_world_atoms(root).map_err(|e| format!("emerge: {e}"))?;
    for name in read_world_sets(root).map_err(|e| format!("emerge: {e}"))? {
        let mut seen = HashSet::new();
        atoms.extend(resolve_custom_set(config_root, &name, &mut seen)?);
    }
    Ok(atoms)
}

/// Real `@installed` (`EverythingSet.load`, `_sets/dbapi.py`): a
/// `cat/pkg:slot` atom for every package under `<root>/var/db/pkg` --
/// always slot-qualified, even for a lone installed slot (bug #338959,
/// "avoid the possibility of unwanted upgrades"). Deduplicated + sorted
/// here for deterministic output; real portage's own `_setAtoms` is an
/// unordered set.
pub(crate) fn installed_set_atoms(root: &Path) -> Vec<String> {
    let mut atoms: Vec<String> = portage_repo::all_installed_packages(root)
        .into_iter()
        .map(|p| format!("{}/{}:{}", p.category, p.package, p.slot))
        .collect();
    atoms.sort();
    atoms.dedup();
    atoms
}

/// Real `@preserved-rebuild` (`PreservedLibraryConsumerSet.load`,
/// `lib/portage/_sets/libs.py`): the installed packages that still link
/// against a library only kept alive by `FEATURES=preserve-libs`. For
/// every path in the `preserved_libs_registry`,
/// `consumers |= linkmap.findConsumers(path, greedy=False)`; then
/// `consumers -= <every preserved path>` ("Don't rebuild packages just
/// because they contain preserved libs that happen to be consumers of
/// other preserved libs"); then each surviving consumer path maps to
/// `{cp}:{slot}` of its owning installed package (real
/// `mapPathsToAtoms` via `linkmap.getOwners` -- here the linkmap object's
/// own recorded `owner`, since every consumer is itself an indexed
/// `NEEDED.ELF.2` object). A consumer path with no indexed owner is
/// skipped (real: a preserved lib of an already-uninstalled package).
/// Slot-qualified like `@installed`, sorted + deduplicated for
/// deterministic output. Empty registry -> empty list.
///
/// A preserved library whose owning package has already left the vdb is
/// no longer an indexed object, so `find_consumers` can't look it up --
/// the same `basename == soname` fallback `ebuild_merge::find_unused_
/// preserved_libs` uses (`needed_elf::soname_consumers`) stands in for
/// real `LinkageMapELF.rebuild()`'s `scanelf`-of-the-registry branch.
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
        let consumers = match needed_elf::find_consumers(root, &map, &defpath, lib, None, false) {
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

/// Backlog #232: the pure "what would be added" half of
/// `update_world_file` (real `Scheduler._world_atom` +
/// `depgraph.saveNomergeFavorites` -- see the writer's own doc comment
/// for the full grounding), split out so the
/// selective-without-`--oneshot` deferral arm
/// (`_emerge/actions.py:514-516` -- the prompt moves into
/// `depgraph.saveNomergeFavorites`) can show real's
/// `Would you like to add these packages to your world favorites?`
/// prompt (`depgraph.py:11376-11386`) *before* anything is recorded.
/// Returns the new world atoms in argument order (a duplicate argument
/// repeats); both the writer and the `--ask` display sort their own
/// print order like real (`depgraph.py:11373-11374`).
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
            // A `@set` target belongs in `world_sets`, not `world` --
            // a v1 cut (see `read_world_sets`).
            continue;
        }
        let Some(atom) = parse_atom(raw) else {
            continue;
        };
        // The resolved entry for this cp (a top-level `NoVisibleCandidate`
        // aborts the whole resolve before we get here, so it's present).
        let Some(entry) = entries
            .iter()
            .find(|e| e.category == atom.category && e.package == atom.package)
        else {
            continue;
        };
        // Real "unslotted system packages will not be stored in world".
        let cpv_any = format!("{}/{}-0", atom.category, atom.package);
        let in_system = system_atoms
            .iter()
            .any(|a| match_from_list(a, &[cpv_any.as_str()]).is_some_and(|m| !m.is_empty()));
        let unslotted = entry.slot.as_deref().unwrap_or("0") == "0";
        if in_system && unslotted && atom.repo.is_none() {
            continue;
        }

        // Real `create_world_atom`: keep the arg's `:slot` iff (a) the
        // arg actually carried one and it matches the resolved slot, and
        // (b) `cat/pkg` is "slotted" -- >1 SLOT in the repo, or a lone
        // SLOT that isn't "0". `atom.slot` is the main slot only (a
        // `:2/9` arg still records `cat/pkg:2`, matching `pkg.slot_atom`).
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

/// Real `Scheduler._world_atom` + `depgraph.saveNomergeFavorites`: after
/// a successful non-`--pretend` `emerge <atom>`, each directly-requested
/// **plain** target atom (not a dependency, not a `@set`) is recorded in
/// `<root>/var/lib/portage/world` -- whether it merged or was already
/// installed. `--oneshot`/`--onlydeps` suppress this entirely (real
/// `_world_atom`'s own early-return set). The recorded atom is the
/// argument's own `cat/pkg` (plus `::repo` when the arg carried one),
/// EXCEPT that a `cat/pkg:slot` argument is recorded slot-qualified when
/// `cat/pkg` is genuinely slotted -- real `create_world_atom`: "If the
/// argument atom is precise enough to identify a specific slot then a
/// slot atom will be returned." "Slotted" here is real's own test: more
/// than one `SLOT` available in the repo, or a single `SLOT` that isn't
/// `"0"`. v1 cuts still: a *version*-pinned arg that identifies one slot
/// (real records the slot atom there too); the vdb-only multislot
/// fallback; system-virtual exclusion; and the `@set` target -> `world_sets`
/// half (needs `emerge @set` build support, not implemented yet).
/// An unslotted `@system` member is not recorded (real "unslotted system
/// packages will not be stored in world"). Already-present atoms are
/// left alone; when anything is added the file is rewritten sorted +
/// deduplicated (real `WorldSelectedPackagesSet.write`). Prints the real
/// `>>> Recording <atom> in "world" favorites file...` line per addition.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn update_world_file(
    root: &Path,
    target_atoms: &[&str],
    entries: &[GraphEntry],
    system_atoms: &[String],
    repos: &[portage_repo::RepoConfig],
    oneshot: bool,
    onlydeps: bool,
) -> Result<(), String> {
    if oneshot || onlydeps {
        return Ok(());
    }
    let mut current = read_world_atoms(root)?;
    let mut added = world_file_additions(&current, target_atoms, entries, system_atoms, repos);
    // Real sorts the combined additions before both the `--ask` prompt
    // and the `>>> Recording` loop (`depgraph.py:11373-11374`); each
    // file half sorts its own print order here (the file itself was
    // always rewritten sorted; only multi-addition stdout order
    // changes, which no pin records -- the cross-file interleave of a
    // mixed world + world_sets run stays file-grouped, see the arm).
    added.sort();
    for world_atom in &added {
        println!(">>> Recording {world_atom} in \"world\" favorites file...");
        current.push(world_atom.clone());
    }

    if !added.is_empty() {
        current.sort();
        current.dedup();
        let db = portage_vdb::for_root(root);
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.set_world(&portage_vdb::World { atoms: current })
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Backlog #232: the pure "what would be added" half of
/// `update_world_sets_file` (see its doc comment for the grounding),
/// split out for the same pre-record `--ask` prompt as
/// `world_file_additions`. Takes the `@`-prefixed current lines and
/// returns the new ones, in argument order; the writer sorts its own
/// print order like real (`depgraph.py:11373-11374`).
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

/// Backlog #232 review (Important-2): real's `[usersets]`
/// `world-candidate` default -- every file set under
/// `<config_root>/etc/portage/sets` is a world candidate
/// (`lib/portage/_sets/__init__.py:133`, shipped as
/// `cnf/sets/portage.conf`) unless the user overrides it in
/// `sets.conf` (`man portage.5`: settings there override; same-named
/// sections merge across files with later wins, via
/// `util/configparser.py:read_configs`). Real applies the merged
/// `[usersets]` flag to every set its multiset builds
/// (`_sets/__init__.py:258-262`); the deferral gate
/// (`actions.py:500-512`) and the record (`depgraph.py:11366`) then
/// skip the non-candidates. Returns false only on an explicit
/// `world-candidate = false` (real `ConfigParser.getboolean`
/// spelling) in the `[usersets]` section of
/// `<config_root>/etc/portage/sets.conf`, or of a file under
/// `<config_root>/etc/portage/sets.conf/` (sorted walk, later wins --
/// real's own `_getfiles` yields the directory form or the file form,
/// never both). Section names are case-sensitive, option names are
/// not (real `ConfigParser` semantics). Anything missing, unreadable,
/// or unparsable stays a candidate -- portuale never suppresses a
/// record it cannot prove real suppresses. That includes a
/// `world-candidate` value `getboolean` rejects, where real's
/// `_parse` (`_sets/__init__.py:259-262`) raises and aborts: a
/// deliberate leniency, not fidelity. Cuts: per-set single
/// sections (real routes those through `singleBuilder`, which needs a
/// `filename`, and a same-named file set then takes the "Redefinition
/// of set" error path), repo `sets.conf` files (portuale models no
/// per-repo set config) and the host-global `sets.conf` real also
/// merges (`_sets/__init__.py:352-359`) are not consulted, and the
/// directory form is read one level deep with no skips, where real
/// walks it recursively and skips dot, tilde and VCS entries
/// (`:365-375`).
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
    // Real `getboolean` vocabulary; anything else (or a missing `=`)
    // leaves the running value alone (see the doc comment).
    fn as_bool(value: &str) -> Option<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "yes" | "true" | "on" => Some(true),
            "0" | "no" | "false" | "off" => Some(false),
            _ => None,
        }
    }
    let mut candidate = true;
    // Section state carries across files. This is a leniency, not real's
    // behaviour: real `read_configs` (`util/configparser.py:29-58`) calls
    // `read_file` once per file, which resets the section, so bare keys at
    // the top of a later fragment raise `MissingSectionHeaderError` and
    // abort. Malformed input only.
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

/// Real `depgraph.saveNomergeFavorites`'s own `@set` half: after a
/// successful non-`--pretend` `emerge @name`, each directly-given
/// user-defined set arg is recorded as `@name` in
/// `<root>/var/lib/portage/world_sets` (real `WorldSelectedSetsSet`).
/// `@world`/`@system` are never routed here (they resolve without a set
/// file, so they never land in `set_names`). Same suppression set as the
/// world file itself (`--oneshot`/`--onlydeps`; `--buildpkgonly` is
/// gated at the call site). Already-present names are left alone; on any
/// add the file is rewritten sorted + deduplicated (comment / non-`@`
/// lines dropped, exactly like `WorldSelectedSetsSet.write`). Prints the
/// real `>>> Recording @name in "world_sets" favorites file...` line per
/// addition. v1 cut: the real `world_candidate` gate for built-in sets
/// like `@security` -- portuale only knows `@world`/`@system` as
/// built-ins, and everything reaching here already resolved via a real
/// set file, so it's a user set by construction. The user-level
/// `[usersets] world-candidate = false` override is filtered at the
/// call sites instead (see `usersets_world_candidate`), which pass an
/// already-filtered `set_names`.
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
    // Sorted print order like real (`depgraph.py:11373-11374`) -- see the
    // same comment in `update_world_file`.
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

/// Reads `<root>/var/lib/portage/world_sets` (real portage's own
/// `WORLD_SETS_FILE` -- `lib/portage/const.py`), a file genuinely
/// SEPARATE from the world file above, listing every `@name` set
/// reference the user has directly selected (e.g. via a prior `emerge
/// SEPARATE from the world file above, listing every `@name` set
/// reference the user has directly selected (e.g. via a prior `emerge
/// --noreplace @some-set`) -- real `WorldSelectedSetsSet`, whose own
/// validator (`lib/portage/_sets/files.py`) just checks each line
/// starts with `@`. Real `@world` is the union of `WorldSelectedSetsSet`
/// (this) with `WorldSelectedPackagesSet` (`read_world_atoms` above) --
/// see `WorldSelectedSet.load`'s own `chain(self._pkgset, self._setset)`.
/// A missing file is not an error, same "absence is a real, valid
/// state" precedent the world file itself already established. Returns
/// each name with its own leading `@` stripped, ready for
/// `resolve_custom_set`.
///
/// Read through [`portage_vdb::InstalledDb::world_sets`] (S1.6).
pub(crate) fn read_world_sets(root: &Path) -> Result<Vec<String>, String> {
    portage_vdb::for_root(root)
        .world_sets()
        .map(|w| w.sets)
        .map_err(world_read_error)
}

/// Resolves one custom, file-based package set by `name` (no leading
/// `@`), real portage's own default `usersets` source
/// (`lib/portage/_sets/__init__.py`'s own `_create_default_config`:
/// `class = StaticFileSet`, `directory =
/// <config_root>/etc/portage/sets`, one file per set, the file's own
/// path relative to that directory becoming the set's name) -- reads
/// `<config_root>/etc/portage/sets/<name>`, same line format as the
/// world file itself (one atom per line, `#`-comment/blank-line
/// handling identical), *except* a line starting with `@` is itself
/// another nested set reference here, resolved recursively -- real
/// `StaticFileSet`'s own validator (unlike `WorldSelectedPackagesSet`'s
/// stricter one) explicitly accepts a `@`-prefixed line too, and real
/// `SetConfig.getSetAtoms` walks every such non-atom entry, recursing
/// into any that start with `@` (`lib/portage/_sets/__init__.py`).
/// `seen` is that same recursion's own `ignorelist` -- a name already
/// being expanded on the current path contributes nothing further
/// (silently, not an error) rather than looping forever; a *fresh*
/// `seen` set is used for each top-level name in `read_world_sets`'s
/// own list, matching real `getSetAtoms(setname, ignorelist=None)`'s
/// own per-top-level-call default.
///
/// A `name` with no matching file is a real, immediate error (real
/// `PackageSetNotFound`, eventually surfaced and fatal at every real
/// call site in `lib/_emerge/actions.py`/`depgraph.py`) -- deliberately
/// NOT the same "absence is valid" tolerance `read_world_atoms`/
/// `read_world_sets` give their own *files*: those are optional,
/// implicitly-checked-for state (a fresh `ROOT` may simply never have
/// either), but a name explicitly listed in `world_sets` (or referenced
/// by another set) pointing at nothing is a real configuration error,
/// not an absence to tolerate.
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

/// Real `_unmerge_display`'s own `installed_sets` -- every custom set
/// directly or indirectly selected via `world_sets` (real
/// `WorldSelectedSetsSet`), paired with its *direct* atoms only (not
/// recursively flattened, unlike `resolve_custom_set` -- the "still
/// listed in the following package sets" warning names the set that
/// *directly* contains the package). BFS over the `@`-references, cycle-
/// guarded. A referenced-but-missing set is dropped silently here (real
/// portage `eerror`s "Unknown set" and moves on) -- a documented
/// narrowing.
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
