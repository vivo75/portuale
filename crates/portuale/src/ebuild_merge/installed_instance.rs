//! INSTALL_MASK and what is already installed.

use crate::ebuild_merge::*;

/// `treewalk()`'s `preinst_mask` + `install_mask_dir` step, before the
/// collision check and before any copy: apply `options.install_mask` to
/// the staged image, then -- when a `no{man,info,doc}` feature folded a
/// `/usr/share/*` entry in -- `rmdir` a now-empty `<d>/usr/share`. The
/// resolved mask is also written to `<build_info>/INSTALL_MASK`. A no-op
/// with an empty mask -- and then no file, matching the `[[ -n ${x} ]]`
/// guard.
///
/// Shared with the remote-merge server (`remote_bundle::build_bundle`).
// Portage: vartree.py, treewalk
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

/// The installed `SLOT` (main slot only) of an installed
/// `<category>/<package>-<version>`, through the shared
/// `vdb_entry_slot` seam. A missing or empty `SLOT` reads as `"0"`.
/// Always `Some` in practice: callers only query listed versions.
/// Portage: vartree.py, aux_get
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

/// The installed same-slot instance this merge upgrades over: among
/// every other installed version of this `category/package/slot`, the
/// one with the highest `COUNTER`. `None` when none exists.
/// Portage: vartree.py, _installed_instance
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

/// The versions of the installed same-slot instances this merge
/// replaces, for `REPLACING_VERSIONS`. `None` for a first-ever install.
/// Portage: vartree.py, treewalk; doebuild.py, doebuild_environment
pub(crate) fn replacing_versions(pn: &str, installed_instance_pf: Option<&str>) -> Option<String> {
    let version = installed_instance_pf?.strip_prefix(pn)?.strip_prefix('-')?;
    (!version.is_empty()).then(|| version.to_string())
}

/// Whether the installed `<package>-<version>` already claims `abs_path`
/// in its `CONTENTS`, through the `PackagesDb` seam. `abs_path` is a
/// logical absolute path; the leading `/` is stripped to meet the seam's
/// shape.
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

/// The `CONTENTS` text of `category/pf`: the live entry first, falling
/// back to its `-MERGING-<pf>` temporary when the live one is absent
/// (mid-merge, the new entry is not yet renamed into place, so an
/// ownership check against the replacing version sees the just-written
/// list). Outside a merge the live entry always exists when queried.
/// Portage: vartree.py, dblink.getcontents
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

/// One `stat` through the installed-db seam; a missing entry, file or
/// error is `false`.
pub(crate) fn entry_file_is_regular(root: &Path, category: &str, pf: &str, name: &str) -> bool {
    portage_vdb::for_root(root)
        .file_meta(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .is_some_and(|meta| meta.mode & 0o170000 == 0o100000)
}

/// One file of the live entry `category/pf` as UTF-8 text. `None` when
/// missing, unreadable or not UTF-8.
pub(crate) fn read_entry_text(root: &Path, category: &str, pf: &str, name: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_file(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Same `CONTENTS`-ownership check as `owns_path`, but keyed by a bare
/// `category`/`pf` pair (the vdb directory-name convention) for callers
/// holding only the directory name. Stays a direct `CONTENTS` read: the
/// seam keys by split `(package, version)` and has no `pf`-keyed query.
pub(crate) fn owns_path_pf(root: &Path, category: &str, pf: &str, abs_path: &str) -> bool {
    read_contents_pf(root, category, pf).is_some_and(|text| contents_text_owns(&text, abs_path))
}

/// Whether a `CONTENTS` text lists `abs_path`.
pub(crate) fn contents_text_owns(text: &str, abs_path: &str) -> bool {
    text.lines().any(|line| {
        let mut parts = line.split_whitespace();
        parts.next();
        parts.next() == Some(abs_path)
    })
}

/// The node type a `CONTENTS` text records for `abs_path`.
pub(crate) fn contents_text_node_type(text: &str, abs_path: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let node_type = parts.next()?;
        (parts.next() == Some(abs_path)).then(|| node_type.to_string())
    })
}

/// The `CONTENTS` text of a package being merged over `category/pf`: its
/// `-MERGING-<pf>` temporary first, the live entry only when there is no
/// temporary. On a same-version reinstall the live entry is the copy being
/// replaced, so the live-first [`read_contents_pf`] would credit the new
/// package with the old files.
/// Portage: vartree.py, dblink.treewalk (`self.dbdir` is `dbtmpdir`)
pub(crate) fn read_replacing_contents_pf(root: &Path, category: &str, pf: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_pending_file(&portage_vdb::EntryKey::new(category, pf), "CONTENTS")
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .or_else(|| read_contents_pf(root, category, pf))
}

/// An `obj` entry's content MD5, or a `sym` entry's target string --
/// the values `protect_decision` compares for `protect_if_modified`.
/// `None` for other node types or unowned paths.
// Portage: vartree.py, dblink._protect
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
