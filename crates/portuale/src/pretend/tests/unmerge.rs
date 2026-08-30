use super::*;

#[test]
fn still_listed_parents_applies_the_higher_slot_refinement() {
    // A higher different slot still matching the set atom suppresses the parent warning.
    let root = fixtures_root();
    let sets = vec![(
        "dualslotset".to_string(),
        vec!["dev-libs/dualslotpkg".to_string()],
    )];

    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());

    assert_eq!(
        still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "2.0"),
        vec!["dualslotset"]
    );
}

#[test]
fn still_listed_parents_is_empty_when_no_set_lists_the_cp() {
    let root = fixtures_root();
    let sets = vec![("someset".to_string(), vec!["dev-libs/other".to_string()])];
    assert!(still_listed_parents(&root, &sets, "dev-libs", "dualslotpkg", "1.0").is_empty());
}
