use super::*;

#[test]
fn eclass_locations_value_quotes_the_containing_repo_root() {
    // Canonicalized first, as `compute_environment` always hands it.
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/repo")
        .canonicalize()
        .unwrap();
    let pkg_dir = repo_root.join("dev-libs/eclasspkg");
    let value = eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root"));
    // Portage: bin/ebuild.sh, the PORTAGE_ECLASS_LOCATIONS eval.
    assert_eq!(value, format!("'{}'", repo_root.display()));
}

#[test]
fn eclass_locations_value_is_empty_outside_any_repo_checkout() {
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_value_none").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("dev-libs/standalone");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(
        eclass_locations_value(&pkg_dir, Path::new("/dev/null/no-config-root")),
        ""
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn eclass_locations_value_puts_the_own_repo_first_then_masters_in_declared_order() {
    // Portage: config.py, eclass_locations (own repo first, then masters).
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_masters_order").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let main = tmp.join("main");
    let secondary = tmp.join("secondary");
    let overlay = tmp.join("overlay");
    for repo in [&main, &secondary, &overlay] {
        std::fs::create_dir_all(repo.join("profiles")).unwrap();
    }
    std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
    std::fs::write(secondary.join("profiles/repo_name"), "secondary\n").unwrap();
    std::fs::write(overlay.join("profiles/repo_name"), "overlay\n").unwrap();
    std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
    std::fs::write(
        tmp.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n\
             [main]\nlocation = {}\n\n\
             [secondary]\nlocation = {}\n\n\
             [overlay]\nlocation = {}\nmasters = main secondary\n",
            main.display(),
            secondary.display(),
            overlay.display(),
        ),
    )
    .unwrap();

    let pkg_dir = overlay.join("dev-libs/overlaypkg");
    let value = eclass_locations_value(&pkg_dir, &tmp);
    assert_eq!(
        value,
        format!(
            "'{}' '{}' '{}'",
            overlay.display(),
            secondary.display(),
            main.display()
        ),
        "own repo first, then masters in real declared order"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn eclass_locations_value_does_not_duplicate_the_own_repo_when_it_is_also_a_master() {
    // The own repo is not duplicated when it is also a master.
    let tmp = TempDir::new("ebuild-phases-test-eclass_locations_no_dup").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let main = tmp.join("main");
    std::fs::create_dir_all(main.join("profiles")).unwrap();
    std::fs::write(main.join("profiles/repo_name"), "main\n").unwrap();
    std::fs::create_dir_all(tmp.join("etc/portage")).unwrap();
    std::fs::write(
        tmp.join("etc/portage/repos.conf"),
        format!(
            "[DEFAULT]\nmain-repo = main\n\n[main]\nlocation = {}\n",
            main.display()
        ),
    )
    .unwrap();

    let pkg_dir = main.join("dev-libs/mainpkg");
    let value = eclass_locations_value(&pkg_dir, &tmp);
    assert_eq!(value, format!("'{}'", main.display()));

    let _ = std::fs::remove_dir_all(&tmp);
}
