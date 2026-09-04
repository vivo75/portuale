//! The local PKGDIR index (`Packages`).

use crate::binpkg::*;

/// The parsed `<pkgdir>/Packages` pool one `$PKGDIR` file scan vouches
/// files against: the basename lookup, the stanzas behind it, and which
/// stanzas the walk consumed (the leftovers seed the stale-index
/// re-injection). One struct so the per-file scan stays under the
/// argument-count lint.
pub(crate) struct IndexVouch<'a> {
    /// `PATH` basename -> stanza indices into `stanzas`.
    pub(crate) by_basename: HashMap<&'a str, Vec<usize>>,
    /// The parsed `<pkgdir>/Packages` stanzas.
    pub(crate) stanzas: &'a [HashMap<String, String>],
    /// Stanza indices the walk consumed, parallel to `stanzas`.
    pub(crate) vouched: &'a mut [bool],
}

/// The fast-path key set: the `_aux_cache_keys` plus `CPV`/`SIZE`, minus
/// the digest keys (digests are never computed at scan, so a stanza
/// missing only digests must still hit the fast path). `CPV`/`SIZE`/`SLOT`/
/// `_mtime_` ride along; only `_mtime_`/`SIZE` compare by value.
// Portage: `bintree._pkgindex_keys`, `bintree._pkgindex_hashes`.
pub(crate) const PKGINDEX_MINIMUM_KEYS: &[&str] = &[
    "BDEPEND",
    "BUILD_ID",
    "BUILD_TIME",
    "CHOST",
    "DEFINED_PHASES",
    "DEPEND",
    "EAPI",
    "IDEPEND",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "PROVIDES",
    "RDEPEND",
    "repository",
    "REQUIRES",
    "RESTRICT",
    "SIZE",
    "SLOT",
    "USE",
    "_mtime_",
    "CPV",
];

/// Every minimum key must be present on the stanza (presence only; values
/// compare solely for `_mtime_`/`SIZE`).
pub(crate) fn stanza_has_minimum_keys(stanza: &HashMap<String, String>) -> bool {
    PKGINDEX_MINIMUM_KEYS
        .iter()
        .all(|key| stanza.contains_key(*key))
}

/// Arguments of [`scan_binpkg_file`]; `vouch`'s pool lifetime (`'v`)
/// stays apart from the borrow (`'a`).
pub(crate) struct ScanBinpkgFileArgs<'a, 'v> {
    pub(crate) file: &'a Path,
    pub(crate) basename: &'a str,
    pub(crate) category: &'a str,
    pub(crate) path_field: String,
    pub(crate) vouch: &'a mut IndexVouch<'v>,
    pub(crate) multi_instance: bool,
    pub(crate) trust_index: bool,
}

/// One `$PKGDIR` binpkg file -> its synthesized `Packages`-style entry, or
/// `None` if it isn't a binpkg or is a misnamed multi-instance file.
/// Reuses an unchanged index entry (matched by basename, `_mtime_`/`SIZE`
/// agreeing, minimum keys present) when one exists, else parses the
/// archive's own embedded metadata. In trusted mode a stat-matching but
/// key-incomplete stanza falls through to the stale-vouch arm, which reuses
/// it verbatim, so the added minimum-keys check cannot change trusted-mode
/// output. `multi_instance` selects the naming contract: `<pf>-<build_id>`
/// with `PF` from the archive and `BUILD_ID` from the filename when set,
/// `<pf>` when clear.
pub(crate) fn scan_binpkg_file(
    args: ScanBinpkgFileArgs<'_, '_>,
) -> Result<Option<HashMap<String, String>>, BinpkgError> {
    let ScanBinpkgFileArgs {
        file,
        basename,
        category,
        path_field,
        vouch,
        multi_instance,
        trust_index,
    } = args;
    let is_gpkg = basename.ends_with(".gpkg.tar");
    let ext = if is_gpkg {
        ".gpkg.tar"
    } else if basename.ends_with(".xpak") {
        ".xpak"
    } else if basename.ends_with(".tbz2") {
        ".tbz2"
    } else {
        return Ok(None);
    };
    let Ok(st) = fs::metadata(file) else {
        return Ok(None);
    };
    let mtime = file_mtime(&st);
    let size = st.len();

    // The live mtime/size must agree with the stanza, which must also
    // carry every minimum key; otherwise the stanza is re-parsed, not
    // vouched.
    if let Some(candidates) = vouch.by_basename.get(basename)
        && let Some(hit) = candidates.iter().copied().find(|&i| {
            let d = &vouch.stanzas[i];
            d.get("_mtime_").and_then(|m| m.parse::<i64>().ok()) == Some(mtime)
                && d.get("SIZE").and_then(|s| s.parse::<u64>().ok()) == Some(size)
                && stanza_has_minimum_keys(d)
        })
    {
        let mut entry = vouch.stanzas[hit].clone();
        entry.insert("PATH".to_string(), path_field);
        vouch.vouched[hit] = true;
        return Ok(Some(entry));
    }

    // Trusted mode: an index stanza for this basename vouches for the file
    // even when `_mtime_`/`SIZE` no longer agree (a truncated or otherwise
    // corrupted binpkg is reused verbatim, stale digests included: the
    // merge-time verification checks the file against exactly those index
    // values). Only `CPV` is required, and the stanza must belong to this
    // file's own category. A file no stanza vouches for keeps the reindex
    // behavior below. With `trust_index == false` this whole arm is off.
    if trust_index
        && let Some(candidates) = vouch.by_basename.get(basename)
        && let Some(hit) = candidates.iter().copied().find(|&i| {
            let d = &vouch.stanzas[i];
            d.contains_key("CPV")
                && d.get("CPV")
                    .is_some_and(|cpv| cpv.starts_with(&format!("{category}/")))
        })
    {
        let mut entry = vouch.stanzas[hit].clone();
        entry.insert("PATH".to_string(), path_field);
        vouch.vouched[hit] = true;
        return Ok(Some(entry));
    }

    // Stale, moved, or unindexed: re-derive from the file itself (`.xpak`
    // is byte-identical to `.tbz2`).
    let mut meta = if is_gpkg {
        read_gpkg_metadata(file)?
    } else {
        read_xpak_metadata(file)?
    };
    let stem = basename.strip_suffix(ext).unwrap();
    let embedded_pf = meta.get("PF").filter(|p| !p.is_empty()).cloned();

    let pf = if multi_instance {
        // The archive's embedded `PF` is the ground truth; the filename
        // must be exactly `<PF>-<build_id>`.
        let Some(real_pf) = embedded_pf else {
            return Ok(None);
        };
        let Some(build_id) = stem
            .strip_prefix(&real_pf)
            .and_then(|s| s.strip_prefix('-'))
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        else {
            return Ok(None);
        };
        meta.insert("BUILD_ID".to_string(), build_id.to_string());
        real_pf
    } else {
        // A category-level file whose stem disagrees with the archive's
        // own `PF` (e.g. a misplaced `-<build_id>` file) is skipped.
        if let Some(real_pf) = &embedded_pf
            && real_pf != stem
        {
            return Ok(None);
        }
        stem.to_string()
    };

    meta.insert("CPV".to_string(), format!("{category}/{pf}"));
    meta.entry("CATEGORY".to_string())
        .or_insert_with(|| category.to_string());
    meta.entry("PF".to_string()).or_insert_with(|| pf.clone());
    if let Some(repo) = meta.remove("repository") {
        meta.entry("REPO".to_string()).or_insert(repo);
    }
    meta.insert("SIZE".to_string(), size.to_string());
    meta.insert("PATH".to_string(), path_field);
    meta.insert("_mtime_".to_string(), mtime.to_string());
    Ok(Some(meta))
}
