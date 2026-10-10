use super::*;

/// A stale `-MERGING-<pf>` entry left by a killed merge is invisible to
/// installed-package enumeration (Portage's `vartree._excluded_dirs`).
#[test]
fn all_installed_packages_skips_a_stale_merging_entry() {
    let root = tmp_vdb("dev-libs", "realpkg-1.0", &[]);
    let stale = root.join("var/db/pkg/dev-libs/-MERGING-realpkg-2.0");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("SLOT"), "0\n").unwrap();
    std::fs::write(stale.join("COUNTER"), "41\n").unwrap();
    std::fs::write(stale.join("CONTENTS"), "obj /usr/bin/stale abc 123\n").unwrap();

    let cpvs: Vec<String> = all_installed_packages(&root)
        .iter()
        .map(InstalledPackage::cpv)
        .collect();
    assert_eq!(cpvs, vec!["dev-libs/realpkg-1.0".to_string()]);
    assert_eq!(
        installed_versions(&root, "dev-libs", "realpkg"),
        vec!["1.0".to_string()]
    );
}

/// `read_vdb_string` collapses multi-line values to one space-separated line
/// (Portage's `_aux_get` normalisation).
#[test]
fn read_vdb_string_collapses_multiline_values_to_one_space_separated_line() {
    let root = tmp_vdb(
        "dev-libs",
        "multiline-1.0",
        &[("RDEPEND", b"dev-libs/a\n\n  dev-libs/b\tdev-libs/c  \n")],
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multiline", "1.0", "RDEPEND"),
        "dev-libs/a dev-libs/b dev-libs/c"
    );
}

/// The trailing newline Portage's writer leaves must not survive the read.
#[test]
fn read_vdb_string_drops_the_trailing_newline() {
    let root = tmp_vdb("dev-libs", "newline-1.0", &[("repository", b"gentoo\n")]);
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "newline", "1.0", "repository"),
        "gentoo"
    );
}

/// An absent field file reads as `""`, not a panic.
#[test]
fn read_vdb_string_missing_file_is_still_empty() {
    let root = tmp_vdb("dev-libs", "absent-1.0", &[]);
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "absent", "1.0", "RDEPEND"),
        ""
    );
}

/// A key outside Portage's 23-field set is served raw, never collapsed.
#[test]
fn vdb_aux_get_reads_an_out_of_set_key_raw() {
    let root = tmp_vdb(
        "dev-libs",
        "rawkey-1.0",
        &[(
            "CONTENTS",
            b"obj /usr/lib/foo.so abc123 123\nsym /usr/lib/bar.so\n",
        )],
    );
    assert_eq!(
        vdb_aux_get(&root, "dev-libs", "rawkey", "1.0", "CONTENTS"),
        "obj /usr/lib/foo.so abc123 123\nsym /usr/lib/bar.so\n"
    );
    // Absent out-of-set file is still `""`.
    assert_eq!(
        vdb_aux_get(&root, "dev-libs", "rawkey", "1.0", "NEEDED.ELF.2"),
        ""
    );
}

/// Field files decode lossy like Portage (`errors="replace"`).
#[test]
fn read_vdb_string_decodes_invalid_utf8_lossy_like_portage() {
    let root = tmp_vdb(
        "dev-libs",
        "latin1-1.0",
        &[("DESCRIPTION", b"caf\xe9 package\n")],
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "latin1", "1.0", "DESCRIPTION"),
        "caf\u{fffd} package"
    );
}

/// The committed multilinevdb fixture is the integration half of the same
/// contract: embedded newlines served exactly like Portage's `_aux_get`.
#[test]
fn read_vdb_string_normalises_the_committed_multiline_fixture() {
    let root = fixtures_root();
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "IUSE"),
        "alpha beta"
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "RDEPEND"),
        "dev-libs/samepkg virtual/libc"
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "multilinevdb", "1.0", "DEFINED_PHASES"),
        "install info"
    );
}

/// A validated snapshot serves a field with no open of the individual file.
#[test]
fn vdb_aux_get_serves_a_valid_snapshot_without_opening_the_field_file() {
    let root = tmp_vdb(
        "dev-libs",
        "snap-1.0",
        &[("RDEPEND", b"dev-libs/individual\n")],
    );
    write_snapshot(
        &root,
        "dev-libs",
        "snap-1.0",
        "#format=1\nRDEPEND=dev-libs/snapshot\n",
        true,
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snap", "1.0", "RDEPEND"),
        "dev-libs/snapshot"
    );
}

/// A validated snapshot is complete: a field it lacks reads as `""`.
#[test]
fn vdb_aux_get_serves_empty_for_a_field_absent_from_a_valid_snapshot() {
    let root = tmp_vdb(
        "dev-libs",
        "snapabsent-1.0",
        &[("RDEPEND", b"dev-libs/individual\n")],
    );
    write_snapshot(
        &root,
        "dev-libs",
        "snapabsent-1.0",
        "#format=1\nSLOT=0\n",
        true,
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapabsent", "1.0", "RDEPEND"),
        ""
    );
}

/// A snapshot whose `#format=` is not 1 is rejected, falling back to files.
#[test]
fn vdb_aux_get_rejects_a_wrong_format_version() {
    let root = tmp_vdb(
        "dev-libs",
        "snapfmt-1.0",
        &[("RDEPEND", b"dev-libs/individual\n")],
    );
    write_snapshot(
        &root,
        "dev-libs",
        "snapfmt-1.0",
        "#format=2\nRDEPEND=dev-libs/snapshot\n",
        true,
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapfmt", "1.0", "RDEPEND"),
        "dev-libs/individual"
    );
}

/// A missing, stale or unparsable `#dir_mtime=`, or a garbage `#format=`,
/// rejects the snapshot.
#[test]
fn vdb_aux_get_falls_back_on_a_missing_stale_or_garbage_stamp() {
    let dir = |root: &Path| root.join("var/db/pkg/dev-libs/snapbad-1.0");
    let body = "#format=1\nRDEPEND=dev-libs/snapshot\n";
    let individual = "dev-libs/individual";
    // Missing `#dir_mtime=` entirely.
    let root = tmp_vdb(
        "dev-libs",
        "snapbad-1.0",
        &[("RDEPEND", b"dev-libs/individual\n")],
    );
    write_snapshot(&root, "dev-libs", "snapbad-1.0", body, false);
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapbad", "1.0", "RDEPEND"),
        individual
    );
    // Off-by-one stamp.
    let path = write_snapshot(&root, "dev-libs", "snapbad-1.0", body, false);
    append_line(
        &path,
        &format!("#dir_mtime={}", dir_mtime_ns(&dir(&root)) + 1),
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapbad", "1.0", "RDEPEND"),
        individual
    );
    // Garbage in the mtime header.
    let path = write_snapshot(&root, "dev-libs", "snapbad-1.0", body, false);
    append_line(&path, "#dir_mtime=not-a-number");
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapbad", "1.0", "RDEPEND"),
        individual
    );
    // Garbage in the format header.
    write_snapshot(
        &root,
        "dev-libs",
        "snapbad-1.0",
        "#format=one\nRDEPEND=dev-libs/snapshot\n",
        true,
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapbad", "1.0", "RDEPEND"),
        individual
    );
}

/// No `metadata` file at all falls back to the individual files.
#[test]
fn vdb_aux_get_falls_back_without_a_metadata_file() {
    let root = tmp_vdb(
        "dev-libs",
        "snapnone-1.0",
        &[("RDEPEND", b"dev-libs/individual\n")],
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapnone", "1.0", "RDEPEND"),
        "dev-libs/individual"
    );
}

/// `vdb_entry_slot` is the `pf`-addressed wrapper around `vdb_aux_get("SLOT")`.
#[test]
fn vdb_entry_slot_matches_vdb_aux_get_on_a_fixture_entry() {
    let root = tmp_vdb("dev-libs", "entryslot-1.2.3-r1", &[("SLOT", b"0/1.2\n")]);
    assert_eq!(
        vdb_entry_slot(&root, "dev-libs", "entryslot-1.2.3-r1"),
        vdb_aux_get(&root, "dev-libs", "entryslot", "1.2.3-r1", "SLOT")
    );
    assert_eq!(
        vdb_entry_slot(&root, "dev-libs", "entryslot-1.2.3-r1"),
        "0/1.2"
    );
}

/// A multi-line `SLOT` collapses, then translates to `"0"` as invalid.
#[test]
fn vdb_entry_slot_collapses_a_multiline_slot_to_one_space_separated_line() {
    let root = tmp_vdb("dev-libs", "entryslot-2.0", &[("SLOT", b"0/1.2\n0/1.3\n")]);
    assert_eq!(vdb_entry_slot(&root, "dev-libs", "entryslot-2.0"), "0");
}

/// An unsplittable directory name yields `""`, like an absent `SLOT`.
#[test]
fn vdb_entry_slot_returns_empty_for_an_unsplittable_pf() {
    let root = tmp_vdb("dev-libs", "not-a-version-dir", &[]);
    assert_eq!(vdb_entry_slot(&root, "dev-libs", "not-a-version-dir"), "");
}

/// A present-but-invalid `SLOT` reads as `"0"` (Portage's `_get_slot_re`).
#[test]
fn vdb_entry_slot_translates_a_present_but_invalid_slot_to_0() {
    let root = tmp_vdb("dev-libs", "badslot-1.0", &[("SLOT", b"!!bad slot!!\n")]);
    assert_eq!(vdb_entry_slot(&root, "dev-libs", "badslot-1.0"), "0");
}

/// A valid `main/sub` slot is not flattened to `"0"`.
#[test]
fn vdb_entry_slot_keeps_a_valid_sub_slot() {
    let root = tmp_vdb("dev-libs", "subslot-2.0", &[("SLOT", b"0/2\n")]);
    assert_eq!(vdb_entry_slot(&root, "dev-libs", "subslot-2.0"), "0/2");
}

/// A collapsed multi-line value is invalid too, so it also reads as `"0"`.
#[test]
fn vdb_entry_slot_translates_a_collapsed_multiline_slot_to_0() {
    let root = tmp_vdb("dev-libs", "multislot-2.0", &[("SLOT", b"0\n1\n")]);
    assert_eq!(vdb_entry_slot(&root, "dev-libs", "multislot-2.0"), "0");
}

/// Snapshot parse (Portage's `_read_metadata_file`): `#` lines ignored,
/// `=`-less lines skipped, last duplicate wins.
#[test]
fn vdb_aux_get_snapshot_parse_ignores_comments_and_lets_the_last_key_win() {
    let root = tmp_vdb("dev-libs", "snapparse-1.0", &[]);
    write_snapshot(
        &root,
        "dev-libs",
        "snapparse-1.0",
        "#format=1\n#comment\nno-equals-line\nRDEPEND=first\nRDEPEND=last=with=equals\n",
        true,
    );
    assert_eq!(
        read_vdb_string(&root, "dev-libs", "snapparse", "1.0", "RDEPEND"),
        "last=with=equals"
    );
}

/// `resolve_vdb_entry` returns the direct key for an existing entry,
/// and the direct key unchanged for a file or an absent entry.
#[test]
fn resolve_vdb_entry_returns_the_direct_key() {
    let root = tmp_vdb("dev-libs", "statmeta-1.0", &[]);
    let got = resolve_vdb_entry(&root, "dev-libs", "statmeta", "1.0");
    assert_eq!(got, portage_vdb::EntryKey::new("dev-libs", "statmeta-1.0"));
    assert!(vdb_entry_is_dir(&root, "dev-libs", "statmeta", "1.0"));
    // A *file* at the entry path is not a directory.
    let file_root = tmp_vdb("dev-libs", "statmeta-2.0", &[]);
    let file_dir = file_root.join("var/db/pkg/dev-libs/statmeta-2.0");
    std::fs::remove_dir_all(&file_dir).unwrap();
    std::fs::write(&file_dir, b"not a dir").unwrap();
    assert!(!vdb_entry_is_dir(&file_root, "dev-libs", "statmeta", "2.0"));
    // An absent entry: the unresolved direct key.
    let missing = resolve_vdb_entry(&root, "dev-libs", "absent", "1.0");
    assert_eq!(
        missing,
        portage_vdb::EntryKey::new("dev-libs", "absent-1.0")
    );
    assert!(!vdb_entry_is_dir(&root, "dev-libs", "absent", "1.0"));
}

/// A second lookup on the same instance does not re-read the snapshot.
#[test]
fn vdb_aux_memo_serves_the_cached_snapshot_until_the_dir_mtime_moves() {
    let root = tmp_vdb("dev-libs", "memofresh-1.0", &[]);
    write_snapshot(
        &root,
        "dev-libs",
        "memofresh-1.0",
        "#format=1\nRDEPEND=from-snapshot-1\n",
        true,
    );
    let dir = root.join("var/db/pkg/dev-libs/memofresh-1.0");
    let read = || read_vdb_string(&root, "dev-libs", "memofresh", "1.0", "RDEPEND");
    assert_eq!(read(), "from-snapshot-1");
    // In-place rewrite: the dir mtime does not move -> memo hit.
    std::fs::write(dir.join("metadata"), "#format=1\nRDEPEND=from-snapshot-2\n").unwrap();
    assert_eq!(read(), "from-snapshot-1");
    // A dir mtime change invalidates; the rewritten file is rejected and the
    // (absent) individual file is served as "".
    bump_dir_mtime(&dir);
    assert_eq!(read(), "");
    // A fresh valid snapshot is picked up on the next lookup, once the dir
    // mtime moves the way the merge path replaces a whole entry.
    bump_dir_mtime(&dir);
    let ns = dir_mtime_ns(&dir);
    std::fs::write(
        dir.join("metadata"),
        format!("#format=1\nRDEPEND=from-snapshot-3\n#dir_mtime={ns}\n"),
    )
    .unwrap();
    assert_eq!(read(), "from-snapshot-3");
}

/// The fallback path memoises too, per key, lazily, on the same mtime signal.
#[test]
fn vdb_aux_memo_caches_the_fallback_path_per_key() {
    let root = tmp_vdb(
        "dev-libs",
        "memofallback-1.0",
        &[("RDEPEND", b"fallback-one\n"), ("IUSE", b"alpha\n")],
    );
    let dir = root.join("var/db/pkg/dev-libs/memofallback-1.0");
    let read = |key: &str| read_vdb_string(&root, "dev-libs", "memofallback", "1.0", key);
    assert_eq!(read("RDEPEND"), "fallback-one");
    // In-place rewrite -> memo hit.
    std::fs::write(dir.join("RDEPEND"), b"fallback-two\n").unwrap();
    assert_eq!(read("RDEPEND"), "fallback-one");
    // A different key resolves and memoises on its own.
    assert_eq!(read("IUSE"), "alpha");
    std::fs::write(dir.join("IUSE"), b"beta\n").unwrap();
    assert_eq!(read("IUSE"), "alpha");
    // Dir mtime change -> the individual files are re-read.
    bump_dir_mtime(&dir);
    std::fs::write(dir.join("RDEPEND"), b"fallback-three\n").unwrap();
    assert_eq!(read("RDEPEND"), "fallback-three");
}

/// `installed_candidates` serves repeats from its per-cp cache, invalidated
/// by the category dir mtime.
#[test]
fn installed_candidates_caches_per_cp_and_invalidates_on_vdb_change() {
    let root = tmp_vdb("dev-libs", "cachetest-1.0", &[]);
    let one = vec![("1.0".to_string(), "0".to_string(), "0".to_string())];
    // First call scans; the repeat is a cache hit.
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
    // A negative result is cached too.
    assert!(installed_candidates(&root, "dev-libs", "nocachetest").is_empty());
    // Adding a version bumps the category mtime and invalidates.
    let dir = root
        .join("var/db/pkg")
        .join("dev-libs")
        .join("cachetest-2.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SLOT"), "0\n").unwrap();
    let mut got = installed_candidates(&root, "dev-libs", "cachetest");
    got.sort();
    assert_eq!(
        got,
        vec![
            ("1.0".to_string(), "0".to_string(), "0".to_string()),
            ("2.0".to_string(), "0".to_string(), "0".to_string()),
        ]
    );
    // Removing it invalidates back.
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(installed_candidates(&root, "dev-libs", "cachetest"), one);
}

#[test]
fn candidate_use_flags_display_resolves_use_expand_flags_for_a_resume_entry() {
    // `emerge --resume` rebuilds each entry from a bare `cat/pkg-ver`; this
    // re-derives the `USE=` display for the fixture's USE_EXPAND setup.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let display = candidate_use_flags_display(&repos, &config, "dev-libs", "useexpandpkg", "1.0");
    assert_eq!(
        display,
        vec![
            ("video_cards_amdgpu".to_string(), false),
            ("video_cards_nvidia".to_string(), true),
        ]
    );
    // An unlocatable candidate is an empty list, never a panic.
    assert!(
        candidate_use_flags_display(&repos, &config, "dev-libs", "nope-does-not-exist", "9")
            .is_empty()
    );
}

#[test]
fn candidate_effective_use_flags_treats_a_missing_iuse_key_as_empty_iuse() {
    // Portage's md5-cache writer omits empty fields, but a package with an
    // empty IUSE still has the full effective USE.
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf resolves");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    // `aubreaktop`'s cache has no `IUSE=` line.
    let flags = candidate_effective_use_flags(&repos, &config, "dev-libs", "aubreaktop", "1.0");
    assert!(!flags.is_empty(), "missing IUSE must not blank the USE set");
    assert!(
        flags
            .iter()
            .any(|f| f == "baz" || f == "confflag" || f == "foo"),
        "expected a profile USE flag, got {flags:?}"
    );
    assert!(candidate_effective_use_flags(&repos, &config, "dev-libs", "nope", "9").is_empty());
}

#[test]
fn running_root_satisfies_atom_checks_the_given_roots_own_real_vdb() {
    // The provider exists only as a hand-seeded vdb entry (no ebuild anywhere).
    let root = fixtures_root();
    let config = test_config();
    assert!(running_root_satisfies_atom(
        "dev-libs/rootdepsprovider",
        &root,
        &config
    ));
    assert!(running_root_satisfies_atom(
        "dev-libs/rootdepsprovider:0",
        &root,
        &config
    ));
    assert!(!running_root_satisfies_atom(
        "dev-libs/rootdepsprovider:1",
        &root,
        &config
    ));
    assert!(!running_root_satisfies_atom(
        "dev-libs/nonexistentprovider",
        &root,
        &config
    ));
}
