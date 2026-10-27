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
    TempDir,
    std::path::PathBuf,
    Vec<portage_repo::RepoConfig>,
    portage_profile::Config,
) {
    let fixtures = fixtures_root();
    let base = TempDir::new(&format!("pretend-test-news_notice_{tag}"));
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
fn single_resume_root(tag: &str) -> TempDir {
    let root = TempDir::new(&format!("resume_single_{tag}"));
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
    // The spawned child would otherwise merge the host's installed
    // system repos.conf (Portage `load_repository_config`; portuale reads
    // the absolute path too now), so its repo set -- and these
    // exact-output pins -- would depend on the host. Pin the child's repo
    // set to the fixture text: same sections and (config-root-relative)
    // locations as the on-disk file. On-disk repos.conf parsing itself is
    // pinned by the pmtest contract suite (filtered) and by
    // portage-repo's file-based unit tests.
    env.push((
        "PORTAGE_REPOSITORIES".to_string(),
        fixture_repos_conf_text(),
    ));
    env
}

/// The merged text of a `repos.conf` dir, walked exactly like
/// production (`recursive_config_files`), for `PORTAGE_REPOSITORIES`
/// pinning in spawned test children.
fn repos_conf_dir_text(dir: &std::path::Path) -> String {
    let mut text = String::new();
    if let Ok(files) = portage_util::recursive_config_files(dir) {
        for path in files {
            if let Ok(chunk) = std::fs::read_to_string(&path) {
                text.push_str(&chunk);
                text.push('\n');
            }
        }
    }
    text
}

/// The merged text of the fixture `repos.conf` dir, for
/// `PORTAGE_REPOSITORIES` pinning in [`news_resolve_env`].
fn fixture_repos_conf_text() -> String {
    repos_conf_dir_text(&fixtures_root().join("etc/portage/repos.conf"))
}

/// A stub `eselect` on `PATH`, logging its argv to a marker file. Returns the
/// stub dir (to prepend to `PATH`) and the marker path.
#[cfg(unix)]
fn eselect_stub(tag: &str) -> (TempDir, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new(&format!("eselect_stub_{tag}"));
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
    let base = TempDir::new(&format!("ask_read_news_{label}"));
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

/// `pre_exec` putting the child in the pty's foreground group, so a `^C` byte
/// written to the master arrives as SIGINT. Only async-signal-safe syscalls run here.
#[cfg(unix)]
fn pty_foreground_pre_exec() -> std::io::Result<()> {
    // SAFETY: `setsid`/`ioctl`/`tcsetpgrp`/`getpid` are plain
    // syscalls on fd 0, which std has already duped onto the pty
    // slave before `pre_exec` runs.
    unsafe {
        if libc::setsid() < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0 as libc::c_ulong) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpid()) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Drain the child's piped stdout until `marker` appears, with a hard deadline
/// so a prompt regression fails instead of hanging. Returns every byte read so far.
#[cfg(unix)]
fn read_stdout_until_marker(
    pipe: &mut std::process::ChildStdout,
    marker: &[u8],
    what: &str,
) -> Vec<u8> {
    use std::os::fd::AsRawFd;
    let mut seen = Vec::new();
    let mut buf = [0u8; 512];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        if !marker.is_empty() && seen.windows(marker.len()).any(|w| w == marker) {
            return seen;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!(
                "timed out waiting for {what}; got so far: {}",
                String::from_utf8_lossy(&seen)
            );
        }
        let mut pfd = libc::pollfd {
            fd: pipe.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: a single valid `pollfd`; the timeout fits in a
        // `c_int` by construction (120 s ceiling).
        let pr = unsafe { libc::poll(&mut pfd, 1, remaining.as_millis() as libc::c_int) };
        assert!(pr >= 0, "poll on child stdout failed");
        if pr == 0 {
            continue;
        }
        let n = std::io::Read::read(pipe, &mut buf).expect("read child stdout");
        if n == 0 {
            panic!(
                "child exited before printing {what}; got: {}",
                String::from_utf8_lossy(&seen)
            );
        }
        seen.extend_from_slice(&buf[..n]);
    }
}

/// Spawn `emerge --ask ...` for a SIGINT test: pty stdin in the child's
/// foreground group, both outputs piped, stderr drained on a thread from the
/// start. Returns the child, the master, the stdout pipe, and the drain handle.
#[cfg(unix)]
fn sigint_prompt_child(
    portuale_bin: &std::path::Path,
    args: &[&str],
    env: Vec<(String, String)>,
    master: std::fs::File,
    slave_stdio: std::process::Stdio,
) -> (
    std::process::Child,
    std::fs::File,
    std::process::ChildStdout,
    std::thread::JoinHandle<Vec<u8>>,
) {
    let mut cmd = std::process::Command::new(portuale_bin);
    cmd.args(args);
    cmd.stdin(slave_stdio);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.envs(env);
    // SAFETY: `pty_foreground_pre_exec` runs only async-signal-safe
    // syscalls (`setsid`, `ioctl`, `tcsetpgrp`, `getpid`) on the
    // already-duped stdio fds; it allocates nothing and touches no
    // locks. (Fully qualified: another `CommandExt` is in scope
    // via `super::*`, and this toolchain's `pre_exec` is `unsafe`.)
    unsafe {
        <std::process::Command as std::os::unix::process::CommandExt>::pre_exec(
            &mut cmd,
            pty_foreground_pre_exec,
        );
    }
    let mut child = cmd.spawn().expect("portuale emerge spawns");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let err_handle = std::thread::spawn(move || {
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut stderr_pipe, &mut v).expect("drain child stderr");
        v
    });
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    (child, master, stdout_pipe, err_handle)
}

/// Spawns `args` on a pty with the env `make_env` builds, waits for
/// `prompt` on stdout, then sends `^C`. Asserts exit 130 with one
/// `Interrupted.` and no `Quitting.`. Returns stdout.
/// Portage: UserQuery.py, query.
#[cfg(unix)]
fn ctrl_c_at_prompt(
    label: &str,
    args: &[&str],
    make_env: fn(&std::path::Path, &std::path::Path) -> Vec<(String, String)>,
    prompt: &[u8],
) -> String {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new(label);
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = make_env(&root, &base.join("pt"));
    let (master, slave_stdio) = pty_pair();
    let (child, mut master, mut stdout_pipe, err_handle) =
        sigint_prompt_child(&portuale_bin, args, env, master, slave_stdio);
    let mut stdout = read_stdout_until_marker(&mut stdout_pipe, prompt, label);
    master.write_all(b"\x03").expect("Ctrl-C the prompt");
    let out_handle = std::thread::spawn(move || {
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut stdout_pipe, &mut v).expect("drain child stdout");
        v
    });
    let status = wait_pty_status(child);
    drop(master);
    stdout.extend(out_handle.join().expect("stdout drain"));
    let stderr = err_handle.join().expect("stderr drain");
    assert_eq!(
        status.code(),
        Some(130),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        status.signal(),
        None,
        "SIGINT at the prompt must exit 130, not die by signal"
    );
    let text = String::from_utf8_lossy(&stdout).into_owned();
    assert_eq!(text.matches("Interrupted.").count(), 1, "{text}");
    assert!(
        !text.contains("Quitting."),
        "an interrupted prompt must not print `Quitting.`: {text}"
    );
    let _ = std::fs::remove_dir_all(&base);
    text
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

/// Run the built binary twice under `--pretend` -- once with
/// `--X=True`, once with the bare `--X` -- and assert the two runs
/// are byte-identical (exit code, stdout, stderr).
/// Portage `true_y_or_n` admits `=True` for these options.
fn true_spelling_matches_bare(flag_eq_true: &str, flag_bare: &str, atom: &str, expected_rc: i32) {
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("true_y_or_n");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let run = |flag: &str| {
        std::process::Command::new(&portuale_bin)
            .args(["emerge", "--pretend", flag, atom])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(env.clone())
            .output()
            .expect("portuale emerge spawns")
    };
    // A warm-up run establishes the updates record, so both compared
    // runs skip the block and the ambient repo set behind it.
    run(flag_bare);
    let spelled = run(flag_eq_true);
    let bare = run(flag_bare);
    for (label, output) in [("spelled", &spelled), ("bare", &bare)] {
        assert_eq!(
            output.status.code(),
            Some(expected_rc),
            "--pretend {flag_bare} {atom} ({label}) must exit {expected_rc}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    assert_eq!(
        String::from_utf8_lossy(&spelled.stdout),
        String::from_utf8_lossy(&bare.stdout),
        "--pretend {flag_eq_true} {atom} must print like the bare flag",
    );
    assert_eq!(
        String::from_utf8_lossy(&spelled.stderr),
        String::from_utf8_lossy(&bare.stderr),
        "--pretend {flag_eq_true} {atom} must warn like the bare flag",
    );
    let _ = std::fs::remove_dir_all(&base);
}

/// The `portuale` binary next to the current test executable.
fn built_portuale_bin() -> std::path::PathBuf {
    crate::self_exe::built_portuale_bin()
}

/// A scratch `ROOT` with a stale multi-item resume list. Returns the root plus
/// the file's bytes before the run under test.
fn stale_resume_root(tag: &str) -> (TempDir, Vec<u8>) {
    let root = TempDir::new(&format!("resume_no_write_{tag}"));
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

/// Bounded wait returning the exit status alone, for children whose outputs are
/// already owned by the caller's drain threads.
#[cfg(unix)]
fn wait_pty_status(mut child: std::process::Child) -> std::process::ExitStatus {
    let bound = pty_watchdog_bound();
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if start.elapsed() >= bound => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "watchdog: pty child exceeded {bound:?} and was killed \
                     (status-only shape; see the test's pre-wait marker text)"
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
        build_time: None,
        deps: Vec::new(),
    }
}

// Serialize the one test that mutates the process environment, in
// the `PLACED_CONFIG_ENV_LOCK` (`remote.rs`) style.
static INFO_SCALAR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod abort_color;
mod atoms;
mod attrs;
mod autoclean;
mod autounmask;
mod bdeps_presence;
mod blockers;
mod circular;
mod conflicts;
mod env_config;
mod error_lines;
mod expand;
mod flag_spellings;
mod gates;
mod global_updates;
mod help;
mod info;
mod json;
mod new_options;
mod news;
mod prompt;
mod remote;
mod resume;
mod unmerge;
mod user_virtuals;
mod world;
