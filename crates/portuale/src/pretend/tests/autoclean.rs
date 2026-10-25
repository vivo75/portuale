use super::*;

/// Backlog #404 (02.346 S11): auto-clean runs the `--clean` selection
/// system-wide after a successful merge with a user-set `AUTOCLEAN=yes`.
/// Scratch vdb with two same-slot instances of one cp: `old-1` (to be
/// selected) and `old-2` (protected). Field files only; no `CONTENTS`
/// means no owned paths, so the unmerge just drops the entry.
/// Portage: `_emerge/actions.py`, `action_build` (`>>> Auto-cleaning
/// packages...` then `unmerge(..., "clean", [], autoclean=1)`), gated on
/// `settings.get("AUTOCLEAN") == "yes"` (make.globals default `no`).
fn autoclean_leftover_root(tag: &str) -> std::path::PathBuf {
    let root = TempDir::new(&format!("autoclean_{tag}")).keep();
    for (pf, counter) in [("old-1", "7"), ("old-2", "8")] {
        let entry = root.join("var/db/pkg/dev-libs").join(pf);
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("SLOT"), "0\n").unwrap();
        std::fs::write(entry.join("COUNTER"), format!("{counter}\n")).unwrap();
        std::fs::write(entry.join("CONTENTS"), "").unwrap();
    }
    root
}

#[test]
fn autoclean_gate_reads_the_resolved_autoclean_value() {
    // Portage: make.globals ships `AUTOCLEAN="no"`; only an explicit
    // `yes` (make.conf, profile, or the calling env over the config)
    // triggers the post-merge clean.
    assert!(!autoclean_enabled(&portage_profile::Config::default()));
    let mut no = portage_profile::Config::default();
    no.other_vars
        .insert("AUTOCLEAN".to_string(), "no".to_string());
    assert!(!autoclean_enabled(&no));
    let mut yes = portage_profile::Config::default();
    yes.other_vars
        .insert("AUTOCLEAN".to_string(), "yes".to_string());
    assert!(autoclean_enabled(&yes));
}

#[test]
fn autoclean_unmerge_arm_removes_the_outdated_instance() {
    // Portage: same-CP+slot older instance selected, newer protected
    // (S404 oracle cell cl01c).
    let color = crate::color::Colorizer::new(false);
    let root = autoclean_leftover_root("unmerge");
    let config = portage_profile::Config::default();
    run_autoclean_after_merge(RunAutocleanAfterMergeArgs {
        root: &root,
        config_root: &root,
        config: &config,
        repos: &[],
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        ask: false,
        quiet: true,
        noinfo: true,
        color: &color,
    });
    assert!(
        !root.join("var/db/pkg/dev-libs/old-1").exists(),
        "the outdated same-slot instance must be unmerged"
    );
    assert!(
        root.join("var/db/pkg/dev-libs/old-2").is_dir(),
        "the protected newer instance must survive"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn autoclean_no_outdated_arm_changes_nothing() {
    // Portage: `>>> No outdated packages were found on your system.`
    // (S404 oracle cells cl02/cl05).
    let color = crate::color::Colorizer::new(false);
    let root = TempDir::new("autoclean_no_outdated").keep();
    let entry = root.join("var/db/pkg/dev-libs/solo-1");
    std::fs::create_dir_all(&entry).unwrap();
    std::fs::write(entry.join("SLOT"), "0\n").unwrap();
    std::fs::write(entry.join("COUNTER"), "1\n").unwrap();
    std::fs::write(entry.join("CONTENTS"), "").unwrap();
    let config = portage_profile::Config::default();
    run_autoclean_after_merge(RunAutocleanAfterMergeArgs {
        root: &root,
        config_root: &root,
        config: &config,
        repos: &[],
        shell: crate::ebuild_phases::ShellBackend::default(),
        debug: false,
        ask: false,
        quiet: true,
        noinfo: true,
        color: &color,
    });
    assert!(
        entry.is_dir(),
        "a single instance is protected: nothing may be removed"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Backlog #560 (fix round): with a user-set `AUTOCLEAN=yes`, the
/// post-merge tail prints `>>> Using system located in ROOT tree
/// <root>/` exactly once. Portage prints it inside `_unmerge_display`
/// (`unmerge.py:187-193`, no action gate); portuale's
/// `run_autoclean_after_merge` wrapper printed it too, doubling the
/// line (live `AUTOCLEAN=yes` merge proof in the task report; the
/// shared `run_prune_nodeps_or_clean` header is the single Portage
/// source). AUTOCLEAN=no/default silence needs no new pin: the gate
/// stays off by `autoclean_gate_reads_the_resolved_autoclean_value`
/// and the wrapper is only reachable through it (`mod.rs`, both
/// untouched). Re-exec pattern per
/// `vdb_fuse::tests::a_detached_daemon_writes_its_stderr_to_the_log_file`:
/// the child runs the printing scenario, the parent counts the lines.
#[test]
fn autoclean_tail_prints_the_using_system_header_exactly_once() {
    if std::env::var_os("PORTUALE_AUTOCLEAN_TAIL_CHILD").is_some() {
        let color = crate::color::Colorizer::new(false);
        let root = TempDir::new("autoclean_tail_once").keep();
        let entry = root.join("var/db/pkg/dev-libs/solo-1");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("SLOT"), "0\n").unwrap();
        std::fs::write(entry.join("COUNTER"), "1\n").unwrap();
        std::fs::write(entry.join("CONTENTS"), "").unwrap();
        let config = portage_profile::Config::default();
        run_autoclean_after_merge(RunAutocleanAfterMergeArgs {
            root: &root,
            config_root: &root,
            config: &config,
            repos: &[],
            shell: crate::ebuild_phases::ShellBackend::default(),
            debug: false,
            ask: false,
            quiet: false,
            noinfo: true,
            color: &color,
        });
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--test-threads=1",
            "--exact",
            "pretend::tests::autoclean::autoclean_tail_prints_the_using_system_header_exactly_once",
            "--nocapture",
        ])
        .env("PORTUALE_AUTOCLEAN_TAIL_CHILD", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "child scenario run failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.contains(">>> Using system located in ROOT tree"))
            .count(),
        1,
        "the header must print exactly once; full child stdout:\n{stdout}"
    );
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.contains(">>> Auto-cleaning packages..."))
            .count(),
        1,
        "the opening line must print exactly once; full child stdout:\n{stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l == ">>> No outdated packages were found on your system."),
        "the empty-clean line must survive the fix; full child stdout:\n{stdout}"
    );
}
