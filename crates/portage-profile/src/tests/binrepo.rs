use super::*;

#[test]
fn parse_binrepos_reads_getbinpkg_exclude_and_include_keys() {
    // Backlog #347 S9: the per-binrepo `getbinpkg-exclude/include` keys
    // (Portage `binrepo/config.py:42-75`, Q7 default yes). Grounded in S0
    // cell `g04-repokey` (a `getbinpkg-exclude = app-misc/s346-gbin-a`
    // key filters the remote index like the CLI flag).
    let conf = "\
[r]
sync-uri = file:///srv/pkgs
getbinpkg-exclude = app-misc/s346-gbin-a app-misc/s346-gbin-b
getbinpkg-include = app-misc/s346-gbin-c
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert!(warnings.is_empty());
    assert_eq!(
        repos[0].getbinpkg_exclude,
        vec![
            "app-misc/s346-gbin-a".to_string(),
            "app-misc/s346-gbin-b".to_string()
        ]
    );
    assert_eq!(
        repos[0].getbinpkg_include,
        vec!["app-misc/s346-gbin-c".to_string()]
    );
}

#[test]
fn parse_binrepos_drops_bad_getbinpkg_atoms_with_portages_warning() {
    // Backlog #347 S9: only package names and slot atoms are allowed
    // (Portage `repository/config.py:91` `_find_bad_atoms`). Grounded in
    // S0 cell `g05-repobadatom` (`=bad/atom-1` warns and is dropped).
    let conf = "\
[r]
sync-uri = file:///srv/pkgs
getbinpkg-exclude = =bad/atom-1 app-misc/s346-gbin-a
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert_eq!(
        repos[0].getbinpkg_exclude,
        vec!["app-misc/s346-gbin-a".to_string()]
    );
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].text,
        "\n!!! The following atoms are invalid in getbinpkg-exclude attribute for \
         binrepo [r] (only package names and slot atoms allowed):\n\n    =bad/atom-1"
    );
}

#[test]
fn parse_binrepos_conflicting_repo_keys_warn_and_cancel() {
    // Backlog #347 S9: atoms in both of one binrepo's own lists warn and
    // are dropped from both (Portage `binrepo/config.py:60-75`). The text
    // is Portage's own, byte for byte (note the missing "and").
    let conf = "\
[r]
sync-uri = file:///srv/pkgs
getbinpkg-exclude = app-misc/s346-gbin-a
getbinpkg-include = app-misc/s346-gbin-a app-misc/s346-gbin-b
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert!(repos[0].getbinpkg_exclude.is_empty());
    assert_eq!(
        repos[0].getbinpkg_include,
        vec!["app-misc/s346-gbin-b".to_string()]
    );
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].text,
        "\n!!! The following atoms appear in both the getbinpkg-exclude \
         getbinpkg-include lists for binrepo [r]:\n\n    app-misc/s346-gbin-a"
    );
}

#[test]
fn parse_binrepos_reads_sections_and_implicit_portage_binhost_entries() {
    let conf = "\
[gentoobinhost]
sync-uri = https://binhost.example.org/amd64/
priority = 5

[low]
sync-uri = file:///srv/pkgs
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(
        conf,
        "https://binhost.example.org/amd64/  https://other.example/b/",
        &mut warnings,
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
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert_eq!(repos.len(), 2);
    assert!(repos[0].frozen, "frozenrepo is frozen");
    assert!(!repos[1].frozen, "plainrepo defaults to unfrozen");
}

/// Backlog #539: every `[section]` inherits the `[DEFAULT]` keys, like
/// Portage's configparser (`BinRepoConfigLoader` reads
/// `dict(parser[section].items())`, which includes defaults). A keyless
/// section under `[DEFAULT] verify-signature = false` opts out of
/// `gpkg_only`; without the default the stock-global `true` applies.
#[test]
fn parse_binrepos_sections_inherit_default_keys() {
    let conf = "\
[DEFAULT]
verify-signature = false
frozen = true

[plain]
sync-uri = file:///srv/plain
priority = 2
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert!(warnings.is_empty());
    assert_eq!(repos.len(), 1);
    // Keyless section: both keys come from `[DEFAULT]`.
    assert!(!repos[0].verify_signature);
    assert!(repos[0].frozen);
    assert_eq!(repos[0].priority, 2);

    // The section's own keys win over the defaults.
    let conf = "\
[DEFAULT]
verify-signature = false

[plain]
sync-uri = file:///srv/plain
verify-signature = yes
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert!(warnings.is_empty());
    assert!(repos[0].verify_signature);

    // No `[DEFAULT]`: the stock-global default (`verify-signature = true`)
    // and the parser default (`frozen = false`) apply.
    let conf = "\
[plain]
sync-uri = file:///srv/plain
";
    let mut warnings = Vec::new();
    let repos = parse_binrepos(conf, "", &mut warnings);
    assert!(warnings.is_empty());
    assert!(repos[0].verify_signature);
    assert!(!repos[0].frozen);
}
