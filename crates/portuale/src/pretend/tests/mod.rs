use super::*;

use portage_util::TempDir;

fn fixtures_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// A [`NewsContext`] for the unit tests: the fixture tree's own
/// `default` profile and `amd64`, owned by this process's own
/// uid/gid (real's unprivileged mode) so no chown is ever needed.
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

/// An isolated ROOT for the fix-round-1 notice tests: a temp dir
/// sharing the fixtures' own vdb read-only (the same isolation
/// `unread_news_counts_reuses_the_check_news_evaluation` and the
/// contract suite's `_check_news_isolated_root` use) plus a config
/// with `news` in the resolved `FEATURES`. Returns the scratch
/// base (for cleanup), the ROOT, the two fixture repos, and the
/// config. Prints from the notice helpers land in the test
/// capture; assertions observe the state dir (evaluation ran) or
/// its absence (gated off before any evaluation).
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
/// `--resume --skipfirst` empties it and takes the "nothing to do"
/// exit without merging anything (fast, hermetic).
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

/// Plain recursive copy (files + dirs + symlinks) for fixture
/// trees (`fixtures/var` has no symlinks; `fixtures/etc/portage`
/// carries `make.profile`, repointed absolutely after the copy).
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

/// `fixture_resolve_env` plus `FEATURES="news"` opted in -- the
/// same layering the contract suite's `_news_env` uses.
fn news_resolve_env(
    root: &std::path::Path,
    portage_tmpdir: &std::path::Path,
) -> Vec<(String, String)> {
    let mut env = fixture_resolve_env(root, portage_tmpdir);
    env.push(("FEATURES".to_string(), "news".to_string()));
    env
}

/// A stub `eselect` on `PATH`: an executable shell script logging
/// its argv to a marker file. Returns the stub dir (to prepend to
/// `PATH`) and the marker path.
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

/// Runs `emerge <ask_flags> --oneshot dev-libs/schedok` on a pty and
/// answers "No" to both the news prompt and the merge prompt. Asserts
/// the shared outcome (exit 130, one news notice, no eselect spawn)
/// and returns stdout for the caller's own checks.
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

/// `pre_exec` for the backlog #240 SIGINT tests: the child leaves
/// the test's session and process group (`setsid`), takes the pty
/// slave as its controlling terminal (`TIOCSCTTY`), and makes
/// itself the foreground group (`tcsetpgrp`). Only then does the
/// slave line discipline turn a `^C` byte written to the master
/// into a real SIGINT for the child (a plain spawned child has no
/// foreground group, so the byte is swallowed and the test would
/// hang). Only async-signal-safe syscalls run here.
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

/// Drain the child's piped stdout until `marker` appears, with a
/// hard deadline so a prompt regression fails loudly instead of
/// hanging the suite. Returns every byte read so far; the caller
/// keeps draining the pipe afterwards. EOF before the marker
/// panics with the output so far (the child died before
/// prompting).
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

/// Spawn `emerge --ask ...` for a backlog #240 SIGINT test: pty
/// stdin in the child's foreground group (so `^C` raises SIGINT),
/// both outputs piped, stderr drained on a thread from the start
/// (so a chatty resolve can never block on a full pipe while the
/// test polls stdout for the prompt). Returns the child, the
/// master, the stdout pipe, and the stderr-drain handle.
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
/// `prompt` on stdout, then sends `^C` through the pty. Asserts what
/// every interrupted prompt shares with Portage's `UserQuery.query`
/// (`_emerge/UserQuery.py:74-76`): exit 130 by exit, not by signal,
/// one `Interrupted.` and no `Quitting.`. Returns stdout.
///
/// The `^C` goes out only after the prompt text is seen, so it cannot
/// strike during startup or the resolve, where SIGINT must still kill
/// the process. Delivery needs the foreground group from
/// `pty_foreground_pre_exec`.
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
    let base = TempDir::new(label).keep();
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

/// ROOT the test caller cannot own, so `privileges::is_privileged`
/// is false for a non-root caller: `/usr` reliably exists, is
/// root-owned and not world-writable, so its non-existent child is
/// an unowned target (the same trick as
/// `privileges::tests::a_dir_the_caller_cannot_write_is_not_privileged`
/// and pmtest's `..._refuse_to_merge_into_a_root_owned_tree`).
/// Every #246 pin returns early for a root caller -- the privilege
/// gate only fires for a non-root caller, like those pins.
/// Returns `None` for a root caller (the test returns early).
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
/// `--X=True`, once with the bare `--X` -- against the committed
/// fixture tree on a shared scratch `ROOT`, and assert the two runs
/// are byte-identical (exit code, stdout, stderr). Backlog #241:
/// real's `true_y_or_n` choices
/// (`_emerge/main.py:320-322,365-371,405-415,446-449,612-623,697,732-747`;
/// normalized `in true_y` per option) admit `=True` for eleven
/// options portuale parsed narrowly (`=y`/`=n` only, rc 2
/// `unrecognized option` for nine of them and a wrong
/// `(choose from "y", "n")` rejection for the two
/// autounmask-keep options); `=True` must behave exactly like the
/// bare flag. `expected_rc` pins the shared exit code (0 for the
/// resolving shapes, 1 for the two autounmask-keep shapes whose
/// fixtures fail like real). `--pretend` never writes, so sharing
/// one `ROOT` across both runs is hermetic.
fn true_spelling_matches_bare(flag_eq_true: &str, flag_bare: &str, atom: &str, expected_rc: i32) {
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("true_y_or_n").keep();
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

/// The `portuale` binary next to the current test executable (same
/// layout `ask_without_a_tty_...` already relies on).
fn built_portuale_bin() -> std::path::PathBuf {
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");
    portuale_bin
}

/// A scratch `ROOT` with nothing but a stale multi-item resume list
/// (multi-item so the `resume_backup` rotation would *also* rewrite
/// the file if it ran -- the byte comparison below pins the rotation
/// stays out too). Returns the root plus the file's bytes before the
/// run under test.
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

/// Fixture-config env for a spawned `portuale emerge` that resolves
/// (never merges): the committed fixture tree as config, the scratch
/// dir as `ROOT`, everything else absolute so the test holds from
/// any working directory.
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

/// The slave path of `master`, through the reentrant `ptsname_r`:
/// plain `ptsname` returns a pointer into one static buffer, so two
/// tests creating ptys on parallel threads could open each other's
/// slave -- crossed answers, `--ask should only be used in a
/// terminal`, EIO at `TIOCSCTTY`, hung prompts (#321).
#[cfg(unix)]
fn pty_slave_path(master: libc::c_int) -> std::ffi::CString {
    let mut buf = [0 as libc::c_char; 128];
    // SAFETY: `buf` is a valid writable buffer of the stated length.
    let rc = unsafe { libc::ptsname_r(master, buf.as_mut_ptr(), buf.len()) };
    assert_eq!(rc, 0, "ptsname_r failed");
    // SAFETY: `ptsname_r` nul-terminated `buf` on success.
    unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_owned()
}

/// A pty pair without any external helper: the child answers `--ask`
/// through the slave (its stdin), satisfying the up-front isatty
/// gate, while the test writes the answer into the master. `libc`
/// is already a dependency (`stdin_is_tty` uses it).
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
        // Backlog #234 / #321: close-on-exec on both ends, set at open
        // time (`O_CLOEXEC`), not by a later `fcntl`: a sibling test
        // forking in that window keeps a private copy of the fd past its
        // exec (a child's fd table is copied at fork), which under load
        // holds this pty open. Without it the
        // spawned child inherits the master, so dropping the test's
        // `master` never delivers EOF to the child's slave read (the
        // EOF test hung on exactly this). The stdio dup onto fd 0
        // clears the flag on the dup, so the child's stdin survives.
        let master_file = std::fs::File::from_raw_fd(master);
        let slave_stdio = std::process::Stdio::from(std::os::fd::OwnedFd::from_raw_fd(slave));
        (master_file, slave_stdio)
    }
}

/// Backlog #259: bound every wait on a pty-driven child. Plain
/// `wait_with_output`/`wait` has no timeout, so a child stuck in
/// `read_prompt_line`'s blocking `libc::read` (the #259 hang: the
/// slave sat in `wait_woken` and the test binary waited 20 hours)
/// takes the whole `cargo test` run with it. Polls `try_wait`
/// against a bound and kills the child when it expires.
///
/// The bound is 60 s by default -- orders of magnitude past these
/// prompt tests' real runtime (sub-second) and still short enough
/// to fail a CI job instead of hanging it.
/// `PORTUALE_TEST_PTY_WATCHDOG_SECS` overrides it: set it low to
/// reproduce a hang, or high to give a loaded machine room. The
/// `watchdog:` prefix on the timeout message is the race-chase hook
/// -- the first timeout's captured output is the evidence for why
/// the child never woke.
fn pty_watchdog_bound() -> std::time::Duration {
    let secs = std::env::var("PORTUALE_TEST_PTY_WATCHDOG_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);
    std::time::Duration::from_secs(secs.max(1))
}

/// Bounded wait collecting the child's piped stdout/stderr. The
/// pty-driven `--ask`/`--read-news` tests answer through the
/// master and read the merged result back; on timeout the child is
/// killed and the panic carries whatever it had printed, which is
/// what the #259 race chase reads.
#[cfg(unix)]
fn wait_pty_output(mut child: std::process::Child) -> std::process::Output {
    let bound = pty_watchdog_bound();
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

/// Bounded wait for the `sigint_prompt_child` shape, whose stdout
/// and stderr are already owned by the caller's drain threads --
/// there is nothing left for `wait_with_output` to collect, so this
/// returns the exit status alone. Same bound and `watchdog:`
/// panic; the pre-wait marker text the caller already holds is the
/// chase evidence for these tests.
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
        deps: Vec::new(),
    }
}

// Serialize the one test that mutates the process environment, in
// the `PLACED_CONFIG_ENV_LOCK` (`remote.rs`) style.
static INFO_SCALAR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod atoms;
mod attrs;
mod blockers;
mod circular;
mod conflicts;
mod env_config;
mod flag_spellings;
mod gates;
mod help;
mod info;
mod json;
mod news;
mod prompt;
mod remote;
mod resume;
mod unmerge;
mod world;
