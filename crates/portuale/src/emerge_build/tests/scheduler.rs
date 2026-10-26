use super::*;

/// Backlog #197 fix round 1: one failed `-jN` build fires each
/// failure event exactly once. Real `_build_exit`'s failure arm
/// assigns absolutely (`Scheduler.py:1662`, `failed =
/// len(self._failed_pkgs)` -- 1 for a single failure), then drops
/// the freed build slot (`running`, `:1663-1665`): exactly two
/// `>>> Jobs:` lines, ending at `failed == 1`. The pre-fix code
/// fired `job_failed` + `build_finished` twice (once in the `Err`
/// arm, once in the `Some` arm), reaching `failed == 2`. Drives
/// the real scheduler (`run_build_scheduler`) over the `schedbad`
/// fixture (its `src_install` dies) with a caller-owned display,
/// so the counters pin the wiring, not just the display.
#[test]
fn run_build_scheduler_single_failure_counts_failed_exactly_once() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();

    let bad = source_entry(
        "schedbad",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let entries = vec![bad];

    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let display = StatusDisplay::new(
        StatusMode {
            background: true,
            show_jobs: false,
        },
        1,
        crate::color::Colorizer::new(false),
    );
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    let err = run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect_err("schedbad's own failure must fail the run");
    // The `Failed to emerge` block is printed; the error only says so.
    assert!(is_reported_failure(&err.message), "{err}");
    assert_eq!(
        display.jobs_line(),
        ">>> Jobs: 0 of 1 complete, 1 failed                                     "
    );
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&portage_tmpdir);
}

#[test]
fn system_loadavg_1min_reads_a_plausible_value() {
    // Linux CI: /proc/loadavg exists and its first field is a
    // non-negative float. Anywhere it can't be read, the throttle is
    // simply disabled (0.0), never a stall.
    let la = system_loadavg_1min();
    assert!(la >= 0.0 && la.is_finite(), "{la}");
}

/// Backlog #376: a build thread that panics reports a failure for its
/// entry instead of leaving the scheduler waiting forever on its result.
#[test]
fn a_panicking_build_thread_becomes_a_failure_message() {
    let entry = source_entry(
        "schedbad",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let payload = std::panic::catch_unwind(|| panic!("boom {}", 7)).unwrap_err();
    let msg = super::scheduler::worker_panic_message(&entry, payload);
    assert!(msg.contains("schedbad"), "{msg}");
    assert!(msg.contains("build thread panicked: boom 7"), "{msg}");
}

fn vdb_mtime(root: &std::path::Path, cpv: &str) -> std::time::SystemTime {
    let dir = root.join("var/db/pkg").join(cpv);
    std::fs::metadata(&dir)
        .unwrap_or_else(|e| panic!("{} should exist after the run: {e}", dir.display()))
        .modified()
        .expect("mtime readable")
}

fn scope_options(
    config_root: &std::path::Path,
    system: &[&str],
    scope: crate::pretend::MergeWaitScope,
) -> ebuild_merge::MergeOptions {
    ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.to_path_buf(),
        resolved_config: Some(std::sync::Arc::new(portage_profile::Config {
            system_packages: system.iter().map(|s| s.to_string()).collect(),
            ..portage_profile::Config::default()
        })),
        merge_wait_scope: scope,
        // Pinned premise, not ambient state: without this the run-wide
        // list falls back to the process `$FEATURES`, so an exported
        // `FEATURES=merge-wait` would barrier even the `none` run.
        // `parallel-fetch` is inert here (no `SRC_URI` anywhere).
        features: "parallel-fetch".to_string(),
        ..ebuild_merge::MergeOptions::default()
    }
}

fn scheduler_display(max: usize) -> StatusDisplay {
    StatusDisplay::new(
        StatusMode {
            background: true,
            show_jobs: false,
        },
        max,
        crate::color::Colorizer::new(false),
    )
}

/// Oracle: `LLM/evidence/2026-10-23-512/captures/01-scope-deep.portage.*` —
/// the `@system` merge lands with the slow one, under `1 merge wait`.
/// Backlog #512, item (1).
#[test]
fn merge_wait_scope_deep_holds_a_system_merge_until_builds_drain() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let entries = vec![new("schedsys"), new("schedwait")];
    let options = scope_options(
        &config_root,
        &["*dev-libs/schedsys"],
        crate::pretend::MergeWaitScope::Deep,
    );
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("both entries merge");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") > vdb_mtime(&root, "dev-libs/schedwait-1.0"),
        "the system merge lands after the slow build drains"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Oracle: `04-scope-none.portage.*` — nothing barriers, so the fast
/// system merge lands long before the slow one. Backlog #512, item (1).
#[test]
fn merge_wait_scope_none_merges_everything_in_parallel() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let entries = vec![new("schedsys"), new("schedwait")];
    let options = scope_options(
        &config_root,
        &["*dev-libs/schedsys"],
        crate::pretend::MergeWaitScope::None,
    );
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("both entries merge");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") < vdb_mtime(&root, "dev-libs/schedwait-1.0"),
        "without a barrier the fast merge lands first"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Oracle: `09-cycle.portage.*` — `schedcycb` installs before `schedcyca`
/// (rc 0). Before the fix portuale merged nothing here: every build waited
/// on a merge that could never start. Backlog #512, item (3).
#[test]
fn cycle_post_merge_dep_does_not_gate_the_build() {
    use portage_repo::{DepEdge, DepPriority};
    let edge = |package: &str, key: u8, priority: DepPriority| DepEdge {
        atom: format!("dev-libs/{package}"),
        evaluated: format!("dev-libs/{package}"),
        category: "dev-libs".into(),
        package: package.into(),
        priority,
        disjunctive: false,
        alt: None,
        key,
    };
    let mut a = source_entry(
        "schedcyca",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    a.deps = vec![edge(
        "schedcycb",
        0,
        DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
    )];
    a.required_by = vec![("dev-libs".into(), "schedcycb".into())];
    let mut b = source_entry(
        "schedcycb",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    b.deps = vec![edge(
        "schedcyca",
        2,
        DepPriority {
            runtime_post: true,
            ..DepPriority::default()
        },
    )];
    b.required_by = vec![("dev-libs".into(), "schedcyca".into())];
    let entries = vec![a, b];
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("the cycle merges");
    assert!(
        vdb_mtime(&root, "dev-libs/schedcycb-1.0") < vdb_mtime(&root, "dev-libs/schedcyca-1.0"),
        "the post-merge edge breaks the cycle: cycb first, like Portage"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Oracle: `10-pretend.portage.*` — each package's pretend block replays
/// whole, in merge-list order. Backlog #512, item (2).
#[test]
fn pkg_pretend_replays_in_merge_list_order() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let entries = [new("schedppa"), new("schedppb")];
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let mut replay: Vec<u8> = Vec::new();
    let wrapped: Vec<SourcePretendEntry<'_>> = entries
        .iter()
        .map(|entry| SourcePretendEntry {
            entry,
            repos: &repos,
            root: &root,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
        })
        .collect();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &wrapped,
        jobs: 2,
        replay: &mut replay,
    });
    assert!(failures.is_empty(), "{failures:?}");
    let text = String::from_utf8_lossy(&replay);
    let a = text
        .find("SCHED-PRETEND schedppa")
        .expect("ppa's block replays");
    let b = text
        .find("SCHED-PRETEND schedppb")
        .expect("ppb's block replays");
    assert!(a < b, "merge-list order, not finish order:\n{text}");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// A dying `pkg_pretend` reports the run and never starts a build: the
/// slow entry's build log is never even opened. Backlog #512, item (2).
#[test]
fn pkg_pretend_failure_aborts_before_anything_builds() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let entries = vec![new("schedppfail"), new("schedwait")];
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    let err = run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect_err("a dying pretend fails the run");
    assert!(is_reported_failure(&err.message), "{err}");
    assert!(
        !root.join("var/db/pkg/dev-libs/schedppfail-1.0").exists(),
        "nothing merges after a pretend failure"
    );
    assert!(
        !root.join("var/db/pkg/dev-libs/schedwait-1.0").exists(),
        "the slow entry never builds"
    );
    assert!(
        !portage_tmpdir
            .join("portage/dev-libs/schedwait-1.0/temp/build.log")
            .exists(),
        "the slow entry's build never starts"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Oracle: `07-system-closure.portage.*` — under `system`, a runtime-only
/// reachable package merges in parallel while the `@system` member waits.
/// Backlog #512, item (1); review follow-up I2 (the `deep`/`none` pair
/// alone would not catch a mis-wired scope set).
#[test]
fn merge_wait_scope_system_holds_only_system_members() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    // `schedsys` is the `@system` member here; `schedwait` is reachable
    // from nothing and only slows the run down.
    let entries = vec![new("schedsys"), new("schedwait")];
    let options = scope_options(
        &config_root,
        &["*dev-libs/schedsys"],
        crate::pretend::MergeWaitScope::System,
    );
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("both entries merge");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") > vdb_mtime(&root, "dev-libs/schedwait-1.0"),
        "the system merge lands after the slow build drains"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Portage queues every finished gated merge and drains the queue in
/// build-finish order: `_build_exit` appends unconditionally
/// (`Scheduler.py:1833-1836`) and `_schedule_tasks` moves the whole queue
/// to the merge queue once no job and no merge runs
/// (`Scheduler.py:2245-2280`). Two gated entries where the slow one
/// finishes last must still land second: before the fix the last finisher
/// skipped the queue (`in_flight == 0`) and landed first.
#[test]
fn gated_merges_land_in_finish_order() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    // Both gated through the deep-system set; `schedwait` sleeps 8 s so it
    // always finishes (and, before the fix, lands) last.
    let entries = vec![new("schedsys"), new("schedwait")];
    let options = scope_options(
        &config_root,
        &["*dev-libs/schedsys", "*dev-libs/schedwait"],
        crate::pretend::MergeWaitScope::Deep,
    );
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("both entries merge");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") < vdb_mtime(&root, "dev-libs/schedwait-1.0"),
        "finish order is land order, even for the last finisher"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Portage moves the whole merge-wait queue to the merge queue in one pass
/// (`Scheduler.py:2255-2280`) and holds new builds until the moved merges
/// installed (`_merge_wait_scheduled`, `Scheduler.py:2471`): a build freed
/// by the first landed merge still waits for the rest of the queue. Three
/// gated entries (`schedwait` slowest, so it always finishes last) plus a
/// fourth depending on the first: before the fix the slow finisher skipped
/// the queue and landed ahead of the earlier two.
#[test]
fn queued_merges_land_before_a_freed_dependent_builds() {
    use portage_repo::{DepEdge, DepPriority};
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let edge = |package: &str| DepEdge {
        atom: format!("dev-libs/{package}"),
        evaluated: format!("dev-libs/{package}"),
        category: "dev-libs".into(),
        package: package.into(),
        priority: DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
        disjunctive: false,
        alt: None,
        key: 0,
    };
    let mut first = new("schedsys");
    first.required_by = vec![("dev-libs".into(), "schedleaf-a".into())];
    let mut leaf = new("schedleaf-a");
    leaf.deps = vec![edge("schedsys")];
    let entries = vec![first, new("schedok"), new("schedwait"), leaf];
    let options = scope_options(
        &config_root,
        &[
            "*dev-libs/schedsys",
            "*dev-libs/schedok",
            "*dev-libs/schedwait",
        ],
        crate::pretend::MergeWaitScope::Deep,
    );
    let display = scheduler_display(4);
    let policy = mrg_director::LoadAwarePolicy::new(3, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("all entries merge");
    let slow = vdb_mtime(&root, "dev-libs/schedwait-1.0");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") < slow,
        "the first finisher lands before the slow last finisher"
    );
    assert!(
        vdb_mtime(&root, "dev-libs/schedok-1.0") < slow,
        "the second finisher lands before the slow last finisher"
    );
    // Post-merge clean removes `${T}/build.log`, so the freed dependent's
    // start is pinned through its merge instead: it lands after the whole
    // queue it was freed by.
    let leaf = vdb_mtime(&root, "dev-libs/schedleaf-a-1.0");
    for cpv in [
        "dev-libs/schedsys-1.0",
        "dev-libs/schedok-1.0",
        "dev-libs/schedwait-1.0",
    ] {
        assert!(
            vdb_mtime(&root, cpv) < leaf,
            "the freed dependent merges only after the whole queue landed"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// A `package.env` `FEATURES=merge-wait` holds one package's merge even
/// when the run-wide list (and the scope) would let it through.
/// Portage: `_build_exit`, `"merge-wait" in build.settings.features`.
/// Backlog #512, item (1); review follow-up I1.
#[test]
fn package_env_merge_wait_holds_one_packages_merge() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let new = |p: &str| {
        source_entry(
            p,
            PretendOutcome::New {
                version: "1.0".into(),
            },
        )
    };
    let entries = vec![new("schedsys"), new("schedwait")];
    let mut options = scope_options(&config_root, &[], crate::pretend::MergeWaitScope::None);
    options.package_env_vars = vec![(
        "dev-libs/schedsys".to_string(),
        vec![("FEATURES".to_string(), "merge-wait".to_string())],
    )];
    let display = scheduler_display(2);
    let policy = mrg_director::LoadAwarePolicy::new(2, None);
    run_build_scheduler(RunBuildSchedulerArgs {
        entries: &entries,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
        keep_going: false,
        buildpkg: None,
        buildpkg_exclude: &[],
        policy: &policy,
        display: &display,
    })
    .expect("both entries merge");
    assert!(
        vdb_mtime(&root, "dev-libs/schedsys-1.0") > vdb_mtime(&root, "dev-libs/schedwait-1.0"),
        "the package.env merge-wait holds schedsys past the slow build"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&portage_tmpdir);
}

/// A pass entry that runs no ebuild: the runner's scheduling, logging
/// and replay are pinned without phases.
struct FakePretendEntry<'a> {
    entry: GraphEntry,
    log_dir: PathBuf,
    shared: &'a FakeShared,
    /// When set, the pretend fails unless the replay sink already holds
    /// this marker: pins live replay at one job (seen) and buffering
    /// above one job (never seen mid-run).
    need: Option<&'static str>,
    /// When set, the pretend writes its log and then fails: pins that a
    /// failing entry's output still replays before the failure records.
    fail: bool,
    /// How long the pretend waits for a second entry to be in flight.
    delay: std::time::Duration,
}

/// The knobs every fake entry of one pass shares: the event record, the
/// live-concurrency counters and the replay sink a later entry can read.
#[derive(Default)]
struct FakeShared {
    events: std::sync::Mutex<Vec<String>>,
    in_flight: std::sync::atomic::AtomicUsize,
    max_concurrent: std::sync::atomic::AtomicUsize,
    sink: FakeSink,
}

/// A replay sink a fake entry can read back mid-pass.
#[derive(Clone, Default)]
struct FakeSink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for FakeSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl PretendEntry for FakePretendEntry<'_> {
    fn entry(&self) -> &GraphEntry {
        &self.entry
    }
    fn needs_pretend(&self) -> bool {
        true
    }
    fn entry_tmpdir(&self) -> Result<PathBuf, String> {
        Ok(self.log_dir.clone())
    }
    fn run_pretend(&self) -> PretendRun {
        use std::sync::atomic::Ordering;
        let name = self.entry.package.clone();
        self.shared
            .events
            .lock()
            .unwrap()
            .push(format!("start-{name}"));
        let running = self.shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared
            .max_concurrent
            .fetch_max(running, Ordering::SeqCst);
        if let Some(need) = self.need {
            let sink = self.shared.sink.0.lock().unwrap();
            let seen = String::from_utf8_lossy(&sink);
            if !seen.contains(need) {
                self.shared.in_flight.fetch_sub(1, Ordering::SeqCst);
                return PretendRun {
                    log: None,
                    result: Err(format!(
                        "{name}: started before the previous output replayed"
                    )),
                };
            }
        }
        // Wait (up to `delay`) until a second entry has been in flight. Every
        // entry waits, so the overlap shows up whichever thread the
        // scheduler runs first on a loaded machine.
        let deadline = std::time::Instant::now() + self.delay;
        while self.shared.max_concurrent.load(Ordering::SeqCst) < 2
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let log = self.log_dir.join(format!("{name}.pretend.log"));
        fs::create_dir_all(&self.log_dir).unwrap();
        fs::write(
            &log,
            format!(">>> Running pre-merge checks for dev-libs/{name}-1.0\noutput-{name}\n"),
        )
        .unwrap();
        self.shared
            .events
            .lock()
            .unwrap()
            .push(format!("logged-{name}:{}", log.display()));
        self.shared
            .events
            .lock()
            .unwrap()
            .push(format!("finish-{name}"));
        self.shared.in_flight.fetch_sub(1, Ordering::SeqCst);
        // A failing pretend still hands back its log: the runner replays
        // the buffered output before recording the failure.
        if self.fail {
            return PretendRun {
                log: Some(log),
                result: Err(format!("{name}: pkg_pretend failed (1)")),
            };
        }
        PretendRun {
            log: Some(log),
            result: Ok(()),
        }
    }
}

fn fake_entries<'a>(
    names: &[&str],
    log_root: &Path,
    shared: &'a FakeShared,
    delay: std::time::Duration,
) -> Vec<FakePretendEntry<'a>> {
    names
        .iter()
        .map(|name| FakePretendEntry {
            entry: source_entry(
                name,
                PretendOutcome::New {
                    version: "1.0".into(),
                },
            ),
            // The `${T}` mirror the actual run composes into.
            log_dir: log_root.join(format!("portage/dev-libs/{name}-1.0/temp")),
            shared,
            need: None,
            fail: false,
            delay,
        })
        .collect()
}

/// Portage creates each buffered pretend log with `tempfile.mkstemp` in
/// the package's `${T}` (`Scheduler.py:1290`), never at a predictable
/// path: the old `$PORTAGE_TMPDIR/pretend-<pid>-<idx>.log` file (written
/// with a truncating, symlink-following `fs::write` as root in a
/// world-writable directory) let a planted symlink redirect the write.
/// After the pass the sentinel is unchanged, every log is a fresh file
/// under its entry's `${T}` with Portage's `.pretend.log` suffix, and no
/// regular file appears at an old predictable path.
#[test]
fn pretend_pass_ignores_a_planted_symlink_and_logs_o_excl_under_t() {
    let portage_tmpdir = tempdir();
    let sentinel = portage_tmpdir.join("sentinel.txt");
    fs::write(&sentinel, "untouched\n").unwrap();
    let pid = std::process::id();
    for idx in 0..2 {
        std::os::unix::fs::symlink(
            &sentinel,
            portage_tmpdir.join(format!("pretend-{pid}-{idx}.log")),
        )
        .unwrap();
    }
    let shared = FakeShared::default();
    let entries = fake_entries(
        &["ppfa", "ppfb"],
        &portage_tmpdir,
        &shared,
        std::time::Duration::ZERO,
    );
    let mut replay: Vec<u8> = Vec::new();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &entries,
        jobs: 2,
        replay: &mut replay,
    });
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        "untouched\n",
        "the planted link was never followed"
    );
    for idx in 0..2 {
        let link = portage_tmpdir.join(format!("pretend-{pid}-{idx}.log"));
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the old predictable path is still the planted link: {}",
            link.display()
        );
    }
    let events = shared.events.lock().unwrap();
    for name in ["ppfa", "ppfb"] {
        let logged = events
            .iter()
            .find_map(|e| e.strip_prefix(&format!("logged-{name}:")))
            .unwrap_or_else(|| panic!("{name} logged to a capture file"));
        let logged = PathBuf::from(logged);
        let t_dir = portage_tmpdir.join(format!("portage/dev-libs/{name}-1.0/temp"));
        assert!(
            logged.starts_with(&t_dir),
            "{name} logged under its ${{T}}: {}",
            logged.display()
        );
        assert!(
            logged
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".pretend.log")),
            "{name} log carries Portage's suffix: {}",
            logged.display()
        );
        assert!(
            logged.parent() != Some(portage_tmpdir.as_path()),
            "{name} log is not at the old predictable path: {}",
            logged.display()
        );
        assert!(!logged.exists(), "the replayed log is removed");
    }
    // Nothing but the two planted links matches the old name shape.
    for dir_entry in fs::read_dir(&portage_tmpdir).unwrap() {
        let dir_entry = dir_entry.unwrap();
        let file_name = dir_entry.file_name();
        let file_name = file_name.to_str().unwrap();
        if file_name.starts_with("pretend-") {
            assert!(
                dir_entry.file_type().unwrap().is_symlink(),
                "no new regular pretend log: {file_name}"
            );
        }
    }
    let text = String::from_utf8_lossy(&replay);
    let fa = text
        .find(">>> Running pre-merge checks for dev-libs/ppfa-1.0\noutput-ppfa\n")
        .expect("ppfa's block replays whole");
    let fb = text
        .find(">>> Running pre-merge checks for dev-libs/ppfb-1.0\noutput-ppfb\n")
        .expect("ppfb's block replays whole");
    assert!(fa < fb, "merge-list order, not finish order:\n{text}");
    let _removed = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Portage creates the buffered pretend log with `tempfile.mkstemp` in
/// the package's `${T}` (`Scheduler.py:1290`): `O_EXCL` with Portage's
/// `.pretend.log` suffix, never the old predictable path. The per-entry
/// run composes exactly that file (status line first, then the staged
/// capture) and hands it back for the pass to replay and remove, so the
/// final text matches the always-buffered run byte for byte.
#[test]
fn pretend_log_is_created_o_excl_under_the_entry_t_dir() {
    let config_root = fixtures_root();
    let repos = find_repos(&config_root).unwrap();
    let root = tempdir();
    let portage_tmpdir = tempdir();
    let entry = source_entry(
        "schedppa",
        PretendOutcome::New {
            version: "1.0".into(),
        },
    );
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let wrapped = SourcePretendEntry {
        entry: &entry,
        repos: &repos,
        root: &root,
        portage_tmpdir: &portage_tmpdir,
        options: &options,
    };
    let run = wrapped.run_pretend();
    assert!(run.result.is_ok(), "{:?}", run.result);
    let log = run.log.expect("a source entry always logs");
    let t_dir = portage_tmpdir.join("portage/dev-libs/schedppa-1.0/temp");
    assert_eq!(
        log.parent(),
        Some(t_dir.as_path()),
        "the log lives in the entry's ${{T}}: {}",
        log.display()
    );
    assert!(
        log.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".pretend.log")),
        "the log carries Portage's suffix: {}",
        log.display()
    );
    assert!(log.is_file(), "the log is a fresh file: {}", log.display());
    let text = fs::read_to_string(&log).unwrap();
    assert!(
        text.starts_with(">>> Running pre-merge checks for dev-libs/schedppa-1.0\n"),
        "the status line heads the block: {text}"
    );
    assert!(
        text.contains("SCHED-PRETEND schedppa"),
        "the capture follows: {text}"
    );
    let _removed = std::fs::remove_file(&log);
    let _removed = std::fs::remove_dir_all(&root);
    let _removed = std::fs::remove_dir_all(&portage_tmpdir);
}

/// Portage clamps the pass to `max(1, min(max_jobs, len))`
/// (`Scheduler.py:961-964`, where a bare `--jobs` counts as the entry
/// count), and one job runs strictly sequentially: no entry starts before
/// the previous one finishes. The buffered run still completes every
/// entry with merge-list-order replay.
#[test]
fn pretend_pass_is_sequential_at_one_job_and_clamps_max_jobs() {
    assert_eq!(pretend_max_jobs(3, 2), 2);
    assert_eq!(pretend_max_jobs(1, 5), 1);
    assert_eq!(pretend_max_jobs(usize::MAX, 4), 4);
    // One job: strict start/finish sequencing, never two at once.
    let shared = FakeShared::default();
    let tmp_root = tempdir();
    let entries = fake_entries(
        &["pseqa", "pseqb"],
        &tmp_root,
        &shared,
        std::time::Duration::ZERO,
    );
    let mut replay: Vec<u8> = Vec::new();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &entries,
        jobs: 1,
        replay: &mut replay,
    });
    assert!(failures.is_empty(), "{failures:?}");
    let events = shared.events.lock().unwrap();
    let pos = |marker: &str| {
        events
            .iter()
            .position(|e| e == marker)
            .unwrap_or_else(|| panic!("{marker} recorded: {events:?}"))
    };
    assert!(
        pos("start-pseqa") < pos("finish-pseqa") && pos("finish-pseqa") < pos("start-pseqb"),
        "at one job nothing interleaves: {events:?}"
    );
    assert!(
        events.iter().any(|e| e.starts_with("logged-pseqa:")),
        "the first entry logged: {events:?}"
    );
    assert_eq!(
        shared
            .max_concurrent
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one job never overlaps"
    );
    // Three jobs with two entries: both run at once, replayed in
    // merge-list order.
    let shared = FakeShared::default();
    let tmp_root = tempdir();
    let entries = fake_entries(
        &["pseqa", "pseqb"],
        &tmp_root,
        &shared,
        std::time::Duration::from_secs(30),
    );
    let mut replay: Vec<u8> = Vec::new();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &entries,
        jobs: 3,
        replay: &mut replay,
    });
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        shared
            .max_concurrent
            .load(std::sync::atomic::Ordering::SeqCst),
        2,
        "two entries overlap at three jobs: {:?}",
        shared.events.lock().unwrap()
    );
    assert_eq!(
        String::from_utf8_lossy(&replay),
        ">>> Running pre-merge checks for dev-libs/pseqa-1.0\noutput-pseqa\n\
         >>> Running pre-merge checks for dev-libs/pseqb-1.0\noutput-pseqb\n",
        "buffered output replays whole, in merge-list order"
    );
    let _removed = std::fs::remove_dir_all(&tmp_root);
}

/// At one job each package's output is written before the next pretend
/// starts (Portage's unbuffered path: `buffered = max_jobs > 1`,
/// `Scheduler.py:974`). The second entry fails unless the shared sink
/// already holds the first entry's output; above one job the same entry
/// fails, pinning that buffering is preserved there. Either way the
/// final text is the same status line plus phase output per entry in
/// merge-list order, so the contract suite pins no new output shape here.
#[test]
fn pretend_pass_writes_each_output_before_the_next_starts_at_one_job() {
    let shared = FakeShared::default();
    let tmp_root = tempdir();
    let mut entries = fake_entries(
        &["psteama", "psteamb"],
        &tmp_root,
        &shared,
        std::time::Duration::ZERO,
    );
    entries[1].need = Some("output-psteama");
    let mut replay = shared.sink.clone();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &entries,
        jobs: 1,
        replay: &mut replay,
    });
    assert!(failures.is_empty(), "{failures:?}");
    let sink = shared.sink.0.lock().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&sink),
        ">>> Running pre-merge checks for dev-libs/psteama-1.0\noutput-psteama\n\
         >>> Running pre-merge checks for dev-libs/psteamb-1.0\noutput-psteamb\n",
        "at one job each block replays before the next starts"
    );
    // Above one job the replay is still pending mid-run, so the same
    // entry fails: buffering is preserved.
    let shared = FakeShared::default();
    let tmp_root = tempdir();
    let mut entries = fake_entries(
        &["psteama", "psteamb"],
        &tmp_root,
        &shared,
        std::time::Duration::ZERO,
    );
    entries[1].need = Some("output-psteama");
    let mut replay = shared.sink.clone();
    let failures = run_pretend_pass(RunPretendPassArgs {
        entries: &entries,
        jobs: 2,
        replay: &mut replay,
    });
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].0, 1);
    assert!(
        failures[0]
            .1
            .contains("before the previous output replayed"),
        "{}",
        failures[0].1
    );
    let _removed = std::fs::remove_dir_all(&tmp_root);
}

/// Portage replays a failing entry's buffered output and then records
/// the failure (`Scheduler.py:1008`, `_emit_pkg_pretend_result`; the
/// capture is read whatever the return code, `Scheduler.py:1316-1325`).
/// The pass replays the failing block in merge-list order at one job
/// and above one job, and still reports the failure.
#[test]
fn pretend_pass_replays_a_failing_entry_output_before_recording_the_failure() {
    for jobs in [1, 2] {
        let shared = FakeShared::default();
        let tmp_root = tempdir();
        let mut entries = fake_entries(
            &["pfailok", "pfailbad"],
            &tmp_root,
            &shared,
            std::time::Duration::ZERO,
        );
        entries[1].fail = true;
        let mut replay: Vec<u8> = Vec::new();
        let failures = run_pretend_pass(RunPretendPassArgs {
            entries: &entries,
            jobs,
            replay: &mut replay,
        });
        assert_eq!(failures.len(), 1, "jobs={jobs}: {failures:?}");
        assert_eq!(failures[0].0, 1, "jobs={jobs}: {failures:?}");
        let text = String::from_utf8_lossy(&replay);
        let ok = text
            .find("output-pfailok")
            .unwrap_or_else(|| panic!("jobs={jobs}: the passing block replays:\n{text}"));
        let bad = text
            .find("output-pfailbad")
            .unwrap_or_else(|| panic!("jobs={jobs}: the failing block replays:\n{text}"));
        assert!(ok < bad, "jobs={jobs}: merge-list order:\n{text}");
        let _removed = std::fs::remove_dir_all(&tmp_root);
    }
}

/// The same pin through the actual phases: `schedppfail`'s `pkg_pretend`
/// dies after an `eerror`, and its staged capture still replays, after
/// the passing entry's block, in merge-list order, at one job and
/// above one job. Portage: `Scheduler.py:1008`
/// (`_emit_pkg_pretend_result`) and `Scheduler.py:1316-1325`.
#[test]
fn pretend_pass_replays_a_real_failing_pretend_output() {
    for jobs in [1, 2] {
        let config_root = fixtures_root();
        let repos = find_repos(&config_root).unwrap();
        let root = tempdir();
        let portage_tmpdir = tempdir();
        let new = |p: &str| {
            source_entry(
                p,
                PretendOutcome::New {
                    version: "1.0".into(),
                },
            )
        };
        let entries = [new("schedppseqa"), new("schedppfail")];
        let options = ebuild_merge::MergeOptions {
            distdir: tempdir(),
            config_root: config_root.clone(),
            ..ebuild_merge::MergeOptions::default()
        };
        let wrapped: Vec<SourcePretendEntry<'_>> = entries
            .iter()
            .map(|entry| SourcePretendEntry {
                entry,
                repos: &repos,
                root: &root,
                portage_tmpdir: &portage_tmpdir,
                options: &options,
            })
            .collect();
        let mut replay: Vec<u8> = Vec::new();
        let failures = run_pretend_pass(RunPretendPassArgs {
            entries: &wrapped,
            jobs,
            replay: &mut replay,
        });
        assert_eq!(failures.len(), 1, "jobs={jobs}: {failures:?}");
        assert_eq!(failures[0].0, 1, "jobs={jobs}: {failures:?}");
        assert!(
            failures[0].1.contains("pkg_pretend failed"),
            "jobs={jobs}: {}",
            failures[0].1
        );
        let text = String::from_utf8_lossy(&replay);
        let ok = text
            .find("SEQA-MARKER")
            .unwrap_or_else(|| panic!("jobs={jobs}: the passing block replays:\n{text}"));
        let bad = text
            .find("SCHED-PRETEND-FAIL schedppfail")
            .unwrap_or_else(|| panic!("jobs={jobs}: the failing block replays:\n{text}"));
        assert!(ok < bad, "jobs={jobs}: merge-list order:\n{text}");
        let _removed = std::fs::remove_dir_all(&root);
        let _removed = std::fs::remove_dir_all(&portage_tmpdir);
    }
}
