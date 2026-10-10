use super::*;

#[test]
fn world_file_additions_reports_only_new_plain_targets() {
    // Backlog #232: the deferral arm's pre-record computation --
    // new atoms in argument order; already-present atoms, `@set`
    // targets (the `world_sets` half), and args with no resolved
    // entry contribute nothing.
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
    // Nothing new -> empty (the `--ask` prompt stays silent too).
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
    // Backlog #232: the `@set` half of the deferral arm's
    // pre-record computation -- `@`-prefixed lines for names not
    // already recorded, in argument order.
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
fn usersets_world_candidate_honors_only_an_explicit_usersets_override() {
    // Backlog #232 review (Important-2): real
    // `lib/portage/_sets/__init__.py:133` + `:258-262` -- file sets
    // are candidates unless `sets.conf` says otherwise.
    fn cfg(tag: &str) -> std::path::PathBuf {
        let dir = TempDir::new(&format!("portuale-usersets-{tag}")).keep();
        std::fs::create_dir_all(dir.join("etc/portage")).expect("mkdir");
        dir
    }
    // No `sets.conf` at all: the shipped default (candidate).
    assert!(usersets_world_candidate(&cfg("absent")));
    // An explicit `[usersets] world-candidate = false` flips it;
    // option names are case-insensitive, section names are not.
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
    // Every `getboolean` false spelling counts; `true` spellings
    // flip back (later lines win, like real's merged sections).
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
    // The `sets.conf/` directory form: sorted walk, later wins.
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
    // Unrelated sections never matter.
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
    // `adep` isn't a requested atom -> not recorded; `wanted` is.
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

    // A second run is idempotent, and `--oneshot` writes nothing.
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

    // An unslotted `@system` member is never recorded.
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
    // dev-libs/dualslotpkg has SLOT=1 and SLOT=2 in the repo (neither
    // "0") -> "slotted"; dev-libs/packagepkg is SLOT="0" only.
    let repos = portage_repo::find_repos(&fixtures_root()).expect("repos");
    let tmp = TempDir::new("portuale-worldslot").keep();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("var/lib/portage")).unwrap();

    // Slotted cp + `:1` arg -> `dev-libs/dualslotpkg:1`.
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

    // Bare arg for the same slotted cp -> plain `cat/pkg`.
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

    // Unslotted cp (SLOT="0" only) + `:0` arg -> still plain `cat/pkg`.
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

    // A new set is appended; the comment line is dropped on rewrite,
    // `@existing` is kept, output sorted.
    update_world_sets_file(&root, &["fresh".to_string()], false, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("var/lib/portage/world_sets")).unwrap(),
        "@existing\n@fresh\n"
    );

    // Idempotent, and `--oneshot` / `--onlydeps` write nothing.
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
    let tmp = TempDir::new("portuale-instset").keep();
    let mk = |cat: &str, dir: &str, slot: &str| {
        let d = tmp.join("var/db/pkg").join(cat).join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("CATEGORY"), format!("{cat}\n")).unwrap();
        std::fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    };
    // A lone-slot package is still slot-qualified (bug #338959); a
    // sub-slot is dropped (only the main slot); output sorted.
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
