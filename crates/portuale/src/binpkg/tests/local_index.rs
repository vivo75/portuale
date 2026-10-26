use super::*;

#[test]
fn populate_local_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file() {
    // The fixture dir holds a `.tbz2` and a `.gpkg.tar` plus an
    // index-only `Packages` file covering unrelated CPVs. The scan
    // walks unconditionally, so both files are freshly parsed from
    // disk regardless of what the index claims.
    // Portage: bintree.py, _populate_local
    let entries = populate_local_pkgdir(&fixture("pkgdir"), true, false).expect("scan succeeds");
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
        populate_local_pkgdir(Path::new("/nonexistent/pkgdir"), true, false)
            .unwrap()
            .is_empty()
    );
    let scratch = ScratchDir::new("scan-empty").unwrap();
    assert!(
        populate_local_pkgdir(scratch.path(), true, false)
            .unwrap()
            .is_empty()
    );
}

/// An `Invalid` file is skipped and the rest of the pool still
/// resolves; the exact warning line is pinned by the CLI contract
/// test, not here.
// Portage: bintree.py, _populate_local
#[test]
fn populate_local_pkgdir_skips_invalid_files_and_keeps_the_pool() {
    let scratch = ScratchDir::new("scan-skip-invalid").unwrap();
    let pkgdir = scratch.path();
    fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
    fs::copy(
        fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
        pkgdir.join("dev-libs/packagepkg-1.0.tbz2"),
    )
    .unwrap();
    let invalid =
        build_gpkg_with_inner_entries("invalidpkg-1.0", &[inner_entry("other/KEY", b"outside\n")]);
    fs::copy(&invalid, pkgdir.join("dev-libs/invalidpkg-1.0.gpkg.tar")).unwrap();

    let entries = populate_local_pkgdir(pkgdir, true, false)
        .expect("an invalid file must not abort the scan");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("CPV").map(String::as_str),
        Some("dev-libs/packagepkg-1.0")
    );
}

/// The other class: a `Fatal` file still stops the whole scan.
#[test]
fn populate_local_pkgdir_aborts_on_fatal_files() {
    let scratch = ScratchDir::new("scan-fatal").unwrap();
    let pkgdir = scratch.path();
    fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
    // Every xpak read error is Fatal (`read_xpak_metadata`).
    fs::write(
        pkgdir.join("dev-libs/fatalpkg-1.0.tbz2"),
        b"not an xpak binpkg",
    )
    .unwrap();

    let err =
        populate_local_pkgdir(pkgdir, true, false).expect_err("a fatal file must abort the scan");
    assert!(err.contains("fatalpkg-1.0.tbz2"), "{err}");
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
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
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
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "a stale index entry must still be trusted, not re-derived"
    );
}

#[test]
fn populate_local_pkgdir_accepts_a_truncated_binpkg_the_index_vouches_for() {
    // A truncated file the index vouches for is selected: the scan
    // returns the stanza verbatim (stale `SIZE` included) so the merge
    // path can verify against it.
    let scratch = ScratchDir::new("index-vouched-truncated").unwrap();
    let pkgdir = scratch.path();
    fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
    let whole = fs::read(fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar")).unwrap();
    let dest = pkgdir.join("dev-libs/gpkgreadpkg-1.0.gpkg.tar");
    fs::write(&dest, &whole[..whole.len() / 2]).unwrap();
    let half = fs::metadata(&dest).unwrap().len();
    assert!(half < whole.len() as u64);
    use md5::Digest as _;
    let md5: String = md5::Md5::digest(&whole)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    fs::write(
        pkgdir.join("Packages"),
        format!(
            "TIMESTAMP: 0\n\nCPV: dev-libs/gpkgreadpkg-1.0\nSLOT: 0\nSIZE: {}\nMD5: {md5}\n_mtime_: 1\nPATH: dev-libs/gpkgreadpkg-1.0.gpkg.tar\n",
            whole.len()
        ),
    )
    .unwrap();
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("CPV").map(String::as_str),
        Some("dev-libs/gpkgreadpkg-1.0")
    );
    assert_eq!(
        entries[0].get("SIZE").map(String::as_str),
        Some(whole.len().to_string()).as_deref(),
        "the vouched entry keeps the index SIZE, not the truncated file's"
    );
    assert_eq!(
        entries[0].get("MD5").map(String::as_str),
        Some(md5).as_deref()
    );
}

#[test]
fn populate_local_pkgdir_vouches_by_category_plus_basename() {
    // The vouch matches by category + basename, not the first
    // same-basename stanza: the same `PF` can exist in two categories.
    // The vouched path never parses the container.
    let scratch = ScratchDir::new("index-vouched-category").unwrap();
    let pkgdir = scratch.path();
    for cat in ["cat-a", "cat-b"] {
        fs::create_dir_all(pkgdir.join(cat)).unwrap();
        fs::write(pkgdir.join(cat).join("same-1.0.tbz2"), b"not-a-real-binpkg").unwrap();
    }
    fs::write(
        pkgdir.join("Packages"),
        "TIMESTAMP: 0\n\
         \n\
         CPV: cat-a/same-1.0\nSLOT: 0\nSIZE: 17\n_mtime_: 1\nPATH: cat-a/same-1.0.tbz2\n\
         \n\
         CPV: cat-b/same-1.0\nSLOT: 0\nSIZE: 17\n_mtime_: 1\nPATH: cat-b/same-1.0.tbz2\n",
    )
    .unwrap();
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(entries.len(), 2, "{entries:?}");
    for entry in &entries {
        let cpv = entry.get("CPV").map(String::as_str).unwrap_or("");
        // Each file reuses its own category's stanza: `CPV` and
        // `PATH` agree on the category (`<cat>/same-1.0`).
        assert_eq!(
            entry.get("PATH").map(String::as_str),
            Some(format!("{cpv}.tbz2")).as_deref(),
            "stanza {cpv} must vouch for its own category's file"
        );
    }
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
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
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
    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
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

    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "trusted mode keeps the stale stanza verbatim"
    );

    let entries = populate_local_pkgdir(pkgdir, false, false).expect("scan succeeds");
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

    let entries = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("SLOT").map(String::as_str),
        Some("99-not-the-real-slot"),
        "trusted mode keeps the key-incomplete stanza verbatim"
    );

    let entries = populate_local_pkgdir(pkgdir, false, false).expect("scan succeeds");
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

#[test]
fn populate_local_pkgdir_reindex_drops_a_removed_files_index_stanza() {
    // A removed file's stanza is re-injected in trusted mode but
    // absent in reindex mode; the survivor's entry is identical
    // either way.
    let scratch = ScratchDir::new("reindex-removed").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    let dest = cat_dir.join("packagepkg-1.0.tbz2");
    fs::copy(fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"), &dest).unwrap();
    let st = fs::metadata(&dest).unwrap();
    fs::write(
        pkgdir.join("Packages"),
        format!(
            "TIMESTAMP: 0\n\
             \n\
             CPV: dev-libs/gonepkg-1.0\nSLOT: 0\nSIZE: 12345\n_mtime_: 1\nPATH: dev-libs/gonepkg-1.0.tbz2\n\
             \n\
             BDEPEND: \nBUILD_ID: \nBUILD_TIME: 0\nCHOST: x86_64-pc-linux-gnu\nDEFINED_PHASES: install\nDEPEND: \nEAPI: 8\nIDEPEND: \nIUSE: \nKEYWORDS: amd64\nLICENSE: MIT\nPDEPEND: \nPROPERTIES: \nPROVIDES: \nRDEPEND: dev-libs/samepkg\nrepository: gentoo\nREQUIRES: \nRESTRICT: \nUSE: \nCPV: dev-libs/packagepkg-1.0\nSLOT: 0\nSIZE: {}\n_mtime_: {}\nPATH: dev-libs/packagepkg-1.0.tbz2\n",
            st.len(),
            file_mtime(&st),
        ),
    )
    .unwrap();

    let trusted = populate_local_pkgdir(pkgdir, true, false).expect("scan succeeds");
    assert_eq!(trusted.len(), 2, "{trusted:?}");

    let reindexed = populate_local_pkgdir(pkgdir, false, false).expect("scan succeeds");
    assert_eq!(reindexed.len(), 1, "{reindexed:?}");
    assert_eq!(
        reindexed[0].get("CPV").map(String::as_str),
        Some("dev-libs/packagepkg-1.0"),
        "only the surviving file stays: {reindexed:?}"
    );
    let survivor = trusted
        .iter()
        .find(|e| e.get("CPV").map(String::as_str) == Some("dev-libs/packagepkg-1.0"))
        .expect("trusted pool still holds the surviving file");
    assert_eq!(
        survivor, &reindexed[0],
        "the surviving file's own entry is identical either way"
    );
}

#[test]
fn populate_local_pkgdir_parses_unindexed_files_in_both_modes() {
    // A file no stanza covers joins the pool in both modes (the walk
    // runs in trusted mode too, so a `Packages`-less `$PKGDIR` still
    // resolves).
    let scratch = ScratchDir::new("reindex-unindexed").unwrap();
    let pkgdir = scratch.path();
    let cat_dir = pkgdir.join("dev-libs");
    fs::create_dir_all(&cat_dir).unwrap();
    fs::copy(
        fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
        cat_dir.join("packagepkg-1.0.tbz2"),
    )
    .unwrap();
    assert!(!pkgdir.join("Packages").exists());

    for trust in [true, false] {
        let entries = populate_local_pkgdir(pkgdir, trust, false).expect("scan succeeds");
        assert_eq!(entries.len(), 1, "trust={trust}: {entries:?}");
        assert_eq!(
            entries[0].get("CPV").map(String::as_str),
            Some("dev-libs/packagepkg-1.0"),
            "trust={trust}: {entries:?}"
        );
        assert_eq!(
            entries[0].get("SLOT").map(String::as_str),
            Some("0"),
            "trust={trust}: parsed from the archive: {entries:?}"
        );
    }
}

/// Under `gpkg_only` (`FEATURES=binpkg-request-signature`)
/// a local xpak is skipped while the gpkg next to it still pools, in both
/// trust modes (no index here, so nothing could vouch either way).
/// Portage: `bintree._populate_local`.
#[test]
fn populate_local_pkgdir_ignores_xpak_but_keeps_gpkg_under_gpkg_only() {
    let scratch = ScratchDir::new("scan-gpkg-only").unwrap();
    let pkgdir = scratch.path();
    fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
    fs::copy(
        fixture("pkgdir/dev-libs/packagepkg-1.0.tbz2"),
        pkgdir.join("dev-libs/packagepkg-1.0.tbz2"),
    )
    .unwrap();
    fs::copy(
        fixture("pkgdir/dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
        pkgdir.join("dev-libs/gpkgreadpkg-1.0.gpkg.tar"),
    )
    .unwrap();

    for trust in [true, false] {
        let plain = populate_local_pkgdir(pkgdir, trust, false).expect("scan succeeds");
        let plain: Vec<&str> = plain
            .iter()
            .filter_map(|e| e.get("CPV").map(String::as_str))
            .collect();
        assert!(plain.contains(&"dev-libs/packagepkg-1.0"), "{plain:?}");
        assert!(plain.contains(&"dev-libs/gpkgreadpkg-1.0"), "{plain:?}");

        let signed = populate_local_pkgdir(pkgdir, trust, true).expect("scan succeeds");
        let signed: Vec<&str> = signed
            .iter()
            .filter_map(|e| e.get("CPV").map(String::as_str))
            .collect();
        assert!(!signed.contains(&"dev-libs/packagepkg-1.0"), "{signed:?}");
        assert!(signed.contains(&"dev-libs/gpkgreadpkg-1.0"), "{signed:?}");
    }
}
