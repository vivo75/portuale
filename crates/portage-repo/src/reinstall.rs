//! When an installed package needs a reinstall (changed deps, slot, repo, USE).

use crate::*;

/// Splits a raw `SLOT` string into `(slot, sub_slot)`: `SLOT="main/sub"`,
/// `sub_slot` defaulting to the slot itself when no `/` is present. An
/// empty string (missing `SLOT` file/key) defaults to `("0", "0")`, the
/// same `"0"` fallback `list_candidates`/`installed_candidates` use.
/// Portage: portage.versions slot parsing.
pub(crate) fn split_slot(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    match raw.split_once('/') {
        Some((slot, sub_slot)) => (slot.to_string(), sub_slot.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/SLOT` and
/// splits it via `split_slot` (the sub-slot kept, not discarded).
pub(crate) fn read_vdb_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> (String, String) {
    split_slot(&read_vdb_string(root, category, package, version, "SLOT"))
}

/// `--changed-slot`: whether `version`'s vdb-recorded `SLOT` (main+sub)
/// differs from the repo's *current* ebuild for that exact version.
/// Portage: depgraph.py, _changed_slot.
///
/// Scope cut: Portage's consumers of `_changed_slot` live in
/// binary-package/slot-operator-rebuild scheduling portuale has none of;
/// ported as another independent `PretendOutcome::Reinstall` trigger
/// instead -- the same "report a reinstall" simplification
/// `--changed-deps` established. Re-reads the repo's raw `SLOT` value
/// directly (not `Candidate::slot`, which keeps the main component only).
/// A repo-side lookup that fails reports "unchanged" (`false`), matching
/// Portage's `_equiv_ebuild(pkg) is None` -> `False`.
pub(crate) fn slot_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let vdb_slot = read_vdb_slot(root, category, package, version);

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let repo_slot = split_slot(
        metadata
            .get("SLOT")
            .map(String::as_str)
            .unwrap_or("")
            .trim(),
    );

    vdb_slot != repo_slot
}

/// `--newrepo`: whether `version`'s vdb-recorded `repository` file differs
/// from `current_repo_name` (already resolved by the caller -- no md5-cache
/// re-read needed, unlike `slot_changed`). A vdb entry with no
/// `repository` file at all is `"__unknown__"`, a distinct value that
/// either equals `current_repo_name` or doesn't.
/// Portage: depgraph.py's straight repo-name comparison.
pub(crate) fn new_repo_changed(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    current_repo_name: &str,
) -> bool {
    let vdb_repo = read_vdb_string(root, category, package, version, "repository");
    let vdb_repo = if vdb_repo.is_empty() {
        "__unknown__"
    } else {
        &vdb_repo
    };
    vdb_repo != current_repo_name
}

/// A candidate's own `is_valid_flag` domain: its declared `IUSE` unioned
/// with the profile's EAPI 5+ `IUSE_EFFECTIVE` (`config.iuse_effective`).
/// Used only for a USE-dep's `.required`/`(+)`/`(-)` check -- deliberately
/// NOT for `--newuse`'s IUSE-*presence* diff, which must stay strictly
/// declared-IUSE.
/// Portage: pkg.iuse.is_valid_flag.
pub(crate) fn valid_iuse(
    declared: &HashSet<String>,
    config: &portage_profile::Config,
) -> HashSet<String> {
    if config.iuse_effective.is_empty() {
        return declared.clone();
    }
    declared.union(&config.iuse_effective).cloned().collect()
}
