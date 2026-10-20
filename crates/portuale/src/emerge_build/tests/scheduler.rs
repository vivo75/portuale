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
    assert!(is_reported_failure(&err), "{err}");
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
