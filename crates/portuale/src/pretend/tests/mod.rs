use super::*;

use portage_util::TempDir;

fn fixtures_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Test `NewsContext`: the fixture `default` profile on `amd64`, self-owned so no
/// chown is ever needed.
fn test_news_context() -> NewsContext {
    NewsContext {
        profile: Some("default".to_string()),
        has_profile: true,
        arch: "amd64".to_string(),
        // SAFETY: `geteuid`/`getegid` have no preconditions.
        inst_uid: unsafe { libc::geteuid() },
        portage_gid: unsafe { libc::getegid() },
    }
}

/// An isolated ROOT sharing the fixtures' vdb read-only, with `news` in the
/// resolved `FEATURES`. Returns the scratch base, the ROOT, the repos, and the config.
fn news_notice_test_tree(
    tag: &str,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    Vec<portage_repo::RepoConfig>,
    portage_profile::Config,
) {
    let fixtures = fixtures_root();
    let base = TempDir::new(&format!("pretend-test-news_notice_{tag}")).keep();
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    std::fs::create_dir_all(root.join("var/lib")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixtures.join("var/db"), root.join("var/db")).unwrap();
    let repo_config = |name: &str, dir: &str| portage_repo::RepoConfig {
        name: name.to_string(),
        location: fixtures.join(dir),
        priority: 0,
        is_main: name == "testrepo",
        masters: Vec::new(),
        profile_formats: Vec::new(),
        cache_formats: Vec::new(),
        aliases: Vec::new(),
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: Vec::new(),
    };
    let repos = vec![
        repo_config("testrepo", "repo"),
        repo_config("overlay", "overlay"),
    ];
    let mut config = portage_profile::Config::default();
    config
        .other_vars
        .insert("FEATURES".to_string(), "sandbox news userpriv".to_string());
    (base, root, repos, config)
}

/// A scratch `ROOT` with a single-item stale resume list, so
/// `--resume --skipfirst` exits with nothing to do and no merge.
fn single_resume_root(tag: &str) -> std::path::PathBuf {
    let root = TempDir::new(&format!("resume_single_{tag}")).keep();
    crate::mtimedb::write_resume_list(
        &root,
        &["dev-libs/stale-a"],
        &[(
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "stale-a".to_string(),
            "1".to_string(),
        )],
        &crate::mtimedb::ResumeOpts::default(),
    )
    .unwrap();
    root
}

/// Plain recursive copy (files + dirs + symlinks) for fixture trees.
#[cfg(unix)]
fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dst_path = dst.join(entry.file_name());
        let ft = entry.file_type().unwrap();
        if ft.is_symlink() {
            let target = std::fs::read_link(entry.path()).unwrap();
            std::os::unix::fs::symlink(target, dst_path).unwrap();
        } else if ft.is_dir() {
            copy_tree(&entry.path(), &dst_path);
        } else if ft.is_file() {
            std::fs::copy(entry.path(), dst_path).unwrap();
        }
    }
}

/// `fixture_resolve_env` with `FEATURES="news"` opted in.
fn news_resolve_env(
    root: &std::path::Path,
    portage_tmpdir: &std::path::Path,
) -> Vec<(String, String)> {
    let mut env = fixture_resolve_env(root, portage_tmpdir);
    env.push(("FEATURES".to_string(), "news".to_string()));
    env
}

/// A stub `eselect` on `PATH`, logging its argv to a marker file. Returns the
/// stub dir (to prepend to `PATH`) and the marker path.
#[cfg(unix)]
fn eselect_stub(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new(&format!("eselect_stub_{tag}")).keep();
    let marker = dir.join("eselect.log");
    std::fs::write(
        dir.join("eselect"),
        format!("#!/bin/sh\necho \"$@\" >> {}\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(dir.join("eselect"), std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, marker)
}

/// Runs `emerge <ask_flags> --oneshot dev-libs/schedok` on a pty, declining both
/// the news prompt and the merge prompt. Returns stdout for the caller's checks.
#[cfg(unix)]
fn ask_read_news_declined(label: &str, ask_flags: [&str; 2]) -> String {
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let (stubdir, marker) = eselect_stub(label);
    let path = format!(
        "{}:{}",
        stubdir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let base = TempDir::new(&format!("ask_read_news_{label}")).keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut env = news_resolve_env(&root, &base.join("pt"));
    env.push(("PATH".to_string(), path));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args([
            "emerge",
            ask_flags[0],
            ask_flags[1],
            "--oneshot",
            "dev-libs/schedok",
        ])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"No\n").expect("decline the news prompt");
    master.write_all(b"No\n").expect("decline the merge prompt");
    let output = wait_pty_output(child);
    drop(master);
    assert_eq!(
        output.status.code(),
        Some(130),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Would you like to read the news items while calculating dependencies?"),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    assert!(
        !marker.exists(),
        "a declined news prompt must not spawn eselect"
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(stubdir);
    stdout.into_owned()
}

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

/// The `portuale` binary next to the current test executable.
fn built_portuale_bin() -> std::path::PathBuf {
    crate::self_exe::built_portuale_bin()
}

/// A scratch `ROOT` with a stale multi-item resume list. Returns the root plus
/// the file's bytes before the run under test.
fn stale_resume_root(tag: &str) -> (std::path::PathBuf, Vec<u8>) {
    let root = TempDir::new(&format!("resume_no_write_{tag}")).keep();
    let stale = vec![
        (
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "stale-a".to_string(),
            "1".to_string(),
        ),
        (
            crate::mtimedb::ResumeEntryKind::Ebuild,
            "dev-libs".to_string(),
            "stale-b".to_string(),
            "1".to_string(),
        ),
    ];
    crate::mtimedb::write_resume_list(
        &root,
        &["dev-libs/stale-a"],
        &stale,
        &crate::mtimedb::ResumeOpts::default(),
    )
    .unwrap();
    let before = std::fs::read(crate::mtimedb::mtimedb_path(&root)).unwrap();
    (root, before)
}

/// Fixture-config env for a spawned `portuale emerge` that resolves: the
/// committed fixture tree as config and the scratch dir as `ROOT`.
fn fixture_resolve_env(
    root: &std::path::Path,
    portage_tmpdir: &std::path::Path,
) -> Vec<(String, String)> {
    let fixtures = fixtures_root();
    vec![
        (
            "PORTAGE_CONFIGROOT".to_string(),
            fixtures.display().to_string(),
        ),
        ("ROOT".to_string(), root.display().to_string()),
        (
            "DISTDIR".to_string(),
            fixtures.join("distfiles").display().to_string(),
        ),
        (
            "PORTAGE_TMPDIR".to_string(),
            portage_tmpdir.display().to_string(),
        ),
    ]
}

/// The slave path of `master`, through the reentrant `ptsname_r`: plain
/// `ptsname` shares one static buffer across threads.
#[cfg(unix)]
fn pty_slave_path(master: libc::c_int) -> std::ffi::CString {
    let mut buf = [0 as libc::c_char; 128];
    // SAFETY: `buf` is a valid writable buffer of the stated length.
    let rc = unsafe { libc::ptsname_r(master, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc, 0, "ptsname_r failed");
    // SAFETY: `ptsname_r` nul-terminated `buf` on success.
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_owned()
}

/// A pty pair: the child answers `--ask` through the slave (its stdin), while
/// the test writes the answer into the master.
#[cfg(unix)]
fn pty_pair() -> (std::fs::File, std::process::Stdio) {
    use std::os::fd::FromRawFd;
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(libc::grantpt(master), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");
        let cname = pty_slave_path(master);
        let slave = libc::open(
            cname.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        assert!(slave >= 0, "pty slave open failed");
        // Both ends are `O_CLOEXEC` at open time, so a forked sibling never holds
        // the pty open past its exec (the stdio dup onto fd 0 clears the flag).
        let master_file = std::fs::File::from_raw_fd(master);
        let slave_stdio = std::process::Stdio::from(std::os::fd::OwnedFd::from_raw_fd(slave));
        (master_file, slave_stdio)
    }
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

fn world_entry(category: &str, package: &str, slot: &str) -> GraphEntry {
    let mut e = entry_with_use(
        PretendOutcome::New {
            version: "1.0".into(),
        },
        "",
        "",
    );
    e.category = category.into();
    e.package = package.into();
    e.slot = Some(slot.into());
    e
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

mod atoms;
mod attrs;
mod blockers;
mod conflicts;
mod env_config;
mod json;
mod news;
mod prompt;
mod remote;
mod resume;
mod unmerge;
mod world;
