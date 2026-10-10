use super::*;

#[test]
#[cfg(unix)]
fn ask_read_news_eof_exits_before_resolve_like_portage() {
    // Backlog #234 (a): real `_emerge/UserQuery.query`
    // (`_emerge/UserQuery.py:74-76`) prints `Interrupted.` and exits
    // `128 + SIGINT` on EOF -- the news prompt's `sys.exit` fires
    // before `action_build`. EOF here is a VEOF byte (`Ctrl-D`)
    // written to the pty master: the slave line discipline turns it
    // into a zero-byte read, exactly how a terminal user types EOF
    // into real's `input()` (which then raises `EOFError`). The
    // master must stay open until the child exits -- closing it
    // first makes the slave stop being a tty at all (Linux drops
    // the termios once the last master closes), so the run would
    // die in the up-front `--ask` isatty gate instead of reaching
    // the prompt. Must print `Interrupted.`, exit 130, and never
    // reach the resolve (`Calculating dependencies` stays absent);
    // before the fix the run skipped the spawn, resolved, and only
    // exited 130 at the later merge prompt.
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
    // Backlog #240: a `^C` at the merge prompt exits from inside
    // `query`, so the prompt's `== "No"` check and its `Quitting.`
    // never run.
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
    // Backlog #240, the news prompt (`_emerge/actions.py:4266-4288`):
    // a `^C` there exits before `action_build`, like the #234 EOF arm,
    // with no eselect spawn and no resolve.
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
    // Backlog #240 follow-up: the config `Selection?` menu goes
    // through the same `UserQuery.query` in real
    // (`_emerge/actions.py:746` through
    // `_emerge/UserQuery.py:74-76`), so a `^C` there prints
    // `Interrupted.` and exits `128 + SIGINT` by exit -- `X`'s own
    // `Quitting.` (`actions.py:747-748`) never runs. Two fake vdb
    // entries make `dev-libs/seltest` match twice, reaching the
    // menu with no ebuild work at all; same `^C`-through-the-pty
    // delivery and prompt-synchronized timing as the merge-prompt
    // test above.
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
fn sigint_during_merge_still_dies_by_signal_like_before() {
    // Backlog #240 scoping pin: the guard lives strictly inside
    // the prompt functions, so a `^C` outside any prompt keeps
    // the default disposition -- death by signal, exactly as
    // before this slice. A promptless `emerge --oneshot` merge of
    // `dev-libs/schedok` runs ebuild phases for well over a
    // second after printing `>>> Emerging`, so a `^C`
    // synchronized on that marker deterministically lands outside
    // any prompt: no `Interrupted.` line, no exit code,
    // `signal() == Some(SIGINT)`.
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
        status.signal(),
        Some(libc::SIGINT),
        "SIGINT outside a prompt must kill the process: stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert_eq!(
        status.code(),
        None,
        "a signal death has no exit code: stdout: {}",
        String::from_utf8_lossy(&stdout),
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
    // Backlog #240's second fix: EOF at the merge prompt printed a
    // spurious `Quitting.` after `Interrupted.` (the `None` arm
    // fell into the decline arm). Real exits from inside
    // `UserQuery.query` (`_emerge/UserQuery.py:74-76`), so only
    // `Interrupted.` prints -- same VEOF delivery as the #234 news
    // EOF test, at the later merge prompt instead.
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
    // Backlog #246: real `_emerge/actions.py:3983-4006` offers a
    // non-privileged caller `Would you like to add --pretend to
    // options?` when `--ask` is set, instead of refusing outright.
    // `emerge --ask --oneshot dev-libs/schedok` with an unowned
    // ROOT must print `This action requires superuser access...`
    // and the question on stdout (real's `writemsg_stdout` +
    // `print(bold(prompt))`; only the `[Yes/No]` choices go to
    // stderr, via readline's `input()` -- host probe 2026-09-29,
    // `/usr/sbin/emerge` 3.0.82.2 as uid 1000, secpass 0,
    // verbatim pty bytes for the No run:
    //   `This action requires superuser access...`
    //   `[01mWould you like to add --pretend to options?[39;49;00m `
    //   (stderr:) `[32;01mYes[39;49;00m/[31;01mNo[39;49;00m] `
    //   rc 130, nothing else printed).
    // The answer here is "No" (rc 130); the arms are pinned
    // separately below.
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
    // Backlog #246, the "No" arm: real returns `128 + SIGINT`
    // with nothing else printed (`actions.py:3997-4003` -- the
    // `== "No"` comparison just returns, no notice, no resolve).
    // Answering `n` must exit 130 with no `Interrupted.` line
    // (that line is the EOF/SIGINT arm's, #240) and must never
    // reach the resolve (`Calculating dependencies` stays absent);
    // the old outright refusal (`superuser access is required` on
    // stderr) must be gone on this path.
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
    // Backlog #231 (c): real catches the spawn's `OSError`
    // (eselect missing) and prints `Please install eselect to use
    // this feature.` (`actions.py:4284-4287`). `PATH` holds only an
    // empty dir, so the spawn must fail; the "Yes" is still
    // consumed and the declined merge prompt still exits 130.
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
    // --ask-enter-invalid: a bare Enter is not a match, a real
    // answer still is (real UserQuery.query's own `if response or
    // not enter_invalid` gate).
    assert_eq!(classify_yes_no("", true), None);
    assert_eq!(classify_yes_no("y", true), Some(true));
    assert_eq!(classify_yes_no("No", true), Some(false));
}

/// Part 2 of the same race: after the first line is served the
/// kernel must hold nothing -- otherwise a child that inherits the
/// fd and reads it (real's helper shape) steals the next answer and
/// the next prompt hangs until the #259 watchdog fires. That is
/// exactly the Z0 capture: both answers pre-written, the news
/// prompt answered, the merge prompt hung with no input left.
#[test]
#[cfg(unix)]
fn prompt_line_buffer_drains_the_kernel_so_a_child_cannot_steal_the_next_answer() {
    // A pty, not a pipe: the canonical line discipline is what
    // queues the second answer in the kernel in the real capture.
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
        // The kernel must be drained: a non-blocking read gets
        // EAGAIN, which is exactly what a stealing child would see.
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

/// Backlog #259's proof: a child that never exits is killed at the
/// bound and the test fails with the `watchdog:` prefix the race
/// chase greps for -- instead of hanging the whole `cargo test` run
/// the way the #259 incident did (20 hours on a pty-blocked child).
#[test]
#[cfg(unix)]
fn pty_watchdog_kills_a_hung_child_and_fails_the_test() {
    // `sleep` is the stand-in for the #259 child stuck in
    // `read_prompt_line`'s blocking read: it will never exit on its
    // own. A 1 s bound keeps the test itself fast.
    // SAFETY: this is the only test setting the watchdog variable,
    // and the value is read once per `wait_pty_*` call in this
    // thread's own child.
    unsafe {
        std::env::set_var("PORTUALE_TEST_PTY_WATCHDOG_SECS", "1");
    }
    let child = std::process::Command::new("sleep")
        .arg("3600")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn the hung child");
    let result = std::panic::catch_unwind(|| wait_pty_output(child));
    assert!(result.is_err(), "the watchdog must panic, not return");
    let msg = result
        .err()
        .and_then(|e| e.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        msg.contains("watchdog:"),
        "the panic must carry the `watchdog:` prefix the chase greps for: {msg}"
    );
    // SAFETY: paired with the `set_var` above in the same test.
    unsafe {
        std::env::remove_var("PORTUALE_TEST_PTY_WATCHDOG_SECS");
    }
}

#[test]
#[cfg(unix)]
fn ask_declined_writes_no_resume_list() {
    // #179: real `_emerge/actions.py:525-536` returns `128 + SIGINT`
    // on a "No" answer before `Scheduler` runs -- a declined run
    // writes nothing (and rotates nothing: the rotation sits after
    // the prompt). `--ask` needs a real TTY (the up-front isatty
    // gate), so the decline is answered through a pty; the stale
    // multi-item list must come back byte-identical.
    use std::io::Write;
    let portuale_bin = built_portuale_bin();
    let (root, before) = stale_resume_root("ask");
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
        std::fs::read(crate::mtimedb::mtimedb_path(&root)).unwrap(),
        before,
        "a declined --ask run must leave a stale resume list byte-identical"
    );
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[cfg(unix)]
fn ask_pretend_offer_yes_continues_as_pretend_like_portage() {
    // Backlog #246, the "Yes" arm: real sets
    // `opts["--pretend"] = True`, pops `--ask`, and the run
    // continues as a pretend run (`actions.py:4004-4006`). Host
    // probe 2026-09-29 (`printf 'y\n' | script -qec
    // "/usr/sbin/emerge --ask --depclean"`): after the offer the
    // run prints the depclean notice, `Calculating dependencies
    // ... done!`, and continues -- with no second prompt, since
    // `--ask` is gone. Here answering `y` must resolve the
    // pretend merge list (`dev-libs/schedok-1.0`) with rc 0 and
    // must not prompt again (`Would you like to merge these
    // packages?` stays absent -- `--ask` is no longer in effect).
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
    // Backlog #234 (b): `--ask=True` parses as ask-on (real
    // `true_y_or_n` choices + `in true_y` normalization,
    // `main.py:321-322,802-805`), so a non-terminal stdin hits real
    // `actions.py:3920-3926`'s gate exactly like a bare `--ask`
    // (before the fix `--ask=True` died as an unrecognized option).
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
    // Real `actions.py:3920-3926`: "--ask" is rejected outright, up
    // front, when stdin isn't a real terminal -- there would be
    // nobody able to answer a prompt piped from a non-interactive
    // source. Checked right after CLI parsing finishes, before any
    // atom resolution, so this fires even for an atom that
    // wouldn't otherwise resolve.
    let mut portuale_bin = std::env::current_exe().expect("current test exe");
    portuale_bin.pop();
    if portuale_bin.ends_with("deps") {
        portuale_bin.pop();
    }
    portuale_bin.push("portuale");

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

/// #259's root race, chased from the Z0 watchdog capture
/// (2026-09-30): `read_prompt_line` is a raw `libc::read` with no
/// userspace leftover, so a multi-line chunk becomes ONE answer and
/// a line left in the kernel is stealable by any child spawned with
/// inherited stdin (`eselect`/`renice`/`ionice` all use `.status()`).
/// Real is protected by Python's buffered `sys.stdin` (`input()`
/// drains the kernel and serves leftovers from userspace). Part 1:
/// one line per call.
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
