// Real `emerge --getbinpkgonly <atom>` execution, WITHOUT `--pretend`:
// resolve the graph against binary candidates only (`--usepkgonly`), then
// for every entry that would newly merge as a *remote* binary, download
// the binpkg from its binhost and merge it into the vdb.
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
//   - `run_getbinpkgonly`: iterate the resolved entries (already in real
//     topological merge order), and for each remote-binary `New`: find
//     its `Packages` record (`find_remote_binpkg`), `wget`/copy the
//     binpkg file into `$PKGDIR`, verify its `SIZE` against the index,
//     and call `ebuild_merge::merge_binpkg` (see that function's own
//     documented v1 cuts).
//
// v1 cuts specific to this module:
//   - `Packages.gz` / `Packages.zst` (a compressed remote index) is not
//     tried -- only the plain `Packages` (real portage's own preference
//     order; the plain file is always served alongside).
//   - live `layout.conf` negotiation (`binpkg-multi-instance`, path
//     layout) is not done -- the index `PATH` field (or the default
//     `<cat>/<pf>.tbz2`) is trusted outright, same "trust the index"
//     stance the `--pretend` half already takes.
//   - digest verification is `SIZE`-only (the pilot has no crypto; the
//     `SHA*`/`MD5` fields are read but not checked -- see
//     `binpkg::read_gpkg_metadata`'s own identical `Manifest`/`.sig` cut).
//   - a source ebuild slipping through the binary-only resolve is a hard
//     error. `New`/`Upgrade`/`Downgrade`/`Reinstall` all merge
//     (`merge_binpkg` unmerges a replaced same-slot version itself).

use crate::ebuild_merge::{self, MergeOptions};
use portage_profile::{BinRepo, Config};
use portage_repo::{find_remote_binpkg, CandidateSource, GraphEntry, PretendOutcome};
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
        crate::fetch::wget_fetch(&format!("{uri}/Packages"), &dest)
            .map_err(|e| format!("binhost {uri}: {e}"))?;
    }
    Ok(())
}

/// Download + merge every remote-binary entry in `entries` (already in
/// real dependency-first merge order). `AlreadyInstalled` dependencies
/// are skipped; `New`/`Upgrade`/`Downgrade`/`Reinstall` are fetched and
/// merged (`merge_binpkg` unmerges a replaced same-slot version itself);
/// `NoVisibleCandidate` and a source-only resolution are hard errors.
pub fn run_getbinpkgonly(
    entries: &[GraphEntry],
    config: &Config,
    root: &Path,
    pkgdir: &Path,
    portage_tmpdir: &Path,
    merge_options: &MergeOptions,
) -> Result<(), String> {
    for entry in entries {
        let cp = format!("{}/{}", entry.category, entry.package);
        let version = match &entry.outcome {
            PretendOutcome::AlreadyInstalled { .. } => continue,
            PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                version.clone()
            }
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to.clone(),
            PretendOutcome::NoVisibleCandidate => {
                return Err(format!("no binary package available for {cp}"));
            }
        };
        if entry.source != CandidateSource::Binary {
            return Err(format!(
                "{cp}-{version}: resolved to a source ebuild, not a binary package"
            ));
        }

        // A remote candidate must be fetched; a local `$PKGDIR` binpkg
        // (`remote_binary == false`) is already on disk.
        let binpkg_path = if entry.remote_binary {
            let (sync_uri, record) = find_remote_binpkg(
                &config.binrepos,
                root,
                &entry.category,
                &entry.package,
                &version,
            )
            .ok_or_else(|| format!("{cp}-{version}: not found in any binhost `Packages` index"))?;
            download_and_verify(
                &sync_uri,
                &record,
                &entry.category,
                &entry.package,
                &version,
                pkgdir,
            )?
        } else {
            resolve_local_binpkg(pkgdir, &entry.category, &entry.package, &version).ok_or_else(
                || format!("{cp}-{version}: no binpkg file under {}", pkgdir.display()),
            )?
        };

        println!(">>> Merging binary package {cp}-{version}...");
        let status = ebuild_merge::merge_binpkg(&binpkg_path, root, portage_tmpdir, merge_options)?;
        if status != 0 {
            return Err(format!("{cp}-{version}: binpkg merge failed ({status})"));
        }
    }
    Ok(())
}

/// `<pkgdir>/<cat>/<pf>.{tbz2,gpkg.tar}`, whichever exists.
fn resolve_local_binpkg(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<std::path::PathBuf> {
    for ext in ["tbz2", "gpkg.tar"] {
        let p = pkgdir
            .join(category)
            .join(format!("{package}-{version}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into
/// `$PKGDIR`, then check its byte size against the index `SIZE`.
fn download_and_verify(
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
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(std::fs::read_to_string(vdb.join("CONTENTS"))
            .unwrap()
            .contains("/usr/share/packagepkg/hello.txt"));
        assert_eq!(
            std::fs::read_to_string(vdb.join("RDEPEND")).unwrap().trim(),
            "dev-libs/samepkg"
        );
        assert_eq!(
            std::fs::read_to_string(vdb.join("SLOT")).unwrap().trim(),
            "0"
        );
        // The saved env + ebuild are kept in the vdb now (real portage
        // does; the pilot needs them for pkg_preinst/pkg_postinst). This
        // fixture's DEFINED_PHASES is `install` only, so no hook ran.
        assert!(vdb.join("environment.bz2").is_file());
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());
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
        assert!(root
            .join("var/db/pkg/dev-libs/binpkgphasepkg-1.0/CONTENTS")
            .is_file());

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
        assert!(root
            .join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
            .is_file());
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
    fn download_and_verify_fetches_then_size_checks() {
        let tmp = tempdir();
        let pkgdir = tmp.join("pkgdir");
        let body =
            std::fs::read(fixtures_root().join("pkgdir/dev-libs/packagepkg-1.0.tbz2")).unwrap();
        let mut routes = HashMap::new();
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        routes.insert("/dev-libs/packagepkg-1.0.tbz2".to_string(), body.clone());
        let (base, _h) = serve(routes, 2);

        let ok_record: HashMap<String, String> =
            [("SIZE", "4618"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let got = download_and_verify(&base, &ok_record, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(std::fs::metadata(&got).unwrap().len(), 4618);

        let bad_record: HashMap<String, String> =
            [("SIZE", "9999"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        let err = download_and_verify(&base, &bad_record, "dev-libs", "packagepkg", "1.0", &pkgdir)
            .unwrap_err();
        assert!(err.contains("!= index SIZE 9999"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_getbinpkgonly_downloads_a_remote_binpkg_and_merges_it() {
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
        let (base, _h) = serve(routes, 2);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
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
        };

        run_getbinpkgonly(
            &[entry],
            &config,
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("getbinpkgonly merge succeeds");

        assert!(
            pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file(),
            "the binpkg was downloaded into $PKGDIR"
        );
        assert!(
            root.join("usr/share/packagepkg/hello.txt").is_file(),
            "the binpkg image was merged into ROOT"
        );
        assert!(root
            .join("var/db/pkg/dev-libs/packagepkg-1.0/CONTENTS")
            .is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_getbinpkgonly_upgrades_over_an_installed_version() {
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
        let (base, _h) = serve(routes, 2);

        let binrepos = vec![BinRepo {
            name: "test".into(),
            sync_uri: base.clone(),
            priority: 1,
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
        };

        run_getbinpkgonly(
            &[entry],
            &config,
            &root,
            &pkgdir,
            &tmp.join("portage_tmpdir"),
            &MergeOptions::default(),
        )
        .expect("getbinpkgonly upgrade succeeds");

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
}
