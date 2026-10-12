//! S3 extended (wildcard) atom tests: parse rows come from the S2 battery
//! (the `S3OK` rows, un-skipped here); match rows are live Portage output.
//!
//! `parse_atom` (= `eapi=None`, no wildcard flag) keeps rejecting extended
//! atoms; `CpBucketIndex::build` relies on that to route them to `other`.

use super::*;

/// Portage `match_from_list` on extended atoms (3.0.82.2), re-run live
/// 2026-10-12 with exactly these candidate strings
/// (`LLM/evidence/2026-10-11-341-oracle/probe_match.py` shape):
/// ```text
/// '*/foo*' -> ['dev-libs/foo-1.0::gentoo', 'dev-libs/foo-1.0-r1::gentoo',
///               'dev-libs/foo-10::gentoo', 'dev-libs/foobar-1.12::x']
/// '=*/foo-*1*' -> ['dev-libs/foo-1.0::gentoo', 'dev-libs/foo-1.0-r1::gentoo',
///                  'dev-libs/foo-10::gentoo']
/// '=dev-libs/foo*-*12*' -> ['dev-libs/foobar-1.12::x']
/// '*/*::x' -> ['dev-libs/foobar-1.12::x']
/// 'dev-libs/fo*' -> ['dev-libs/foo-1.0::gentoo', 'dev-libs/foo-1.0-r1::gentoo',
///                    'dev-libs/foo-10::gentoo', 'dev-libs/foobar-1.12::x']
/// '*-libs/f*o' -> ['dev-libs/foo-1.0::gentoo', 'dev-libs/foo-1.0-r1::gentoo',
///                  'dev-libs/foo-10::gentoo']
/// '*/*:0' -> ['dev-libs/foo-1.0::gentoo', 'dev-libs/foo-1.0-r1::gentoo',
///             'dev-libs/foo-10::gentoo', 'dev-libs/foobar-1.12::x']
/// ```
/// The foo candidates carry `::gentoo`, as in the oracle: a candidate with
/// no repo passes any `::repo` filter (dep:3190-3203; `matches_repo`), so
/// repo-less candidates would all match `*/*::x` (review C-2). `matches_repo`
/// itself is unchanged.
/// Portage: lib/portage/dep/__init__.py:2887-3205 (`match_from_list`).
#[test]
fn extended_atoms_match_like_portage() {
    let c = [
        "dev-libs/foo-1.0::gentoo",
        "dev-libs/foo-1.0-r1::gentoo",
        "dev-libs/foo-10::gentoo",
        "dev-libs/foobar-1.12::x",
    ];
    assert_eq!(match_from_list("*/foo*", &c).unwrap(), c.to_vec());
    assert_eq!(
        match_from_list("=*/foo-*1*", &c).unwrap(),
        vec![c[0], c[1], c[2]]
    );
    assert_eq!(
        match_from_list("=dev-libs/foo*-*12*", &c).unwrap(),
        vec![c[3]]
    );
    assert_eq!(match_from_list("*/*::x", &c).unwrap(), vec![c[3]]);
    assert_eq!(match_from_list("dev-libs/fo*", &c).unwrap(), c.to_vec());
    assert_eq!(
        match_from_list("*-libs/f*o", &c).unwrap(),
        vec![c[0], c[1], c[2]]
    );
    // A slot filter on an extended atom: candidates without slot info pass
    // (Portage `_match_slot` keeps slot-less candidates; `matches_slot` does
    // the same), so `*/*:0` matches all four here.
    assert_eq!(match_from_list("*/*:0", &c).unwrap(), c.to_vec());
}

// Portage `extended_cp_match` (dep:2352-2365): each `*` is `[^/]*`, anchored,
// so a glob never crosses the `/`.
#[test]
fn extended_cp_match_does_not_cross_the_slash() {
    assert!(extended_cp_match("*/*", "dev-libs/foo"));
    assert!(extended_cp_match("dev-libs/*", "dev-libs/foo"));
    assert!(!extended_cp_match("dev-libs/*", "dev-libsfoo/bar"));
    assert!(extended_cp_match("*-libs/f*o", "dev-libs/foo"));
    assert!(!extended_cp_match("*-libs/f*o", "dev-libs/bar"));
    // `.` and `+` are literals, not regex syntax.
    assert!(extended_cp_match("dev-*/foo", "dev-libs/foo"));
    assert!(!extended_cp_match("dev-*/foo", "devXlibs/foo"));
}

// The extended `=*` arm is a substring test on the candidate's `_pkg_str`
// version (`ver[-rN]`, without `-r0`; dep:2954-2972). Portuale candidates
// model `_pkg_str`, so `=*/foo-*r0*` matches nothing here — while Portage on
// a *plain-string* candidate would match `"dev-libs/foo-1.0"` (its `x_ver`
// comes from `catpkgsplit` and includes `-r0`; review M-3, proven live).
#[test]
fn extended_glob_matches_substring_of_pkg_str_version_without_r0() {
    let c = ["dev-libs/foo-1.0", "dev-libs/foo-1.0-r1", "dev-libs/foo-10"];
    assert_eq!(
        match_from_list("=*/foo-*1*", &c).unwrap(),
        vec![c[0], c[1], c[2]]
    );
    assert_eq!(
        match_from_list("=*/foo-*r0*", &c).unwrap(),
        Vec::<&str>::new()
    );
    assert_eq!(match_from_list("=*/foo-*r1*", &c).unwrap(), vec![c[1]]);
}

// A blocker prefix is stripped before the re-parse (dep:2901-2906): a
// blocker atom matches like its bare form.
#[test]
fn blocker_extended_atom_matches_like_its_bare_form() {
    let c = ["dev-libs/foo-1.0::gentoo", "dev-libs/foobar-1.12::x"];
    assert_eq!(
        match_from_list("!*/foo", &c).unwrap(),
        match_from_list("*/foo", &c).unwrap(),
    );
}

// A `simple` `-vr` tail rejects the atom only when it carries a dotted
// component (Portage dep:1988-1989 checks group `simple+3`, the `(\.\d+)`
// inside the tail — live 2026-10-12): a dotless tail stays in the cp, a
// dotted one is invalid, and a tail that fits neither the package charset
// nor `-vr` never matches at all.
#[test]
fn simple_version_tail_rejects_only_dotted_tails() {
    let wild = AtomOptions {
        allow_wildcard: true,
        allow_repo: Some(true),
        ..AtomOptions::default()
    };
    let ok = |s: &str| parse_atom_with(s, &wild, "").expect(s);
    for (s, cp) in [
        ("*/foo-1", "*/foo-1"),
        ("*/foo-1a", "*/foo-1a"),
        ("*/foo-1-r1", "*/foo-1-r1"),
        ("*/foo-1-2", "*/foo-1-2"),
        ("*/foo-1_pre1", "*/foo-1_pre1"),
        ("*/foo-", "*/foo-"),
        ("*/foo--1", "*/foo--1"),
    ] {
        let atom = ok(s);
        assert!(atom.extended, "{s}");
        assert_eq!(format!("{}/{}", atom.category, atom.package), cp, "{s}");
    }
    for s in [
        "*/foo-1.0",
        "*/foo-1.0-r1",
        "*/foo-1.r1",
        "*/foo-bar-1.0",
        "*/-1",
        // The strict grammar catches this first (ambiguous tail), so the
        // wildcard branch never sees it.
        "dev-libs/foo-1",
    ] {
        assert!(parse_atom_with(s, &wild, "").is_err(), "{s}");
    }
}

// The default `parse_atom` keeps rejecting extended atoms, so
// `CpBucketIndex::build` (config_match.rs) routes them to `other`.
#[test]
fn default_parse_atom_still_rejects_extended_atoms() {
    for s in [
        "*/foo",
        "dev-libs/*",
        "*/*",
        "dev-libs/fo*",
        "*-libs/f*o",
        "=*/foo-*1*",
        "=dev-libs/foo*-*12*",
        "*/*:0",
        "*/*::gentoo",
        "!*/foo",
        "dev-libs/+foo",
    ] {
        assert_eq!(parse_atom(s), None, "{s}");
    }
}
