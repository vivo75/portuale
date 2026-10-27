use super::*;

#[test]
fn elf_machine_reads_both_endians_and_rejects_garbage() {
    // Little-endian x86-64 (EM_X86_64 = 62).
    let mut le = vec![0u8; 24];
    le[0..4].copy_from_slice(b"\x7fELF");
    le[4] = 2;
    le[5] = 1;
    le[18] = 62;
    le[19] = 0;
    assert_eq!(elf_machine(&le), Ok(62));
    // Big-endian short (EM_PPC64 = 21 reads byte-swapped).
    let mut be = vec![0u8; 24];
    be[0..4].copy_from_slice(b"\x7fELF");
    be[4] = 2;
    be[5] = 2;
    be[18] = 0;
    be[19] = 21;
    assert_eq!(elf_machine(&be), Ok(21));
    assert!(elf_machine(&le[..10]).is_err());
    let mut bad = le.clone();
    bad[0] = b'X';
    assert!(
        bad.get(0..4)
            .is_some_and(|magic| elf_machine(&bad).is_err() && magic != b"\x7fELF")
    );
    let mut enc = le.clone();
    enc[5] = 9;
    assert!(elf_machine(&enc).is_err());
}

#[test]
fn arch_matches_maps_machines_to_uname_names() {
    assert_eq!(arch_matches(62, "x86_64"), Some(true));
    assert_eq!(arch_matches(62, "amd64"), Some(true));
    assert_eq!(arch_matches(183, "aarch64"), Some(true));
    assert_eq!(arch_matches(183, "arm64"), Some(true));
    assert_eq!(arch_matches(3, "i686"), Some(true));
    assert_eq!(arch_matches(3, "i386"), Some(true));
    assert_eq!(arch_matches(40, "armv7l"), Some(true));
    assert_eq!(arch_matches(243, "riscv64"), Some(true));
    assert_eq!(arch_matches(21, "ppc64le"), Some(true));
    assert_eq!(arch_matches(22, "s390x"), Some(true));
    // Definite mismatches.
    assert_eq!(arch_matches(62, "aarch64"), Some(false));
    assert_eq!(arch_matches(183, "x86_64"), Some(false));
    assert_eq!(arch_matches(3, "x86_64"), Some(false));
    // Unknown pairs pass (a mismatch only when both are known).
    assert_eq!(arch_matches(9999, "x86_64"), None);
    assert_eq!(arch_matches(62, ""), None);
    assert_eq!(arch_matches(9999, ""), None);
}

#[test]
fn bin_search_dirs_prefers_the_override_pair() {
    let mut ctx = ctx_with_args("h", None);
    assert_eq!(bin_search_dirs(&ctx), vec!["/opt/bin", "/usr/local/bin"]);
    ctx.portuale_dir = Some("/srv/bin".to_string());
    assert_eq!(bin_search_dirs(&ctx), vec!["/srv/bin"]);
    // Per directory, `portuale` then `portuale-<hash>`.
    assert_eq!(
        bin_candidates(&bin_search_dirs(&ctx), S8_SHORT),
        vec![
            "/srv/bin/portuale".to_string(),
            format!("/srv/bin/portuale-{S8_SHORT}"),
        ]
    );
}

#[test]
fn bin_gates_use_the_first_match_in_d5_order() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
    // Both a plain and a hashed candidate match: the first (the plain
    // `/opt/bin/portuale`) wins.
    let values = s8_values(
        &[S8_DIGEST, "missing", S8_DIGEST, "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(plan, ClientBinPlan::Use("/opt/bin/portuale".to_string()));
    // Only the hashed name matches.
    let values = s8_values(
        &["missing", S8_DIGEST, "missing", "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Use(format!("/opt/bin/portuale-{S8_SHORT}"))
    );
    // A later directory's match wins when nothing earlier matches.
    let values = s8_values(
        &["missing", "missing", S8_DIGEST, "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Use("/usr/local/bin/portuale".to_string())
    );
}

#[test]
fn bin_gates_plan_an_install_when_nothing_matches() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
    let values = s8_values(
        &["missing", "missing", "missing", "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
    );
    // An unwritable first directory falls through to the second.
    let values = s8_values(
        &["missing", "missing", "missing", "missing"],
        &[("yes", "no", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Install(format!("/usr/local/bin/portuale-{S8_SHORT}"))
    );
}

#[test]
fn bin_gates_report_a_mismatched_plain_portuale_and_leave_it_alone() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
    // A non-matching plain `portuale` is reported and left alone: the
    // install goes next to it as `portuale-<hash>`, never over it.
    let values = s8_values(
        &[S8_WRONG, "missing", "missing", "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("/opt/bin/portuale") && n.contains("left alone")),
        "{notes:?}"
    );
}

#[test]
fn bin_gates_replace_a_corrupted_hashed_binary() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
    // A `portuale-<hash>` whose content does not match its name is
    // planned for replacement at the same path.
    let values = s8_values(
        &["missing", S8_WRONG, "missing", "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (plan, failures, notes) = evaluate_bin_gates(&values, &pre, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(
        plan,
        ClientBinPlan::Install(format!("/opt/bin/portuale-{S8_SHORT}"))
    );
    assert!(
        notes.iter().any(|n| n.contains("planned for replacement")),
        "{notes:?}"
    );
}

#[test]
fn bin_gates_fail_an_arch_mismatch_naming_the_binary_option() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 183);
    let values = s8_values(
        &["missing", "missing", "missing", "missing"],
        &[("yes", "yes", "755"), ("yes", "yes", "755")],
        "x86_64",
    );
    let (_, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("--remote-portuale-binary"),
        "{failures:?}"
    );
    assert!(failures[0].contains("x86_64"), "{failures:?}");
    // Unknown pairs pass (both sides must be known to mismatch).
    let unknown = s8_pre(&["/opt/bin"], 9999);
    let values = s8_values(&["missing", "missing"], &[("yes", "yes", "755")], "x86_64");
    let (plan, failures, _) = evaluate_bin_gates(&values, &unknown, false);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(matches!(plan, ClientBinPlan::Install(_)), "{plan:?}");
}

#[test]
fn bin_gates_fail_without_a_writable_dir() {
    let pre = s8_pre(&["/opt/bin", "/usr/local/bin"], 62);
    // Neither default directory usable, no override: the message names
    // the option and the root-login alternative.
    let values = s8_values(
        &["missing", "missing", "missing", "missing"],
        &[("no", "no", "unknown"), ("yes", "no", "755")],
        "x86_64",
    );
    let (_, failures, _) = evaluate_bin_gates(&values, &pre, false);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("--remote-portuale-dir"),
        "{failures:?}"
    );
    assert!(failures[0].contains("root"), "{failures:?}");
    // With the override, the failure names the override directory.
    let over = s8_pre(&["/srv/bin"], 62);
    let values = s8_values(
        &["missing", "missing"],
        &[("no", "no", "unknown")],
        "x86_64",
    );
    let (_, failures, _) = evaluate_bin_gates(&values, &over, true);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("--remote-portuale-dir") && failures[0].contains("/srv/bin"),
        "{failures:?}"
    );
}

#[test]
fn bin_gates_refuse_a_world_writable_dir_without_the_sticky_bit() {
    let pre = s8_pre(&["/srv/bin"], 62);
    // `0777`: world-writable, no sticky bit -- refused even though
    // the directory exists and is writable.
    let values = s8_values(&["missing", "missing"], &[("yes", "yes", "777")], "x86_64");
    let (_, failures, _) = evaluate_bin_gates(&values, &pre, true);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("sticky"), "{failures:?}");
    // `1777` (sticky) and `0755` (not world-writable) are usable.
    for mode in ["1777", "755", "775"] {
        let values = s8_values(&["missing", "missing"], &[("yes", "yes", mode)], "x86_64");
        let (plan, failures, _) = evaluate_bin_gates(&values, &pre, true);
        assert!(failures.is_empty(), "{mode}: {failures:?}");
        assert_eq!(
            plan,
            ClientBinPlan::Install(format!("/srv/bin/portuale-{S8_SHORT}")),
            "{mode}"
        );
    }
}

/// End to end: the first run installs `portuale-<hash>` (with the
/// `install-bin 0` line) and merges the unit; the second run reuses
/// it with no `install-bin` line.
#[test]
fn s8_install_then_reuse_over_local_transport() {
    let tree = S8Tree::fresh("install-reuse");
    let bin = s8_portuale_bin();
    assert!(bin.is_file(), "run cargo build --release -p portuale first");
    let digest = s8_sha256(&bin);
    let short: String = digest.chars().take(16).collect();
    let installed = tree.bindir.join(format!("portuale-{short}"));

    let first = s8_run(&tree, &[]);
    let stdout = s8_stdout(&first);
    assert_eq!(
        first.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        stdout.contains("portuale-remote: install-bin 0"),
        "first run must report the install:\n{stdout}"
    );
    assert!(
        stdout.contains(">>> Remote merged dev-libs/binpkgrmpkg-1.0"),
        "the unit merges after the install:\n{stdout}"
    );
    assert_eq!(s8_sha256(&installed), digest);

    let second = s8_run(&tree, &[]);
    let stdout = s8_stdout(&second);
    assert_eq!(
        second.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !stdout.contains("install-bin"),
        "a reuse must print no install-bin line:\n{stdout}"
    );
    assert_eq!(s8_sha256(&installed), digest);
}

/// End to end: a matching plain `portuale` placed first is reused --
/// nothing is installed next to it.
#[test]
fn s8_matching_plain_portuale_is_reused() {
    let tree = S8Tree::fresh("plain-match");
    let bin = s8_portuale_bin();
    let digest = s8_sha256(&bin);
    let short: String = digest.chars().take(16).collect();
    std::fs::copy(&bin, tree.bindir.join("portuale")).unwrap();

    let run = s8_run(&tree, &[]);
    let stdout = s8_stdout(&run);
    assert_eq!(
        run.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        !stdout.contains("install-bin"),
        "a matching plain binary installs nothing:\n{stdout}"
    );
    assert!(
        !tree.bindir.join(format!("portuale-{short}")).exists(),
        "no portuale-<hash> may appear next to a matching plain binary"
    );
}

/// End to end: a non-matching plain `portuale` is left byte-identical,
/// and `portuale-<hash>` is installed next to it.
#[test]
fn s8_mismatched_plain_portuale_is_left_alone() {
    let tree = S8Tree::fresh("plain-mismatch");
    let bin = s8_portuale_bin();
    let digest = s8_sha256(&bin);
    let short: String = digest.chars().take(16).collect();
    let plain = tree.bindir.join("portuale");
    std::fs::write(&plain, b"an operator's own install").unwrap();

    let run = s8_run(&tree, &[]);
    let stdout = s8_stdout(&run);
    assert_eq!(
        run.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("portuale-remote: install-bin 0"),
        "the hashed binary still installs:\n{stdout}"
    );
    assert_eq!(
        std::fs::read(&plain).unwrap(),
        b"an operator's own install",
        "the mismatched plain binary must be byte-identical"
    );
    assert_eq!(
        s8_sha256(&tree.bindir.join(format!("portuale-{short}"))),
        digest
    );
}

/// End to end: a corrupted `portuale-<hash>` (wrong digest) is replaced.
#[test]
fn s8_corrupted_hashed_binary_is_replaced() {
    let tree = S8Tree::fresh("corrupt-replace");
    let bin = s8_portuale_bin();
    let digest = s8_sha256(&bin);
    let short: String = digest.chars().take(16).collect();
    let hashed = tree.bindir.join(format!("portuale-{short}"));
    std::fs::write(&hashed, b"corrupted bytes").unwrap();

    let run = s8_run(&tree, &[]);
    let stdout = s8_stdout(&run);
    assert_eq!(
        run.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("portuale-remote: install-bin 0"),
        "the corrupted binary is reinstalled:\n{stdout}"
    );
    assert_eq!(s8_sha256(&hashed), digest);
}

/// End to end: `--pretend` installs nothing (pretend stays local, with
/// the remote options dropped).
#[test]
fn s8_pretend_installs_nothing() {
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

    let fixtures = s8_fixtures();
    let base = TempDir::new("portuale-remote-s8-pretend");
    let root = base.join("root");
    copy_tree(&fixtures.join("var"), &root.join("var"));
    let bindir = base.join("bin");
    std::fs::create_dir_all(&bindir).unwrap();
    let binhost = base.join("binhost");
    std::fs::create_dir_all(binhost.join("dev-libs")).unwrap();
    std::fs::copy(
        fixtures.join("pkgdir/dev-libs/binpkgrmpkg-1.0.tbz2"),
        binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"),
    )
    .unwrap();
    let size = std::fs::metadata(binhost.join("dev-libs/binpkgrmpkg-1.0.tbz2"))
        .unwrap()
        .len();
    std::fs::write(
        binhost.join("Packages"),
        format!(
            "TIMESTAMP: 0\nVERSION: 0\nPACKAGES: 1\n\nBUILD_ID: 1\nCPV: dev-libs/binpkgrmpkg-1.0\nDEFINED_PHASES: -\nEAPI: 8\nKEYWORDS: amd64\nPATH: dev-libs/binpkgrmpkg-1.0.tbz2\nREPO: testrepo\nSIZE: {size}\nSLOT: 0\nUSE:\n"
        ),
    )
    .unwrap();
    let clientetc = base.join("clientetc");
    copy_tree(
        &fixtures.join("etc/portage"),
        &clientetc.join("etc/portage"),
    );
    let profile_link = clientetc.join("etc/portage/make.profile");
    let _ = std::fs::remove_file(&profile_link);
    std::os::unix::fs::symlink(fixtures.join("repo/profiles/default"), &profile_link).unwrap();
    std::fs::write(
        clientetc.join("etc/portage/binrepos.conf"),
        format!(
            // Unsigned test binhost: opt out of the verifying default, so
            // the `.tbz2` entry is served.
            "[tmpbinhost]\nsync-uri = file://{}\npriority = 1\nverify-signature = false\n",
            binhost.display()
        ),
    )
    .unwrap();
    for entry in std::fs::read_dir(clientetc.join("etc/portage/repos.conf")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            let mut line = line.to_string();
            if line.trim_start().starts_with("location") && line.contains('=') {
                let value = line
                    .split('=')
                    .nth(1)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if !value.is_empty() && !value.starts_with('/') {
                    line = line.replace(
                        value.as_str(),
                        fixtures.join(&value).display().to_string().as_str(),
                    );
                }
            }
            lines.push(line);
        }
        lines.push(String::new());
        std::fs::write(entry.path(), lines.join("\n")).unwrap();
    }
    let output = std::process::Command::new(s8_portuale_bin())
        .args([
            "mrg",
            "--pretend",
            "--getbinpkgonly",
            "--remote-hostname",
            "localtest",
            "--remote-transport",
            "local",
            "--remote-root",
            root.to_str().unwrap(),
            "--remote-workdir",
            base.join("work").to_str().unwrap(),
            "--remote-etc-portage",
            &format!("server:{}", clientetc.display()),
            "--remote-portuale-dir",
            bindir.to_str().unwrap(),
            "dev-libs/binpkgrmpkg",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("PORTAGE_CONFIGROOT", &clientetc)
        .env("ROOT", &root)
        .env("PORTAGE_RUNNING_ROOT", &root)
        .env("DISTDIR", fixtures.join("distfiles"))
        .env("PORTAGE_TMPDIR", base.join("pt"))
        .env("FEATURES", "sandbox")
        .output()
        .expect("portuale mrg spawns");
    let stdout = s8_stdout(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries: Vec<String> = std::fs::read_dir(&bindir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries.is_empty(),
        "--pretend must install nothing: {entries:?}\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
