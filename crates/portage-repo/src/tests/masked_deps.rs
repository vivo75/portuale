use super::*;

#[test]
fn atom_all_use_unmasked_is_true_for_the_unsatuseor_alternative() {
    // Portage's `all_use_unmasked` probe (bug 515584): an off-by-default flag
    // keeps the choice selectable, not demoted to `other`.
    let root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    assert!(atom_all_use_unmasked(
        &repos,
        "dev-libs/unsatusealt[unsatuseorflag]",
        &config,
        &[]
    ));
}
