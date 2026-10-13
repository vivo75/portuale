use super::*;

/// #127: the public splitter's advertised disambiguation, pinned
/// for the first time -- a digit-led word belongs to the name
/// (`foo-1bar-2.0` -> `foo-1bar`) and a `-r<digits>` word is not a
/// version on its own (`foo-r1-2.0` must not mis-split as
/// `("foo", "r1-2.0")`). Expected values are real `_pkgsplit`
/// (`pkgsplit('foo-1bar-2.0') == ('foo-1bar', '2.0', 'r0')`,
/// `pkgsplit('foo-r1-2.0') == ('foo-r1', '2.0', 'r0')`, system
/// Portage 2026-09-23).
#[test]
fn split_pf_keeps_a_digit_led_word_in_the_package_name() {
    assert_eq!(
        split_pf("foo-1bar-2.0"),
        Some(("foo-1bar".to_string(), "2.0".to_string()))
    );
}

#[test]
fn split_pf_does_not_mis_split_a_revision_led_name() {
    assert_eq!(
        split_pf("foo-r1-2.0"),
        Some(("foo-r1".to_string(), "2.0".to_string()))
    );
}

#[test]
fn list_candidates_reads_the_real_sub_slot_not_just_the_main_slot() {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let candidates = list_candidates(&repos, "dev-libs", "subslotpkg").expect("list_candidates");
    let c = candidates
        .iter()
        .find(|c| c.version == "1.0")
        .expect("subslotpkg-1.0 must be listed");
    assert_eq!(c.slot, "0");
    assert_eq!(c.sub_slot, "2");
}

/// An ebuild with an unsupported head EAPI and no usable cache entry is
/// still listed — as a masked candidate carrying the head EAPI — while a
/// supported EAPI without metadata stays invisible.
/// Portage: getmaskingstatus.py (`EAPI` reason), porttree.py
/// (`_pull_valid_cache` disregards the entry but the cp stays).
#[test]
fn list_candidates_keeps_unsupported_eapi_ebuilds_as_masked() {
    // No `metadata/md5-cache` dir at all (and no depend-phase provider in
    // this test binary), so `repo_aux_metadata` fails outright and the
    // head-EAPI fallback decides.
    let base = TempDir::new("portage-repo-348-list").keep();
    let repo = base.join("repo");
    let dir = repo.join("app-misc/eapi99pkg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("eapi99pkg-1.ebuild"), "EAPI=99\nSLOT=\"0\"\n").unwrap();
    let repos = vec![RepoConfig {
        name: "t348".to_string(),
        location: repo.clone(),
        priority: 50,
        is_main: false,
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    }];
    let candidates = list_candidates(&repos, "app-misc", "eapi99pkg").expect("list");
    let c = candidates
        .iter()
        .find(|c| c.version == "1")
        .expect("EAPI-99 ebuild stays listed");
    assert_eq!(c.eapi, "99");
    // Supported EAPI, same absence of metadata: invisible, as before.
    let dir = repo.join("app-misc/okpkg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("okpkg-1.ebuild"), "EAPI=8\nSLOT=\"0\"\n").unwrap();
    let candidates = list_candidates(&repos, "app-misc", "okpkg").expect("list");
    assert!(candidates.is_empty());
    let _ = std::fs::remove_dir_all(&base);
}
