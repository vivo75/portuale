use super::*;

#[test]
fn world_file_additions_reports_only_new_plain_targets() {
    // New atoms in argument order; already-present, `@set`, and unresolved targets add nothing.
    let entries = vec![
        world_entry("dev-libs", "wanted", "0"),
        world_entry("dev-libs", "existing", "0"),
    ];
    assert_eq!(
        world_file_additions(
            &["dev-libs/existing".to_string()],
            &[
                "dev-libs/wanted",
                "dev-libs/existing",
                "@someset",
                "dev-libs/ghost"
            ],
            &entries,
            &[],
            &[],
        ),
        vec!["dev-libs/wanted".to_string()],
    );
    assert!(
        world_file_additions(
            &[
                "dev-libs/existing".to_string(),
                "dev-libs/wanted".to_string()
            ],
            &["dev-libs/wanted"],
            &entries,
            &[],
            &[],
        )
        .is_empty()
    );
}

#[test]
fn world_sets_additions_reports_only_new_sets() {
    // New `@`-prefixed lines in argument order.
    assert_eq!(
        world_sets_additions(
            &["@kept".to_string()],
            &["kept".to_string(), "fresh".to_string()],
        ),
        vec!["@fresh".to_string()],
    );
    assert!(world_sets_additions(&["@kept".to_string()], &["kept".to_string()]).is_empty());
}

#[test]
fn custom_set_drops_invalid_lines_with_portage_line_numbers() {
    // Portage `ItemFileLoader` (`portage/env/loaders.py:203-221`): every
    // non-`@` set line is validated with the set-file `ValidAtomValidator`
    // and a bad line drops with `<path>: Validation failed at line: N,
    // data X`. The eight lines are S0's sets/world set (oracle run5);
    // `isvalidatom(line, allow_repo=True)` live on this host (Portage
    // 3.0.82.2, 2026-10-12) keeps only lines 3 and 8.
    let dir = TempDir::new("portuale-s8-sets").keep();
    let sets = dir.join("etc/portage/sets");
    std::fs::create_dir_all(&sets).expect("mkdir");
    std::fs::write(
        sets.join("s341"),
        "*/t341-dep\ndev-libs/t341-*\ndev-libs/t341-dep::t341\n=dev-libs/t341-dep-1-2\n=dev-libs/t341-dep-1-02\ndev-libs/t341-dep-1\n!dev-libs/t341-dep\ndev-libs/t341-dep[x]\n",
    )
    .expect("write");
    let mut seen = std::collections::HashSet::new();
    let out = resolve_custom_set(&dir, "s341", &mut seen).expect("set reads");
    assert_eq!(
        out.atoms,
        vec![
            "dev-libs/t341-dep::t341".to_string(),
            "dev-libs/t341-dep[x]".to_string()
        ]
    );
    let p = sets.join("s341").display().to_string();
    assert_eq!(
        out.errors,
        vec![
            format!("{p}: Validation failed at line: 1, data */t341-dep"),
            format!("{p}: Validation failed at line: 2, data dev-libs/t341-*"),
            format!("{p}: Validation failed at line: 4, data =dev-libs/t341-dep-1-2"),
            format!("{p}: Validation failed at line: 5, data =dev-libs/t341-dep-1-02"),
            format!("{p}: Validation failed at line: 6, data dev-libs/t341-dep-1"),
            format!("{p}: Validation failed at line: 7, data !dev-libs/t341-dep"),
        ]
    );
}

#[test]
fn usersets_world_candidate_honors_only_an_explicit_usersets_override() {
    // Portage: _sets/__init__.py. File sets are candidates unless `sets.conf` says otherwise.
    // Option names are case-insensitive, section names are not; later lines win.
    fn cfg(tag: &str) -> std::path::PathBuf {
        let dir = TempDir::new(&format!("portuale-usersets-{tag}")).keep();
        std::fs::create_dir_all(dir.join("etc/portage")).expect("mkdir");
        dir
    }
    assert!(usersets_world_candidate(&cfg("absent")));
    let dir = cfg("false");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[usersets]\nWORLD-CANDIDATE = false\n",
    )
    .expect("write");
    assert!(!usersets_world_candidate(&dir));
    let dir = cfg("wrong-section");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[UserSets]\nworld-candidate = false\n",
    )
    .expect("write");
    assert!(usersets_world_candidate(&dir));
    let dir = cfg("spellings");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[usersets]\nworld-candidate = no\nworld-candidate = 0\n",
    )
    .expect("write");
    assert!(!usersets_world_candidate(&dir));
    let dir = cfg("revert");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[usersets]\nworld-candidate = false\nworld-candidate = True\n",
    )
    .expect("write");
    assert!(usersets_world_candidate(&dir));
    // An unparsable value is ignored, never a suppression.
    let dir = cfg("garbage");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[usersets]\nworld-candidate = maybe\n",
    )
    .expect("write");
    assert!(usersets_world_candidate(&dir));
    let dir = cfg("dir");
    let dotd = dir.join("etc/portage/sets.conf");
    std::fs::create_dir_all(&dotd).expect("mkdir");
    std::fs::write(
        dotd.join("10-first.conf"),
        "[usersets]\nworld-candidate = false\n",
    )
    .expect("write");
    std::fs::write(
        dotd.join("20-second.conf"),
        "[usersets]\nworld-candidate = on\n",
    )
    .expect("write");
    assert!(usersets_world_candidate(&dir));
    let dir = cfg("other");
    std::fs::write(
        dir.join("etc/portage/sets.conf"),
        "[noworldset]\nworld-candidate = false\n",
    )
    .expect("write");
    assert!(usersets_world_candidate(&dir));
}

#[test]
fn update_world_file_records_a_new_target_and_skips_deps_and_oneshot() {
    let tmp = TempDir::new("portuale-world").keep();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/lib/portage")).unwrap();
    std::fs::write(root.join("var/lib/portage/world"), "dev-libs/existing\n").unwrap();

    let entries = vec![
        world_entry("dev-libs", "wanted", "0"),
        world_entry("dev-libs", "adep", "0"),
    ];
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/wanted"],
        entries: &entries,
        system_atoms: &[],
        repos: &[],
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        "dev-libs/existing\ndev-libs/wanted\n"
    );

    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/wanted"],
        entries: &entries,
        system_atoms: &[],
        repos: &[],
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/wanted2"],
        entries: &entries,
        system_atoms: &[],
        repos: &[],
        oneshot: true,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        "dev-libs/existing\ndev-libs/wanted\n"
    );

    // Unslotted `@system` members are never recorded.
    std::fs::write(root.join("var/lib/portage/world"), "").unwrap();
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/wanted"],
        entries: &entries,
        system_atoms: &["dev-libs/wanted".to_string()],
        repos: &[],
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        ""
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn update_world_file_slot_qualifies_a_slotted_target() {
    // A cp is "slotted" with several SLOTs, or one non-`"0"` SLOT.
    let repos = portage_repo::find_repos(&fixtures_root()).expect("repos");
    let tmp = TempDir::new("portuale-worldslot").keep();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/lib/portage")).unwrap();

    let entries = vec![world_entry("dev-libs", "dualslotpkg", "1")];
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/dualslotpkg:1"],
        entries: &entries,
        system_atoms: &[],
        repos: &repos,
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        "dev-libs/dualslotpkg:1\n"
    );

    std::fs::write(root.join("var/lib/portage/world"), "").unwrap();
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/dualslotpkg"],
        entries: &entries,
        system_atoms: &[],
        repos: &repos,
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        "dev-libs/dualslotpkg\n"
    );

    std::fs::write(root.join("var/lib/portage/world"), "").unwrap();
    let pkg_entries = vec![world_entry("dev-libs", "packagepkg", "0")];
    update_world_file(UpdateWorldFileArgs {
        root: &root,
        target_atoms: &["dev-libs/packagepkg:0"],
        entries: &pkg_entries,
        system_atoms: &[],
        repos: &repos,
        oneshot: false,
        onlydeps: false,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world")).unwrap(),
        "dev-libs/packagepkg\n"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn update_world_sets_file_records_a_new_set_and_skips_oneshot() {
    let tmp = TempDir::new("portuale-worldsets").keep();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/lib/portage")).unwrap();
    std::fs::write(
        root.join("var/lib/portage/world_sets"),
        "# a comment\n@existing\n",
    )
    .unwrap();

    // Rewritten sorted; comments and non-`@` lines are dropped.
    update_world_sets_file(&root, &["fresh".to_string()], false, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world_sets")).unwrap(),
        "@existing\n@fresh\n"
    );

    update_world_sets_file(&root, &["fresh".to_string()], false, false).unwrap();
    update_world_sets_file(&root, &["another".to_string()], true, false).unwrap();
    update_world_sets_file(&root, &["another".to_string()], false, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world_sets")).unwrap(),
        "@existing\n@fresh\n"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn installed_set_atoms_are_slot_qualified_sorted_and_deduped() {
    // Always slot-qualified, main slot only; sorted and deduplicated.
    let tmp = TempDir::new("portuale-instset").keep();
    let mk = |cat: &str, dir: &str, slot: &str| {
        let d = tmp.join("var/db/pkg").join(cat).join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), format!("{cat}\n")).unwrap();
        std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    };
    mk("dev-libs", "zpkg-2.0", "0");
    mk("dev-libs", "apkg-1.0", "3/7");
    mk("app-misc", "mid-1.0", "0");

    assert_eq!(
        installed_set_atoms(&tmp),
        vec![
            "app-misc/mid:0".to_string(),
            "dev-libs/apkg:3".to_string(),
            "dev-libs/zpkg:0".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
