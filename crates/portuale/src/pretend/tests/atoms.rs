use super::*;

#[test]
fn bare_name_qualification() {
    let tree = [
        "app-portage/eix".to_string(),
        "app-misc/dup".to_string(),
        "dev-libs/dup".to_string(),
        "dev-libs/solo".to_string(),
        "virtual/vpref".to_string(),
        "dev-libs/vpref".to_string(),
    ];
    assert!(matches!(
        qualify_bare_name("eix", &tree),
        BareName::Qualified(cp) if cp == "app-portage/eix"
    ));
    assert!(matches!(
        qualify_bare_name("solo", &tree),
        BareName::Qualified(cp) if cp == "dev-libs/solo"
    ));
    // two non-virtual matches -> ambiguous, sorted.
    assert!(matches!(
        qualify_bare_name("dup", &tree),
        BareName::Ambiguous(m) if m == ["app-misc/dup", "dev-libs/dup"]
    ));
    // one non-virtual + one virtual -> the non-virtual wins silently.
    assert!(matches!(
        qualify_bare_name("vpref", &tree),
        BareName::Qualified(cp) if cp == "dev-libs/vpref"
    ));
    assert!(matches!(
        qualify_bare_name("nope", &tree),
        BareName::NoMatch
    ));
}

#[test]
fn dep_expand_token_covers_versioned_and_slotted_bare_names() {
    let tree = [
        "app-portage/eix".to_string(),
        "app-misc/dup".to_string(),
        "dev-libs/dup".to_string(),
    ];
    // already qualified / not a package token -> untouched
    assert!(matches!(
        dep_expand_token("app-portage/eix", &tree),
        DepExpand::Unchanged
    ));
    assert!(matches!(
        dep_expand_token(">=app-portage/eix-1.2", &tree),
        DepExpand::Unchanged
    ));
    assert!(matches!(
        dep_expand_token("@world", &tree),
        DepExpand::Unchanged
    ));
    // pure bare
    assert!(matches!(
        dep_expand_token("eix", &tree),
        DepExpand::Qualified(q) if q == "app-portage/eix"
    ));
    // bare + version: real inserts the missing "=" (dep_expand's
    // backward-compat path)
    assert!(matches!(
        dep_expand_token("eix-1.2", &tree),
        DepExpand::Qualified(q) if q == "=app-portage/eix-1.2"
    ));
    // bare + operator + version
    assert!(matches!(
        dep_expand_token(">=eix-1.2", &tree),
        DepExpand::Qualified(q) if q == ">=app-portage/eix-1.2"
    ));
    // bare + slot
    assert!(matches!(
        dep_expand_token("eix:0", &tree),
        DepExpand::Qualified(q) if q == "app-portage/eix:0"
    ));
    // still ambiguous / no-match after stripping the version
    assert!(matches!(
        dep_expand_token("dup-2.0", &tree),
        DepExpand::Ambiguous(m) if m == ["app-misc/dup", "dev-libs/dup"]
    ));
    assert!(matches!(
        dep_expand_token("nope-1.0", &tree),
        DepExpand::NoMatch
    ));
}
