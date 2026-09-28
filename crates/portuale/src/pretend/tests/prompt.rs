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

#[test]
#[cfg(unix)]
fn ask_declined_writes_no_resume_list() {
    // Portage: actions.py. A declined `--ask` run writes nothing.
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
