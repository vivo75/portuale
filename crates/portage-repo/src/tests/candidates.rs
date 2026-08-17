use super::*;

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
