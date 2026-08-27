//! The `md5-dict` aux-cache validator (shared by `--regen`'s repo-cache shortcut
//! and the depcachedir rung of the depend-phase fallback): `true` when an
//! on-disk cache entry still validates, so the `depend` phase can be skipped.
//!
//! The formats differ in exactly one field: the pregen cache stores eclasses as
//! `name\tmd5` pairs, the writable depcachedir as `name\tpath\tmd5` triples
//! (`store_eclass_paths` selects which); validation compares the checksum only.
// Portage: portdbapi._pull_valid_cache, cache/template.py, eclass_cache.py

use md5::{Digest as _, Md5};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// Whether `EAPI` is supported: `0..=9` plus the testing (`9-pre1`) and
/// deprecated (`3_pre1`/`3_pre2`/`4_pre1`/`5_pre1`/`5_pre2`/`6_pre1`/`7_pre1`)
/// sets. Anything else means the cache entry is disregarded so `depend` re-runs.
/// Callers normalize a missing/empty string to `"0"` first.
// Portage: portage.eapi_is_supported, porttree.py
pub fn eapi_is_supported(eapi: &str) -> bool {
    let eapi = eapi.trim();
    matches!(
        eapi,
        "0" | "1"
            | "2"
            | "3"
            | "4"
            | "5"
            | "6"
            | "7"
            | "8"
            | "9"
            | "9-pre1"
            | "3_pre1"
            | "3_pre2"
            | "4_pre1"
            | "5_pre1"
            | "5_pre2"
            | "6_pre1"
            | "7_pre1"
    )
}

/// Whether `:=` slot-operator dependencies exist in this EAPI (EAPI 5+).
/// Call only with an `eapi_is_supported` EAPI; reads by numeric prefix, so the
/// deprecated underscore EAPIs (which Portage's own parser rejects) read by
/// prefix here -- unreachable in any vdb or tree in practice.
// Portage: _get_eapi_attrs(eapi).slot_operator, FakeVartree._apply_dynamic_deps
pub fn eapi_has_slot_operator(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().is_ok_and(|n| n >= 5)
}

/// Whether a `||` group an inactive conditional emptied is dropped (EAPI 6 and
/// below). Call only with an `eapi_is_supported` EAPI; unparseable input reads
/// `false`, matching Portage's defaults for `None`/unsupported EAPIs.
// Portage: _get_eapi_attrs(eapi).empty_groups_always_true
pub fn eapi_empty_groups_always_true(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().is_ok_and(|n| n <= 6)
}

/// Whether `IUSE_EFFECTIVE` exists in this EAPI (EAPI 5+) and therefore governs
/// the `PORTAGE_USE` masking and the valid-flag domain. Same numeric-prefix
/// reading as `eapi_has_slot_operator`.
// Portage: _get_eapi_attrs(eapi).iuse_effective
pub fn eapi_has_iuse_effective(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().is_ok_and(|n| n >= 5)
}

/// Whether `IDEPEND` exists in this EAPI (EAPI 8+). An older EAPI's `IDEPEND`
/// assignment never reaches saved metadata, so it reads as empty.
// Portage: eapi_has_idepend, bin/ebuild.sh depend phase
pub fn eapi_has_idepend(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().is_ok_and(|n| n >= 8)
}

/// Arguments of [`cache_entry_is_valid`].
pub struct CacheEntryIsValidArgs<'a> {
    pub ebuild_path: &'a Path,
    pub repo_location: &'a Path,
    pub masters: &'a [PathBuf],
    pub category: &'a str,
    pub pf: &'a str,
    pub store_eclass_paths: bool,
}

/// `true` when `metadata/md5-cache/<category>/<pf>` validates, so the `depend`
/// phase can be skipped. Any I/O failure (no cache file, no ebuild, no eclass
/// dir) is `false` -- `depend` runs, as when no auxdb hits.
// Portage: portdbapi._pull_valid_cache, porttree.py, cache/template.py
pub fn cache_entry_is_valid(args: CacheEntryIsValidArgs<'_>) -> bool {
    let CacheEntryIsValidArgs {
        ebuild_path,
        repo_location,
        masters,
        category,
        pf,
        store_eclass_paths,
    } = args;
    let cache_file = repo_location
        .join("metadata/md5-cache")
        .join(category)
        .join(pf);
    let text = match std::fs::read_to_string(&cache_file) {
        Ok(t) => t,
        Err(_) => return false,
    };
    entry_is_valid(
        &text,
        ebuild_path,
        repo_location,
        masters,
        store_eclass_paths,
    )
}

/// The path-independent half of [`cache_entry_is_valid`]: one entry's bytes in
/// `flat_hash` format, accepted or not against the current ebuild. Shared by
/// `--regen`, the depcachedir rung and `repo_aux_metadata`'s read-path validation.
// Portage: cache/template.py::validate_entry
pub fn entry_is_valid(
    text: &str,
    ebuild_path: &Path,
    repo_location: &Path,
    masters: &[PathBuf],
    store_eclass_paths: bool,
) -> bool {
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            return false;
        };
        fields.insert(k, v);
    }
    let ebuild_bytes = match std::fs::read(ebuild_path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let ebuild_md5 = format!("{:x}", Md5::digest(&ebuild_bytes));
    if fields.get("_md5_").is_none_or(|v| *v != ebuild_md5) {
        return false;
    }
    let eapi = fields.get("EAPI").map(|s| s.trim()).unwrap_or("");
    let eapi = if eapi.is_empty() { "0" } else { eapi };
    if !eapi_is_supported(eapi) {
        return false;
    }
    let eclasses_raw = fields.get("_eclasses_").map(|s| s.trim()).unwrap_or("");
    if eclasses_raw.is_empty() {
        return true;
    }
    let parts: Vec<&str> = eclasses_raw.split('\t').collect();
    let width = if store_eclass_paths { 3 } else { 2 };
    if !parts.len().is_multiple_of(width) {
        return false;
    }
    let porttrees: Vec<&Path> = masters
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(repo_location))
        .collect();
    for pair in parts.chunks(width) {
        // Only the checksum is compared; a triple's path element is ignored.
        let (name, want_md5) = if store_eclass_paths {
            (pair[0], pair[2])
        } else {
            (pair[0], pair[1])
        };
        // `md5` values are 32 hex digits; anything else is corruption.
        // Portage: _md5_deserializer
        if want_md5.len() != 32 || !want_md5.bytes().all(|b| b.is_ascii_hexdigit()) {
            return false;
        }
        let Some(got_md5) = porttrees
            .iter()
            .rev()
            .find_map(|tree| eclass_md5(&tree.join("eclass").join(format!("{name}.eclass"))))
        else {
            return false;
        };
        if got_md5 != want_md5 {
            return false;
        }
    }
    true
}

/// The md5 of one eclass file, memoised per path and invalidated on mtime/size
/// change. Only matters when one process reads the same eclass many times (the
/// resolver's per-candidate validation).
// Portage: eclass_cache.hashed_path
fn eclass_md5(path: &Path) -> Option<String> {
    struct Cached {
        mtime: Option<std::time::SystemTime>,
        size: u64,
        md5: Option<String>,
    }
    type Memo = HashMap<PathBuf, Cached>;
    static MEMO: OnceLock<RwLock<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| RwLock::new(HashMap::new()));

    let (mtime, size) = match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => (None, 0),
    };
    if let Ok(guard) = memo.read()
        && let Some(hit) = guard.get(path)
        && hit.mtime == mtime
        && hit.size == size
    {
        return hit.md5.clone();
    }
    let md5 = std::fs::read(path)
        .ok()
        .map(|bytes| format!("{:x}", Md5::digest(&bytes)));
    if let Ok(mut guard) = memo.write() {
        guard.insert(
            path.to_path_buf(),
            Cached {
                mtime,
                size,
                md5: md5.clone(),
            },
        );
    }
    md5
}

/// The `masters`-then-self chain a validator needs for `_eclasses_`, memoised per
/// repo location. An undescribed repo (or unreadable config) yields an empty
/// chain: `_eclasses_` then fails validation, so `depend` re-runs (the safe side).
pub fn repo_masters_for_location(repo_location: &Path) -> Vec<PathBuf> {
    type Memo = HashMap<PathBuf, Vec<PathBuf>>;
    static MEMO: OnceLock<RwLock<Memo>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| RwLock::new(HashMap::new()));

    if let Ok(guard) = memo.read()
        && let Some(masters) = guard.get(repo_location)
    {
        return masters.clone();
    }
    let masters = crate::repo_config_for_location(repo_location)
        .map(|repo| repo.masters)
        .unwrap_or_default();
    if let Ok(mut guard) = memo.write() {
        guard.insert(repo_location.to_path_buf(), masters.clone());
    }
    masters
}
