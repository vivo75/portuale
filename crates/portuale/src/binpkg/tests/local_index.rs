use super::*;

#[test]
fn populate_local_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file() {
    // The fixture dir holds a `.tbz2` and a `.gpkg.tar` plus an
    // index-only `Packages` file covering unrelated CPVs. The scan
    // walks unconditionally, so both files are freshly parsed from
    // disk regardless of what the index claims.
    // Portage: bintree.py, _populate_local
    let entries = populate_local_pkgdir(&fixture("pkgdir"), true).expect("scan succeeds");
    let by_cpv: HashMap<&str, &HashMap<String, String>> = entries
        .iter()
        .map(|e| (e.get("CPV").unwrap().as_str(), e))
        .collect();

    let tbz2 = by_cpv["dev-libs/packagepkg-1.0"];
    assert_eq!(tbz2.get("SLOT").map(String::as_str), Some("0"));
    assert_eq!(tbz2.get("EAPI").map(String::as_str), Some("8"));
    assert_eq!(tbz2.get("CATEGORY").map(String::as_str), Some("dev-libs"));
    assert_eq!(tbz2.get("PF").map(String::as_str), Some("packagepkg-1.0"));
    assert_eq!(
        tbz2.get("PATH").map(String::as_str),
        Some("dev-libs/packagepkg-1.0.tbz2")
    );
    assert!(tbz2.get("SIZE").is_some_and(|s| s.parse::<u64>().is_ok()));
    assert!(
        tbz2.get("_mtime_")
            .is_some_and(|m| m.parse::<i64>().is_ok())
    );

    let gpkg = by_cpv["dev-libs/gpkgreadpkg-1.0"];
    assert_eq!(gpkg.get("KEYWORDS").map(String::as_str), Some("amd64"));
    assert_eq!(
        gpkg.get("DEPEND").map(String::as_str),
        Some("dev-libs/newpkg")
    );
    // `repository` -> `REPO` (the `Packages` field name).
    assert_eq!(gpkg.get("REPO").map(String::as_str), Some("gentoo"));
    assert!(!gpkg.contains_key("repository"));

    // Entries are CPV-sorted for a deterministic candidate pool.
    let cpvs: Vec<&str> = entries
        .iter()
        .map(|e| e.get("CPV").unwrap().as_str())
        .collect();
    let mut sorted = cpvs.clone();
    sorted.sort_unstable();
    assert_eq!(cpvs, sorted);
}

#[test]
fn populate_local_pkgdir_of_a_missing_or_empty_dir_is_empty() {
    assert!(
        populate_local_pkgdir(Path::new("/nonexistent/pkgdir"), true)
            .unwrap()
            .is_empty()
    );
    let scratch = ScratchDir::new("scan-empty").unwrap();
    assert!(
        populate_local_pkgdir(scratch.path(), true)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn populate_local_pkgdir_trusts_an_index_entry_even_when_stale() {
    // A stale stanza is reused verbatim (proven by a bogus SLOT the
    // index claims that the archive itself does not carry); the
    // corruption surfaces at merge via the digest check.
    let scratch = ScratchDir::new("mtime-staleness").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    let real_tbz2 = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
    let dest = cat_dir.join("packagepkg-1.0.tbz2");
    fs::copy(&real_tbz2, &dest).unwrap();
    let st = fs::metadata(&dest).unwrap();
    let real_size = st.len();
    let real_mtime = file_mtime(&st);

    // A trustworthy entry (matching SIZE/_mtime_) is reused verbatim,
    // even though its SLOT is fabricated -- proving the fast path
    // really does skip re-parsing the file.
    let trusted = format!(
        "CPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {real_size}\n_mtime_: {real_mtime}\nPATH: dev-libs/packagepkg-1.0.tbz2\n"
    );
    fs::write(
        pkgdir.join("Packages"),
        format!("TIMESTAMP: 0\n\n{trusted}"),
    )
    .unwrap();
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "an unchanged index entry must be trusted, not re-derived"
    );

    // The same claim with a wrong _mtime_: still trusted verbatim;
    // the size/digest mismatch is merge time's job, not the scan's.
    let stale = format!(
        "CPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {real_size}\n_mtime_: {}\nPATH: dev-libs/packagepkg-1.0.tbz2\n",
        real_mtime + 1000
    );
    fs::write(pkgdir.join("Packages"), format!("TIMESTAMP: 0\n\n{stale}")).unwrap();
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "a stale index entry must still be trusted, not re-derived"
    );
}

#[test]
fn populate_local_pkgdir_reinjects_a_removed_files_index_stanza() {
    // A removed file's stanza rejoins the pool verbatim while the
    // surviving file keeps its fast-path entry.
    let scratch = ScratchDir::new("index-orphan-stale").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    let real_tbz2 = fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2");
    let dest = cat_dir.join("packagepkg-1.0.tbz2");
    fs::copy(&real_tbz2, &dest).unwrap();
    let st = fs::metadata(&dest).unwrap();
    let real_size = st.len();
    let real_mtime = file_mtime(&st);
    fs::write(
        pkgdir.join("Packages"),
        format!(
            "TIMESTAMP: 0\n\
             \n\
             CPV: dev-libs/gonepkg-1.0\nSLOT: 0\nSIZE: 12345\n_mtime_: 1\nPATH: dev-libs/gonepkg-1.0.tbz2\n\
             \n\
             CPV: dev-libs/packagepkg-1.0\nSLOT: 0\nSIZE: {real_size}\n_mtime_: {real_mtime}\nPATH: dev-libs/packagepkg-1.0.tbz2\n"
        ),
    )
    .unwrap();
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 2, "{entries:?}");
    // `CPV`-sorted: the orphan sorts first.
    assert_eq!(
        entries[0].get("CPV").map(String::as_str),
        Some("dev-libs/gonepkg-1.0")
    );
    assert_eq!(
        entries[0].get("PATH").map(String::as_str),
        Some("dev-libs/gonepkg-1.0.tbz2"),
        "the orphan keeps the index PATH verbatim"
    );
    assert_eq!(
        entries[1].get("CPV").map(String::as_str),
        Some("dev-libs/packagepkg-1.0")
    );

    // The lone-orphan shape (no file on disk at all) still yields
    // the stanza.
    fs::remove_file(&dest).unwrap();
    fs::write(
        pkgdir.join("Packages"),
        "TIMESTAMP: 0\n\
         \n\
         CPV: dev-libs/gonepkg-1.0\nSLOT: 0\nSIZE: 12345\n_mtime_: 1\nPATH: dev-libs/gonepkg-1.0.tbz2\n",
    )
    .unwrap();
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("CPV").map(String::as_str),
        Some("dev-libs/gonepkg-1.0")
    );
}

#[test]
fn populate_local_pkgdir_reindex_reparses_a_changed_file() {
    // Reindex mode reuses a stanza only when `_mtime_` and `SIZE`
    // agree and every minimum key is present; a changed file is
    // re-parsed from disk. Trusted mode keeps the stale stanza.
    let scratch = ScratchDir::new("reindex-changed").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    let dest = cat_dir.join("packagepkg-1.0.tbz2");
    fs::copy(fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"), &dest).unwrap();
    let st = fs::metadata(&dest).unwrap();
    let real_size = st.len();
    let real_mtime = file_mtime(&st);
    fs::write(
        pkgdir.join("Packages"),
        format!(
            "TIMESTAMP: 0\n\nCPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {}\n_mtime_: {}\nPATH: dev-libs/packagepkg-1.0.tbz2\n",
            real_size + 1,
            real_mtime + 1000,
        ),
    )
    .unwrap();

    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "trusted mode keeps the stale stanza verbatim"
    );

    let entries = populate_local_pkgdir(pkgdir, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("0"),
        "reindex mode re-parses the changed file: {entries:?}"
    );
    assert_eq!(
        entries[0].get("SIZE").map(String::as_str),
        Some(real_size.to_string()).as_deref(),
        "reindex mode records the live size, not the stale claim"
    );
    assert_eq!(
        entries[0].get("_mtime_").map(String::as_str),
        Some(real_mtime.to_string()).as_deref(),
        "reindex mode records the live mtime, not the stale claim"
    );
}

#[test]
fn populate_local_pkgdir_reindex_reparses_a_key_incomplete_stanza() {
    // A stanza matching in stat fields but missing a minimum key
    // (no `EAPI` here) is re-parsed in reindex mode, still vouched
    // verbatim in trusted mode.
    let scratch = ScratchDir::new("reindex-key-incomplete").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    let dest = cat_dir.join("packagepkg-1.0.tbz2");
    fs::copy(fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"), &dest).unwrap();
    let st = fs::metadata(&dest).unwrap();
    let real_size = st.len();
    let real_mtime = file_mtime(&st);
    let stanza = format!(
        "TIMESTAMP: 0\n\nCPV: dev-libs/packagepkg-1.0\nSLOT: 99-not-the-real-slot\nSIZE: {real_size}\n_mtime_: {real_mtime}\nPATH: dev-libs/packagepkg-1.0.tbz2\nDEFINED_PHASES: install\nRDEPEND: dev-libs/samepkg\nUSE: \n",
    );
    fs::write(pkgdir.join("Packages"), &stanza).unwrap();

    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "trusted mode keeps the key-incomplete stanza verbatim"
    );

    let entries = populate_local_pkgdir(pkgdir, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("0"),
        "reindex mode re-parses the key-incomplete stanza: {entries:?}"
    );
    assert_eq!(
        entries[0].get("SIZE").map(String::as_str),
        Some(real_size.to_string()).as_deref(),
    );
    assert_eq!(
        entries[0].get("_mtime_").map(String::as_str),
        Some(real_mtime.to_string()).as_deref(),
    );
}
