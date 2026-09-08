use super::*;

#[test]
fn shadow_parsers_read_contents_and_filemeta_shapes() {
    // Only obj/sym lines own paths; dir lines merge freely.
    assert_eq!(
        contents_owned_paths("obj /a/b 0123 456\ndir /a\nsym /c -> /d 789\njunk\n"),
        vec!["/a/b".to_string(), "/c".to_string()]
    );
    // filemeta keeps kind + rel; garbage lines drop out.
    assert_eq!(
        parse_filemeta_paths(
            "obj abc 123 usr/bin/x\nsym def 456 etc/link tgt\ndir ghi 789 usr/share\nbogus\n"
        ),
        vec![
            ("obj".to_string(), "usr/bin/x".to_string()),
            ("sym".to_string(), "etc/link".to_string()),
            ("dir".to_string(), "usr/share".to_string()),
        ]
    );
    // `_pkgsplit` longest-version rule for the same-package check.
    assert!(VdbShadow::same_package("foo-1bar-2.0", "foo-1bar"));
    assert!(!VdbShadow::same_package("foo-2.0", "foo-1bar"));
    assert!(!VdbShadow::same_package("noversion", "noversion"));
}

#[test]
fn shadow_precheck_fails_only_foreign_owners() {
    let dir = TempDir::new("portuale-remote-shadow").keep();
    let vdb = dir.join("vdb");
    std::fs::create_dir_all(vdb.join("dev-libs/oldpkg-1.0")).unwrap();
    std::fs::write(
        vdb.join("dev-libs/oldpkg-1.0/CONTENTS"),
        "obj /usr/bin/foreign abc 1\nsym /usr/bin/shared -> /x 2\n",
    )
    .unwrap();
    std::fs::create_dir_all(vdb.join("dev-libs/newpkg-2.0")).unwrap();
    std::fs::write(
        vdb.join("dev-libs/newpkg-2.0/CONTENTS"),
        "obj /usr/bin/own def 3\n",
    )
    .unwrap();
    let shadow = VdbShadow::load(&vdb);
    // Same package, any version: passes for the driver to refine.
    assert!(
        shadow_precheck(
            &shadow,
            "dev-libs/newpkg-1.0",
            "dev-libs",
            "newpkg",
            "obj m 1 usr/bin/own\n",
        )
        .is_ok()
    );
    // Foreign owner: fails before shipping.
    let err = shadow_precheck(
        &shadow,
        "dev-libs/newpkg-1.0",
        "dev-libs",
        "newpkg",
        "obj m 1 usr/bin/foreign\n",
    )
    .unwrap_err();
    assert!(err.contains("not shipping"), "{err}");
    assert!(err.contains("oldpkg-1.0"), "{err}");
    // Unowned path: passes.
    assert!(
        shadow_precheck(
            &shadow,
            "dev-libs/newpkg-1.0",
            "dev-libs",
            "newpkg",
            "obj m 1 usr/bin/fresh\ndir m 1 usr/share\n",
        )
        .is_ok()
    );
    // Missing vdb dir: empty shadow, everything passes.
    let empty = VdbShadow::load(&dir.join("absent"));
    assert!(
        shadow_precheck(
            &empty,
            "dev-libs/newpkg-1.0",
            "dev-libs",
            "newpkg",
            "obj m 1 usr/bin/foreign\n",
        )
        .is_ok()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn binary_plan_check_accepts_binaries_and_names_sources() {
    use portage_repo::PretendOutcome;
    assert!(check_binary_plan(&[binary_entry("a", true)]).is_ok());
    assert!(check_binary_plan(&[]).is_ok());
    let mut installed = binary_entry("b", true);
    installed.outcome = PretendOutcome::AlreadyInstalled {
        version: "1.0".to_string(),
    };
    installed.source = portage_repo::CandidateSource::Ebuild;
    assert!(check_binary_plan(&[installed]).is_ok());
    let err = check_binary_plan(&[binary_entry("c", false)]).unwrap_err();
    assert!(err.contains("dev-libs/c-1.0"), "{err}");
}

/// Backlog #183: the vdb shadow skips in-progress `-MERGING-<pf>`
/// entries, so a stale half-written `CONTENTS` never blocks a bundle
/// -- real `vardbapi._excluded_dirs` (`vartree.py`).
#[test]
fn shadow_ignores_a_stale_merging_entry() {
    let dir = TempDir::new("portuale-remote-shadow-merging").keep();
    let vdb = dir.join("vdb");
    std::fs::create_dir_all(vdb.join("dev-libs/-MERGING-stalepkg-1.0")).unwrap();
    std::fs::write(
        vdb.join("dev-libs/-MERGING-stalepkg-1.0/CONTENTS"),
        "obj /usr/bin/stale abc 1\n",
    )
    .unwrap();
    let shadow = VdbShadow::load(&vdb);
    assert!(shadow.owner("/usr/bin/stale").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
