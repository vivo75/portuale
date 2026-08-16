//! INSTALL_MASK and what is already installed.

use crate::ebuild_merge::*;

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

/// One file of the live entry `category/pf` as UTF-8 text. `None` when
/// missing, unreadable or not UTF-8.
pub(crate) fn read_entry_text(root: &Path, category: &str, pf: &str, name: &str) -> Option<String> {
    portage_vdb::for_root(root)
        .read_file(&portage_vdb::EntryKey::new(category, pf), name)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
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
