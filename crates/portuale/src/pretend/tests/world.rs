use super::*;

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
