//! The applied-updates AUTOCLEAN force: whether any repo's
//! `profiles/updates/` holds a changed file with >=1 valid directive.
//! Read-only interim: the scan parses but records nothing and the force
//! stays false until the moves themselves are applied.

use crate::pretend::*;
use portage_repo::RepoConfig;

/// Whole-second `st_mtime` of `path`, or `None` when unreadable.
// Portage: lib/portage/update.py, grab_updates (`mystat[stat.ST_MTIME]`)
fn file_mtime(path: &Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(m.mtime())
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        None
    }
}

/// One root's updates check: true iff a changed updates file yields >=1
/// valid directive. Read-only: the record is consulted but never
/// extended, so a non-writable root never fails the run.
// Portage: lib/portage/_global_updates.py, _do_global_updates
fn check_root_updates(repos: &[RepoConfig], root: &Path) -> bool {
    let record = crate::mtimedb::read_updates_mtimes(root);
    let mut valid_total = 0usize;
    // Repos sharing one updates directory scan it once.
    // Portage: _global_updates.py, updpath_map
    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    for repo in repos {
        let updpath = repo.location.join("profiles").join("updates");
        if !seen.insert(updpath.clone()) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&updpath) else {
            continue;
        };
        for entry in rd.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_file() {
                continue;
            }
            let path = entry.path();
            let Some(mtime) = file_mtime(&path) else {
                continue;
            };
            let key = path.display().to_string();
            // `int(record.get(path, -1)) != st_mtime`: only changed
            // files are even parsed.
            // Portage: lib/portage/update.py, grab_updates
            if record.get(&key) == Some(&mtime) {
                continue;
            }
            // Lossy read like `errors="replace"`; an unreadable file
            // is skipped, never fatal. Error tracking (applied but
            // unrecorded) returns with the record in a later piece.
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            valid_total +=
                portage_repo::parse_updates_content(&String::from_utf8_lossy(&bytes)).len();
        }
    }
    valid_total > 0
}

/// The two-root core: both roots' updates state is scanned, but no force
/// is ever reported while the moves stay unapplied. Split out so tests
/// need no process-global env (`PORTAGE_RUNNING_ROOT` is also written
/// by unrelated tests running on other threads).
pub(crate) fn updates_force_for_roots(repos: &[RepoConfig], target: &Path, running: &Path) -> bool {
    // The scan still runs: it keeps the updates parser exercised on real
    // repos while recording nothing.
    check_root_updates(repos, target);
    if running != target {
        check_root_updates(repos, running);
    }
    false
}

/// The target root's applied-updates force, for the merge gate. Always
/// false while the moves stay unapplied: the check scans but neither
/// records nor forces. Runs for every action except help/info/sync/version
/// and `--package-moves=n`, pre-resolve.
// Portage: lib/_emerge/actions.py, run_action
pub(crate) fn global_updates_force(
    opts: &EmergeOpts<'_>,
    repos: &[RepoConfig],
    root: &Path,
) -> bool {
    if !opts.package_moves || opts.info_action || opts.sync_action || opts.version_action {
        return false;
    }
    updates_force_for_roots(repos, root, &portage_repo::running_root_from_env())
}
