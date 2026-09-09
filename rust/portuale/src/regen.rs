//! `emerge --regen` (real `_emerge/actions.py::action_regen` +
//! `_emerge/MetadataRegen.py`): regenerate every repo's on-disk
//! `metadata/md5-cache/<cat>/<pf>` by running each ebuild's `depend`
//! phase and writing the result in real `portage.cache.flat_hash`
//! (`md5_database`) format -- `KEY=value` lines, keys sorted, empty
//! values omitted, `_md5_=<md5 of the ebuild file>` last.
//!
//! Real portage runs the `depend` phases through its scheduler, up to
//! `--jobs` concurrently with the `--load-average` gate (real
//! `action_regen(settings, portdb, max_jobs, max_load)` ->
//! `MetadataRegen(portdb, max_jobs, max_load)` -> `AsyncScheduler` +
//! `PollScheduler._can_add_job`: at most `max_jobs` tasks in flight, and
//! no *additional* task while the 1-minute load average is at or above
//! `max_load` -- the first task always runs so the scheduler can't
//! deadlock; a bare `--jobs` is unlimited (`True`), `--jobs=0` means the
//! CPU count (real `main.py:1023-1041`), absent means serial). Each
//! ebuild's `depend` phase is independent, so the cache *content* a
//! `--jobs`-threaded run writes is byte-identical to the serial one --
//! this port runs the same work list through a `std::thread::scope`
//! dispatch loop with the same two gates (see `emerge_build.rs`'s own
//! `--jobs` scheduler, which shares `system_loadavg_1min`).
//!
//! Two deliberate, documented divergences, both in service of portuale's
//! determinism (a hard constraint -- real interleaves completions
//! nondeterministically on stderr):
//! - `Processing <cp>` lines print up front in `cp` order, before any
//!   work is dispatched (real prints them from its `_process_iter`
//!   generator while tasks run concurrently).
//! - failure lines (` * <err>`) report in work-list order after every
//!   worker joins, not in completion order (real `_task_exit` reports as
//!   each task exits). Same failure set, same exit code, stable order.
//! - same-`(category, pf)` items never run concurrently: the builddir
//!   (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`, incl. `temp/
//!   .depend-metadata`) is keyed by cpv, not by repo, so an overlay and
//!   its master carrying the same version would share it. Real serializes
//!   the same sharing the other way -- real `doebuild()` takes a
//!   per-builddir lock (`EbuildBuildDir.async_lock()`); portuale holds
//!   the second item back at dispatch instead of blocking inside the
//!   phase, same net effect with no lock machinery.
//!
//! Stale-entry pruning and the eclass masters chain (below) *do* change
//! on-disk output and are implemented.
//!
//! Like every real filesystem-mutating `emerge` action, this rejects
//! `--pretend` (real `actions.py:4106-4111`) at the CLI layer before
//! reaching here.

use crate::ebuild_phases;
use md5::{Digest as _, Md5};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Real `portage.dbapi.porttree`'s own md5-cache key set
/// (`portage.auxdbkeys`), plus the two synthetic keys
/// `portage.cache.flat_hash` always adds (`_eclasses_`, `_<chf>_` =
/// `_md5_`). Real `flat_hash.database.__init__` writes
/// `sorted(known_keys | {"_eclasses_", "_md5_"})` -- and `_` (0x5F) sorts
/// after every uppercase letter, so the two synthetic keys land last.
const WRITE_KEYS: &[&str] = &[
    "BDEPEND",
    "DEFINED_PHASES",
    "DEPEND",
    "DESCRIPTION",
    "EAPI",
    "HOMEPAGE",
    "IDEPEND",
    "INHERIT",
    "INHERITED",
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

/// One ebuild's `depend`-phase unit of `MetadataRegen` work: the ebuild
/// file plus the repo whose cache entry it (re)writes. `category`/`pf`
/// double as the dispatch key -- see `run_parallel`'s own doc comment.
struct RegenWorkItem {
    ebuild_path: PathBuf,
    repo_location: PathBuf,
    masters: Vec<PathBuf>,
    category: String,
    pf: String,
}

pub fn run(
    config_root: &Path,
    root: &Path,
    debug: bool,
    jobs: usize,
    load_average: Option<f64>,
) -> ExitCode {
    // `--debug`/`-d`: run each `depend` phase with `PORTAGE_DEBUG=1`
    // (real `bin/ebuild.sh` `set -x`).
    let repos = match portage_repo::find_repos(config_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp/portage"));

    // Real `MetadataRegen._iter_metadata_processes`: iterate `cp_all()`
    // and, per `cat/pkg`, every ebuild version. `Regenerating cache
    // entries...` then `Processing <cp>` per cp (stdout), `done!` at the
    // end (`action_regen`).
    println!("Regenerating cache entries...");

    let mut failures = 0u32;
    // Real `MetadataRegen`'s own `_valid_pkgs`: every `(category, pf)`
    // actually found on disk, per repo location -- fed to `_cleanup`'s
    // "global cleanse" diff against the on-disk cache afterward
    // (`MetadataRegen.py:142-189`). Plain `emerge --regen` (no explicit
    // `cp` filter) always runs the global-cleanse variant.
    let mut valid_per_repo: HashMap<PathBuf, HashSet<(String, String)>> = HashMap::new();
    // The full `MetadataRegen._process_iter` work list, collected (and
    // its `Processing <cp>` lines printed) before any `depend` phase runs
    // -- see the module doc comment for why the lines print up front.
    let mut work: Vec<RegenWorkItem> = Vec::new();
    for cp in portage_repo::all_cp(&repos) {
        println!("Processing {cp}");
        let (category, package) = match cp.split_once('/') {
            Some(x) => x,
            None => continue,
        };
        for repo in &repos {
            let pkg_dir = repo.location.join(category).join(package);
            let Ok(entries) = std::fs::read_dir(&pkg_dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let Some(pf) = name.strip_suffix(".ebuild") else {
                    continue;
                };
                valid_per_repo
                    .entry(repo.location.clone())
                    .or_default()
                    .insert((category.to_string(), pf.to_string()));
                work.push(RegenWorkItem {
                    ebuild_path: entry.path(),
                    repo_location: repo.location.clone(),
                    masters: repo.masters.clone(),
                    category: category.to_string(),
                    pf: pf.to_string(),
                });
            }
        }
    }

    let results: Vec<Result<(), String>> = if jobs <= 1 {
        work.iter()
            .map(|item| run_work_item(item, root, config_root, &portage_tmpdir, debug))
            .collect()
    } else {
        run_parallel(
            &work,
            root,
            config_root,
            &portage_tmpdir,
            debug,
            jobs,
            load_average,
        )
    };
    for r in results {
        if let Err(e) = r {
            eprintln!(" * {e}");
            failures += 1;
        }
    }

    // Prune stale entries: an on-disk `metadata/md5-cache/<cat>/<pf>`
    // whose ebuild no longer exists in that same repo (real `_cleanup`'s
    // `dead_nodes` diff -- `del auxdb[y]` per repo location).
    for repo in &repos {
        let valid = valid_per_repo.get(&repo.location);
        prune_stale_entries(&repo.location, valid);
    }

    println!("done!");
    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// One `RegenWorkItem`'s `depend` phase + cache write (real
/// `EbuildMetadataPhase` + `portdb._write_cache`, via `regen_one`).
fn run_work_item(
    item: &RegenWorkItem,
    root: &Path,
    config_root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<(), String> {
    regen_one(
        &item.ebuild_path,
        &item.repo_location,
        &item.masters,
        &item.category,
        &item.pf,
        root,
        config_root,
        portage_tmpdir,
        debug,
    )
}

/// Real `AsyncScheduler._schedule_tasks` for the `MetadataRegen` case:
/// dispatch work-list items (in order) onto up to `jobs` worker threads,
/// holding off *additional* workers while the 1-minute load average is at
/// or above `load_average` (real `PollScheduler._can_add_job` -- the
/// first worker always runs, so this can't deadlock), and never running
/// two items with the same `(category, pf)` concurrently (the builddir is
/// shared per cpv across repos -- see the module doc comment). Results
/// come back in work-list order, so the caller's failure report is
/// deterministic regardless of completion order.
#[allow(clippy::too_many_arguments)]
fn run_parallel(
    work: &[RegenWorkItem],
    root: &Path,
    config_root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
    jobs: usize,
    load_average: Option<f64>,
) -> Vec<Result<(), String>> {
    use std::collections::VecDeque;
    use std::sync::mpsc;

    let mut results: Vec<Option<Result<(), String>>> = (0..work.len()).map(|_| None).collect();
    if work.is_empty() {
        return Vec::new();
    }
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(usize, Result<(), String>)>();
        let mut pending: VecDeque<usize> = (0..work.len()).collect();
        let mut in_flight = 0usize;
        let mut in_flight_keys: HashSet<(String, String)> = HashSet::new();
        let mut done = 0usize;
        while done < work.len() {
            // Dispatch in work-list order while there is capacity. The
            // first pending item whose builddir key isn't already running
            // goes next; anything key-blocked waits for a completion.
            while in_flight < jobs {
                if in_flight >= 1
                    && let Some(la) = load_average
                    && crate::emerge_build::system_loadavg_1min() > la
                {
                    break;
                }
                let pos = next_dispatchable(&pending, &in_flight_keys, work);
                let Some(pos) = pos else { break };
                let idx = pending.remove(pos).expect("position came from pending");
                in_flight_keys.insert((work[idx].category.clone(), work[idx].pf.clone()));
                in_flight += 1;
                let tx = tx.clone();
                let item = &work[idx];
                scope.spawn(move || {
                    let r = run_work_item(item, root, config_root, portage_tmpdir, debug);
                    let _ = tx.send((idx, r));
                });
            }
            // Progress is guaranteed: the load gate never blocks the
            // first dispatch, and with nothing in flight no key is
            // blocked -- so a pending item always dispatches, and a
            // completion is always on its way when we wait here.
            let (idx, r) = rx.recv().expect("a worker result is always pending");
            in_flight -= 1;
            in_flight_keys.remove(&(work[idx].category.clone(), work[idx].pf.clone()));
            results[idx] = Some(r);
            done += 1;
        }
    });
    results
        .into_iter()
        .map(|r| r.expect("every work item reported exactly once"))
        .collect()
}

/// First position in `pending` whose builddir key isn't already running
/// -- the "in work-list order, skip what would race" half of
/// `run_parallel`'s dispatch. Split out so the ordering/blocking rule is
/// unit-testable without running a `depend` phase.
fn next_dispatchable(
    pending: &std::collections::VecDeque<usize>,
    in_flight_keys: &HashSet<(String, String)>,
    work: &[RegenWorkItem],
) -> Option<usize> {
    pending.iter().position(|&idx| {
        !in_flight_keys.contains(&(work[idx].category.clone(), work[idx].pf.clone()))
    })
}

/// Real `MetadataRegen._cleanup`'s "global cleanse" (`MetadataRegen.py:
/// 142-166`): every on-disk cache entry under `repo_location`'s
/// `metadata/md5-cache/<cat>/` not in `valid` (i.e. no matching ebuild
/// was found this run) gets removed. `valid` is `None` when the repo
/// had no `cp` at all this run -- every existing entry is then stale.
fn prune_stale_entries(repo_location: &Path, valid: Option<&HashSet<(String, String)>>) {
    let empty = HashSet::new();
    let valid = valid.unwrap_or(&empty);
    let cache_root = repo_location.join("metadata/md5-cache");
    let Ok(cats) = std::fs::read_dir(&cache_root) else {
        return;
    };
    for cat_entry in cats.filter_map(Result::ok) {
        if !cat_entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let category = cat_entry.file_name().to_string_lossy().to_string();
        let Ok(files) = std::fs::read_dir(cat_entry.path()) else {
            continue;
        };
        for f in files.filter_map(Result::ok) {
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

#[allow(clippy::too_many_arguments)]
fn regen_one(
    ebuild_path: &Path,
    repo_location: &Path,
    masters: &[PathBuf],
    category: &str,
    pf: &str,
    root: &Path,
    config_root: &Path,
    portage_tmpdir: &Path,
    debug: bool,
) -> Result<(), String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let md = ebuild_phases::run_depend_phase(&env, root, config_root, debug)?;

    let ebuild_bytes =
        std::fs::read(ebuild_path).map_err(|e| format!("{}: {e}", ebuild_path.display()))?;
    let ebuild_md5 = format!("{:x}", Md5::digest(&ebuild_bytes));

    // Real `flat_hash._setitem`: `for k in self._write_keys: v =
    // values.get(k); if not v: continue; write f"{k}={v}\n"`. Sorted
    // keys, empty values skipped.
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
    if let Some(ec) = eclasses_field(&md, repo_location, masters) {
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

    let cache_dir = repo_location.join("metadata/md5-cache").join(category);
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    let cache_file = cache_dir.join(pf);
    let tmp = cache_dir.join(format!(".{pf}.regen"));
    std::fs::write(&tmp, out.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &cache_file).map_err(|e| format!("{}: {e}", cache_file.display()))?;
    Ok(())
}

/// Real `metadata.database._setitem`: `_eclasses_` is serialized as
/// `name\tmd5\tname\tmd5...` (`serialize_eclasses(..., "md5")`). Built
/// from the `depend` phase's own `INHERITED` list -- for each eclass
/// name, the md5 of the file that wins across the repo's own masters
/// chain.
///
/// Real `eclass_cache.cache.update_eclasses` (`eclass_cache.py:108-148`)
/// walks `porttrees` (`masters` in declared order, then the repo itself
/// -- `repository/config.py:1267-1276`) and, for a same-named eclass
/// present in more than one tree, keeps the *earliest* (most-master)
/// copy only when a later tree's copy has the exact same `mtime`
/// (treated as "identical to the master"); a later tree with a
/// genuinely differing copy overrides it. Since `mtime` is a real-world
/// proxy for "did the content change", and this crate only ever cares
/// about the resulting content hash, the equivalent, simpler rule used
/// here is: the *last* tree in `masters`-then-self order that has the
/// file wins outright. This produces the identical MD5 in every case
/// where content actually differs across the chain (real's whole
/// mtime-equality dance only matters when content is identical anyway,
/// in which case either copy's MD5 is the same) -- a documented,
/// behavior-preserving simplification.
fn eclasses_field(
    md: &std::collections::HashMap<String, String>,
    repo_location: &Path,
    masters: &[PathBuf],
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
        let bytes = porttrees.iter().rev().find_map(|tree| {
            std::fs::read(tree.join("eclass").join(format!("{name}.eclass"))).ok()
        })?;
        parts.push(name.to_string());
        parts.push(format!("{:x}", Md5::digest(&bytes)));
    }
    Some(parts.join("\t"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn work_item(category: &str, pf: &str) -> RegenWorkItem {
        RegenWorkItem {
            ebuild_path: PathBuf::from(format!("/repo/{category}/pkg/{pf}.ebuild")),
            repo_location: PathBuf::from("/repo"),
            masters: Vec::new(),
            category: category.to_string(),
            pf: pf.to_string(),
        }
    }

    fn key(category: &str, pf: &str) -> (String, String) {
        (category.to_string(), pf.to_string())
    }

    /// Dispatch is work-list order, skipping only what would share a
    /// builddir with something already running (real `doebuild()`'s own
    /// per-builddir lock, held at dispatch instead of inside the phase).
    #[test]
    fn dispatch_takes_the_first_item_whose_key_is_not_running() {
        let work = vec![
            work_item("dev-libs", "a-1.0"),
            work_item("dev-libs", "a-1.0"),
            work_item("dev-libs", "b-1.0"),
        ];
        let pending: VecDeque<usize> = (0..3).collect();
        // Nothing running: the head goes.
        assert_eq!(next_dispatchable(&pending, &HashSet::new(), &work), Some(0));
        // Head's key running: the same-key second item is skipped, the
        // next key goes.
        let running: HashSet<(String, String)> = [key("dev-libs", "a-1.0")].into();
        assert_eq!(next_dispatchable(&pending, &running, &work), Some(2));
        // Every key running: nothing is dispatchable (the caller waits
        // for a completion instead of spinning).
        let all: HashSet<(String, String)> =
            [key("dev-libs", "a-1.0"), key("dev-libs", "b-1.0")].into();
        assert_eq!(next_dispatchable(&pending, &all, &work), None);
        // Empty queue: nothing to dispatch.
        assert_eq!(
            next_dispatchable(&VecDeque::new(), &HashSet::new(), &work),
            None
        );
    }
}
