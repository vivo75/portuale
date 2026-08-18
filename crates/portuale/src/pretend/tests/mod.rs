use super::*;

use portage_util::TempDir;

/// An unowned ROOT for the privilege tests; `None` for a root caller, whose test
/// returns early.
#[cfg(unix)]
fn ask_pretend_unowned_root() -> Option<std::path::PathBuf> {
    // SAFETY: `geteuid` has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return None;
    }
    Some(std::path::PathBuf::from(
        "/usr/portuale-ask-pretend-test-does-not-exist",
    ))
}

/// Bound every wait on a pty-driven child (default 60 s, overridable); on expiry
/// kill the child and panic with a `watchdog:` prefix carrying its output so far.
fn pty_watchdog_bound() -> std::time::Duration {
    let secs = std::env::var("PORTUALE_TEST_PTY_WATCHDOG_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);
    std::time::Duration::from_secs(secs.max(1))
}

/// Bounded wait collecting the child's piped stdout/stderr; on timeout the child
/// is killed and the panic carries whatever it had printed.
#[cfg(unix)]
fn wait_pty_output(child: std::process::Child) -> std::process::Output {
    wait_pty_output_bound(child, pty_watchdog_bound())
}

/// [`wait_pty_output`] with an explicit bound, so tests never mutate the
/// process-global `PORTUALE_TEST_PTY_WATCHDOG_SECS` override: that variable is
/// read by every pty wait in this process, and a test that sets it widens the
/// kill window of every parallel pty test to its own short bound.
#[cfg(unix)]
fn wait_pty_output_bound(
    mut child: std::process::Child,
    bound: std::time::Duration,
) -> std::process::Output {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().expect("collect child output");
            }
            Ok(None) if start.elapsed() >= bound => {
                let _ = child.kill();
                let output = child
                    .wait_with_output()
                    .expect("collect killed child output");
                panic!(
                    "watchdog: pty child exceeded {bound:?} and was killed\n\
                     stdout: {}\n\
                     stderr: {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(e) => panic!("watchdog: try_wait failed: {e}"),
        }
    }
}

fn entry_with_use(outcome: PretendOutcome, pv_use: &str, p_use: &str) -> GraphEntry {
    let grp = |s: &str| {
        if s.is_empty() {
            vec![]
        } else {
            vec![("USE".to_string(), s.to_string())]
        }
    };
    GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "foo".into(),
        outcome,
        blockers: vec![],
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        repo_name: Some("testrepo".into()),
        oldbest: vec![],
        use_flags_display: vec![("bar".into(), true), ("baz".into(), false)],
        use_expand_display: grp(pv_use),
        use_expand_display_p: grp(p_use),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: vec![],
        required_by: vec![],
        source: portage_repo::CandidateSource::Ebuild,
        provenance: Default::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}
mod attrs;
mod env_config;
