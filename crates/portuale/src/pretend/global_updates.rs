//! The applied-updates AUTOCLEAN force: whether any repo's
//! `profiles/updates/` holds a changed file with >=1 valid directive,
//! measured against the `updates` mtime record in `<EROOT>/var/cache/edb/mtimedb`.

use crate::pretend::*;
use portage_repo::RepoConfig;

/// A bare `cat/pkg` (no version/slot/operator/blocker/use/repo), else `None`.
fn bare_cp(s: &str) -> Option<(String, String)> {
    let a = portage_dep::parse_atom(s)?;
    if a.blocker != portage_dep::Blocker::None
        || a.operator != portage_dep::Operator::None
        || a.version.is_some()
        || a.slot.is_some()
        || a.slot_operator.is_some()
        || a.use_deps.is_some()
        || a.repo.is_some()
    {
        return None;
    }
    Some((a.category, a.package))
}

/// Any well-formed atom that is not a blocker. A `slotmove` package may
/// carry a version (`update.py` only rejects blockers there); whether it
/// matches anything at apply time is the apply side's business.
fn update_atom(s: &str) -> bool {
    portage_dep::parse_atom(s).is_some_and(|a| a.blocker == portage_dep::Blocker::None)
}

/// A slot name is `[A-Za-z0-9_][A-Za-z0-9+_.-]*` (the `/subslot` form is
/// rejected by the same `slot_re`, which has no `/` alternative here).
// Portage: versions.py, _get_slot_re (`_slot`); update.py, parse_updates
fn valid_slot_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '.' | '-'))
}

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

/// `(valid directives, error-free)` for one updates file's text: blank
/// lines are skipped, anything else malformed marks the file errored
/// (applied but unrecorded). Silent by design -- a malformed updates
/// file never fails a merge.
// Portage: lib/portage/update.py, parse_updates
fn parse_updates_file(text: &str) -> (usize, bool) {
    let mut valid = 0usize;
    let mut errors = false;
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        match toks.as_slice() {
            ["move", a, b] if bare_cp(a).is_some() && bare_cp(b).is_some() => {
                valid += 1;
            }
            ["slotmove", pkg, old, new]
                if update_atom(pkg) && valid_slot_name(old) && valid_slot_name(new) =>
            {
                valid += 1;
            }
            _ => {
                errors = true;
            }
        }
    }
    (valid, !errors)
}

/// One root's updates check: true iff a changed updates file yields >=1
/// valid directive. Error-free mtimes merge into the record and commit
/// on true only; every I/O error is swallowed, so a non-writable root
/// never fails the run.
// Portage: lib/portage/_global_updates.py, _do_global_updates (`retupd`,
// error files applied but unrecorded); lib/portage/util/mtimedb.py
// (`MtimeDB.__write_to_disk` swallows `OSError`)
fn check_root_updates(repos: &[RepoConfig], root: &Path) -> bool {
    let mut record = crate::mtimedb::read_updates_mtimes(root);
    let mut fresh = std::collections::BTreeMap::new();
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
            // is skipped, never fatal.
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let (valid, clean) = parse_updates_file(&String::from_utf8_lossy(&bytes));
            valid_total += valid;
            if clean {
                fresh.insert(key, mtime);
            }
        }
    }
    if valid_total == 0 {
        return false;
    }
    record.extend(fresh);
    let _ = crate::mtimedb::write_updates_mtimes(root, &record);
    true
}

/// The two-root core: the target root's force, maintaining the running
/// root's record as a side effect when it differs. Split out so tests
/// need no process-global env (`PORTAGE_RUNNING_ROOT` is also written
/// by unrelated tests running on other threads).
pub(crate) fn updates_force_for_roots(repos: &[RepoConfig], target: &Path, running: &Path) -> bool {
    let target_force = check_root_updates(repos, target);
    if running != target {
        check_root_updates(repos, running);
    }
    target_force
}

/// The target root's applied-updates force, for the merge gate: true
/// when this run newly applied updates there. Runs for every action
/// except help/info/sync/version and `--package-moves=n`, pre-resolve,
/// so `--pretend` and failed merges still commit the record; the
/// running root's record is maintained as a side effect when it
/// differs, but its force never reaches the tail (which reads the
/// target settings).
// Portage: lib/_emerge/actions.py, run_action (`_global_updates` over the
// target root config, then the running one when different; the force
// sets `AUTOCLEAN` on that root_config's settings while `action_build`
// reads the target `settings`)
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
