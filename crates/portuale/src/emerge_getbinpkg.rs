//! `emerge --getbinpkg` / `--getbinpkgonly` execution: refresh each remote
//! binhost's live index (in `--pretend` exactly like in a non-pretend merge),
//! resolve the graph, then merge every resolved entry -- per entry on its
//! `source` (a `Binary` candidate is downloaded+merged, anything else is
//! built+merged from source).
//!
//! Scope cuts a caller must know: a compressed remote index is tried
//! (`Packages.gz`/`Packages.zst`, then plain `Packages`); no `layout.conf`
//! negotiation (the index `PATH` is trusted); digest checks cover `SIZE` and
//! the `MD5`/`SHA1` fields present, plus a downloaded gpkg's internal
//! manifest and signature layer at merge time.

use crate::ebuild_merge::{self, MergeOptions};
use mrg_director::BinpkgIndex;
use mrg_director::MergeEngine;
use portage_profile::{BinRepo, Config};
use portage_repo::{GraphEntry, PretendOutcome, RepoConfig};
use std::path::Path;

/// Seconds since the epoch as `f64` (Portage's `time.time()`).
fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Arguments of [`run_merge_plan`].
pub struct RunMergePlanArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub pkgdir: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub merge_options: &'a MergeOptions,
    pub keep_going: bool,
    pub buildpkg: Option<&'a crate::ebuild_package::PackageOptions>,
    pub buildpkg_exclude: &'a [String],
    /// The scheduler-display mode, computed once by the caller. The mixed plan
    /// runs serially, so the mode only carries the blank/`>>>` rule; `Jobs:`
    /// events for the mixed plan are a later slice.
    pub mode: crate::emerge_build::StatusMode,
}

/// `>>> Failed to emerge <cpv>[ for <root>][, Log file:]` plus, when the
/// non-empty build log is found, `>>>  '<log>'`. Each message writes its own
/// leading blank line first; the log line's extra space is Portage's own
/// `f" '{log}'"`. The ` for <root>` suffix appears when `ROOT != "/"`.
// Portage: lib/_emerge/Scheduler.py, _failed_pkg_msg
pub(crate) fn failed_pkg_msg(cpv: &str, root: &Path, log_path: Option<&Path>) -> String {
    let for_root = if root == Path::new("/") {
        String::new()
    } else {
        format!(" for {}", root.display())
    };
    match log_path {
        Some(log) => format!(
            "\n>>> Failed to emerge {cpv}{for_root}, Log file:\n\n>>>  '{}'\n",
            log.display()
        ),
        None => format!("\n>>> Failed to emerge {cpv}{for_root}\n"),
    }
}

/// The error for a merge-time binpkg digest failure whose output is already
/// printed: the CLI boundary exits 1 with no resume notice and no `emerge:` line.
pub(crate) fn binpkg_digest_failure(cpv: &str) -> String {
    format!("{cpv}: binpkg digest verification failed")
}

/// The missing-file block as one unit (each line `\n`-terminated, no
/// surrounding blanks): the stale-index arm when the cpv is still in the
/// `<pkgdir>/Packages` index, else the `!!! Fetching Binary failed` arm.
/// Pure shaper, for tests and the `build.log` write alike.
// Portage: _emerge/BinpkgVerifier.py, _start
pub(crate) fn missing_binpkg_block(cpv: &str, index_lists_cpv: bool) -> String {
    if index_lists_cpv {
        format!(
            "!!! Tried to use non-existent binary for '{cpv}'\n\
             !!! Likely caused by an outdated index. Run 'emaint binhost -f'.\n"
        )
    } else {
        format!("!!! Fetching Binary failed for '{cpv}'\n")
    }
}

/// Whether the local `<pkgdir>/Packages` index still lists this cpv. Like
/// [`local_index_record`] minus the basename tie-break (no file to name it
/// after): with several same-`CPV` stanzas the entry's `BUILD_ID` picks.
// Portage: _emerge/BinpkgVerifier.py, _start
fn local_index_lists_cpv(
    pkgdir: &Path,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> bool {
    let cpv = format!("{category}/{package}-{version}");
    let mut candidates = portage_repo::read_packages_index(pkgdir)
        .into_iter()
        .filter(|e| e.get("CPV").is_some_and(|c| c == &cpv))
        .peekable();
    if candidates.peek().is_none() {
        return false;
    }
    if let Some(want) = build_id.filter(|s| !s.is_empty()) {
        let narrowed: Vec<_> = candidates
            .filter(|e| e.get("BUILD_ID").is_some_and(|b| b == want))
            .collect();
        // A `BUILD_ID` matching nothing still leaves the plain `CPV` hit: Portage
        // resolves a plain string to the latest instance, so any surviving
        // instance keeps this true.
        if !narrowed.is_empty() {
            return true;
        }
    }
    true
}

/// The error for a merge-time missing binpkg whose output is already printed:
/// exit 1 with no resume notice and no `emerge:` line (a merge failure exits
/// via `FAILURE`, exactly like the digest arm).
pub(crate) fn binpkg_missing_failure(cpv: &str) -> String {
    format!("{cpv}: non-existent binary package")
}

/// Whether `e` is exactly one `binpkg_missing_failure` (same lone-
/// failure shape as [`is_binpkg_digest_failure`]).
pub(crate) fn is_binpkg_missing_failure(e: &str) -> bool {
    !e.contains('\n') && e.ends_with(": non-existent binary package")
}

/// The on-disk binpkg for `<cat>/<package>-<version>` in `$PKGDIR`: the
/// single-instance `<cat>/<pf>.{tbz2,gpkg.tar}`, or the multi-instance
/// `<cat>/<pn>/<pf>-<build_id>.{xpak,gpkg.tar}` (a `<pn>/` subdir plus a
/// `-<build_id>` suffix). A `build_id` picks the exact file; without one the
/// `<pn>/` subdir is scanned (highest `<id>` first).
// Portage: lib/portage/dbapi/bintree.py, getname
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
    let mut candidates: Vec<(u64, std::path::PathBuf)> =
        portage_util::read_dir_entries(&instance_dir)
            .ok()?
            .into_iter()
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

/// Arguments of [`download_and_verify`].
pub(crate) struct DownloadAndVerifyArgs<'a> {
    pub(crate) sync_uri: &'a str,
    pub(crate) record: &'a std::collections::HashMap<String, String>,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) pkgdir: &'a Path,
}

/// Fetch `<sync_uri>/<PATH>` (or the default `<cat>/<pf>.tbz2`) into `$PKGDIR`,
/// then verify it against the index `SIZE` and the `MD5`/`SHA1` fields present.
/// A mismatch removes the file and fails.
// Portage: lib/portage/dbapi/bintree.py, gettbz2
pub(crate) fn download_and_verify(
    args: DownloadAndVerifyArgs<'_>,
) -> Result<std::path::PathBuf, String> {
    let DownloadAndVerifyArgs {
        sync_uri,
        record,
        category,
        package,
        version,
        pkgdir,
    } = args;
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

    // The `MD5`/`SHA1` fields are the md5/sha1 of the whole file, and every
    // digest present is checked; a mismatch removes the file and fails.
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
    use portage_repo::find_remote_binpkg;
    use portage_util::TempDir;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn fixtures_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-getbinpkg").keep()
    }

    /// Serves each `routes` entry over loopback HTTP, one response per
    /// connection, for `requests` connections total (the `Packages` fetch plus
    /// the run's binpkg fetches).
    fn serve(
        routes: HashMap<String, Vec<u8>>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        serve_with_status(
            routes
                .into_iter()
                .map(|(p, b)| (p, ("200 OK".to_string(), b)))
                .collect(),
            requests,
        )
    }

    /// [`serve`] with an explicit status line per route, for fault injection;
    /// unlisted paths still 404.
    fn serve_with_status(
        routes: HashMap<String, (String, Vec<u8>)>,
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
                    Some((s, b)) => (s.clone(), b.clone()),
                    None => ("404 Not Found".to_string(), Vec::new()),
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
        // A genuine binhost index always carries both gated headers, so the
        // helper stamps both.
        let mut s = format!("TIMESTAMP: 0\nVERSION: 0\nPACKAGES: {}\n\n", entries.len());
        for e in entries {
            s.push_str(e);
            s.push_str("\n\n");
        }
        s.into_bytes()
    }

    fn graph_entry(package: &str, source: CandidateSource, version: &str) -> GraphEntry {
        GraphEntry {
            discovery: 0,
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

    /// A writable copy of the committed GnuPG test keyring (see `binpkg.rs`).
    /// `gpg` refuses a homedir it doesn't own, so the tree itself is never used.
    fn test_gpg_home() -> std::path::PathBuf {
        fn copy_dir(src: &std::path::Path, dest: &std::path::Path) {
            std::fs::create_dir_all(dest).unwrap();
            for entry in portage_util::read_dir_entries(src).unwrap() {
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
                .join("../../fixtures/helpers/gpg-keyring"),
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
        // The committed signed fixture merges end to end with the test keyring:
        // policy enforced, image + metadata land in the vdb like any other merge.
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

        // A complete vdb entry, with CONTENTS naming the file and the
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
        // The saved env + ebuild are kept in the vdb now (needed for
        // pkg_preinst/pkg_postinst). This fixture's DEFINED_PHASES is `install`
        // only, so no hook ran.
        assert!(vdb.join("environment.bz2").is_file());
        assert!(vdb.join("packagepkg-1.0.ebuild").is_file());

        // The vdb entry carries a consolidated `metadata` file: `#format=1`
        // header, sorted `KEY=value` lines, `#dir_mtime=` last and matching the
        // entry dir's own `st_mtime_ns` (a stale value is rejected).
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
    fn merge_binpkg_collision_protect_aborts_when_another_package_owns_the_file() {
        // `_collision_protect` (shared with the source merge): a pre-existing vdb
        // entry owning the same path aborts the binpkg merge rather than
        // overwriting it.
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

        // The fixture's preinst `die`s if the payload is merged and postinst
        // `die`s if it is not -- so both lines prove the treewalk() ordering
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

    /// `pkg_pretend` runs for a binary package too, folded into `merge_binpkg`'s
    /// hook chain before `pkg_setup`; the fixture proves it ran at the right point.
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

    /// The vdb `environment.bz2` is regenerated from the live merge-time
    /// environment, so it carries merge-time `FEATURES`, not build-time ones.
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
        // The phase env is filtered to the whitelist, so a non-whitelisted
        // process-env var never reaches the regenerated vdb env.
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

    /// A binary merge's calling env is narrowed to the whitelist, so a
    /// harness/portuale-only key in `build_env` never reaches the vdb env.
    #[test]
    fn merge_binpkg_filters_non_whitelisted_build_env_from_the_vdb_environment() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();

        let opts = MergeOptions {
            features: "mergetimefeat".to_string(),
            build_env: vec![
                (
                    "CARGO_MANIFEST_DIR".to_string(),
                    "/tmp/not-whitelisted".to_string(),
                ),
                ("L1_SKIP_PORTAGE_UPGRADE".to_string(), "0".to_string()),
                ("FEATURES".to_string(), "mergetimefeat".to_string()),
            ],
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
            !env.contains("CARGO_MANIFEST_DIR") && !env.contains("L1_SKIP_PORTAGE_UPGRADE"),
            "non-whitelisted build_env keys must be filtered from the vdb env"
        );
        assert!(
            env.contains(r#"FEATURES="mergetimefeat""#),
            "whitelisted FEATURES must survive the filter"
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

        // The full interleaving: 2.0 setup+preinst, then 1.0's prerm+postrm (from
        // 1.0's own vdb-stored environment, inside the replace loop), then 2.0
        // postinst.
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

        // An older same-slot version already installed: it owns one file the new
        // binpkg also ships (a shared path) and one it does not (a genuine orphan).
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
        // Committed fixture .tbz2 digests (`md5sum`/`sha1sum`, not invented).
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
        let got = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &ok,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
        })
        .unwrap();
        assert_eq!(got, pkgdir.join("dev-libs/packagepkg-1.0.tbz2"));
        assert_eq!(std::fs::metadata(&got).unwrap().len(), 4618);

        // Wrong SIZE -> rejected, file removed.
        let bad_size = record(&[("SIZE", "9999"), ("PATH", "dev-libs/packagepkg-1.0.tbz2")]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_size,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
        })
        .unwrap_err();
        assert!(err.contains("!= index SIZE 9999"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE, wrong MD5 -> rejected, file removed.
        let bad_md5 = record(&[
            ("SIZE", "4618"),
            ("MD5", "00000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_md5,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
        })
        .unwrap_err();
        assert!(err.contains("MD5 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());

        // Right SIZE + MD5, wrong SHA1 -> rejected, file removed (every digest
        // present is verified, not just MD5).
        let bad_sha1 = record(&[
            ("SIZE", "4618"),
            ("MD5", FIXTURE_MD5),
            ("SHA1", "0000000000000000000000000000000000000000"),
            ("PATH", "dev-libs/packagepkg-1.0.tbz2"),
        ]);
        let err = download_and_verify(DownloadAndVerifyArgs {
            sync_uri: &base,
            record: &bad_sha1,
            category: "dev-libs",
            package: "packagepkg",
            version: "1.0",
            pkgdir: &pkgdir,
        })
        .unwrap_err();
        assert!(err.contains("SHA1 mismatch"), "{err}");
        assert!(!pkgdir.join("dev-libs/packagepkg-1.0.tbz2").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Remote versions of `package` the resolver would see for
    /// `binrepos` under `root` -- i.e. the live-refresh override when a
    /// refresh ran, else the on-disk index.
    fn remote_versions(binrepos: &[BinRepo], root: &Path, package: &str) -> Vec<String> {
        let local = portage_repo::BinaryIndex::from_entries(Vec::new());
        portage_repo::list_remote_binary_candidates(binrepos, root, &local, "dev-libs", package)
            .into_iter()
            .map(|c| c.version)
            .collect()
    }
}
