//! `emerge --regen`: regenerate every repo's `metadata/md5-cache` by running each
//! ebuild's `depend` phase (Portage `action_regen` + `MetadataRegen`), with the
//! `depend` phases dispatched over a thread pool behind the `--jobs` and
//! `--load-average` gates.
//!
//! Determinism cuts a caller must know: `Processing <cp>` lines print up front
//! in `cp` order; failure lines report in work-list order after all workers join;
//! same-`(category, pf)` items never run concurrently (shared builddir).
//! The `_pull_valid_cache` shortcut, the `cp_retry` loop (at most 3 passes),
//! stale-entry pruning, and the eclass masters chain are implemented. A repo whose
//! `cache-formats` lack `md5-dict` is skipped with exit 1. Rejects `--pretend`.

use crate::ebuild_phases;
use md5::{Digest as _, Md5};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// One ebuild's `depend`-phase work unit. `category`/`pf` is the dispatch key;
/// `cp` is the retry unit.
struct RegenWorkItem {
    ebuild_path: PathBuf,
    repo_location: PathBuf,
    masters: Vec<PathBuf>,
    category: String,
    pf: String,
    cp: String,
}

/// A failed item: the failure-report message plus the returncode the retry
/// decision keys off.
/// Portage: MetadataRegen.py, _task_exit.
struct RegenFailure {
    code: i32,
    message: String,
}
/// Arguments of [`regen_one`].
struct RegenOneArgs<'a> {
    ebuild_path: &'a Path,
    repo_location: &'a Path,
    masters: &'a [PathBuf],
    category: &'a str,
    pf: &'a str,
    root: &'a Path,
    config_root: &'a Path,
    portage_tmpdir: &'a Path,
    debug: bool,
}
