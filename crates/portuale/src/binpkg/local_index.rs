//! The local PKGDIR index (`Packages`).

use crate::binpkg::*;

/// The parsed `<pkgdir>/Packages` pool one `$PKGDIR` file scan
/// vouches files against: the basename lookup, the stanzas behind it,
/// and which stanzas the walk consumed (the leftovers are the
/// removed-file candidates for the stale-index re-injection, backlog
/// #187). One struct so the per-file scan stays under the
/// argument-count lint.
pub(crate) struct IndexVouch<'a> {
    /// `PATH` basename -> stanza indices into `stanzas`.
    pub(crate) by_basename: HashMap<&'a str, Vec<usize>>,
    /// The parsed `<pkgdir>/Packages` stanzas.
    pub(crate) stanzas: &'a [HashMap<String, String>],
    /// Stanza indices the walk consumed, parallel to `stanzas`.
    pub(crate) vouched: &'a mut [bool],
}

/// Real `bintree._populate_local`'s own behavior, both halves: walk
/// `pkgdir` for binpkg *files* and synthesize one `Packages`-style
/// entry per file from its own embedded metadata
/// (`read_xpak_metadata` / `read_gpkg_metadata`), fast-pathed
/// against any already-parsed `<pkgdir>/Packages` entries first -- real
/// `bintree.py:1108-1136`'s own "Validate data from the package index
/// and try to avoid reading the xpak if possible" comment names this
/// exact optimization. A candidate entry (matched by basename) whose
/// own `_mtime_`/`SIZE` still agree with the live file's `stat(2)` and
/// which carries real's own `minimum_keys`
/// ([`PKGINDEX_MINIMUM_KEYS`]) is
/// reused verbatim -- its `PATH` is refreshed in case the file moved,
/// matching real's own `if oldpath != mypath: d["PATH"] = mypath`.
/// A basename-matched entry whose `_mtime_`/`SIZE` no longer agree is
/// *still* reused verbatim (backlog #174): that is real's own default,
/// `FEATURES=pkgdir-index-trusted` (in real `make.globals`' own default
/// `FEATURES`, `cnf/make.globals:81`), under which `_populate_local`
/// runs with `reindex=False`, never stats or reads the file at scan,
/// and leaves size/digest verification to merge-time `BinpkgVerifier`
/// -- so only `CPV` is required of the vouched stanza, not the
/// reindex fast path's `minimum_keys`.
/// Only a file no stanza vouches for is freshly parsed via
/// `read_xpak_metadata`/`read_gpkg_metadata`, and gets a fresh
/// `_mtime_`/`SIZE` recorded on the returned entry so a *later* call can
/// hit the fast path. This real revalidation replaces portuale's own
/// former "present `Packages` is always trusted outright, no scan at
/// all" stance (`portage_repo::read_packages_index`'s own doc comment
/// used to describe that as portuale's long-standing default) -- the
/// caller (`pretend.rs`'s own `--usepkg` CLI-boundary setup) now uses
/// this function unconditionally, `Packages` present or not.
///
/// `$PKGDIR` layout is `<pkgdir>/<category>/<pf>.{tbz2,gpkg.tar}` for a
/// single instance, and -- under real `FEATURES=binpkg-multi-instance`
/// -- `<pkgdir>/<category>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}` (real
/// `bintree._allocate_filename_multi`: a `<cat>/<pn>` subdir, a
/// `-<build_id>` suffix, and the `.xpak` extension for the xpak format).
/// A `.xpak` file is byte-format-identical to a `.tbz2`
/// (`[image tarball][XPAK …STOP]` -- real `bin/misc-functions.sh` builds
/// the same archive either way), so `read_xpak_metadata` reads it
/// unchanged; the multi-instance `BUILD_ID` comes from the filename,
/// validated against the archive's own embedded `PF`. `CPV` is
/// `<category>/<pf>` (the `<pf>` from the embedded metadata for a
/// multi-instance file, from the filename otherwise), `SIZE` from the
/// file's own byte size (real `bintree`'s own `st_size`), `REPO` from
/// the embedded `repository`, `PATH` from the relative path. Entries are
/// `CPV`-sorted for a deterministic pool order.
/// `trust_index` is `populate_local_pkgdir`'s own flag: when false
/// (`FEATURES=-pkgdir-index-trusted`) the stale-vouch arm below is
/// skipped and a changed file falls through to the fresh parse, real
/// `_populate_local(reindex=True)` (`bintree.py:1179-1351`).
///
/// A file the reader rejects is split exactly like real's own caller
/// (`bintree.py:1185-1199`): an `Invalid` file (real's
/// `PortagePackageException`/`SignatureException` family -- see
/// [`BinpkgError`]) is reported with real's own
/// `!!! Invalid binary package: '<path>', <reason>` (noiselevel=-1, so
/// unconditional) and skipped; a `Fatal` one stops the whole scan, as
/// real's uncaught `KeyError`/`AttributeError`/`RecursionError` would.
/// Portuale's reader messages embed the path already (the merge path's
/// callers rely on that), so the reported reason strips the leading
/// `'<path>: '` to keep real's one-path shape.
///
/// A `<pkgdir>/Packages` stanza no file on disk answers to (the file
/// was removed without refreshing the index) is re-injected verbatim
/// (backlog #187): that is real's own default too --
/// `FEATURES=pkgdir-index-trusted` runs `_populate_local` with
/// `reindex=False`, which `cpv_inject`s every stanza without stating
/// any file (`bintree.py:1057-1064`) and leaves the failure to
/// merge-time `BinpkgVerifier` -- so the resolver still selects the
/// binary and the merge aborts with real's `!!! Tried to use
/// non-existent binary` pair instead of silently falling back to the
/// ebuild. Only stanzas whose `PATH` names no live file inject (a
/// present file was already consumed by the walk above, so every
/// existing-file behavior is unchanged), and only stanzas carrying a
/// `CPV` (without one there is no candidate to select; real's own
/// `_pkg_str` needs it too).
///
/// `trust_index` selects real's two `_populate_local` modes (backlog
/// #199, `bintree.py:936-938`'s own `reindex =
/// "pkgdir-index-trusted" not in features or force_reindex`): `true`
/// is the default (`FEATURES=pkgdir-index-trusted`, in real
/// `make.globals`' own default `FEATURES`) -- the stale-vouch arm
/// above and the orphan re-injection below stay on, exactly the
/// long-standing behavior. `false` is
/// `FEATURES=-pkgdir-index-trusted` (`reindex=True`): only the
/// `_mtime_`/`SIZE` fast path is trusted, a file whose stat fields no
/// longer agree is re-parsed from disk (`bintree.py:1179-1351`), and a
/// stanza with no live file is dropped (`bintree.py:1353-1356`).
/// Files no stanza covers are parsed from disk in both modes (real's
/// reindex behavior for them; in trusted mode real never walks at all,
/// but this scan always walks so a `Packages`-less `$PKGDIR` still
/// resolves -- the pre-#199 behavior this flag must not change).
/// Real rewrites a changed `Packages` back under lock
/// (`populate`'s own second `_populate_local()` +
/// `_pkgindex_write`, only when a file was added/changed/moved);
/// portuale never writes it back (recomputed each run -- see
/// `Config::scanned_binpkgs`), so the flag only shapes the pool.
///
/// v1 cut: the old flat `<pkgdir>/All/<pf>.tbz2` layout (real's own
/// `mydir != "All"` fallback) is not walked. A *misnamed* multi-instance
/// file (one whose `<pf>-<id>` stem disagrees with its embedded `PF`, or
/// whose subdir isn't `<pn>`) is skipped, matching real's own
/// `invalid_name`/`name_split` `continue`.
pub fn populate_local_pkgdir(
    pkgdir: &Path,
    trust_index: bool,
) -> Result<Vec<HashMap<String, String>>, String> {
    let existing = portage_repo::read_packages_index(pkgdir);
    let mut by_basename: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, e) in existing.iter().enumerate() {
        if let Some(path) = e.get("PATH")
            && let Some(basename) = Path::new(path).file_name().and_then(|n| n.to_str())
        {
            by_basename.entry(basename).or_default().push(i);
        }
    }
    // Which `existing` stanzas the file walk consumes (fast-path or
    // index-vouched reuse below); the rest are orphan candidates for
    // the stale-index re-injection.
    let mut vouched = vec![false; existing.len()];
    let mut vouch = IndexVouch {
        by_basename,
        stanzas: &existing,
        vouched: &mut vouched,
    };

    // Real's own caller (`bintree.py:1185-1199`): `Invalid` files are
    // reported with real's exact message shape and skipped; `Fatal` ones
    // propagate. The reader messages already carry `'<path>: '` (the
    // merge path reads them that way), so strip that prefix for the
    // warning -- real's `<e>` is the bare exception text.
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
                // A `<cat>/<pn>` multi-instance package dir (real
                // `bintree._allocate_filename_multi`): its files are
                // `<pf>-<build_id>.{xpak,gpkg.tar}`.
                for mfile in read_dir_sorted(&entry)? {
                    let Some(mname) = mfile.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    let path_field = format!("{category}/{name}/{mname}");
                    let Some(e) = record(
                        &mfile,
                        scan_binpkg_file(
                            &mfile,
                            mname,
                            category,
                            path_field,
                            &mut vouch,
                            true,
                            trust_index,
                        ),
                    )?
                    else {
                        continue;
                    };
                    // Real's `tuple(catsplit(mydir)) == name_split[:2]`:
                    // the containing subdir must be this package's own
                    // `<pn>` (from its now-known `CPV`).
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
            if let Some(e) = record(
                &entry,
                scan_binpkg_file(
                    &entry,
                    name,
                    category,
                    path_field,
                    &mut vouch,
                    false,
                    trust_index,
                ),
            )? {
                out.push(e);
            }
        }
    }
    // Backlog #187 -- the stale-index half of real's `reindex=False`
    // (see this function's own doc comment): every never-vouched
    // stanza whose `PATH` names no live file is a removed binpkg real
    // still selects, so it rejoins the pool verbatim here instead of
    // vanishing into an ebuild fallback. Under
    // `FEATURES=-pkgdir-index-trusted` (backlog #199, real
    // `_populate_local(reindex=True)`, `bintree.py:1353-1356`) such a
    // stanza is dropped instead -- the whole arm is skipped.
    if trust_index {
        for (i, stanza) in existing.iter().enumerate() {
            if vouch.vouched[i] {
                continue;
            }
            let Some(cpv) = stanza.get("CPV").filter(|c| !c.is_empty()) else {
                continue;
            };
            // Real defaults a PATH-less stanza to `<cpv>.tbz2`
            // (`bintree.py:1047-1055`); either way, a live file there was
            // already consumed by the walk above, so only the missing half
            // injects -- every existing-file behavior is unchanged.
            let rel = stanza
                .get("PATH")
                .filter(|p| !p.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("{cpv}.tbz2"));
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

/// Real `_populate_local`'s own `minimum_keys`
/// (`bintree.py:1018`: `self._pkgindex_keys.difference(self._pkgindex_hashes)`):
/// the `_aux_cache_keys` set (`bintree.py:96-120`) plus `CPV`/`SIZE`
/// (`bintree.py:550-551`), minus the `_pkgindex_hashes` digests
/// (`MD5`, `SHA1` -- `bintree.py:548`; real never computes digests at
/// scan, `bintree.py:1014-1017`, so a stanza missing only digests must
/// still hit the fast path). `MD5` is the one hash that is also an aux
/// key, hence the only name dropped from the aux list; `SHA1` was
/// never in it. `CPV`/`SIZE`/`SLOT`/`_mtime_` ride along (real checks
/// presence of the whole set, values only for `_mtime_`/`SIZE`).
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

/// Real `bintree.py:1124`'s own `if not minimum_keys.difference(d)`:
/// every minimum key must be present on the stanza (presence only --
/// real compares values solely for `_mtime_`/`SIZE`).
pub(crate) fn stanza_has_minimum_keys(stanza: &HashMap<String, String>) -> bool {
    PKGINDEX_MINIMUM_KEYS
        .iter()
        .all(|key| stanza.contains_key(*key))
}

/// One `$PKGDIR` binpkg file -> its synthesized `Packages`-style entry,
/// or `None` if it isn't a binpkg or is a misnamed multi-instance file
/// (real's own `invalid_name`/`name_split` `continue`). Reuses an
/// unchanged `<pkgdir>/Packages` entry (matched by basename, with
/// `_mtime_` and `SIZE` still agreeing -- real's "avoid reading the
/// xpak if possible") when one exists, else parses the archive's own
/// embedded metadata. The fast path additionally requires real's own
/// `minimum_keys` (`bintree.py:1014-1018`: `_pkgindex_keys`, i.e. the
/// `_aux_cache_keys` of `bintree.py:96-120` plus `CPV`/`SIZE`
/// (`bintree.py:548-551`), minus the `_pkgindex_hashes` digests, all
/// present on the stanza -- `bintree.py:1124`'s own `if not
/// minimum_keys.difference(d)`), so a stat-matching but key-incomplete
/// stanza falls through to the fresh parse under
/// `FEATURES=-pkgdir-index-trusted`, exactly like real. In trusted
/// mode the same miss falls through to the stale-vouch arm below,
/// which reuses the stanza verbatim -- so the added check cannot
/// change trusted-mode output. `vouch` carries the index pool and records each
/// consumed stanza (see [`IndexVouch`]). `multi_instance` selects the
/// naming contract: when set, the
/// file is `<pf>-<build_id>.{xpak,gpkg.tar}` with `PF` taken from the
/// archive and `BUILD_ID` from the filename; when clear, it is
/// `<pf>.{tbz2,gpkg.tar}`.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn scan_binpkg_file(
    file: &Path,
    basename: &str,
    category: &str,
    path_field: String,
    vouch: &mut IndexVouch<'_>,
    multi_instance: bool,
    trust_index: bool,
) -> Result<Option<HashMap<String, String>>, BinpkgError> {
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

    // Real's own fast-path gate (`bintree.py:1115-1126`): the live
    // `lstat(2)` mtime seconds and byte size must agree with the
    // stanza's `_mtime_`/`SIZE`, and the stanza must carry every one
    // of real's `minimum_keys` -- so a stat-matching but
    // key-incomplete stanza is re-parsed, not vouched.
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

    // Real's own default (`FEATURES=pkgdir-index-trusted` is in real
    // `make.globals`' own default `FEATURES`, `cnf/make.globals:81`):
    // `_populate_local(reindex=False)` never walks `$PKGDIR` at all --
    // every `<pkgdir>/Packages` stanza is injected verbatim and the
    // file's own bytes are first touched at merge, by `BinpkgVerifier`
    // (backlog #174). So an index stanza for this basename vouches for
    // the file even when its `_mtime_`/`SIZE` no longer agree with the
    // live file (a truncated or otherwise corrupted binpkg): reuse it
    // verbatim -- stale `SIZE`/digests included, since the merge-time
    // verification (`verify_binpkg_against_index`) checks the file
    // against exactly those index values -- instead of re-parsing the
    // container and pre-rejecting it here with `!!! Invalid binary
    // package`. Only `CPV` is required of the stanza (real injects
    // stanzas unconditionally on this path -- notably, a
    // portuale-written `--buildpkg` stanza may carry no `SLOT`), and the
    // stanza must belong to this file's own category (`CPV` starts with
    // `{category}/`): the same `PF` can exist in two categories with the
    // same basename, and the first same-basename stanza would vouch for
    // the wrong file. A file no stanza vouches for keeps the reindex
    // behavior: parse the container now, rejecting it at scan when its
    // metadata can't be read (real's own `PortagePackageException` /
    // `SignatureException` arm, `bintree.py:1185-1199`). With
    // `trust_index == false` (`FEATURES=-pkgdir-index-trusted`, backlog
    // #199) this whole arm is off: real `_populate_local(reindex=True)`
    // re-reads any file the fast path above did not match
    // (`bintree.py:1179-1351`).
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

    // Stale, moved, or unindexed -- re-derive from the file itself. A
    // `.xpak` file is byte-format-identical to a `.tbz2`, so
    // `read_xpak_metadata` reads both.
    let mut meta = if is_gpkg {
        read_gpkg_metadata(file)?
    } else {
        read_xpak_metadata(file)?
    };
    let stem = basename.strip_suffix(ext).unwrap();
    let embedded_pf = meta.get("PF").filter(|p| !p.is_empty()).cloned();

    let pf = if multi_instance {
        // The archive's own embedded `PF` is the ground truth; the
        // filename must be exactly `<PF>-<build_id>` (real
        // `_parse_build_id` + `myfile != f"{mypf}-{build_id}.<ext>"`).
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
        // Real's `myfile != mypf + ".<ext>"` -> `invalid_name`: a
        // category-level file whose stem disagrees with the archive's
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
