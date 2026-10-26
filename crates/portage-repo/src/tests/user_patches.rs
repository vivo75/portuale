//! Unit pins for the `--binpkg-respect-user-patches` filter, the
//! `user patches` mask reason, the ignored-binaries block renderer and
//! the `_eliminate_ignored_binaries` port.

use super::*;

fn patch_config(tag: &str, content: &[u8]) -> portage_profile::Config {
    let dir = TempDir::new(&format!("portage-repo-test-user-patches-{tag}")).keep();
    let patch_dir = dir.join("etc/portage/patches/app-misc/up");
    std::fs::create_dir_all(&patch_dir).expect("patch dir");
    std::fs::write(patch_dir.join("user.patch"), content).expect("patch file");
    let mut config = test_config();
    config.user_patches = portage_profile::UserPatchSet::load(&dir);
    assert!(config.user_patches.is_some(), "patch sets load");
    config
}

fn binary_with_digest(version: &str, digest: &str) -> Candidate {
    let mut c = candidate(version, &["amd64"]);
    c.source = CandidateSource::Binary;
    c.binary_use = Some(HashSet::new());
    c.repo_name = "testrepo".to_string();
    c.user_patches = digest.to_string();
    c
}

#[test]
fn filter_accepts_a_matching_digest_and_rejects_a_differing_one() {
    let content = b"--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+a\n";
    let config = patch_config("filter", content);
    // The module's own digest is the comparison half.
    let set = config.user_patches.clone().expect("sets");
    let matching = set.digest("app-misc", "up", "1.0", "0");
    assert!(!matching.is_empty());
    assert!(binary_user_patches_ok(
        &binary_with_digest("1.0", &matching),
        "app-misc",
        "up",
        &config
    ));
    assert!(!binary_user_patches_ok(
        &binary_with_digest("1.0", &"0".repeat(64)),
        "app-misc",
        "up",
        &config
    ));
    // No patches directory at all: `""` matches `""` (bug 984255).
    let bare = test_config();
    assert!(bare.user_patches.is_none());
    assert!(binary_user_patches_ok(
        &binary_with_digest("1.0", ""),
        "app-misc",
        "up",
        &bare
    ));
    assert!(!binary_user_patches_ok(
        &binary_with_digest("1.0", &matching),
        "app-misc",
        "up",
        &bare
    ));
}

#[test]
fn mask_reason_is_user_patches_after_changed_deps() {
    let content = b"--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+a\n";
    let config = patch_config("mask", content);
    let set = config.user_patches.clone().expect("sets");
    let matching = set.digest("app-misc", "up", "1.0", "0");
    let mut mismatched = binary_with_digest("1.0", &"0".repeat(64));
    // Visible (keywords accepted, license default): the gate decides.
    let gate = || BinaryRespectUseGate {
        respect_use: false,
        newuse: false,
        changed_use: false,
        user_patches: true,
        ebuilds: &[],
        changed_deps: None,
    };
    let rows = binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
        binaries: std::slice::from_ref(&mismatched),
        atom_str: "app-misc/up",
        config: &config,
        gate: gate(),
    })
    .expect("a dropped binary reports");
    assert_eq!(
        rows,
        vec![(
            "app-misc/up-1.0::testrepo".to_string(),
            vec!["user patches".to_string()]
        )]
    );
    // Gate off: selectable, no rows.
    let off = BinaryRespectUseGate {
        user_patches: false,
        ..gate()
    };
    assert!(
        binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
            binaries: std::slice::from_ref(&mismatched),
            atom_str: "app-misc/up",
            config: &config,
            gate: off,
        })
        .is_none()
    );
    // A matching digest is selectable even with the gate on.
    mismatched.user_patches = matching;
    assert!(
        binary_masked_rows_for_atom(BinaryMaskedRowsArgs {
            binaries: std::slice::from_ref(&mismatched),
            atom_str: "app-misc/up",
            config: &config,
            gate: gate(),
        })
        .is_none()
    );
}

#[test]
fn ignored_block_renders_exactly_like_portage() {
    // The leading blank line belongs to the block, one blank line
    // separates rows from NOTE, every line ends in `\n` like Portage's
    // `writemsg`.
    let rows = vec![IgnoredUserPatchBinary {
        category: "app-misc".to_string(),
        package: "up-b".to_string(),
        version: "1.0".to_string(),
        slot: "0".to_string(),
        build_id: Some("1".to_string()),
        files: vec!["app-misc/up-b/user.patch".to_string()],
    }];
    let root = Path::new("/var/tmp/ev509-tree/root");
    let expected = concat!(
        "\n",
        "!!! The following binary packages have been ignored due to user patches:\n",
        "\n",
        "     app-misc/up-b-1.0-1 for /var/tmp/ev509-tree/root/\n",
        "\n",
        "NOTE: The --binpkg-respect-user-patches=n option will prevent\n",
        "      emerge from ignoring these binary packages.\n",
        "      Using --binpkg-respect-user-patches=y will silence this\n",
        "      warning.\n",
    );
    assert_eq!(
        render_ignored_user_patch_binaries(&rows, root, false).expect("block"),
        expected
    );
    let verbose = render_ignored_user_patch_binaries(&rows, root, true).expect("block");
    assert!(verbose.contains(
        "\n!!! These user patches are triggering this warning:\n\n    app-misc/up-b/user.patch\n"
    ));
    // `ROOT=/`: no suffix; no rows: no block.
    let bare = render_ignored_user_patch_binaries(&rows, Path::new("/"), false).expect("block");
    assert!(bare.contains("     app-misc/up-b-1.0-1\n"));
    assert!(!bare.contains(" for /"));
    assert!(render_ignored_user_patch_binaries(&[], root, false).is_none());
    assert!(render_ignored_user_patch_binaries(&[], root, true).is_none());
}

#[test]
fn elimination_keeps_a_same_version_ebuild_row_but_drops_newer_selections() {
    // Portage `_eliminate_ignored_binaries`: a higher
    // version selected, or an equal-version binary selected, clears the row;
    // a same-version ebuild selected keeps it (oracle shape 02 shows it).
    let row = IgnoredUserPatchBinary {
        category: "app-misc".to_string(),
        package: "up".to_string(),
        version: "1.0".to_string(),
        slot: "0".to_string(),
        build_id: Some("1".to_string()),
        files: Vec::new(),
    };
    let ebuild_1 = SelectedPackage {
        version: "1.0",
        source: CandidateSource::Ebuild,
        slot: "0",
    };
    assert_eq!(
        eliminate_ignored(vec![row.clone()], std::slice::from_ref(&ebuild_1)).len(),
        1
    );
    let ebuild_2 = SelectedPackage {
        version: "2.0",
        source: CandidateSource::Ebuild,
        slot: "0",
    };
    assert!(eliminate_ignored(vec![row.clone()], std::slice::from_ref(&ebuild_2)).is_empty());
    let binary_1 = SelectedPackage {
        version: "1.0",
        source: CandidateSource::Binary,
        slot: "0",
    };
    assert!(eliminate_ignored(vec![row.clone()], std::slice::from_ref(&binary_1)).is_empty());
}
