use super::*;

/// Backlog #445: `--pretend --autounmask dev-libs/maskneedpkg` proposes the
/// unmask for the package.mask'd dependency instead of the plain masked
/// block. Expected value from Portage 3.0.82.2 (host staged-fixture probe,
/// `PORTAGE_CONFIGROOT`/`ROOT` at the staged fixtures,
/// `PYTHONHASHSEED=0`): rc 1, one `[ebuild N #]` row per package, the
/// `package.unmask` changes block with the `# required by` chain, and the
/// `--autounmask-keep-masks` NOTE. Backlog #462: the header
/// (`These are the packages…` / `Calculating…` / `Dependency resolution
/// took…`, wall-clock seconds cut per the determinism convention) prints
/// before the rows, and the unmask block carries Portage's
/// `getmaskingreason` lines (`# <file>:` + the file's comment block).
/// Oracle: `LLM/evidence/2026-10-21-f462/portage.txt` cell c1.
#[test]
fn autounmask_proposes_unmask_for_masked_dep() {
    let portuale_bin = built_portuale_bin();
    let base = TempDir::new("autounmask_masked_dep").keep();
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    let env = fixture_resolve_env(&root, &base.join("pt"));
    let run_once = || {
        std::process::Command::new(&portuale_bin)
            .args([
                "emerge",
                "--pretend",
                "--autounmask",
                "dev-libs/maskneedpkg",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(env.clone())
            .output()
            .expect("portuale emerge spawns")
    };
    // A warm-up run establishes the updates record, so the pinned run
    // below skips the block and the ambient repo set behind it.
    run_once();
    let output = run_once();
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
            "\n\
             These are the packages that would be merged, in order:\n\
             \n\
             Calculating dependencies ... done!\n\
             Dependency resolution took (backtrack: 0/20).\n\
             \n\
             [ebuild  N    #] dev-libs/maskeddep-1.0 to {}\n\
             [ebuild  N     ] dev-libs/maskneedpkg-1.0 to {}\n",
            root.display(),
            root.display(),
        ),
    );
    let mask_file = fixtures_root().join("etc/portage/package.mask");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "\nThe following mask changes are necessary to proceed:\n\
             \u{0020}(see \"package.unmask\" in the portage(5) man page for more details)\n\
             # required by dev-libs/maskneedpkg-1.0::testrepo\n\
             # required by dev-libs/maskneedpkg (argument)\n\
             # {mask}:\n\
             # fixture package.mask: two atoms stay masked, one is added then removed\n\
             # again via \"-atom\" to exercise removal within this same source, and a\n\
             # fourth \"-atom\" line removes an entry the repo-level\n\
             # repo/profiles/package.mask added, not this file -- proving -atom\n\
             # removal now applies across all three real sources (repo, profile\n\
             # chain, user), not just within the user-level file alone.\n\
             # fixture package.mask for the masked-dependency disclosure slice:\n\
             # dev-libs/maskeddep stays masked so a dependency on it reports the\n\
             # \"All ebuilds ... have been masked\" block (never unmasked anywhere).\n\
             =dev-libs/maskeddep-1.0\n\
             \n\
             NOTE: The --autounmask-keep-masks option will prevent emerge\n\
             \u{0020}\u{0020}\u{0020}\u{0020}\u{0020}\u{0020}from creating package.unmask or ** keyword changes.\n",
            mask = mask_file.display(),
        ),
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
