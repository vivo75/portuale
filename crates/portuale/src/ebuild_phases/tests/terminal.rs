//! Phases and the controlling terminal (backlog #408).
//!
//! Oracle: a probe under a real pty (`LLM/evidence/2026-10-18-408-s0/`):
//! `bash -i -c true` in a `process_group(0)` child stops with SIGTTIN even
//! with stdin on `/dev/null`; it finishes in the parent's group or under
//! `setsid`. Portage: `bin/pid-ns-init` (`os.setsid()`), `doebuild.py`
//! (`_global_pid_phases`).
//!
//! Each test re-executes this test binary in a child that owns a fresh pty as
//! its controlling terminal; the `#[ignore]`d `inner_*` tests are the bodies
//! that run there.

use super::*;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::io::{AsRawFd as _, FromRawFd as _};
use std::os::unix::process::CommandExt as _;
use std::time::{Duration, Instant};

const INNER_ENV: &str = "PORTUALE_TEST_PTY_DIR";

/// Master side of a fresh pty plus the slave's path.
fn open_pty() -> (std::fs::File, std::ffi::CString) {
    // SAFETY: plain libc calls on a descriptor this function owns.
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(fd >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(fd), 0);
        assert_eq!(libc::unlockpt(fd), 0);
        let name = std::ffi::CStr::from_ptr(libc::ptsname(fd)).to_owned();
        (std::fs::File::from_raw_fd(fd), name)
    }
}

/// Run the ignored test `name` of this binary with a controlling pty, give
/// it `to_master` on the master side, and return `(finished, master output)`.
fn run_with_pty(name: &str, dir: &Path, to_master: &str, limit: Duration) -> (bool, String) {
    use std::io::{Read as _, Write as _};
    let (mut master, slave_path) = open_pty();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(std::ffi::OsStr::from_bytes(slave_path.to_bytes()))
        .unwrap();
    let slave_fd = slave.as_raw_fd();
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--ignored",
        "--exact",
        name,
        "--nocapture",
        "--test-threads=1",
    ])
    .env(INNER_ENV, dir)
    .stdin(slave.try_clone().unwrap())
    .stdout(slave.try_clone().unwrap())
    .stderr(slave.try_clone().unwrap());
    // SAFETY: `setsid` and `ioctl` are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() < 0 || libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    drop(slave);
    master.write_all(to_master.as_bytes()).unwrap();
    let started = Instant::now();
    let finished = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if started.elapsed() > limit {
            // The inner test sits in a stopped group: kill it by pid and
            // take its group down with it.
            // SAFETY: plain ints naming our own child.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
                libc::kill(child.id() as i32, libc::SIGKILL);
            }
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut out = String::new();
    let mut buf = [0u8; 4096];
    // SAFETY: nonblocking drain of what the child already wrote.
    unsafe {
        libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
    }
    while let Ok(n) = master.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    (finished, out)
}

/// A phase script run through `spawn_bash_script` for phase `phase`.
fn run_script(dir: &Path, phase: &str, body: &str, interactive: bool) -> i32 {
    let script = dir.join("phase.sh");
    std::fs::write(&script, body).unwrap();
    spawn_bash_script(SpawnBashScriptArgs {
        script: &script,
        argv_word: phase,
        vars: Vec::new(),
        iso: Isolation::default(),
        category: "test-cat",
        pf: "test-pf-1.0",
        log_file: PhaseLog::Capture(&dir.join("phase.log")),
        context: phase,
        interactive,
    })
    .unwrap()
}

#[test]
#[ignore = "runs inside run_with_pty"]
fn inner_interactive_bash_in_a_build_phase() {
    let dir = PathBuf::from(std::env::var(INNER_ENV).unwrap());
    assert_eq!(run_script(&dir, "src_test", "bash -i -c true\n", false), 0);
}

/// `bash -i` inside a build phase must not stop the phase (the freeze of
/// `app-shells/bash`'s `make check`).
#[test]
fn an_interactive_bash_in_a_build_phase_does_not_stop_it() {
    let tmp = TempDir::new("portuale-test-pty-bash-i");
    let (finished, out) = run_with_pty(
        "ebuild_phases::tests::terminal::inner_interactive_bash_in_a_build_phase",
        tmp.path(),
        "",
        Duration::from_secs(20),
    );
    assert!(finished, "the phase stopped (SIGTTIN); output:\n{out}");
}

#[test]
#[ignore = "runs inside run_with_pty"]
fn inner_config_phase_reads_the_terminal() {
    let dir = PathBuf::from(std::env::var(INNER_ENV).unwrap());
    let body = format!(
        "read x\necho \"got:$x\" > '{}'\n",
        dir.join("answer").display()
    );
    assert_eq!(run_script(&dir, "config", &body, false), 0);
}

/// `pkg_config` is interactive: it reads the terminal in the foreground.
#[test]
fn the_config_phase_can_read_the_terminal() {
    let tmp = TempDir::new("portuale-test-pty-config");
    let (finished, out) = run_with_pty(
        "ebuild_phases::tests::terminal::inner_config_phase_reads_the_terminal",
        tmp.path(),
        "hello\n",
        Duration::from_secs(20),
    );
    assert!(finished, "the config phase stopped; output:\n{out}");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("answer")).unwrap(),
        "got:hello\n"
    );
}

/// Placement table: Portage's `_global_pid_phases` stay in the parent group.
#[test]
fn only_the_global_pid_phases_stay_in_the_parent_group() {
    for phase in ["config", "preinst", "prerm", "postinst", "postrm"] {
        assert_eq!(
            PhasePlacement::for_phase(phase),
            PhasePlacement::ParentGroup
        );
    }
    for phase in [
        "setup",
        "src_compile",
        "src_test",
        "install",
        "package",
        "clean",
    ] {
        assert_eq!(PhasePlacement::for_phase(phase), PhasePlacement::NewSession);
    }
}

#[test]
#[ignore = "runs inside run_with_pty"]
fn inner_stdin_of_a_build_phase() {
    let dir = PathBuf::from(std::env::var(INNER_ENV).unwrap());
    let body = format!(
        "readlink /proc/self/fd/0 > '{}'\n",
        dir.join("fd0").display()
    );
    let interactive = dir.join("interactive").exists();
    assert_eq!(run_script(&dir, "src_compile", &body, interactive), 0);
}

/// A non-interactive build phase gets `/dev/null` as stdin, an
/// `interactive` package keeps the terminal. Portage:
/// `AbstractEbuildProcess.py:140-146`.
#[test]
fn stdin_is_dev_null_unless_the_package_is_interactive() {
    let name = "ebuild_phases::tests::terminal::inner_stdin_of_a_build_phase";
    let tmp = TempDir::new("portuale-test-pty-stdin");
    let (finished, out) = run_with_pty(name, tmp.path(), "", Duration::from_secs(20));
    assert!(finished, "{out}");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("fd0"))
            .unwrap()
            .trim(),
        "/dev/null"
    );
    let tmp = TempDir::new("portuale-test-pty-stdin-interactive");
    std::fs::write(tmp.path().join("interactive"), "").unwrap();
    let (finished, out) = run_with_pty(name, tmp.path(), "", Duration::from_secs(20));
    assert!(finished, "{out}");
    assert!(
        std::fs::read_to_string(tmp.path().join("fd0"))
            .unwrap()
            .trim()
            .starts_with("/dev/pts/"),
        "an interactive package keeps the terminal"
    );
}

/// Portage tests the raw `PROPERTIES` words (`AbstractEbuildProcess.py:143`),
/// so a USE-conditional `interactive` counts whatever the flag.
#[test]
fn a_conditional_interactive_property_still_counts() {
    assert!(raw_properties_name_interactive("foo? ( interactive )"));
    assert!(raw_properties_name_interactive("live interactive"));
    assert!(!raw_properties_name_interactive("live test_network"));
    assert!(!raw_properties_name_interactive("noninteractive"));
}
