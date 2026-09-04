use super::*;

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
