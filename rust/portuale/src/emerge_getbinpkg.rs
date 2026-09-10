// Real `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` execution,
// WITHOUT `--pretend`: refresh each remote binhost's live index, resolve
// the graph, then merge every resolved entry -- dispatching per entry on
// its `source` (a `Binary` candidate is downloaded+merged, anything else
// is built+merged from source).
//
// The `--pretend` half of `--getbinpkg`/`--getbinpkgonly` already shipped
// (real `bintree`'s `binrepos.conf`/`PORTAGE_BINHOST` parsing, remote
// binhost candidates from each binhost's *cached* `Packages` index, the
// `g` bracket column). This module is the other half: the live index
// refresh + the file download + the merge.
//
//   - `refresh_binhost_indexes`: real `bintree._populate_remote`, for
//     `http(s)` binhosts -- `wget <sync_uri>/Packages` into the same
//     `<EROOT>/var/cache/edb/binhost/<host>/<path>/Packages` cache
//     location `list_remote_binary_candidates` reads. A `file://` binhost
//     needs no refresh (its `packages_dir` IS the source). Run BEFORE
//     resolution, so the resolver sees the fresh pool.
//   - `run_merge_plan`: iterate the resolved entries (already in real
//     topological merge order), and per entry -- `Binary` ->
//     `merge_one_binary_entry` (find its `Packages` record via
//     `find_remote_binpkg`, `wget`/copy into `$PKGDIR`, `SIZE`-check,
//     `ebuild_merge::merge_binpkg`); else ->
//     `emerge_build::merge_one_source_entry`. Real `--getbinpkg`'s own
//     "prefer a binary, fall back to source" is the resolver's job, not
//     this loop's; `--getbinpkgonly` (binary-only resolve) simply never
//     produces a non-`Binary` entry.
//
// v1 cuts specific to this module:
//   - `Packages.gz` / `Packages.zst` (a compressed remote index) ARE
//     tried now (`refresh_binhost_indexes` fetches `Packages.<ext>` and
//     pipes it through `gzip -dc` / `zstd -dc` into the plain `Packages`
//     cache file), with the plain `Packages` as the fallback. Not
//     modelled: `Packages.bz2`/`.lz4` and real portage's exact
//     preference ordering.
//   - live `layout.conf` negotiation (`binpkg-multi-instance`, path
//     layout) is not done -- the index `PATH` field (or the default
//     `<cat>/<pf>.tbz2`) is trusted outright, same "trust the index"
//     stance the `--pretend` half already takes.
//   - digest verification checks `SIZE` and the `Packages` record's
//     `MD5` and `SHA1` (the md5/sha1 of the whole `.tbz2`, real
//     `bintree`'s own fields, `_pkgindex_hashes = ["MD5", "SHA1"]`).
//     Each field present is verified (real `_get_digests` collects every
//     valid checksum key present, `digestCheck` verifies them all). A downloaded gpkg
//     additionally has its internal `Manifest` (`DATA` BLAKE2B/SHA512
//     lines) *and* its GPG signature layer (detached `.sig` sidecars +
//     clear-signed `Manifest`, real `_verify_binpkg` -- see
//     `binpkg::GpgVerify`) verified at merge time
//     (`binpkg::extract_binpkg` -> `verify_gpkg_manifest`).

use crate::ebuild_merge::{self, MergeOptions};
use mrg_director::MergeEngine;
use portage_profile::{BinRepo, Config};
use portage_repo::{GraphEntry, PretendOutcome, RepoConfig, find_remote_binpkg};
use std::path::Path;

/// Real `bintree._populate_remote`: for each `http(s)` binrepo, download
/// its live `Packages` index into the local edb cache
/// (`BinRepo::packages_dir`). A `file://` binrepo is left as-is. Failures
/// are surfaced (a `--getbinpkgonly` run with an unreachable binhost
/// should say so, not silently resolve against a stale/empty pool).
pub fn refresh_binhost_indexes(binrepos: &[BinRepo], root: &Path) -> Result<(), String> {
    for binrepo in binrepos {
        let uri = binrepo.sync_uri.trim_end_matches('/');
        if uri.starts_with("file://") {
            continue;
        }
        let cache_dir = binrepo.packages_dir(root);
        std::fs::create_dir_all(&cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
        let dest = cache_dir.join("Packages");

        // Real `bintree._populate_remote` prefers a compressed index when
        // the binhost serves one (`Packages.gz` / `Packages.zst`),
        // decompressing it into the same plain `Packages` cache file
        // `list_remote_binary_candidates` reads. Fall back to the plain
        // `Packages` if neither compressed form is there.
        let mut got = false;
        for (ext, tool) in [("gz", "gzip"), ("zst", "zstd")] {
            let compressed = cache_dir.join(format!("Packages.{ext}"));
            if crate::fetch::wget_fetch(&format!("{uri}/Packages.{ext}"), &compressed).is_ok() {
                let out =
                    std::fs::File::create(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                let status = std::process::Command::new(tool)
                    .arg("-dc")
                    .arg(&compressed)
                    .stdout(std::process::Stdio::from(out))
                    .status()
                    .map_err(|e| format!("binhost {uri}: spawning {tool}: {e}"))?;
                let _ = std::fs::remove_file(&compressed);
                if !status.success() {
                    return Err(format!("binhost {uri}: {tool} -dc Packages.{ext} failed"));
                }
                got = true;
                break;
            }
        }
        if !got {
            crate::fetch::wget_fetch(&format!("{uri}/Packages"), &dest)
                .map_err(|e| format!("binhost {uri}: {e}"))?;
        }
    }
    Ok(())
}

/// Real `emerge --getbinpkg <atom>` / `--getbinpkgonly <atom>` (no
/// `--pretend`): merge every resolved entry (already in real dependency-
/// first merge order), dispatching **per entry** on `entry.source` --
/// real `--getbinpkg`'s own "prefer a binary package, fall back to a
/// source build" is entirely the resolver's job (it already stamped the
/// right `source` on each `GraphEntry`); this just executes the plan.
///
///   - a `Binary` entry -> download it (if remote) and merge it
///     (`merge_one_binary_entry` -> `ebuild_merge::merge_binpkg`, all
///     four `pkg_*` hooks + same-slot replace).
///   - anything else -> build + merge from source
///     (`emerge_build::merge_one_source_entry` -> `ebuild_merge::
///     run_merge`).
///   - `AlreadyInstalled` is a silent no-op either way.
///
/// `--getbinpkgonly` (binary-only resolve, `usepkgonly`) simply never
/// yields a non-`Binary` entry, so the same function serves both.
#[allow(clippy::too_many_arguments)]
pub fn run_merge_plan(
    entries: &[GraphEntry],
    config: &Config,
    repos: &[RepoConfig],
    root: &Path,
    pkgdir: &Path,
    portage_tmpdir: &Path,
    merge_options: &MergeOptions,
    keep_going: bool,
    buildpkg: Option<&crate::ebuild_package::PackageOptions>,
    buildpkg_exclude: &[String],
) -> Result<(), String> {
    crate::emerge_build::run_merge_loop(entries, keep_going, |entry| {
        // The director seam executes every unit: derive the entry's
        // `MergeUnit` and dispatch on its kind through the real source /
        // binary engines (real `MergeListItem._start`'s own `type_name`
        // routing). An entry with nothing to merge (`AlreadyInstalled` /
        // `NoVisibleCandidate`) stays a silent no-op, exactly the merge
        // functions' own early return.
        let Some(unit) = crate::merge_engines::merge_unit_for_entry(entry, root) else {
            return Ok(());
        };
        let ctx = mrg_director::MergeContext {
            root: root.to_path_buf(),
            builddir: portage_tmpdir.to_path_buf(),
            jobs: 1,
            keep_going,
        };
        let outcome = match unit.kind {
            mrg_director::MergeKind::Source => {
                let bp = buildpkg.filter(|opts| {
                    crate::emerge_build::entry_buildpkg_wanted(
                        entry,
                        repos,
                        buildpkg_exclude,
                        opts.buildpkg_live,
                    )
                });
                crate::merge_engines::SourceEngine {
                    repos,
                    root,
                    portage_tmpdir,
                    options: merge_options,
                    buildpkg: bp,
                    buildpkg_exclude,
                }
                .execute(&unit, &ctx)
            }
            mrg_director::MergeKind::Binary => crate::merge_engines::BinaryEngine {
                config,
                root,
                pkgdir,
                portage_tmpdir,
                options: merge_options,
            }
            .execute(&unit, &ctx),
        };
        match outcome {
            mrg_director::MergeOutcome::Merged | mrg_director::MergeOutcome::Skipped(_) => Ok(()),
            mrg_director::MergeOutcome::Failed(e) => Err(e),
        }
    })
}

/// One `Binary` entry of `run_merge_plan`'s own loop: `AlreadyInstalled`
/// is a silent no-op; `New`/`Upgrade`/`Downgrade`/`Reinstall` are
/// fetched (remote) or located (`$PKGDIR`) and merged (`merge_binpkg`
/// unmerges a replaced same-slot version itself).
pub(crate) fn merge_one_binary_entry(
    entry: &GraphEntry,
    config: &Config,
    root: &Path,
    pkgdir: &Path,
    portage_tmpdir: &Path,
    merge_options: &MergeOptions,
) -> Result<(), String> {
    let cp = format!("{}/{}", entry.category, entry.package);
    let version = match &entry.outcome {
        PretendOutcome::AlreadyInstalled { .. } => return Ok(()),
        PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
            version.clone()
        }
        PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
        PretendOutcome::NoVisibleCandidate => {
            return Err(format!("no binary package available for {cp}"));
        }
    };

    let local = resolve_local_binpkg(
        pkgdir,
        &entry.category,
        &entry.package,
        &version,
        entry.build_id.as_deref(),
    );
    // A `$PKGDIR` file already on disk wins; otherwise only a *remote*
    // entry (`entry.remote_binary`, set by the resolver for a
    // binhost-sourced candidate) may fetch from a binhost. A resumed
    // entry always lands here with `remote_binary: false`
    // (`resume_entry` records only `cat/pkg-ver`, so "was this fetched
    // remotely" is not re-derived) -- and real replays a resumed
    // binary against its *local* bintree, matched by the recorded
    // `mtimedb["resume"]["binpkgs"]` build metadata, never by
    // re-hitting the binhost. A resumed binary that was never
    // downloaded therefore fails here (real: `PackageNotFound`, "An
    // expected package is not available") instead of silently fetching
    // a possibly-different file. The same holds for a fresh
    // local-`$PKGDIR` entry: its fetch already happened, so a missing
    // file is an error, not a refetch.
    let binpkg_path = match local {
        Some(path) => path,
        None if entry.remote_binary => {
            let (sync_uri, record) = find_remote_binpkg(
                &config.binrepos,
                root,
                &entry.category,
                &entry.package,
                &version,
            )
            .ok_or_else(|| {
                format!(
                    "{cp}-{version}: no binpkg file under {} and not in any binhost `Packages` index",
                    pkgdir.display()
                )
            })?;
            download_and_verify(
                &sync_uri,
                &record,
                &entry.category,
                &entry.package,
                &version,
                pkgdir,
            )?
        }
        None => {
            return Err(format!(
                "{cp}-{version}: no binpkg file under {}",
                pkgdir.display()
            ));
        }
    };

    println!(">>> Merging binary package {cp}-{version}...");
    let status = ebuild_merge::merge_binpkg(&binpkg_path, root, portage_tmpdir, merge_options)?;
    if status != 0 {
        return Err(format!("{cp}-{version}: binpkg merge failed ({status})"));
    }
    Ok(())
}

/// The on-disk binpkg for `<cat>/<package>-<version>` in `$PKGDIR`.
///
/// Two real naming contracts (real `bintree.getname` /
/// `_allocate_filename` vs `_allocate_filename_multi`):
///  - single instance: `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`
///  - `FEATURES=binpkg-multi-instance`: `<pkgdir>/<cat>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}`
///    -- a `<pn>/` subdir and a `-<build_id>` suffix.
///
/// A `build_id` (from the resolved candidate / `Packages` index) picks
/// the exact multi-instance file; without one, the `<pn>/` subdir is
/// still scanned for a `<pf>-<id>` file (an older index with no
/// `BUILD_ID` field), preferring the highest `<id>`.
pub(crate) fn resolve_local_binpkg(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> Option<std::path::PathBuf> {
    let pf = format!("{package}-{version}");

    // Single-instance layout.
    for ext in ["tbz2", "gpkg.tar"] {
        let p = pkgdir.join(category).join(format!("{pf}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }

    // Multi-instance layout: `<cat>/<pn>/<pf>-<build_id>.<ext>`.
    let instance_dir = pkgdir.join(category).join(package);
    if let Some(build_id) = build_id.filter(|s| !s.is_empty()) {
        for ext in ["gpkg.tar", "xpak"] {
            let p = instance_dir.join(format!("{pf}-{build_id}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // Fallback: any `<pf>-<id>.{gpkg.tar,xpak}` in the instance dir,
    // highest numeric `<id>` first.
    let mut candidates: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(&instance_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name
                .strip_suffix(".gpkg.tar")
                .or_else(|| name.strip_suffix(".xpak"))?;
            let id = stem.strip_prefix(&format!("{pf}-"))?;
            Some((id.parse::<u64>().ok()?, e.path()))
        })
        .collect();
    candidates.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    candidates.into_iter().map(|(_, p)| p).next()
}

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into
/// `$PKGDIR`, then verify it against the index `SIZE` and, if present,
/// the `MD5` / `SHA1` fields. A mismatch removes the file and fails.
pub(crate) fn download_and_verify(
    sync_uri: &str,
    record: &std::collections::HashMap<String, String>,
    category: &str,
    package: &str,
    version: &str,
    pkgdir: &Path,
) -> Result<std::path::PathBuf, String> {
    let rel = record
        .get("PATH")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{category}/{package}-{version}.tbz2"));
    let dest = pkgdir.join(&rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let uri = format!("{}/{rel}", sync_uri.trim_end_matches('/'));

    if let Some(local) = uri.strip_prefix("file://") {
        std::fs::copy(local, &dest).map_err(|e| format!("{local}: {e}"))?;
    } else {
        crate::fetch::wget_fetch(&uri, &dest)?;
    }

    if let Some(expected) = record.get("SIZE").and_then(|s| s.parse::<u64>().ok()) {
        let actual = std::fs::metadata(&dest)
            .map_err(|e| format!("{}: {e}", dest.display()))?
            .len();
        if actual != expected {
            let _ = std::fs::remove_file(&dest);
            return Err(format!(
                "{}: downloaded size {actual} != index SIZE {expected}",
                dest.display()
            ));
        }
    }

    // Real `bintree.gettbz2` / `_get_digests` + `digestCheck`: the
    // `Packages` record's `MD5`/`SHA1` fields are the md5/sha1 of the
    // whole `.tbz2`, and real verifies every digest present. Same here --
    // each present field is checked; a mismatch removes the file and
    // fails.
    let want_md5 = record.get("MD5").filter(|s| !s.is_empty()).cloned();
    let want_sha1 = record.get("SHA1").filter(|s| !s.is_empty()).cloned();
    if want_md5.is_some() || want_sha1.is_some() {
        let bytes = std::fs::read(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        if let Some(expected_md5) = want_md5 {
            use md5::Digest as _;
            let actual: String = md5::Md5::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(&expected_md5) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: MD5 mismatch (index {expected_md5}, got {actual})",
                    dest.display()
                ));
            }
        }
        if let Some(expected_sha1) = want_sha1 {
            use sha1::Digest as _;
            let actual: String = sha1::Sha1::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(&expected_sha1) {
                let _ = std::fs::remove_file(&dest);
                return Err(format!(
                    "{}: SHA1 mismatch (index {expected_sha1}, got {actual})",
                    dest.display()
                ));
            }
        }
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_repo::CandidateSource;
    use portage_repo::PretendOutcome;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fixtures_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "portuale-getbinpkg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Serves each `routes` entry (`"/path" -> body`) over real plain
    /// HTTP on `127.0.0.1`, one response per connection, for `requests`
    /// connections total -- enough for the `Packages` fetch + N binpkg
    /// fetches a `--getbinpkg` run makes (each `wget` uses its own
    /// `Connection: close`).
    fn serve(
        routes: HashMap<String, Vec<u8>>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = match routes.get(&path) {
                    Some(b) => ("200 OK", b.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    fn packages_index(entries: &[&str]) -> Vec<u8> {
        let mut s = format!("TIMESTAMP: 0\nPACKAGES: {}\n\n", entries.len());
        for e in entries {
            s.push_str(e);
            s.push_str("\n\n");
        }
        s.into_bytes()
    }

    fn graph_entry(package: &str, source: CandidateSource, version: &str) -> GraphEntry {
        GraphEntry {
            category: "dev-libs".into(),
            package: package.into(),
            outcome: PretendOutcome::New {
                version: version.into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("testrepo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    #[test]
    fn resolve_local_binpkg_finds_the_multi_instance_layout() {
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");

        // Single-instance: `<cat>/<pf>.gpkg.tar`.
        std::fs::create_dir_all(pkgdir.join("media-fonts")).unwrap();
        let single = pkgdir.join("media-fonts/plain-1.gpkg.tar");
        std::fs::write(&single, b"x").unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "plain", "1", None).as_deref(),
            Some(single.as_path())
        );

        // Multi-instance: `<cat>/<pn>/<pf>-<build_id>.gpkg.tar` (real
        // `_allocate_filename_multi`). This is the `noto-20260901-1.gpkg.tar`
        // layout `resolve_local_binpkg` used to miss entirely.
        std::fs::create_dir_all(pkgdir.join("media-fonts/noto")).unwrap();
        let mi = pkgdir.join("media-fonts/noto/noto-20260901-1.gpkg.tar");
        std::fs::write(&mi, b"x").unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", Some("1")).as_deref(),
            Some(mi.as_path()),
            "exact build_id"
        );
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", None).as_deref(),
            Some(mi.as_path()),
            "no build_id -> scan the <pn>/ subdir"
        );

        // Highest build_id wins when several instances are present.
        std::fs::write(
            pkgdir.join("media-fonts/noto/noto-20260901-4.gpkg.tar"),
            b"x",
        )
        .unwrap();
        assert_eq!(
            resolve_local_binpkg(&pkgdir, "media-fonts", "noto", "20260901", None)
                .unwrap()
                .file_name()
                .unwrap(),
            "noto-20260901-4.gpkg.tar"
        );

        assert!(resolve_local_binpkg(&pkgdir, "media-fonts", "absent", "1", Some("1")).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_one_binary_entry_merges_a_multi_instance_local_gpkg() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        // The real gpkg fixture, placed in the multi-instance layout.
        std::fs::create_dir_all(pkgdir.join("dev-libs/gpkgreadpkg")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
            pkgdir.join("dev-libs/gpkgreadpkg/gpkgreadpkg-1.0-1.gpkg.tar"),
        )
        .unwrap();

        let mut entry = graph_entry("gpkgreadpkg", CandidateSource::Binary, "1.0");
        entry.build_id = Some("1".into());

        merge_one_binary_entry(
            &entry,
            &Config::default(),
            &root,
            &pkgdir,
            &tmp.join("pt"),
            &MergeOptions::default(),
        )
        .expect("multi-instance local gpkg merges");

        assert!(
            root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0/CONTENTS")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A writable copy of portage's own committed GnuPG test keyring
    /// (see `binpkg.rs`'s own `test_gpg_home` doc comment), for the
    /// merge-time signature tests below. `gpg` refuses a homedir it
    /// doesn't own outright, so the committed tree itself is never used
    /// directly.
    fn test_gpg_home() -> std::path::PathBuf {
        fn copy_dir(src: &std::path::Path, dest: &std::path::Path) {
            std::fs::create_dir_all(dest).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let to = dest.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &to);
                } else {
                    std::fs::copy(entry.path(), &to).unwrap();
                }
            }
        }
        let dest = tempdir().join("gpg-home");
        copy_dir(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../3rdparty/portage/lib/portage/tests/.gnupg"),
            &dest,
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700)).unwrap();
        dest
    }

    fn test_gpg_verify(home: &std::path::Path) -> crate::binpkg::GpgVerify {
        crate::binpkg::GpgVerify {
            verify_signature: true,
            request_signature: false,
            base_command: crate::binpkg::DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string(),
            gpg_home: home.display().to_string(),
        }
    }

    #[test]
    fn merge_binpkg_verifies_a_signed_gpkg_against_the_test_keyring() {
        // The committed signed fixture (`binpkg.rs`'s own verify tests
        // cover the container layer) merges end to end with the test
        // keyring: signature policy enforced, image + metadata land in
        // the vdb like any other binpkg merge.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let home = test_gpg_home();
        let options = MergeOptions {
            gpg_verify: test_gpg_verify(&home),
            ..MergeOptions::default()
        };

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar"),
            &root,
            &tmp.join("portage_tmpdir"),
            &options,
        )
        .expect("signed gpkg merge succeeds");
        assert_eq!(status, 0);

        let vdb = root.join("var/db/pkg/dev-libs/gpgsignedpkg-1.0");
        assert!(vdb.join("CONTENTS").is_file());
        assert!(
            std::fs::read_to_string(vdb.join("CONTENTS"))
                .unwrap()
                .contains(" /hello.txt "),
            "the image file merged"
        );
        assert!(root.join("hello.txt").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_rejects_a_tampered_signed_gpkg() {
        // Flip a byte of the signed container's `image.tar.gz` (via an
        // unpack-mutate-repack, so the tar framing stays parseable):
        // the merge must fail on the detached `.sig` before anything
        // is unpacked -- no vdb entry, no image file under ROOT.
        let tmp = tempdir();
        let outer = tmp.join("outer");
        std::fs::create_dir_all(&outer).unwrap();
        let src = fixtures_root().join("pkgdir/dev-libs/gpgsignedpkg-1.0.gpkg.tar");
        let status = std::process::Command::new("tar")
            .args(["-xf"])
            .arg(&src)
            .args(["-C"])
            .arg(&outer)
            .status()
            .unwrap();
        assert!(status.success());
        let member = outer.join("gpgsignedpkg-1.0/image.tar.gz");
        let mut bytes = std::fs::read(&member).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;
        std::fs::write(&member, bytes).unwrap();
        let tampered = tmp.join("tampered.gpkg.tar");
        let status = std::process::Command::new("tar")
            .args(["-cf"])
            .arg(&tampered)
            .args(["-C"])
            .arg(&outer)
            .arg("gpgsignedpkg-1.0")
            .status()
            .unwrap();
        assert!(status.success());

        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let home = test_gpg_home();
        let options = MergeOptions {
            gpg_verify: test_gpg_verify(&home),
            ..MergeOptions::default()
        };
        let err =
            ebuild_merge::merge_binpkg(&tampered, &root, &tmp.join("portage_tmpdir"), &options)
                .unwrap_err();
        assert!(err.contains("GnuPG verification failed"), "{err}");
        assert!(
            !root.join("var/db/pkg/dev-libs/gpgsignedpkg-1.0").exists(),
            "nothing is merged on a signature failure"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_installs_a_real_tbz2_into_the_vdb() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        // The image file landed under ROOT.
        let hello = root.join("usr/share/packagepkg/hello.txt");
        assert!(hello.is_file(), "{}", hello.display());
        assert!(std::fs::read_to_string(&hello).unwrap().contains("hello"));

        // A real vdb entry, with CONTENTS naming the file and the
        // binpkg's own RDEPEND copied through.
        let vdb = root.join("var/db/pkg/dev-libs/packagepkg-1.0");
        assert!(vdb.join("CONTENTS").is_file());
        assert!(
            std::fs::read_to_string(vdb.join("CONTENTS"))
                .unwrap()
                .contains("/usr/share/packagepkg/hello.txt")
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("RDEPEND")).unwrap().trim(),
            "dev-libs/samepkg"
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("SLOT")).unwrap().trim(),
            "0"
        );
        // The saved env + ebuild are kept in the vdb now (real portage
        // does; portuale needs them for pkg_preinst/pkg_postinst). This
        // fixture's DEFINED_PHASES is `install` only, so no hook ran.
        assert!(vdb.join("environment.bz2").is_file());
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());

        // Real `_consolidate_to_metadata_file`: the vdb entry carries a
        // consolidated `metadata` file -- `#format=1` header, sorted
        // `KEY=value` lines for the per-field files, `#dir_mtime=` last
        // and matching the entry dir's own `st_mtime_ns` (real's reader
        // rejects a stale value, which is what broke `emerge -C`).
        let metadata = std::fs::read_to_string(vdb.join("metadata")).expect("metadata file");
        assert!(metadata.starts_with("#format=1\n"), "{metadata}");
        assert!(metadata.contains("\nSLOT=0\n"), "{metadata}");
        assert!(
            metadata.contains("\nRDEPEND=dev-libs/samepkg\n"),
            "{metadata}"
        );
        let dir_mtime_line = metadata
            .lines()
            .find_map(|l| l.strip_prefix("#dir_mtime="))
            .expect("#dir_mtime= line");
        assert_eq!(
            metadata.lines().last(),
            Some(format!("#dir_mtime={dir_mtime_line}").as_str()),
            "#dir_mtime= must be the final line"
        );
        {
            use std::os::unix::fs::MetadataExt as _;
            let st = std::fs::metadata(&vdb).unwrap();
            let ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
            assert_eq!(
                dir_mtime_line,
                ns.to_string(),
                "recorded #dir_mtime= must equal the entry dir's st_mtime_ns"
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_gpkg_strips_the_image_prefix_and_records_binpkgmd5_and_metadata() {
        // Regression for the broken `/var/db/pkg/<cat>/<pf>` a gpkg merge
        // used to write: `CONTENTS` prefixed with `/image`, no
        // `BINPKGMD5`, no consolidated `metadata` file -- all of which
        // blocked `emerge -C` (real portage's and portuale's).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("gpkg merge succeeds");
        assert_eq!(status, 0);

        let vdb = root.join("var/db/pkg/dev-libs/gpkgreadpkg-1.0");
        let contents = std::fs::read_to_string(vdb.join("CONTENTS")).unwrap();
        assert!(
            !contents.contains("/image"),
            "CONTENTS must not carry the gpkg `image/` prefix: {contents}"
        );
        assert!(contents.contains(" /hello.txt "), "{contents}");
        assert!(
            root.join("hello.txt").is_file(),
            "the image file merged at the real path, not under /image"
        );

        // Real `_emerge/Binpkg._start_task`: BINPKGMD5 = md5 of the whole
        // binpkg file.
        let want_md5 = {
            use md5::Digest as _;
            let mut h = md5::Md5::new();
            h.update(std::fs::read(&binpkg).unwrap());
            h.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(
            std::fs::read_to_string(vdb.join("BINPKGMD5"))
                .unwrap()
                .trim(),
            want_md5
        );

        // Consolidated metadata file present and stamped.
        let metadata = std::fs::read_to_string(vdb.join("metadata")).expect("metadata file");
        assert!(metadata.starts_with("#format=1\n"), "{metadata}");
        assert!(
            metadata.lines().last().unwrap().starts_with("#dir_mtime="),
            "{metadata}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_collision_protect_aborts_when_another_package_owns_the_file() {
        // Real `dblink.merge()`'s `_collision_protect` -- now shared with
        // the source `merge_after_install`. A pre-existing vdb entry owns
        // `/usr/share/packagepkg/hello.txt`; `FEATURES=collision-protect`
        // must abort the binpkg merge rather than overwrite it.
        let tmp = tempdir();
        let root = tmp.join("root");
        let shared = root.join("usr/share/packagepkg/hello.txt");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, "owned by collideowner\n").unwrap();

        let owner = root.join("var/db/pkg/dev-libs/collideowner-1.0");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(owner.join("SLOT"), "0\n").unwrap();
        std::fs::write(owner.join("PF"), "collideowner-1.0\n").unwrap();
        std::fs::write(owner.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            owner.join("CONTENTS"),
            "obj /usr/share/packagepkg/hello.txt 0000 0\n",
        )
        .unwrap();

        let options = MergeOptions {
            collision_protect: true,
            ..MergeOptions::default()
        };
        let err = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &root,
            &tmp.join("pt"),
            &options,
        )
        .unwrap_err();
        assert!(err.contains("dev-libs/collideowner-1.0"), "{err}");
        assert!(err.contains("/usr/share/packagepkg/hello.txt"), "{err}");
        assert!(err.contains("NOT merged"), "{err}");
        // Nothing was written: the file is untouched, no vdb entry.
        assert_eq!(
            std::fs::read_to_string(&shared).unwrap(),
            "owned by collideowner\n"
        );
        assert!(!root.join("var/db/pkg/dev-libs/packagepkg-1.0").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_runs_pkg_preinst_and_pkg_postinst_from_the_saved_env() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let binpkg = fixtures_root().join("pkgdir/dev-libs/binpkgphasepkg-1.0.tbz2");

        let status = ebuild_merge::merge_binpkg(
            &binpkg,
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0, "both hooks exited 0");

        // The image landed, vdb entry written.
        assert!(root.join("usr/share/binpkgphasepkg/payload.txt").is_file());
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgphasepkg-1.0/CONTENTS")
                .is_file()
        );

        // The fixture's own pkg_preinst `die`s if the payload is already
        // merged and pkg_postinst `die`s if it is not -- so this file
        // existing with both lines proves the real treewalk() ordering
        // (preinst before the copy, postinst after) held.
        let phases = root.join("var/lib/binpkgphasepkg.phases");
        assert_eq!(
            std::fs::read_to_string(&phases).unwrap(),
            "preinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_runs_pkg_setup_then_preinst_then_postinst() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        assert!(root.join("usr/share/binpkgrmpkg/payload-1.0.txt").is_file());
        // Each hook appends its own `<phase>-<PVR>` line, in call order.
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Real `Scheduler._run_pkg_pretend` runs `pkg_pretend` for a binary
    /// package too -- portuale folds it into `merge_binpkg`'s hook chain,
    /// before `pkg_setup`. The fixture's `pkg_pretend` `die`s if its own
    /// payload is already merged, so a `pretend` line landing first (and
    /// the merge still succeeding) proves it ran at the right point.
    #[test]
    fn merge_binpkg_runs_pkg_pretend_before_pkg_setup() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        assert!(
            root.join("usr/share/binpkgpretendpkg/payload.txt")
                .is_file()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgpretendpkg.log")).unwrap(),
            "pretend\nsetup\npreinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// L1-c: real `dblink.treewalk` regenerates the vdb `environment.bz2`
    /// from the *live* merge-time environment (`PORTAGE_UPDATE_ENV`
    /// around the `postinst` phase), so it carries the merge-time
    /// `FEATURES`, not the binpkg's build-time one. `merge_binpkg` runs
    /// that regeneration unconditionally now.
    #[test]
    fn merge_binpkg_regenerates_a_curated_merge_time_vdb_environment() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let opts = MergeOptions {
            features: "mergetimefeat splitdebug".to_string(),
            ..MergeOptions::default()
        };
        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgpretendpkg-1.0.tbz2"),
            &root,
            &tmp.join("portage_tmpdir"),
            &opts,
        )
        .expect("merge succeeds");
        assert_eq!(status, 0);

        let env_bz2 = root.join("var/db/pkg/dev-libs/binpkgpretendpkg-1.0/environment.bz2");
        let out = std::process::Command::new("bzip2")
            .args(["-dc", "--"])
            .arg(&env_bz2)
            .output()
            .expect("bunzip2 the vdb environment");
        let env = String::from_utf8_lossy(&out.stdout);

        assert!(
            env.contains(r#"FEATURES="mergetimefeat splitdebug""#),
            "vdb environment must carry the merge-time FEATURES, got:\n{}",
            env.lines()
                .filter(|l| l.contains("FEATURES="))
                .collect::<Vec<_>>()
                .join("\n")
        );
        // The brush-wrapper's own `___`-prefixed locals must not leak.
        assert!(
            !env.contains("___sfe_") && !env.contains("___save_and_filter_ebuild_env"),
            "the env-regeneration wrapper's internals leaked into the vdb env"
        );
        // L1-f: the phase env is filtered to real's `environ_whitelist`,
        // so a non-whitelisted process-env var (`cargo test` always sets
        // `CARGO_MANIFEST_DIR`) never reaches the regenerated vdb env.
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "test precondition: CARGO_MANIFEST_DIR is set under `cargo test`"
        );
        assert!(
            !env.contains("CARGO_MANIFEST_DIR"),
            "a non-whitelisted process-env var leaked into the vdb environment"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_replace_runs_the_replaced_versions_pkg_prerm_and_pkg_postrm() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let portage_tmpdir = tmp.join("portage_tmpdir");

        // Install 1.0, then merge 2.0 over it (same slot).
        ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("1.0 merges");
        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-2.0.tbz2"),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
        )
        .expect("2.0 merges");
        assert_eq!(status, 0);

        // 2.0 replaced 1.0 in the vdb; 1.0's own file is unmerged.
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgrmpkg-2.0/CONTENTS")
                .is_file()
        );
        assert!(!root.join("var/db/pkg/dev-libs/binpkgrmpkg-1.0").exists());
        assert!(root.join("usr/share/binpkgrmpkg/payload-2.0.txt").is_file());
        assert!(!root.join("usr/share/binpkgrmpkg/payload-1.0.txt").exists());

        // The full real interleaving: 2.0 setup+preinst, then 1.0's
        // prerm+postrm (from 1.0's own vdb-stored environment.bz2, inside
        // treewalk()'s replace loop), then 2.0 postinst.
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgrmpkg.log")).unwrap(),
            "setup-1.0\npreinst-1.0\npostinst-1.0\n\
             setup-2.0\npreinst-2.0\nprerm-1.0\npostrm-1.0\npostinst-2.0\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn merge_binpkg_replaces_a_same_slot_installed_version() {
        let tmp = tempdir();
        let root = tmp.join("root");

        // An older same-slot version already installed: it owns one
        // file the new binpkg also ships (`hello.txt`, a shared path)
        // and one it does not (`old-only.txt`, a genuine orphan).
        let pkgshare = root.join("usr/share/packagepkg");
        std::fs::create_dir_all(&pkgshare).unwrap();
        std::fs::write(pkgshare.join("hello.txt"), "old hello\n").unwrap();
        std::fs::write(pkgshare.join("old-only.txt"), "gone after upgrade\n").unwrap();

        let installed = root.join("var/db/pkg/dev-libs/packagepkg-0.9");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("SLOT"), "0\n").unwrap();
        std::fs::write(installed.join("COUNTER"), "1").unwrap();
        std::fs::write(installed.join("PF"), "packagepkg-0.9\n").unwrap();
        std::fs::write(installed.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            installed.join("CONTENTS"),
            "dir /usr\ndir /usr/share\ndir /usr/share/packagepkg\n\
             obj /usr/share/packagepkg/hello.txt 0000 0\n\
             obj /usr/share/packagepkg/old-only.txt 0000 0\n",
        )
        .unwrap();

        let status = ebuild_merge::merge_binpkg(
            &fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            &root,
            &tmp.join("pt"),
            &MergeOptions::default(),
        )
        .expect("replace merge succeeds");
        assert_eq!(status, 0);

        // The new version is in the vdb; the old one is gone.
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/packagepkg-0.9").exists(),
            "the replaced version's vdb entry is removed"
        );

        // A file only the old version owned is unmerged; a file the new
        // version now owns survives with the new version's content.
        assert!(
            !pkgshare.join("old-only.txt").exists(),
            "the orphaned file is unmerged"
        );
        let hello = pkgshare.join("hello.txt");
        assert!(
            hello.is_file(),
            "the shared file the new version owns stays"
        );
        assert!(std::fs::read_to_string(&hello).unwrap().contains("hello"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_and_verify_fetches_then_size_and_md5_checks() {
        // Real digests of the committed fixture .tbz2 (`md5sum`/`sha1sum`,
        // not invented).
        const FIXTURE_MD5: &str = "54cab52a68eda02d7d41561b8f7d318a";
        const FIXTURE_SHA1: &str = "750847f2903fcc4f84bc657c6bb172501c4db490";
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let mut routes = HashMap::new();
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        let (base, _h) = serve(routes, 4);

        let record = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };

        // SIZE + matching MD5 + matching SHA1 -> ok.
        let ok = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", FIXTURE_SHA1),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let got =
            download_and_verify(&base, &ok, "dev-libs", "packagepkg", "1.0", &pkgdir).unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(std::fs::metadata(&got).unwrap().len(), 4618);

        // Wrong SIZE -> rejected, file removed.
        let bad_size = record(&[("SIZE", "9999"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]);
        let err = download_and_verify(&base, &bad_size, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("!= index SIZE 9999"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE, wrong MD5 -> rejected, file removed.
        let bad_md5 = record(&[
            ("SIZE", "4618"),
            ("MD5", "00000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(&base, &bad_md5, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("MD5 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE + MD5, wrong SHA1 -> rejected, file removed (real
        // `digestCheck` verifies every digest present, not just MD5).
        let bad_sha1 = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", "0000000000000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(&base, &bad_sha1, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("SHA1 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn refresh_binhost_indexes_decompresses_a_packages_gz() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let plain = b"TIMESTAMP: 0\nPACKAGES: 1\n\nCPV: dev-libs/foo-1.0\n\n".to_vec();
        // Real `gzip` output of `plain`.
        let mut gz_child = std::process::Command::new("gzip")
            .arg("-c")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            gz_child.stdin.take().unwrap().write_all(&plain).unwrap();
        }
        let gz = gz_child.wait_with_output().unwrap().stdout;

        let mut routes = HashMap::new();
        routes.insert("/Packages.gz".to_string(), gz);
        let (base, _h) = serve(routes, 1);

        let binrepo = BinRepo {
            name: "test".to_string(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
        };
        refresh_binhost_indexes(std::slice::from_ref(&binrepo), &root).unwrap();
        let cached = binrepo.packages_dir(&root).join("Packages");
        assert_eq!(std::fs::read(&cached).unwrap(), plain);
        assert!(!binrepo.packages_dir(&root).join("Packages.gz").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_downloads_a_remote_binpkg_and_merges_it() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();

        let tbz2 =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let index = packages_index(&[
            "BUILD_ID: 1\nCPV: dev-libs/packagepkg-1.0\nDEFINED_PHASES: install\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/packagepkg-1.0.tbz2\n\
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nSLOT: 0\nUSE:",
        ]);
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), index);
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), tbz2);
        // refresh tries Packages.gz + Packages.zst (both 404 here) before
        // the plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
        }];
        refresh_binhost_indexes(&binrepos, &root).expect("index refresh");
        assert!(
            root.join("var/cache/edb/binhost/127.0.0.1/Packages")
                .is_file(),
            "the live Packages landed in the edb cache"
        );

        let config = Config {
            binrepos: binrepos.clone(),
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entry = GraphEntry {
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::New {
                version: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("gentoo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source: CandidateSource::Binary,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: true,
            build_id: None,
            deps: Vec::new(),
        };

        run_merge_plan(
            &[entry],
            &config,
            &[],
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
            false,
            None,
            &[],
        )
        .expect("getbinpkg merge succeeds");

        assert!(
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file(),
            "the binpkg was downloaded into $PKGDIR"
        );
        assert!(
            root.join("usr/share/packagepkg/hello.txt").is_file(),
            "the binpkg image was merged into ROOT"
        );
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_upgrades_over_an_installed_binpkg() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");

        // packagepkg-0.9 already installed, owning a soon-orphaned file.
        let pkgshare = root.join("usr/share/packagepkg");
        std::fs::create_dir_all(&pkgshare).unwrap();
        std::fs::write(pkgshare.join("old-only.txt"), "orphan\n").unwrap();
        let installed = root.join("var/db/pkg/dev-libs/packagepkg-0.9");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("SLOT"), "0\n").unwrap();
        std::fs::write(installed.join("COUNTER"), "1").unwrap();
        std::fs::write(installed.join("PF"), "packagepkg-0.9\n").unwrap();
        std::fs::write(installed.join("CATEGORY"), "dev-libs\n").unwrap();
        std::fs::write(
            installed.join("CONTENTS"),
            "dir /usr\ndir /usr/share\ndir /usr/share/packagepkg\n\
             obj /usr/share/packagepkg/old-only.txt 0000 0\n",
        )
        .unwrap();

        let tbz2 =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let index = packages_index(&[
            "BUILD_ID: 1\nCPV: dev-libs/packagepkg-1.0\nDEFINED_PHASES: install\n\
             EAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/packagepkg-1.0.tbz2\n\
             RDEPEND: dev-libs/samepkg\nREPO: gentoo\nSIZE: 4618\nSLOT: 0\nUSE:",
        ]);
        let mut routes = HashMap::new();
        routes.insert("/Packages".to_string(), index);
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), tbz2);
        // refresh tries Packages.gz + Packages.zst (both 404 here) before
        // the plain Packages, then the binpkg download = 4 connections.
        let (base, _h) = serve(routes, 4);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
            location: None,
            verify_signature: true,
        }];
        refresh_binhost_indexes(&binrepos, &root).expect("index refresh");

        let config = Config {
            binrepos: binrepos.clone(),
            pkgdir: pkgdir.to_string_lossy().to_string(),
            ..Config::default()
        };
        let entry = GraphEntry {
            category: "dev-libs".into(),
            package: "packagepkg".into(),
            outcome: PretendOutcome::Upgrade {
                from: "0.9".into(),
                to: "1.0".into(),
            },
            blockers: vec![],
            slot: Some("0".into()),
            sub_slot: Some("0".into()),
            repo_name: Some("gentoo".into()),
            oldbest: vec![],
            use_flags_display: vec![],
            use_expand_display: vec![],
            use_expand_display_p: vec![],
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: vec![],
            required_by: vec![],
            source: CandidateSource::Binary,
            provenance: Default::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: true,
            build_id: None,
            deps: Vec::new(),
        };

        run_merge_plan(
            &[entry],
            &config,
            &[],
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
            false,
            None,
            &[],
        )
        .expect("getbinpkg merge succeeds");

        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file(),
            "the new version is installed"
        );
        assert!(
            !root.join("var/db/pkg/dev-libs/packagepkg-0.9").exists(),
            "the old version's vdb entry is gone"
        );
        assert!(
            !pkgshare.join("old-only.txt").exists(),
            "the old version's orphaned file is unmerged"
        );
        assert!(
            root.join("usr/share/packagepkg/hello.txt").is_file(),
            "the new version's own file is present"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_merge_plan_merges_a_binary_and_a_source_entry_in_one_run() {
        // `emerge --getbinpkg`'s own mixed plan: one `Binary` entry
        // (a local `$PKGDIR` `.tbz2`) and one `Source` entry (built from
        // its fixture ebuild), both merged in the same pass.
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2"),
        )
        .unwrap();

        let config_root = fixtures_root();
        let repos = portage_repo::find_repos(&config_root).unwrap();
        let options = MergeOptions {
            distdir: tmp.join("distdir"),
            config_root: config_root.clone(),
            ..MergeOptions::default()
        };

        run_merge_plan(
            &[
                graph_entry("packagepkg", CandidateSource::Binary, "1.0"),
                graph_entry("binpkgphasepkg", CandidateSource::Ebuild, "1.0"),
            ],
            &Config::default(),
            &repos,
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &options,
            false,
            None,
            &[],
        )
        .expect("mixed merge plan succeeds");

        // The Binary entry: merged from the local .tbz2.
        assert!(
            root.join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(root.join("usr/share/packagepkg/hello.txt").is_file());
        // The Source entry: built + merged from its ebuild, hooks ran.
        assert!(
            root.join("var/db/pkg/dev-libs/binpkgphasepkg-1.0/CONTENTS")
                .is_file()
        );
        assert!(root.join("usr/share/binpkgphasepkg/payload.txt").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/binpkgphasepkg.phases")).unwrap(),
            "preinst\npostinst\n"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// A resumed binary entry (`remote_binary: false`, `build_id: None`
    /// -- all `resume_entry` records) with no local `$PKGDIR` file must
    /// FAIL with the local-only error, never re-hit the binhost: real
    /// replays a resumed binary against its local bintree only (a
    /// never-downloaded binary isn't resumable at all). The `file://`
    /// binhost below serves a REAL fixture binary, so pre-fix code
    /// downloads it into `$PKGDIR` and merges successfully -- post-fix
    /// the pkgdir stays empty and the error names the local dir.
    #[test]
    fn merge_one_binary_entry_never_refetches_for_a_non_remote_entry() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let pkgdir = tmp.join("pkgdir");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&pkgdir).unwrap();
        let binhost = tmp.join("binhost");
        std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
        std::fs::copy(
            fixtures_root().join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
            binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"),
        )
        .unwrap();
        let size = std::fs::metadata(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"))
            .unwrap()
            .len();
        std::fs::write(
            binhost.join("Packages"),
            format!(
                "TIMESTAMP: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\n\
                 DEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\n\
                 REPO: testrepo\nSIZE: {size}\nSLOT: 0\nUSE:\n"
            ),
        )
        .unwrap();
        let config = Config {
            binrepos: vec![BinRepo {
                name: "test".into(),
                sync_uri: format!("file://{}", binhost.display()),
                priority: 1,
                location: None,
                verify_signature: false,
            }],
            ..Default::default()
        };
        // Sanity: the binhost really serves this cpv (a fetch WOULD
        // succeed) -- so the failure below proves no fetch was tried.
        assert!(
            find_remote_binpkg(&config.binrepos, &root, "dev-libs", "binpkgrmpkg", "1.0").is_some()
        );

        // Resumed shape: `graph_entry` defaults to `remote_binary:
        // false`, `build_id: None`, exactly like `resume_entry`.
        let entry = graph_entry("binpkgrmpkg", CandidateSource::Binary, "1.0");
        assert!(!entry.remote_binary);
        let err = merge_one_binary_entry(
            &entry,
            &config,
            &root,
            &pkgdir,
            &tmp.join("pt"),
            &MergeOptions::default(),
        )
        .expect_err("a resumed binary with no local file must fail, not refetch");
        assert!(
            err.contains("no binpkg file under"),
            "local-only error, got: {err}"
        );
        assert!(
            !err.contains("binhost"),
            "must not mention the index, got: {err}"
        );
        assert!(
            std::fs::read_dir(&pkgdir).unwrap().next().is_none(),
            "nothing may be downloaded into $PKGDIR"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
