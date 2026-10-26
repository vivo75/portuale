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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

pub fn run(
    config_root: &Path,
    root: &Path,
    debug: bool,
    jobs: usize,
    load_average: Option<f64>,
) -> ExitCode {
    // `--debug`: run each `depend` phase with `PORTAGE_DEBUG=1`.
    let repos = match portage_repo::find_repos(config_root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };

    // A repo whose formats lack `md5-dict` is skipped: portuale has no `pms`
    // writer, and writing the md5-cache there would be the wrong directory.
    let mut skipped_repo = false;
    let repos: Vec<portage_repo::RepoConfig> = repos
        .into_iter()
        .filter(|repo| {
            if portage_repo::regen_writes_md5_cache(repo) {
                return true;
            }
            eprintln!(
                "emerge: --regen: repository '{}': cache-formats = '{}' has no md5-dict \
                 format; skipping (no pms cache writer)",
                repo.name,
                repo.cache_formats.join(" ")
            );
            skipped_repo = true;
            false
        })
        .collect();

    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();

    // Work is collected before any phase runs; each pass's `Processing <cp>`
    // lines print up front. Retries cover unexpectedly-failed cps up to
    // `MAX_TRIES`; `done!` prints once at the very end.
    let mut valid_per_repo: HashMap<PathBuf, HashSet<(String, String)>> = HashMap::new();
    let mut work: Vec<RegenWorkItem> = Vec::new();
    let mut cps_in_order: Vec<String> = Vec::new();
    let categories = portage_repo::Categories::load(&repos, config_root);
    for cp in regen_cps(&repos, &categories.set) {
        let (category, package) = match cp.split_once('/') {
            Some(x) => x,
            None => continue,
        };
        cps_in_order.push(cp.clone());
        for repo in &repos {
            let pkg_dir = repo.location.join(category).join(package);
            let Ok(entries) = portage_util::read_dir_entries(&pkg_dir) else {
                continue;
            };
            for entry in entries {
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
                    cp: cp.clone(),
                });
            }
        }
    }

    // Only the last outcome per item matters.
    let mut results: Vec<Option<Result<(), RegenFailure>>> =
        (0..work.len()).map(|_| None).collect();
    let mut pending_cps: Vec<String> = cps_in_order.clone();
    let mut tries = MAX_TRIES;
    while !pending_cps.is_empty() && tries > 0 {
        println!("Regenerating cache entries...");
        for cp in &pending_cps {
            println!("Processing {cp}");
        }
        let pending_set: HashSet<&str> = pending_cps.iter().map(String::as_str).collect();
        let pass_indices: Vec<usize> = (0..work.len())
            .filter(|&i| pending_set.contains(work[i].cp.as_str()))
            .collect();
        let pass_results: Vec<(usize, Result<(), RegenFailure>)> = if jobs <= 1 {
            pass_indices
                .into_iter()
                .map(|i| {
                    (
                        i,
                        run_work_item(&work[i], root, config_root, &portage_tmpdir, debug),
                    )
                })
                .collect()
        } else {
            run_parallel(RunParallelArgs {
                work: &work,
                pass_indices: &pass_indices,
                root,
                config_root,
                portage_tmpdir: &portage_tmpdir,
                debug,
                jobs,
                load_average,
            })
        };
        for (i, r) in pass_results {
            results[i] = Some(r);
        }
        tries -= 1;
        let unexpected_cps: HashSet<String> = work
            .iter()
            .zip(results.iter())
            .filter_map(|(item, r)| match r {
                Some(Err(f)) if is_retryable_returncode(f.code) => Some(item.cp.clone()),
                _ => None,
            })
            .collect();
        pending_cps = if tries > 0 {
            select_retry_cps(&cps_in_order, &unexpected_cps)
        } else {
            Vec::new()
        };
    }

    let mut failures = 0u32;
    for (item, r) in work.iter().zip(results.iter()) {
        if let Some(Err(f)) = r {
            eprintln!(" * {}", f.message);
            failures += 1;
            // A finally-failed cpv is dropped from the valid set, so pruning
            // deletes any stale on-disk entry for it.
            if let Some(valid) = valid_per_repo.get_mut(&item.repo_location) {
                valid.remove(&(item.category.clone(), item.pf.clone()));
            }
        }
    }

    // Prune on-disk entries whose ebuild no longer exists in that repo.
    for repo in &repos {
        let valid = valid_per_repo.get(&repo.location);
        prune_stale_entries(&repo.location, valid);
    }

    println!("done!");
    if failures == 0 && !skipped_repo {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Every cp the traversal visits: the allow-listed `all_cp` walk (on-disk
/// cps in unlisted categories are never yielded).
/// Portage: MetadataRegen._iter_every_cp over cp_all(categories=...),
/// porttree.py:1097-1128.
fn regen_cps(repos: &[portage_repo::RepoConfig], categories: &BTreeSet<String>) -> Vec<String> {
    portage_repo::all_cp(repos, categories)
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
    let ebuild_md5 = portage_util::hex_lower(&Md5::digest(&ebuild_bytes));

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
        parts.push(portage_util::hex_lower(&Md5::digest(&bytes)));
    }
    Some(parts.join("\t"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::collections::VecDeque;

    fn work_item(category: &str, pf: &str) -> RegenWorkItem {
        RegenWorkItem {
            ebuild_path: PathBuf::from(format!("/repo/{category}/pkg/{pf}.ebuild")),
            repo_location: PathBuf::from("/repo"),
            masters: Vec::new(),
            category: category.to_string(),
            pf: pf.to_string(),
            cp: format!("{category}/pkg"),
        }
    }

    fn key(category: &str, pf: &str) -> (String, String) {
        (category.to_string(), pf.to_string())
    }

    /// Traversal drops on-disk cps whose category is absent from
    /// `settings.categories`: a valid ebuild under an unlisted category is
    /// never visited (the `kde-base/ark` shape; live pair: portuale printed
    /// `Processing zz-unlisted/minipkg` where host Portage 3.0.83 drops it).
    /// Portage: MetadataRegen._iter_every_cp over
    /// cp_all(categories=settings.categories), porttree.py:1097-1128.
    #[test]
    fn regen_traversal_drops_cps_in_unlisted_categories() {
        let dir = TempDir::new("portuale-regen-categories").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        for (category, package) in [("dev-libs", "listedpkg"), ("zz-unlisted", "droppedpkg")] {
            let ebuild = repo
                .join(category)
                .join(package)
                .join(format!("{package}-1.0.ebuild"));
            std::fs::create_dir_all(ebuild.parent().unwrap()).unwrap();
            std::fs::write(&ebuild, "EAPI=8\n").unwrap();
        }
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
        std::fs::write(repo.join("profiles").join("repo_name"), "regentest\n").unwrap();
        std::fs::write(repo.join("profiles").join("categories"), "dev-libs\n").unwrap();
        let cfg = dir.join("cfg");
        let repos = vec![portage_repo::RepoConfig {
            name: "regentest".to_string(),
            location: repo,
            priority: -1000,
            is_main: true,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: Vec::new(),
        }];
        let cps = regen_cps(&repos, &portage_repo::Categories::load(&repos, &cfg).set);
        assert_eq!(cps, vec!["dev-libs/listedpkg".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Dispatch order skips only keys already running.
    #[test]
    fn dispatch_takes_the_first_item_whose_key_is_not_running() {
        let work = vec![
            work_item("dev-libs", "a-1.0"),
            work_item("dev-libs", "a-1.0"),
            work_item("dev-libs", "b-1.0"),
        ];
        let pass: Vec<usize> = vec![0, 1, 2];
        let pending: VecDeque<usize> = (0..3).collect();
        assert_eq!(
            next_dispatchable_in(&pending, &HashSet::new(), &work, &pass),
            Some(0)
        );
        let running: HashSet<(String, String)> = [key("dev-libs", "a-1.0")].into();
        assert_eq!(
            next_dispatchable_in(&pending, &running, &work, &pass),
            Some(2)
        );
        let all: HashSet<(String, String)> =
            [key("dev-libs", "a-1.0"), key("dev-libs", "b-1.0")].into();
        assert_eq!(next_dispatchable_in(&pending, &all, &work, &pass), None);
        assert_eq!(
            next_dispatchable_in(&VecDeque::new(), &HashSet::new(), &work, &pass),
            None
        );
        let subset: Vec<usize> = vec![2, 0];
        let sub_pending: VecDeque<usize> = (0..2).collect();
        assert_eq!(
            next_dispatchable_in(&sub_pending, &running, &work, &subset),
            Some(0)
        );
    }

    /// Only non-1 returncodes retry, up to `MAX_TRIES`, in first-seen order.
    /// Portage: MetadataRegen.py.
    #[test]
    fn retry_decision_matches_portage_task_exit() {
        assert!(!is_retryable_returncode(1));
        assert!(is_retryable_returncode(2));
        assert!(is_retryable_returncode(-1));
        assert!(is_retryable_returncode(130));
        assert_eq!(MAX_TRIES, 3);
    }

    #[test]
    fn retry_set_is_the_unexpectedly_failed_cps_in_first_seen_order() {
        let cps = ["b/cp", "a/cp", "c/cp"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let unexpected: HashSet<String> =
            ["c/cp", "b/cp"].into_iter().map(str::to_string).collect();
        assert_eq!(
            select_retry_cps(&cps, &unexpected),
            vec!["b/cp".to_string(), "c/cp".to_string()]
        );
        assert!(select_retry_cps(&cps, &HashSet::new()).is_empty());
    }

    /// `render_entry` serves both `_eclasses_` shapes; `INHERITED` itself is
    /// never written.
    /// Portage: EbuildMetadataPhase.
    #[test]
    fn render_entry_writes_the_eclass_shape_of_each_rung() {
        let dir = TempDir::new("portuale-regen-render").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let ebuild = repo.join("dev-libs/pkg/pkg-1.0.ebuild");
        std::fs::create_dir_all(ebuild.parent().unwrap()).unwrap();
        std::fs::create_dir_all(repo.join("eclass")).unwrap();
        std::fs::write(&ebuild, "EAPI=8\n").unwrap();
        std::fs::write(repo.join("eclass/myclass.eclass"), "# eclass\n").unwrap();

        let mut md = std::collections::HashMap::new();
        md.insert("EAPI".to_string(), "8".to_string());
        md.insert("INHERITED".to_string(), "myclass".to_string());
        let eclass_md5 = portage_util::hex_lower(&Md5::digest(b"# eclass\n"));

        let pairs = render_entry(&md, &ebuild, &repo, &[], false).unwrap();
        assert!(
            pairs.contains(&format!("_eclasses_=myclass\t{eclass_md5}\n")),
            "{pairs}"
        );
        assert!(!pairs.contains("INHERITED="), "{pairs}");

        let triples = render_entry(&md, &ebuild, &repo, &[], true).unwrap();
        assert!(
            triples.contains(&format!(
                "_eclasses_=myclass\t{}\t{eclass_md5}\n",
                repo.join("eclass").display()
            )),
            "{triples}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A valid on-disk entry skips the `depend` phase entirely: the ebuild
    /// below cannot be sourced, so `Ok` plus an untouched cache file proves
    /// no phase ran.
    /// Portage: MetadataRegen._iter_metadata_processes (MetadataRegen.py:114-120).
    #[test]
    fn regen_one_skips_the_depend_phase_for_a_valid_cache_entry() {
        let dir = TempDir::new("portuale-regen-skip").keep();
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let ebuild = repo.join("dev-libs/pkg/pkg-1.0.ebuild");
        std::fs::create_dir_all(ebuild.parent().unwrap()).unwrap();
        // Unsourcable on purpose: reaching the phase is the failure.
        let ebuild_bytes = b"EAPI=8\nthis is not valid bash (((\n";
        std::fs::write(&ebuild, ebuild_bytes).unwrap();
        let cache_file = repo.join("metadata/md5-cache/dev-libs/pkg-1.0");
        std::fs::create_dir_all(cache_file.parent().unwrap()).unwrap();
        let entry = format!(
            "EAPI=8\n_md5_={}\n",
            portage_util::hex_lower(&Md5::digest(ebuild_bytes))
        );
        std::fs::write(&cache_file, &entry).unwrap();

        let tmp = dir.join("tmp");
        let r = regen_one(RegenOneArgs {
            ebuild_path: &ebuild,
            repo_location: &repo,
            masters: &[],
            category: "dev-libs",
            pf: "pkg-1.0",
            root: &tmp,
            config_root: &tmp,
            portage_tmpdir: &tmp,
            debug: false,
        });
        assert!(r.is_ok());
        assert_eq!(std::fs::read_to_string(&cache_file).unwrap(), entry);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
