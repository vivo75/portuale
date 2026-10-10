//! INSTALL_MASK and what is already installed.

use crate::ebuild_merge::*;

/// Real `dblink.treewalk()`'s `preinst_mask` + `install_mask_dir` step
/// (`vartree.py:4581-4610`, run before `_collision_protect` and before a
/// single file is copied to `${ROOT}`): apply `options.install_mask` to
/// the staged image `d`, deleting every matched path, then -- when a
/// `no{man,info,doc}` `FEATURE` folded a `/usr/share/*` entry in --
/// `rmdir` a now-empty `<d>/usr/share`. The resolved mask is also
/// written to `<build_info>/INSTALL_MASK` (real `preinst_mask` writes it
/// there; `populate_vdb_tmp` then copies it into the vdb
/// wholesale like every other build-info file). A no-op with an empty
/// mask -- and then no `INSTALL_MASK` file, matching real's own
/// `[[ -n ${x} ]] && echo … > INSTALL_MASK`.
///
/// Shared with the remote-merge server (`remote_bundle::build_bundle`,
/// backlog #170): the bundle is staged from the same extraction, so the
/// mask must prune the same image before `collect_filemeta` runs.
pub(crate) fn apply_install_mask(
    d: &Path,
    build_info: &Path,
    options: &MergeOptions,
) -> Result<(), String> {
    let value = options.install_mask.trim();
    if value.is_empty() {
        return Ok(());
    }
    if build_info.is_dir() {
        std::fs::write(build_info.join("INSTALL_MASK"), format!("{value}\n"))
            .map_err(|e| format!("{}: {e}", build_info.join("INSTALL_MASK").display()))?;
    }
    let mask = crate::install_mask::InstallMask::new(value);
    if mask.is_empty() {
        return Ok(());
    }
    crate::install_mask::install_mask_dir(d, &mask)
        .map_err(|e| format!("{}: install_mask_dir: {e}", d.display()))?;
    if options.install_mask_prunes_usr_share {
        let _ = std::fs::remove_dir(d.join("usr/share"));
    }
    Ok(())
}

/// The installed `SLOT` (main slot only) of
/// `<root>/var/db/pkg/<category>/<package>-<version>`, read through the
/// shared [`portage_repo::vdb_entry_slot`] seam every other
/// installed-metadata consumer uses: a valid consolidated `metadata`
/// snapshot serves the field with no per-entry `open()`, and a
/// multi-line value collapses to one space-separated line the way real
/// `_aux_get` normalises (`" ".join(myd.split())`,
/// `vartree.py:975-1053`). A missing or empty `SLOT` reads as
/// `Some("0")`, matching real `aux_get`'s invalid-`SLOT` → `"0"`
/// translation (`:967-972`; O5 ruling 2026-09-23, which folds #115's
/// empty-`SLOT` half into this slice — #115 S1 extends the same
/// translation to present-but-invalid values upstream in
/// `portage_repo::vdb_aux_get`, so they arrive here already `"0"`; the
/// `EAPI` and `_mtime_` thirds stay #115's documented cuts). The return stays
/// `Option<String>` so the five callers' `None` arms keep compiling
/// untouched, but it is now always `Some`: every caller only queries
/// listed installed versions, so "no such entry" cannot reach here in
/// practice (and real `aux_get` raises `KeyError` there — portuale has
/// no such signal, per `vdb_aux_get`'s own doc).
pub(crate) fn read_installed_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<String> {
    let pf = format!("{package}-{version}");
    let raw = portage_repo::vdb_entry_slot(root, category, &pf);
    let main = raw.split('/').next().unwrap_or("");
    if main.is_empty() {
        Some("0".to_string())
    } else {
        Some(main.to_string())
    }
}

/// Real `self._installed_instance` selection (`vartree.py:4409-4418`):
/// among every other real, currently-installed version of this exact
/// `category/package/slot`, the one with the highest real `COUNTER`
/// (real `cpv_counter`, portuale's own real per-package `COUNTER` file
/// -- see `WriteTxn::next_counter`, called by `populate_vdb_tmp`) -- `None` when none exist (a
/// first-ever install, or every other same-slot instance's own
/// `COUNTER` is unreadable). Real `_installed_instance` is only ever
/// set when `slot_matches` (this exact slot has at least one other
/// installed version already) -- naturally true here too, since an
/// empty `own_versions` list has no max at all.
pub(crate) fn installed_instance_pf(
    root: &Path,
    category: &str,
    package: &str,
    slot: &str,
) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .filter(|version| {
            read_installed_slot(root, category, package, version).as_deref() == Some(slot)
        })
        .filter_map(|version| {
            let pf = format!("{package}-{version}");
            let counter: i64 = read_entry_text(root, category, &pf, "COUNTER")?
                .trim()
                .parse()
                .ok()?;
            Some((pf, counter))
        })
        .max_by_key(|(_, counter)| *counter)
        .map(|(pf, _)| pf)
}

/// Real `dblink.treewalk`'s own `REPLACING_VERSIONS` set
/// (`vartree.py:4768-4771`) just before `pkg_preinst`: the versions of
/// the installed same-slot instances this merge replaces. Portuale's
/// per-merge analogue is the single `installed_instance_pf` it already
/// selects (the highest-`COUNTER` same-slot instance -- real
/// `others_in_slot`'s own selection); its version is what an ebuild's
/// `ver_replacing` gate (real for EAPI >= 9, the `eapi9-ver` eclass for
/// EAPI < 9) sees. Real's general `doebuild_environment` setter
/// (`doebuild.py:1317-1322`) builds the same value for
/// `preinst`/`postinst`/`pretend`/`setup` from
/// `vardb.match(cpv_slot) + vardb.match("=" + cpv)`. `None` for a
/// first-ever install (no other same-slot instance), matching real's
/// empty string.
pub(crate) fn replacing_versions(pn: &str, installed_instance_pf: Option<&str>) -> Option<String> {
    let version = installed_instance_pf?.strip_prefix(pn)?.strip_prefix('-')?;
    (!version.is_empty()).then(|| version.to_string())
}

/// Whether the installed package at `<root>/var/db/pkg/<category>/
/// <package>-<version>` already claims `abs_path` in its own real
/// `CONTENTS` (the path field of its recognized `obj`/`sym`/`dir`/
/// `dev`/`fif`/`bin` lines, the same set `format_contents_line` writes),
/// read through the `mrg_director::PackagesDb` seam
/// ([`mrg_director::VdbReader`] over `root`) rather than the file
/// directly. `abs_path` is a logical absolute path (what
/// `find_collisions` builds); the seam's `contents_files` returns the
/// same paths with the vdb record's one leading `/` stripped (its
/// documented shape), so the leading `/` is removed here to meet it --
/// the exact inverse of that strip on every entry
/// `format_contents_line` writes.
pub(crate) fn owns_path(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    abs_path: &str,
) -> bool {
    let relative = abs_path.strip_prefix('/').unwrap_or(abs_path);
    mrg_director::VdbReader::new(root)
        .contents_files(category, package, version)
        .iter()
        .any(|path| path == relative)
}

/// The `CONTENTS` text of the vdb entry `category/pf`: the live entry
/// first, falling back to its `-MERGING-<pf>` temporary when the live
/// one is absent. The fallback is real `dblink.getcontents()` resolving
/// through `dbdir`, which *is* `dbtmpdir` for the package being merged
/// while the replace loop unmerges the instances it replaces (real
/// `treewalk()`: the new entry is renamed into place only after) -- so
/// an `others_in_slot`/`also_keep` ownership check against the replacing
/// version sees the just-written file list. Enumeration skips are
/// unaffected (those never name a `pf` explicitly; see
/// [`portage_util::is_merging_vdb_entry`]): outside a merge the live
/// entry always exists when these are queried, so the fallback only ever
/// fires mid-merge.
pub(crate) fn read_contents_pf(root: &Path, category: &str, pf: &str) -> Option<String> {
    if let Some(text) = read_entry_text(root, category, pf, "CONTENTS") {
        return Some(text);
    }
    portage_vdb::for_root(root)
        .read_pending_file(&portage_vdb::EntryKey::new(category, pf), "CONTENTS")
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// `entry/<name>.is_file()` of old: one `stat` (following symlinks)
/// through the root's [`portage_vdb::InstalledDb::file_meta`]; a missing
/// entry or file, or any error, is `false`.
pub(crate) fn entry_file_is_regular(root: &Path, category: &str, pf: &str, name: &str) -> bool {
    portage_vdb::for_root(root)
        .file_meta(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .is_some_and(|meta| meta.mode & 0o170000 == 0o100000)
}

/// One file of the live entry `category/pf` as UTF-8 text, through the
/// root's [`portage_vdb::InstalledDb::read_file`] (`files`: one `open`
/// of `var/db/pkg/<category>/<pf>/<name>`). `None` when the entry or the
/// file is missing, unreadable or not UTF-8 -- every case today's
/// `read_to_string(..).ok()` callers treated alike.
pub(crate) fn read_entry_text(root: &Path, category: &str, pf: &str, name: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_file(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Same real `CONTENTS`-ownership check as `owns_path`, but keyed by a
/// bare `category`/`pf` pair (`"package-version"`, real portage's own
/// vdb directory-name convention) rather than a split `package`/
/// `version` -- what `blocked_installed_packages` below already has on
/// hand, since it discovers installed packages by scanning real vdb
/// directory names directly rather than through `installed_versions`'s
/// own `package`-scoped lookup.
///
/// Deliberately stays a direct `CONTENTS` read rather than routing
/// through `PackagesDb`: the trait keys `contents_files` by a split
/// `(package, version)` and has no `pf`-keyed query, and both callers
/// (the blocker set above, `ebuild_unmerge`'s same-slot orphan check)
/// hold only the bare vdb directory name -- going through the seam would
/// split the `pf` apart only for `VdbReader` to join it back together.
pub(crate) fn owns_path_pf(root: &Path, category: &str, pf: &str, abs_path: &str) -> bool {
    let Some(text) = read_contents_pf(root, category, pf) else {
        return false;
    };
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next();
        parts.next() == Some(abs_path)
    })
}

/// Real `dblnk._match_contents(relative_path)` + `getcontents()[key][0]`:
/// the node type (`"obj"`/`"dir"`/`"sym"`/...) the installed package
/// `category/pf` recorded for `abs_path` in its own real `CONTENTS`, or
/// `None` if it doesn't own that exact path at all. `ebuild_unmerge`'s
/// own bug #326685 "symlink orphan" detection is the one caller: it
/// needs to know not just *whether* another same-slot instance owns a
/// path (`owns_path_pf` above) but specifically *what type* it recorded
/// it as, to tell "still a symlink there too" apart from "reclassified
/// as a real directory".
pub(crate) fn owned_node_type_pf(
    root: &Path,
    category: &str,
    pf: &str,
    abs_path: &str,
) -> Option<String> {
    let text = read_contents_pf(root, category, pf)?;
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let node_type = parts.next()?;
        if parts.next() == Some(abs_path) {
            Some(node_type.to_string())
        } else {
            None
        }
    })
}

/// Real `dblnk._match_contents(relative_path)` + `getcontents()[key]`,
/// the *value* half `owned_node_type_pf` above deliberately leaves out:
/// an `obj` entry's own content MD5, or a `sym` entry's own target
/// string -- exactly the values real `_protect()`'s own `data[2]`
/// compares against `dest_md5`/`dest_link` for `protect_if_modified`
/// (see `protect_decision`'s own doc comment). `None` for any other
/// node type (`dir`, etc. -- never real `_protect()`-relevant) or a
/// path this instance doesn't own at all.
pub(crate) fn owned_node_value_pf(
    root: &Path,
    category: &str,
    pf: &str,
    abs_path: &str,
) -> Option<(String, String)> {
    let text = read_contents_pf(root, category, pf)?;
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let node_type = parts.next()?;
        if parts.next() != Some(abs_path) {
            return None;
        }
        match node_type {
            "obj" => Some((node_type.to_string(), parts.next()?.to_string())),
            "sym" => {
                if parts.next() != Some("->") {
                    return None;
                }
                Some((node_type.to_string(), parts.next()?.to_string()))
            }
            _ => None,
        }
    })
}
