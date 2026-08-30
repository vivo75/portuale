use super::*;

#[test]
fn restrict_mirror_from_restrict_evaluates_conditionals_against_the_empty_use_set() {
    assert!(restrict_mirror_from_restrict("mirror"));
    assert!(restrict_mirror_from_restrict("fetch mirror"));
    // The deprecated negative spelling counts too (Portage: fetch.py).
    assert!(restrict_mirror_from_restrict("nomirror"));
    assert!(!restrict_mirror_from_restrict(""));
    assert!(!restrict_mirror_from_restrict("fetch strip"));
    // USE-conditional: the fetch-side USE set is always empty here,
    // so `foo? ( mirror )` drops entirely -- not a literal token match
    assert!(!restrict_mirror_from_restrict("foo? ( mirror )"));
    assert!(restrict_mirror_from_restrict("mirror foo? ( strip )"));
}

#[test]
fn restrict_fetch_from_restrict_matches_the_fetch_token_only() {
    assert!(restrict_fetch_from_restrict("fetch"));
    assert!(restrict_fetch_from_restrict("mirror fetch"));
    assert!(!restrict_fetch_from_restrict(""));
    assert!(!restrict_fetch_from_restrict("mirror strip"));
    // USE-conditional drops against the always-empty fetch-side USE
    assert!(!restrict_fetch_from_restrict("foo? ( fetch )"));
}
