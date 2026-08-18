use super::*;

#[test]
fn repository_name_for_finds_the_nearest_ancestor_repo_name() {
    let tmp = tempdir();
    let repo = tmp.join("myrepo");
    let pkg_dir = repo.join("dev-libs/foo");
    std::fs::create_dir_all(repo.join("profiles")).unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(repo.join("profiles/repo_name"), "myrepo\n").unwrap();
    assert_eq!(repository_name_for(&pkg_dir), Some("myrepo".to_string()));
}

#[test]
fn repository_name_for_is_none_when_no_ancestor_has_one() {
    let tmp = tempdir();
    let pkg_dir = tmp.join("dev-libs/foo");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    assert_eq!(repository_name_for(&pkg_dir), None);
}
