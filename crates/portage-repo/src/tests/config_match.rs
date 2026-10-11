use super::*;

#[test]
fn config_entries_matching_equals_a_flat_scan_and_keeps_file_order() {
    // The cp-bucketed lookup (`CpBucketIndex`, portuale's
    // `ExtendedAtomDict`) must return exactly the same entries, in the
    // same file order, as the naive `iter().filter(matches_config_entry)`
    // it replaced -- across an exact cp, a `cat/*`, a `*/*`, an
    // unrelated cp, a versioned constraint that fails, and a repo
    // constraint.
    let entries: Vec<(String, Vec<String>)> = [
        ("*/*", "a"),
        ("dev-libs/bar", "b"),
        ("dev-libs/*", "c"),
        ("app-misc/other", "d"),
        (">=dev-libs/bar-2.0", "e"),
        ("dev-libs/bar::other", "f"),
        ("=dev-libs/bar-1.0", "g"),
    ]
    .iter()
    .map(|(a, t)| (a.to_string(), vec![t.to_string()]))
    .collect();

    for cand in [
        "dev-libs/bar-1.0:0/0::gentoo",
        "dev-libs/bar-3.0:0/0::gentoo",
        "dev-libs/baz-1.0:0/0::gentoo",
        "app-misc/other-1.0:0/0::gentoo",
    ] {
        let parsed = portage_dep::parse_candidate(cand).unwrap();
        let (cat, pkg) = (parsed.category.as_str(), parsed.package.as_str());
        let flat: Vec<&(String, Vec<String>)> = entries
            .iter()
            .filter(|(e, _)| matches_config_entry(e, cand, cat, pkg))
            .collect();
        let bucketed = config_entries_matching(&entries, cand, cat, pkg);
        assert_eq!(flat, bucketed, "candidate {cand}");
    }
}
