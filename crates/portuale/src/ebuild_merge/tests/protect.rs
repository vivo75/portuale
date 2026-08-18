use super::*;

#[test]
fn is_protected_matches_only_under_a_real_protected_directory() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("etc")).unwrap();

    assert!(is_protected(&root, "/etc", "", &root.join("etc/foo.conf")));
    assert!(is_protected(&root, "/etc", "", &root.join("etc")));
    // Real bug #379899-adjacent case: "/etc" must not match
    // "/etcfoobaz" just because it's a string prefix.
    assert!(!is_protected(&root, "/etc", "", &root.join("etcfoobaz")));
    assert!(!is_protected(&root, "/etc", "", &root.join("var/foo")));
}

#[test]
fn is_protected_a_literal_file_entry_matches_only_exactly() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("etc")).unwrap();
    std::fs::write(root.join("etc/single.conf"), b"x").unwrap();

    assert!(is_protected(
        &root,
        "/etc/single.conf",
        "",
        &root.join("etc/single.conf")
    ));
    assert!(!is_protected(
        &root,
        "/etc/single.conf",
        "",
        &root.join("etc/other.conf")
    ));
}

#[test]
fn is_protected_respects_mask_exclusion_via_longest_prefix() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("etc/env.d")).unwrap();

    assert!(is_protected(
        &root,
        "/etc",
        "/etc/env.d",
        &root.join("etc/foo.conf")
    ));
    // Masked: /etc/env.d is a longer, more specific match than /etc.
    assert!(!is_protected(
        &root,
        "/etc",
        "/etc/env.d",
        &root.join("etc/env.d/10-foo")
    ));
}

#[test]
fn new_protect_filename_allocates_sequential_numbers() {
    let tmp = tempdir();
    std::fs::create_dir_all(&tmp).unwrap();
    let dest = tmp.join("foo.conf");
    // A newmd5 that never matches any of these files' own content --
    // isolates this test from the reuse behavior covered by
    // new_protect_filename_reuses_last_file_with_matching_content
    // below.
    let no_match = "no-such-md5";

    assert_eq!(
        new_protect_filename(&dest, no_match).unwrap(),
        tmp.join("._cfg0000_foo.conf")
    );

    std::fs::write(tmp.join("._cfg0000_foo.conf"), b"x").unwrap();
    assert_eq!(
        new_protect_filename(&dest, no_match).unwrap(),
        tmp.join("._cfg0001_foo.conf")
    );

    std::fs::write(tmp.join("._cfg0007_foo.conf"), b"x").unwrap();
    assert_eq!(
        new_protect_filename(&dest, no_match).unwrap(),
        tmp.join("._cfg0008_foo.conf")
    );

    // A same-prefixed file for a *different* basename doesn't count.
    std::fs::write(tmp.join("._cfg0099_other.conf"), b"x").unwrap();
    assert_eq!(
        new_protect_filename(&dest, no_match).unwrap(),
        tmp.join("._cfg0008_foo.conf")
    );
}

#[test]
fn new_protect_filename_reuses_last_file_with_matching_content() {
    let tmp = tempdir();
    std::fs::create_dir_all(&tmp).unwrap();
    let dest = tmp.join("foo.conf");
    std::fs::write(tmp.join("._cfg0000_foo.conf"), b"old update").unwrap();
    std::fs::write(tmp.join("._cfg0001_foo.conf"), b"newest update").unwrap();
    let newest_md5 = md5_hex(&tmp.join("._cfg0001_foo.conf")).unwrap();

    // The highest-numbered sibling's own content already matches --
    // reuse it instead of allocating ._cfg0002_.
    assert_eq!(
        new_protect_filename(&dest, &newest_md5).unwrap(),
        tmp.join("._cfg0001_foo.conf")
    );

    // A newmd5 that doesn't match the highest-numbered sibling
    // allocates a fresh number as usual, even though an *older*
    // sibling (._cfg0000_) would have matched -- real
    // new_protect_filename() only ever compares against the last one.
    let old_md5 = md5_hex(&tmp.join("._cfg0000_foo.conf")).unwrap();
    assert_eq!(
        new_protect_filename(&dest, &old_md5).unwrap(),
        tmp.join("._cfg0002_foo.conf")
    );
}
