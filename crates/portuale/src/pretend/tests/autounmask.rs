use super::*;

/// Backlog #445: `--pretend --autounmask dev-libs/maskneedpkg` proposes the
/// unmask for the package.mask'd dependency instead of the plain masked
/// block. Expected value from Portage 3.0.82.2 (host staged-fixture probe,
/// `PORTAGE_CONFIGROOT`/`ROOT` at the staged fixtures,
/// `PYTHONHASHSEED=0`): rc 1, one `[ebuild N #]` row per package, the
/// `package.unmask` changes block with the `# required by` chain, and the
/// `--autounmask-keep-masks` NOTE. The merge-list header stays absent (the
/// standing autounmask-changes display shape, shared with the
/// `--autounmask-keep-masks=n` pin below -- not re-grounded here).
#[test]
fn autounmask_proposes_unmask_for_masked_dep() {
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("autounmask_masked_dep").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let output = std::process::Command::new(&portuale_bin)
        .args([
            "emerge",
            "--pretend",
            "--autounmask",
            "dev-libs/maskneedpkg",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .envs(env)
        .output()
        .expect("portuale emerge spawns");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "[ebuild  N    #] dev-libs/maskeddep-1.0 to {}\n\
             [ebuild  N     ] dev-libs/maskneedpkg-1.0 to {}\n",
            root.display(),
            root.display(),
        ),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "\nThe following mask changes are necessary to proceed:\n\
         \u{0020}(see \"package.unmask\" in the portage(5) man page for more details)\n\
         # required by dev-libs/maskneedpkg-1.0::testrepo\n\
         # required by dev-libs/maskneedpkg (argument)\n\
         =dev-libs/maskeddep-1.0\n\
         \n\
         NOTE: The --autounmask-keep-masks option will prevent emerge\n\
         \u{0020}\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}from creating package.unmask or ** keyword changes.\n",
    );
}

/// Review focus for #445: `--autounmask=n` and `--autounmask-keep-masks=y`
/// on a masked-only dep keep the plain masked block (no rows, no unmask
/// proposal). Same Portage 3.0.82.2 probe grounding as above.
#[test]
fn autounmask_n_and_keep_masks_y_keep_the_plain_masked_block() {
    let portuale_bin = built_portuale_bin();
    for flag in ["--autounmask=n", "--autounmask-keep-masks=y"] {
        let base = TempDir::new("autounmask_masked_dep_negative").keep();
        let root = base.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let env = fixture_resolve_env(&root, &base.join("pt"));
        let output = std::process::Command::new(&portuale_bin)
            .args(["emerge", "--pretend", flag, "dev-libs/maskneedpkg"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(env)
            .output()
            .expect("portuale emerge spawns");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{flag}\n{stdout}\n{stderr}");
        assert!(
            stdout.contains("These are the packages that would be merged, in order:"),
            "{flag}\n{stdout}"
        );
        assert!(
            !stdout.contains("[ebuild"),
            "{flag} must print no rows\n{stdout}"
        );
        assert!(stderr.contains("have been masked"), "{flag}\n{stderr}");
        assert!(
            !stderr.contains("mask changes are necessary"),
            "{flag} must propose no unmask\n{stderr}"
        );
    }
}
