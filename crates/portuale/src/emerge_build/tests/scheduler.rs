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
    let entries = vec![new("schedppa"), new("schedppb")];
    let options = ebuild_merge::MergeOptions {
        distdir: tempdir(),
        config_root: config_root.clone(),
        ..ebuild_merge::MergeOptions::default()
    };
    let mut replay: Vec<u8> = Vec::new();
    let failures = super::scheduler::run_pretend_pass(
        &entries,
        &repos,
        &root,
        &portage_tmpdir,
        &options,
        2,
        &mut replay,
    );
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
