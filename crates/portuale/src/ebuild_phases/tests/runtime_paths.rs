use super::*;

#[test]
fn repo_root_for_finds_the_nearest_ancestor_repo_root() {
    let tmp = TempDir::new("ebuild-phases-test-repo_root_for_finds").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let repo = tmp.join("myrepo");
    let pkg_dir = repo.join("dev-libs/foo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
    assert_eq!(repo_root_for(&pkg_dir), Some(repo));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn repo_root_for_is_none_when_no_ancestor_has_one() {
    let tmp = TempDir::new("ebuild-phases-test-repo_root_for_none").keep();
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg_dir = tmp.join("dev-libs/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(repo_root_for(&pkg_dir), None);
    let _ = std::fs::remove_dir_all(&tmp);
}
