//! The local PKGDIR index (`Packages`).

use crate::binpkg::*;

/// The parsed `<pkgdir>/Packages` pool one `$PKGDIR` file scan vouches
/// files against: the (directory, basename) lookup, the stanzas behind it,
/// and which stanzas the walk consumed (the leftovers seed the stale-index
/// re-injection). One struct so the per-file scan stays under the
/// argument-count lint.
pub(crate) struct IndexVouch<'a> {
    /// `(PATH` directory, basename) -> stanza indices into `stanzas`.
    /// Portage keys by `(os.path.dirname(path), basename)` (bug 978985):
    /// the same basename in two subdirs with equal `SIZE`/`_mtime_` must
    /// not vouch for each other's file.
    pub(crate) by_basename: HashMap<(&'a str, &'a str), Vec<usize>>,
    /// The parsed `<pkgdir>/Packages` stanzas.
    pub(crate) stanzas: &'a [HashMap<String, String>],
    /// Stanza indices the walk consumed, parallel to `stanzas`.
    pub(crate) vouched: &'a mut [bool],
}

/// Portage's local `gpkg_only` warning, once per scan (`bintree.py`
/// prints it colorized `WARN`; portuale prints it plain like its other
/// binhost warnings).
pub(crate) const LOCAL_XPAK_IGNORED_WARNING: &str =
    "Local XPAK packages are ignored due to 'binpkg-request-signature'.\n";

/// Walk `pkgdir` for binpkg *files* and synthesize one `Packages`-style
/// entry per file from its own embedded metadata, fast-pathed against
/// already-parsed `<pkgdir>/Packages` entries first: a (directory,
/// basename)-matched candidate whose `_mtime_`/`SIZE` still agree with the
/// live file and which carries [`PKGINDEX_MINIMUM_KEYS`] is reused verbatim
/// (its `PATH` refreshed in case the file moved). A (directory,
/// basename)-matched entry whose `_mtime_`/`SIZE` no longer agree is *still*
/// reused verbatim under the default `trust_index = true`: the file is
/// first touched at merge, by digest verification, so only `CPV` is required
/// of the vouched stanza. Only a file no stanza vouches for is freshly
/// parsed, and gets a fresh `_mtime_`/`SIZE` so a later call can hit the
/// fast path.
///
/// `$PKGDIR` layout is `<pkgdir>/<category>/<pf>.{tbz2,gpkg.tar}` for a
/// single instance, and -- under `FEATURES=binpkg-multi-instance` --
/// `<pkgdir>/<category>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}`. A `.xpak`
/// file is byte-format-identical to a `.tbz2`, so `read_xpak_metadata`
/// reads it unchanged; the multi-instance `BUILD_ID` comes from the
/// filename, validated against the archive's own embedded `PF`. Entries
/// are `CPV`-sorted for a deterministic pool order.
///
/// A file the reader rejects is split by class: an `Invalid` file is
/// reported as `!!! Invalid binary package: '<path>', <reason>` and
/// skipped; a `Fatal` one stops the whole scan. Reader messages embed the
/// path already, so the reported reason strips the leading `'<path>: '`.
///
/// A `<pkgdir>/Packages` stanza no file on disk answers to is re-injected
/// verbatim under `trust_index = true`, so the resolver still selects the
/// binary and the merge aborts instead of silently falling back to the
/// ebuild. Only stanzas whose `PATH` names no live file inject, and only
/// stanzas carrying a `CPV`. Under `gpkg_only` a `PATH`-less stanza is
/// skipped instead (it would default to a `.tbz2`); the pre-walk stanza
/// pass already warned for it.
///
/// `gpkg_only` (`FEATURES=binpkg-request-signature`) drops local xpak
/// files with Portage's once-per-scan `Local XPAK packages are ignored…`
/// warning (printed plain, like portuale's other binhost warnings;
/// Portage colorizes `WARN`): in reindex mode every xpak file is skipped
/// with the warning (Portage never vouches there); in trusted mode only
/// xpak files no stanza vouches for are skipped, silently (Portage serves
/// indexed xpak stanzas without ever walking, and never sees the file).
///
/// `trust_index = false` (`FEATURES=-pkgdir-index-trusted`) is the reindex
/// mode: only the `_mtime_`/`SIZE` fast path is trusted, a changed file is
/// re-parsed from disk, and a stanza with no live file is dropped. Files
/// no stanza covers are parsed from disk in both modes, so a
/// `Packages`-less `$PKGDIR` still resolves. The index is never written
/// back (recomputed each run as `Config::scanned_binpkgs`).
///
/// Cut: the old flat `<pkgdir>/All/<pf>.tbz2` layout is not walked. A
/// *misnamed* multi-instance file is skipped.
// Portage: `bintree._populate_local`.
pub fn populate_local_pkgdir(
    pkgdir: &Path,
    trust_index: bool,
    gpkg_only: bool,
) -> Result<Vec<HashMap<String, String>>, String> {
    let existing = portage_repo::read_packages_index(pkgdir);
    let mut by_basename: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in existing.iter().enumerate() {
        if let Some(path) = e.get("PATH")
            && let Some(parent) = Path::new(path).parent().and_then(|p| p.to_str())
            && let Some(basename) = Path::new(path).file_name().and_then(|n| n.to_str())
        {
            by_basename.entry((parent, basename)).or_default().push(i);
        }
    }
    // Which `existing` stanzas the walk consumes; the rest are orphan
    // candidates for the stale-index re-injection.
    let mut vouched = vec![false; existing.len()];
    let mut vouch = IndexVouch {
        by_basename,
        stanzas: &existing,
        vouched: &mut vouched,
    };

    // `Invalid` files are reported and skipped; `Fatal` ones propagate.
    // Reader messages already carry `'<path>: '`, stripped here to keep
    // the one-path warning shape.
    let record = |file: &Path, scanned: Result<Option<HashMap<String, String>>, BinpkgError>| {
        match scanned {
            Ok(entry) => Ok(entry),
            Err(BinpkgError::Invalid(message)) => {
                let reason = message
                    .strip_prefix(&format!("{}: ", file.display()))
                    .unwrap_or(&message);
                eprintln!("!!! Invalid binary package: '{}', {reason}", file.display());
                Ok(None)
            }
            Err(BinpkgError::Fatal(message)) => Err(message),
        }
    };

    let mut out: Vec<HashMap<String, String>> = Vec::new();
    let Ok(cat_paths) = portage_util::read_dir_paths(pkgdir) else {
        return Ok(out);
    };
    // Portage's once-per-scan `gpkg_only` warning state. The stanza loop
    // runs first (a `PATH`-less stanza is warned and skipped there), then
    // the walk: at most one warning per scan, in Portage's order.
    let mut gpkg_warned = false;
    if gpkg_only
        && existing.iter().any(|e| {
            e.get("PATH").is_none_or(|p| p.is_empty())
                && e.get("CPV").is_some_and(|c| !c.is_empty())
        })
    {
        gpkg_warned = true;
        eprint!("{LOCAL_XPAK_IGNORED_WARNING}");
    }
    for cat_path in cat_paths {
        if !cat_path.is_dir() {
            continue;
        }
        let Some(category) = cat_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        for entry in read_dir_sorted(&cat_path)? {
            let Some(name) = entry.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if entry.is_dir() {
                // A `<cat>/<pn>` multi-instance package dir: its files are
                // `<pf>-<build_id>.{xpak,gpkg.tar}`.
                for mfile in read_dir_sorted(&entry)? {
                    let Some(mname) = mfile.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let path_field = format!("{category}/{name}/{mname}");
                    // An xpak file is invisible under `gpkg_only` unless a
                    // stanza vouches for this exact `(dir, basename)`
                    // (trusted mode serves indexed xpak stanzas without
                    // ever walking; reindex mode has no stanza pool). The
                    // warning fires only on the reindex walk: in trusted
                    // mode Portage never sees the file at all.
                    if gpkg_only
                        && (mname.ends_with(".tbz2") || mname.ends_with(".xpak"))
                        && (!trust_index
                            || !vouch
                                .by_basename
                                .contains_key(&(format!("{category}/{name}").as_str(), mname)))
                    {
                        if !trust_index && !gpkg_warned {
                            gpkg_warned = true;
                            eprint!("{LOCAL_XPAK_IGNORED_WARNING}");
                        }
                        continue;
                    }
                    let Some(e) = record(
                        &mfile,
                        scan_binpkg_file(ScanBinpkgFileArgs {
                            file: &mfile,
                            basename: mname,
                            category,
                            path_field,
                            vouch: &mut vouch,
                            multi_instance: true,
                            trust_index,
                        }),
                    )?
                    else {
                        continue;
                    };
                    // The containing subdir must be this package's own `<pn>`
                    // (from its now-known `CPV`).
                    let pn_ok = e
                        .get("CPV")
                        .and_then(|cpv| portage_dep::parse_candidate(cpv))
                        .is_some_and(|c| c.package == name);
                    if pn_ok {
                        out.push(e);
                    }
                }
                continue;
            }
            let path_field = format!("{category}/{name}");
            if gpkg_only
                && (name.ends_with(".tbz2") || name.ends_with(".xpak"))
                && (!trust_index || !vouch.by_basename.contains_key(&(category, name)))
            {
                // Warned only on the reindex walk; see the multi-instance
                // arm above.
                if !trust_index && !gpkg_warned {
                    gpkg_warned = true;
                    eprint!("{LOCAL_XPAK_IGNORED_WARNING}");
                }
                continue;
            }
            if let Some(e) = record(
                &entry,
                scan_binpkg_file(ScanBinpkgFileArgs {
                    file: &entry,
                    basename: name,
                    category,
                    path_field,
                    vouch: &mut vouch,
                    multi_instance: false,
                    trust_index,
                }),
            )? {
                out.push(e);
            }
        }
    }
    // Stale-index re-injection: every never-vouched stanza whose `PATH`
    // names no live file is a removed binpkg still selected, so it rejoins
    // the pool verbatim. Skipped entirely when `trust_index` is false.
    if trust_index {
        for (i, stanza) in existing.iter().enumerate() {
            if vouch.vouched[i] {
                continue;
            }
            let Some(cpv) = stanza.get("CPV").filter(|c| !c.is_empty()) else {
                continue;
            };
            // A PATH-less stanza defaults to `<cpv>.tbz2`; a live file there
            // was already consumed by the walk, so only the missing half
            // injects. Under `gpkg_only` that default is an xpak: it was
            // already warned (and skipped) by the pre-walk stanza pass.
            let rel = match stanza.get("PATH").filter(|p| !p.is_empty()) {
                Some(p) => p.clone(),
                None if gpkg_only => continue,
                None => format!("{cpv}.tbz2"),
            };
            if pkgdir.join(&rel).is_file() {
                continue;
            }
            let mut entry = stanza.clone();
            entry
                .entry("PATH".to_string())
                .or_insert_with(|| rel.clone());
            out.push(entry);
        }
    }
    out.sort_by(|a, b| a.get("CPV").cmp(&b.get("CPV")));
    Ok(out)
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

    let dir = Path::new(&path_field)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");
    // The live mtime/size must agree with the stanza, which must also
    // carry every minimum key; otherwise the stanza is re-parsed, not
    // vouched. The lookup is by (directory, basename): a stanza vouches
    // only for its own dir's file.
    // Portage: bintree.py, _populate_local (`basename_index`).
    if let Some(candidates) = vouch.by_basename.get(&(dir, basename))
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
        && let Some(candidates) = vouch.by_basename.get(&(dir, basename))
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
