use super::*;

#[test]
fn parse_binrepos_reads_sections_and_implicit_portage_binhost_entries() {
    let conf = "\
[gentoobinhost]
sync-uri = https://binhost.example.org/amd64/
priority = 5

[low]
sync-uri = file:///srv/pkgs
";
    let repos = parse_binrepos(
        conf,
        "https://binhost.example.org/amd64/  https://other.example/b/",
    );
    // `low` (prio 0), then the implicit `other.example/b` (prio 1),
    // then `gentoobinhost` (prio 5). The already-listed
    // binhost.example.org URI is not re-added as implicit.
    assert_eq!(
        repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["low", "other.example/b", "gentoobinhost"]
    );
    assert_eq!(repos[0].sync_uri, "file:///srv/pkgs");
    assert_eq!(repos[2].sync_uri, "https://binhost.example.org/amd64");
}

#[test]
fn parse_binrepos_reads_frozen_defaulting_to_false() {
    // Real `binrepo/config.py` `_bool_opts` (`"frozen": "false"`
    // default, backlog #192).
    let conf = "\
[frozenrepo]
sync-uri = https://frozen.example.org/amd64/
frozen = true

[plainrepo]
sync-uri = https://plain.example.org/amd64/
";
    let repos = parse_binrepos(conf, "");
    assert_eq!(repos.len(), 2);
    assert!(repos[0].frozen, "frozenrepo is frozen");
    assert!(!repos[1].frozen, "plainrepo defaults to unfrozen");
}
