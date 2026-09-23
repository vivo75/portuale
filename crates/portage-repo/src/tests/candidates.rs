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
