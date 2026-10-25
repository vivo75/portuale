use super::*;

#[test]
#[cfg(unix)]
fn ask_read_news_eof_exits_before_resolve_like_portage() {
    // Portage: UserQuery.py, query. EOF at the news prompt prints `Interrupted.` and
    // exits 130 before the resolve. The master stays open until the child exits,
    // or the slave stops being a tty and the run dies in the isatty gate.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("ask_read_news_eof").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = news_resolve_env(&root, &base.join("pt"));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args([
            "emerge",
            "--ask",
            "--read-news",
            "--oneshot",
            "dev-libs/schedok",
        ])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    // The pty buffers until the child prompts -- no synchronisation
    // needed.
    master
        .write_all(b"\x04")
        .expect("EOF the news prompt with VEOF");
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
    assert!(stdout.contains("Interrupted."), "{stdout}");
    assert_eq!(
        stdout.matches("news items need reading").count(),
        1,
        "{stdout}"
    );
    assert!(
        !stdout.contains("Calculating dependencies"),
        "an interrupted news prompt must exit before the resolve: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_merge_sigint_prints_interrupted_and_exits_130_like_portage() {
    // Portage: UserQuery.py, query. A `^C` at the merge prompt exits from inside
    // `query`, so its `Quitting.` never prints.
    ctrl_c_at_prompt(
        "ask_merge_sigint",
        &["emerge", "--ask", "--oneshot", "dev-libs/schedok"],
        fixture_resolve_env,
        b"Would you like to merge these packages?",
    );
}

#[test]
#[cfg(unix)]
fn ask_read_news_sigint_exits_before_resolve_like_portage() {
    // Portage: actions.py. A `^C` at the news prompt exits before the resolve,
    // with no eselect spawn.
    let text = ctrl_c_at_prompt(
        "ask_read_news_sigint",
        &[
            "emerge",
            "--ask",
            "--read-news",
            "--oneshot",
            "dev-libs/schedok",
        ],
        news_resolve_env,
        b"Would you like to read the news items",
    );
    assert_eq!(text.matches("news items need reading").count(), 1, "{text}");
    assert!(
        !text.contains("Calculating dependencies"),
        "an interrupted news prompt must exit before the resolve: {text}"
    );
}

#[test]
#[cfg(unix)]
fn ask_config_select_sigint_prints_interrupted_and_exits_130_like_portage() {
    // Portage: actions.py, UserQuery.py, query. A `^C` at the `Selection?` menu
    // prints `Interrupted.` and exits 130; its own `Quitting.` never runs.
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("ask_config_select_sigint").keep();
    let root = base.join("root");
    for v in ["1.0", "2.0"] {
        std::fs::create_dir_all(root.join(format!("var/db/pkg/dev-libs/seltest-{v}"))).unwrap();
    }
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let (master, slave_stdio) = pty_pair();
    let (child, mut master, mut stdout_pipe, err_handle) = sigint_prompt_child(
        &portuale_bin,
        &["emerge", "--ask", "--config", "dev-libs/seltest"],
        env,
        master,
        slave_stdio,
    );
    let mut stdout =
        read_stdout_until_marker(&mut stdout_pipe, b"Selection?", "the Selection? prompt");
    master
        .write_all(b"\x03")
        .expect("Ctrl-C the Selection? prompt");
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
    let text = String::from_utf8_lossy(&stdout);
    assert!(
        text.contains("Please select a package to configure:"),
        "{text}"
    );
    assert_eq!(text.matches("Interrupted.").count(), 1, "{text}");
    assert!(
        !text.contains("Quitting."),
        "an interrupted Selection? prompt must not print `Quitting.` like real: {text}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn sigint_during_merge_exits_130_after_exiting_on_signal_like_portage() {
    // Portage: Scheduler.py, merge (`sighandler`). A `^C` during a merge prints
    // `Exiting on signal 2` and exits 128+2 (backlog #411).
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("merge_sigint_dies_by_signal").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let (master, slave_stdio) = pty_pair();
    let (child, mut master, mut stdout_pipe, err_handle) = sigint_prompt_child(
        &portuale_bin,
        &["emerge", "--oneshot", "dev-libs/schedok"],
        env,
        master,
        slave_stdio,
    );
    let mut stdout = read_stdout_until_marker(&mut stdout_pipe, b">>> Emerging", "the merge start");
    master.write_all(b"\x03").expect("Ctrl-C the running merge");
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
        Some(128 + libc::SIGINT),
        "SIGINT outside a prompt must exit 130: stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(status.signal(), None);
    assert!(
        String::from_utf8_lossy(&stderr).contains("\n\nExiting on signal 2\n"),
        "{}",
        String::from_utf8_lossy(&stderr)
    );
    let text = String::from_utf8_lossy(&stdout);
    assert!(
        !text.contains("Interrupted."),
        "only the prompt arm prints `Interrupted.`: {text}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_merge_eof_prints_interrupted_without_quitting_like_portage() {
    // Portage: UserQuery.py, query. EOF at the merge prompt prints only `Interrupted.`.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("ask_merge_eof").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let (master, slave_stdio) = pty_pair();
    let (child, mut master, mut stdout_pipe, err_handle) = sigint_prompt_child(
        &portuale_bin,
        &["emerge", "--ask", "--oneshot", "dev-libs/schedok"],
        env,
        master,
        slave_stdio,
    );
    let mut stdout = read_stdout_until_marker(
        &mut stdout_pipe,
        b"Would you like to merge these packages?",
        "the merge prompt",
    );
    master
        .write_all(b"\x04")
        .expect("EOF the merge prompt with VEOF");
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
    let text = String::from_utf8_lossy(&stdout);
    assert_eq!(text.matches("Interrupted.").count(), 1, "{text}");
    assert!(
        !text.contains("Quitting."),
        "an EOF merge prompt must not print `Quitting.` like real: {text}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_pretend_offer_prompts_on_stdout_for_a_write_action() {
    // Portage: actions.py. `--ask` with an unowned ROOT offers
    // `Would you like to add --pretend to options?` on stdout. The answer here is "No".
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let Some(unowned) = ask_pretend_unowned_root() else {
        return;
    };
    let base = TempDir::new("ask_pretend_offer").keep();
    let env = fixture_resolve_env(&unowned, &base.join("pt"));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask", "--oneshot", "dev-libs/schedok"])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"n\n").expect("decline the offer");
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("This action requires superuser access..."),
        "the access line must print on stdout like real: {stdout}"
    );
    assert!(
        stdout.contains("Would you like to add --pretend to options?"),
        "the offer must print on stdout like real: {stdout}"
    );
    assert!(
        !stderr.contains("This action requires superuser access..."),
        "the access line is stdout in real, not stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_pretend_offer_no_exits_130_like_portage() {
    // A declined offer exits 130 with no `Interrupted.` and never reaches the resolve.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let Some(unowned) = ask_pretend_unowned_root() else {
        return;
    };
    let base = TempDir::new("ask_pretend_offer_no").keep();
    let env = fixture_resolve_env(&unowned, &base.join("pt"));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask", "--oneshot", "dev-libs/schedok"])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"n\n").expect("decline the offer");
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("Interrupted."),
        "a declined offer is not an interrupt: {stdout}"
    );
    assert!(
        !stdout.contains("Calculating dependencies"),
        "a declined offer must exit before the resolve: {stdout}"
    );
    assert!(
        !stdout.contains("dev-libs/schedok-1.0"),
        "a declined offer must not print a merge list: {stdout}"
    );
    assert!(
        !stderr.contains("superuser access is required"),
        "the offer replaces the outright refusal: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[cfg(unix)]
fn ask_read_news_without_eselect_prints_portages_hint() {
    // Portage: actions.py. A failed eselect spawn prints the install hint.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let emptydir = TempDir::new("ask_read_news_empty_path").keep();
    let base = TempDir::new("ask_read_news_missing").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let mut env = news_resolve_env(&root, &base.join("pt"));
    env.push(("PATH".to_string(), emptydir.display().to_string()));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args([
            "emerge",
            "--ask",
            "--read-news",
            "--oneshot",
            "dev-libs/schedok",
        ])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"Yes\n").expect("answer the news prompt");
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Please install eselect to use this feature."),
        "{stderr}"
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&emptydir);
}

#[test]
fn classify_yes_no_prefix_matches_case_insensitively_and_rejects_garbage() {
    assert_eq!(classify_yes_no("", false), Some(true));
    assert_eq!(classify_yes_no("y", false), Some(true));
    assert_eq!(classify_yes_no("Y", false), Some(true));
    assert_eq!(classify_yes_no("yes", false), Some(true));
    assert_eq!(classify_yes_no("YES", false), Some(true));
    assert_eq!(classify_yes_no("n", false), Some(false));
    assert_eq!(classify_yes_no("No", false), Some(false));
    assert_eq!(classify_yes_no("garbage", false), None);
    assert_eq!(classify_yes_no("yesplease", false), None);
    // With `--ask-enter-invalid`, bare Enter matches nothing; an actual answer
    // still matches (Portage `UserQuery.query` gate).
    assert_eq!(classify_yes_no("", true), None);
    assert_eq!(classify_yes_no("y", true), Some(true));
    assert_eq!(classify_yes_no("No", true), Some(false));
}

// After the first line is served the kernel must hold nothing, or a child
// inheriting the fd could steal the next answer and hang the next prompt.
#[test]
#[cfg(unix)]
fn prompt_line_buffer_drains_the_kernel_so_a_child_cannot_steal_the_next_answer() {
    // A pty, not a pipe: the line discipline queues the second answer in the kernel.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        assert!(master >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(master), 0, "grantpt");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt");
        let cname = pty_slave_path(master);
        let slave = libc::open(
            cname.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        assert!(slave >= 0, "pty slave open");
        assert_eq!(
            libc::write(master, b"No\n".as_ptr() as *const libc::c_void, 3),
            3,
            "queue the first answer"
        );
        assert_eq!(
            libc::write(master, b"Yes\n".as_ptr() as *const libc::c_void, 4),
            4,
            "queue the second answer"
        );
        let mut buf = PromptLineBuffer::new();
        assert_eq!(buf.read_line(slave).as_deref(), Some("No\n"), "first line");
        // Drained means a stealing child would see EAGAIN.
        let flags = libc::fcntl(slave, libc::F_GETFL);
        assert_eq!(
            libc::fcntl(slave, libc::F_SETFL, flags | libc::O_NONBLOCK),
            0,
            "set non-blocking"
        );
        let mut stolen = [0u8; 8];
        let n = libc::read(slave, stolen.as_mut_ptr() as *mut libc::c_void, 8);
        assert!(
            n < 0,
            "the next answer must not sit in the kernel (a child would steal it); read returned {:?}",
            &stolen[..n.max(0) as usize]
        );
        assert_eq!(
            libc::fcntl(slave, libc::F_SETFL, flags),
            0,
            "restore blocking"
        );
        assert_eq!(
            buf.read_line(slave).as_deref(),
            Some("Yes\n"),
            "the next answer is served from the userspace leftover"
        );
        libc::close(slave);
        libc::close(master);
    }
}

// A hung child is killed at the bound with a `watchdog:` panic, instead of
// hanging the whole run.
#[test]
#[cfg(unix)]
fn pty_watchdog_kills_a_hung_child_and_fails_the_test() {
    // `sleep` stands in for a child stuck in a blocking read; a 1 s bound keeps it fast.
    // The bound travels as an argument, never through the process environment:
    // every pty wait in this process reads `PORTUALE_TEST_PTY_WATCHDOG_SECS`,
    // so exporting a 1 s value here would let a parallel pty test kill its own
    // child on a slow machine.
    let child = std::process::Command::new("sleep")
        .arg("3600")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the hung child");
    let result = std::panic::catch_unwind(|| {
        wait_pty_output_bound(child, std::time::Duration::from_secs(1))
    });
    assert!(result.is_err(), "the watchdog must panic, not return");
    let msg = result
        .err()
        .and_then(|e| e.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        msg.contains("watchdog:"),
        "the panic must carry the `watchdog:` prefix the chase greps for: {msg}"
    );
}

#[test]
#[cfg(unix)]
fn ask_declined_writes_no_resume_list() {
    // Portage: actions.py. A declined `--ask` run writes no resume list.
    // Backlog #562: the pre-resolve updates check (`actions.py:3944`)
    // runs before the prompt, so the `updates` record is still
    // committed -- the stale `resume` list itself stays identical.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let (root, _before) = stale_resume_root("ask");
    let before_list = crate::mtimedb::read_resume_list(&root);
    let tmp = TempDir::new("resume_no_write_ask").keep();

    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask", "--oneshot", "dev-libs/schedok"])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(fixture_resolve_env(&root, &tmp))
        .spawn()
        .expect("portuale emerge spawns");
    // The pty buffers until the child prompts -- no synchronisation
    // needed.
    master.write_all(b"No\n").expect("answer the --ask prompt");
    let output = wait_pty_output(child);
    drop(master);
    assert_eq!(
        output.status.code(),
        Some(130),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Quitting."),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        crate::mtimedb::read_resume_list(&root),
        before_list,
        "a declined --ask run must leave a stale resume list identical"
    );
    assert!(
        !crate::mtimedb::read_updates_mtimes(&root).is_empty(),
        "the pre-prompt updates check still commits its record"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn ask_pretend_offer_yes_continues_as_pretend_like_portage() {
    // Portage: actions.py. "Yes" continues as a pretend run with `--ask` dropped,
    // so no merge prompt follows.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let Some(unowned) = ask_pretend_unowned_root() else {
        return;
    };
    let base = TempDir::new("ask_pretend_offer_yes").keep();
    let env = fixture_resolve_env(&unowned, &base.join("pt"));
    let (mut master, slave_stdio) = pty_pair();
    let child = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask", "--oneshot", "dev-libs/schedok"])
        .stdin(slave_stdio)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .spawn()
        .expect("portuale emerge spawns");
    master.write_all(b"y\n").expect("accept the offer");
    let output = wait_pty_output(child);
    drop(master);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Would you like to add --pretend to options?"),
        "the offer must print first: {stdout}"
    );
    assert!(
        stdout.contains("dev-libs/schedok-1.0"),
        "accepting the offer must continue as a pretend run: {stdout}"
    );
    assert!(
        !stdout.contains("Would you like to merge these packages?"),
        "--ask is dropped with the offer, so no merge prompt follows: {stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn ask_true_spelling_hits_the_tty_gate_like_portage() {
    // Portage `true_y_or_n`: `--ask=True` parses as ask-on, hitting the tty gate
    // like a bare `--ask`.
    let portuale_bin = built_portuale_bin();
    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask=True", "dev-libs/does-not-matter"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"--ask\" should only be used in a terminal"),
        "{stderr}"
    );
}

#[test]
fn ask_without_a_tty_exits_immediately_instead_of_prompting() {
    // Portage: actions.py. `--ask` without a terminal exits up front, before resolving.
    let portuale_bin = crate::self_exe::built_portuale_bin();

    let output = std::process::Command::new(&portuale_bin)
        .args(["emerge", "--ask", "dev-libs/does-not-matter"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("portuale emerge spawns");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"--ask\" should only be used in a terminal"),
        "{stderr}"
    );
}

// `read_prompt_line` must return exactly one line per call, buffering leftovers
// in userspace like Portage's buffered `sys.stdin`.
#[test]
#[cfg(unix)]
fn prompt_line_buffer_returns_exactly_one_line_per_call() {
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe");
    let (read_fd, write_fd) = (fds[0], fds[1]);
    let written = unsafe { libc::write(write_fd, b"No\nYes\n".as_ptr() as *const libc::c_void, 7) };
    assert_eq!(written, 7, "write both answers in one chunk");
    unsafe { libc::close(write_fd) };
    let mut buf = PromptLineBuffer::new();
    assert_eq!(
        buf.read_line(read_fd).as_deref(),
        Some("No\n"),
        "first call returns exactly one line"
    );
    assert_eq!(
        buf.read_line(read_fd).as_deref(),
        Some("Yes\n"),
        "second call returns the next line"
    );
    assert_eq!(
        buf.read_line(read_fd),
        None,
        "EOF once both lines are served"
    );
    unsafe { libc::close(read_fd) };
}
