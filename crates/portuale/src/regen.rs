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

/// The md5-cache key set (`portage.auxdbkeys`) plus the two synthetic keys
/// `flat_hash` adds. The `_` prefix sorts last.
const WRITE_KEYS: &[&str] = &[
    "BDEPEND",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "INHERIT",
    "IUSE",
    "KEYWORDS",
    "LICENSE",
    "PDEPEND",
    "PROPERTIES",
    "RDEPEND",
    "REQUIRED_USE",
    "RESTRICT",
    "SLOT",
    "SRC_URI",
    "_eclasses_",
    "_md5_",
];

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

/// One initial run plus up to two retries of the unexpectedly-failed cps.
/// Portage: MetadataRegen.py, metadata_regen_retry.
const MAX_TRIES: u32 = 3;

/// Only a non-1 returncode retries its whole cp; 1 is the "expected" failure
/// (invalid metadata, pre-spawn failure) and is never retried.
/// Portage: MetadataRegen.py, _task_exit.
fn is_retryable_returncode(code: i32) -> bool {
    code != 1
}

/// The unexpectedly-failed cps in first-seen order (Portage iterates a set;
/// same set, stable order).
fn select_retry_cps(cps_in_order: &[String], unexpected_cps: &HashSet<String>) -> Vec<String> {
    cps_in_order
        .iter()
        .filter(|cp| unexpected_cps.contains(cp.as_str()))
        .cloned()
        .collect()
}

/// One item's `depend` phase plus cache write.
/// Portage: MetadataRegen.py, EbuildMetadataPhase.
fn run_work_item(
    item: &RegenWorkItem,
    root: &Path,
    config_root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<(), RegenFailure> {
    regen_one(RegenOneArgs {
        ebuild_path: &item.ebuild_path,
        repo_location: &item.repo_location,
        masters: &item.masters,
        category: &item.category,
        pf: &item.pf,
        root,
        config_root,
        portage_tmpdir,
        debug,
    })
}

/// Arguments of [`run_parallel`].
struct RunParallelArgs<'a> {
    work: &'a [RegenWorkItem],
    pass_indices: &'a [usize],
    root: &'a Path,
    config_root: &'a Path,
    portage_tmpdir: &'a Path,
    debug: bool,
    jobs: usize,
    load_average: Option<f64>,
}

/// Dispatch one pass's items onto up to `jobs` threads in work-list order,
/// holding back additional workers above the load average and never sharing a
/// builddir key. Results return indexed for deterministic folding.
/// Portage: MetadataRegen.py (AsyncScheduler).
fn run_parallel(args: RunParallelArgs<'_>) -> Vec<(usize, Result<(), RegenFailure>)> {
    let RunParallelArgs {
        work,
        pass_indices,
        root,
        config_root,
        portage_tmpdir,
        debug,
        jobs,
        load_average,
    } = args;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    let mut results: Vec<Option<(usize, Result<(), RegenFailure>)>> =
        (0..pass_indices.len()).map(|_| None).collect();
    if pass_indices.is_empty() {
        return Vec::new();
    }
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(usize, (usize, Result<(), RegenFailure>))>();
        let mut pending: VecDeque<usize> = (0..pass_indices.len()).collect();
        let mut in_flight = 0usize;
        let mut in_flight_keys: HashSet<(String, String)> = HashSet::new();
        let mut done = 0usize;
        while done < pass_indices.len() {
            // First pending item whose key isn't running goes next.
            while in_flight < jobs {
                if in_flight >= 1
                    && let Some(la) = load_average
                    && crate::emerge_build::system_loadavg_1min() > la
                {
                    break;
                }
                let pos = next_dispatchable_in(&pending, &in_flight_keys, work, pass_indices);
                let Some(pos) = pos else { break };
                let slot = pending.remove(pos).expect("position came from pending");
                let idx = pass_indices[slot];
                in_flight_keys.insert((work[idx].category.clone(), work[idx].pf.clone()));
                in_flight += 1;
                let tx = tx.clone();
                let item = &work[idx];
                scope.spawn(move || {
                    let r = run_work_item(item, root, config_root, portage_tmpdir, debug);
                    let _ = tx.send((slot, (idx, r)));
                });
            }
            // Progress is guaranteed: the first dispatch is never load-gated.
            let (slot, r) = rx.recv().expect("a worker result is always pending");
            in_flight -= 1;
            let idx = pass_indices[slot];
            in_flight_keys.remove(&(work[idx].category.clone(), work[idx].pf.clone()));
            results[slot] = Some(r);
            done += 1;
        }
    });
    results
        .into_iter()
        .map(|r| r.expect("every work item reported exactly once"))
        .collect()
}

/// First pending slot whose builddir key isn't running. Split out so the
/// ordering rule is unit-testable without running a `depend` phase.
fn next_dispatchable_in(
    pending: &std::collections::VecDeque<usize>,
    in_flight_keys: &HashSet<(String, String)>,
    work: &[RegenWorkItem],
    pass_indices: &[usize],
) -> Option<usize> {
    pending.iter().position(|&slot| {
        let idx = pass_indices[slot];
        !in_flight_keys.contains(&(work[idx].category.clone(), work[idx].pf.clone()))
    })
}

/// Remove on-disk entries with no matching ebuild this run.
/// Portage: MetadataRegen.py, _cleanup.
fn prune_stale_entries(repo_location: &Path, valid: Option<&HashSet<(String, String)>>) {
    let empty = HashSet::new();
    let valid = valid.unwrap_or(&empty);
    let cache_root = repo_location.join("metadata/md5-cache");
    let Ok(cats) = portage_util::read_dir_entries(&cache_root) else {
        return;
    };
    for cat_entry in cats {
        if !cat_entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let category = cat_entry.file_name().to_string_lossy().to_string();
        let Ok(files) = portage_util::read_dir_entries(&cat_entry.path()) else {
            continue;
        };
        for f in files {
            if !f.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let pf = f.file_name().to_string_lossy().to_string();
            if !valid.contains(&(category.clone(), pf)) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
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

fn regen_one(args: RegenOneArgs<'_>) -> Result<(), RegenFailure> {
    let RegenOneArgs {
        ebuild_path,
        repo_location,
        masters,
        category,
        pf,
        root,
        config_root,
        portage_tmpdir,
        debug,
    } = args;
    // Failures carry the returncode Portage would report, so the caller replays
    // the retry decision; only a non-1 phase exit is retryable.
    let fail = |code: i32, message: String| RegenFailure { code, message };
    // A valid on-disk entry skips the `depend` phase entirely (the rewrite would
    // be byte-identical).
    if portage_repo::md5_dict::cache_entry_is_valid(
        portage_repo::md5_dict::CacheEntryIsValidArgs { ebuild_path, repo_location, masters, category, pf, store_eclass_paths: // The repo's pregen cache is `flat_hash.md5_database`:
        // `_eclasses_` is `name\tmd5` pairs (`store_eclass_paths = False`).
        false },
    ) {
        return Ok(());
    }
    let env =
        ebuild_phases::compute_environment(ebuild_path, portage_tmpdir).map_err(|e| fail(1, e))?;
    let md = ebuild_phases::run_depend_phase(&env, root, config_root, debug)
        .map_err(|e| fail(e.code, e.message))?;

    let out =
        render_entry(&md, ebuild_path, repo_location, masters, false).map_err(|e| fail(1, e))?;
    let cache_dir = repo_location.join("metadata/md5-cache").join(category);
    write_entry(&cache_dir, pf, &out).map_err(|e| fail(1, e))?;
    Ok(())
}

/// Render one `depend`-phase result as `flat_hash` cache bytes: sorted keys,
/// empty values skipped. Shared with the depcachedir write-back, which uses
/// `_eclasses_` triples instead of pairs.
/// Portage: flat_hash.py.
pub(crate) fn render_entry(
    md: &std::collections::HashMap<String, String>,
    ebuild_path: &Path,
    repo_location: &Path,
    masters: &[PathBuf],
    store_eclass_paths: bool,
) -> Result<String, String> {
    let ebuild_bytes =
        std::fs::read(ebuild_path).map_err(|e| format!("{}: {e}", ebuild_path.display()))?;
    let ebuild_md5 = format!("{:x}", Md5::digest(&ebuild_bytes));

    let mut fields: BTreeMap<&str, String> = BTreeMap::new();
    for &key in WRITE_KEYS {
        if key == "_md5_" || key == "_eclasses_" {
            continue;
        }
        if let Some(v) = md.get(key)
            && !v.is_empty()
        {
            fields.insert(key, v.clone());
        }
    }
    if let Some(ec) = eclasses_field(md, repo_location, masters, store_eclass_paths) {
        fields.insert("_eclasses_", ec);
    }
    fields.insert("_md5_", ebuild_md5);

    let mut out = String::new();
    for &key in WRITE_KEYS {
        if let Some(v) = fields.get(key) {
            out.push_str(key);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Write one rendered cache entry into `dir` via a temp sibling plus atomic
/// rename, so concurrent writers never collide. Failures are the caller's own
/// to interpret (fatal for `--regen`, best-effort for the depcachedir write-back).
/// Portage: flat_hash.py, _setitem.
pub(crate) fn write_entry(dir: &Path, pf: &str, body: &str) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(
        ".{pf}.{}.{}.regen",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let final_path = dir.join(pf);
    std::fs::rename(&tmp, &final_path).map_err(|e| format!("{}: {e}", final_path.display()))?;
    Ok(())
}

/// Serialize `_eclasses_` from the phase's `INHERITED` list, hashing the copy
/// that wins across the masters chain. The last matching tree wins, which gives
/// the identical MD5 in every case where content actually differs across the chain.
/// Portage: eclass_cache.py.
fn eclasses_field(
    md: &std::collections::HashMap<String, String>,
    repo_location: &Path,
    masters: &[PathBuf],
    store_eclass_paths: bool,
) -> Option<String> {
    let names = md.get("INHERITED").map(String::as_str).unwrap_or("");
    let names: Vec<&str> = names.split_whitespace().collect();
    if names.is_empty() {
        return None;
    }
    let porttrees: Vec<&Path> = masters
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(repo_location))
        .collect();
    let mut parts = Vec::new();
    for name in names {
        let (tree, bytes) = porttrees.iter().rev().find_map(|tree| {
            let path = tree.join("eclass").join(format!("{name}.eclass"));
            std::fs::read(&path).ok().map(|bytes| (*tree, bytes))
        })?;
        parts.push(name.to_string());
        if store_eclass_paths {
            // Triples carry the eclass directory between name and checksum.
            parts.push(tree.join("eclass").display().to_string());
        }
        parts.push(format!("{:x}", Md5::digest(&bytes)));
    }
    Some(parts.join("\t"))
}
