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
