use crate::*;

/// Count of unparseable tokens recorded for `site` (0 when the site has none yet).
/// Only the per-site entry is read: the process-wide total is shared with other
/// parallel tests and is not stable here.
pub(crate) fn site_count(site: &str) -> usize {
    unparsed_dep_sites()
        .iter()
        .find(|(s, _)| *s == site)
        .map_or(0, |(_, n)| *n)
}

#[test]
fn parse_dep_atom_counts_unparseable_per_site() {
    const SITE: &str = "t-site-globals";
    let before = site_count(SITE);
    assert!(parse_dep_atom("dev-libs/foo", SITE).is_some());
    assert_eq!(site_count(SITE), before);
    assert!(parse_dep_atom(">>>bad[", SITE).is_none());
    assert_eq!(site_count(SITE), before + 1);
}

#[test]
fn parse_dep_atom_ignores_grammar_tokens() {
    const SITE: &str = "t-site-grammar";
    let before = site_count(SITE);
    for tok in ["||", "^^", "??", "(", ")", "ssl?", "!ssl?"] {
        assert!(parse_dep_atom(tok, SITE).is_none());
    }
    assert_eq!(site_count(SITE), before);
}
