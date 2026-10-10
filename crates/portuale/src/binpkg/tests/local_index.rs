use super::*;

#[test]
fn populate_local_pkgdir_synthesizes_a_packages_style_entry_per_binpkg_file() {
    // fixtures/pkgdir/dev-libs/ holds a real `.tbz2` and a real
    // `.gpkg.tar` (among other loose binpkgs) plus an index-only
    // `Packages` file (fixtures/pkgdir/Packages) covering unrelated
    // CPVs. `populate_local_pkgdir` walks the directory unconditionally
    // (real `_populate_local`'s default `reindex=True`), so entries
    // for these two files are always freshly parsed from disk
    // regardless of what the index claims.
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
    // `repository` -> `REPO` (real `Packages` field name).
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

/// Real `_populate_local`'s two classes at the scan boundary
/// (`bintree.py:1185-1199`): an `Invalid` file is skipped (with
/// real's warning -- the exact line is pinned by the CLI contract
/// test, not here) and the rest of the pool still resolves.
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

    let entries =
        populate_local_pkgdir(pkgdir, true).expect("an invalid file must not abort the scan");
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0].get("CPV").map(String::as_str),
        Some("dev-libs/packagepkg-1.0")
    );
}

/// The other class: a `Fatal` file (real would traceback) still stops
/// the whole scan.
#[test]
fn populate_local_pkgdir_aborts_on_fatal_files() {
    let scratch = ScratchDir::new("scan-fatal").unwrap();
    let pkgdir = scratch.path();
    fs::create_dir_all(pkgdir.join("dev-libs")).unwrap();
    let fatal = build_gpkg_with_inner_entries(
        "fatalpkg-1.0",
        &[inner_symlink("metadata/DESCRIPTION", "/etc/hostname")],
    );
    fs::copy(&fatal, pkgdir.join("dev-libs/fatalpkg-1.0.gpkg.tar")).unwrap();

    let err = populate_local_pkgdir(pkgdir, true).expect_err("a fatal file must abort the scan");
    assert!(err.contains("not in the archive"), "{err}");
}

#[test]
fn populate_local_pkgdir_trusts_an_index_entry_even_when_stale() {
    // Backlog #174: real's own default (`FEATURES=
    // pkgdir-index-trusted` is in real `make.globals`' own default
    // `FEATURES`) runs `_populate_local` with `reindex=False` -- the
    // `<pkgdir>/Packages` stanza vouches for the file even when its
    // `_mtime_`/`SIZE` no longer agree with the live file, and the
    // file's own bytes are first touched at merge by
    // `BinpkgVerifier`. So a stale stanza is reused verbatim here
    // (proven by a bogus SLOT the index claims that the real
    // `.tbz2` itself does not carry) instead of being re-derived
    // from the file -- the corruption surfaces at merge via
    // `verify_binpkg_against_index`, with real's `!!! Digest
    // verification failed:` block. (This test previously asserted
    // the reindex behavior -- stale means re-parse -- which is only
    // real with an explicit `-pkgdir-index-trusted`.)
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

    // The same claim, but with a wrong _mtime_ -- still stale, so
    // the index is *still* trusted verbatim (backlog #174): the
    // bogus SLOT survives, and the size/digest mismatch is merge
    // time's job, not the scan's.
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
fn populate_local_pkgdir_accepts_a_truncated_binpkg_the_index_vouches_for() {
    // Backlog #174 (the F2 shape): real `emerge --oneshot
    // --usepkgonly l32/faultpkg` with the gpkg truncated to half
    // *selects* the binary -- the `<pkgdir>/Packages` stanza (written
    // by `--regen` before the truncation, so its `SIZE` is the
    // whole-file size) vouches for it at scan under real's default
    // `FEATURES=pkgdir-index-trusted`, and the failure surfaces at
    // merge via `BinpkgVerifier`. Portuale used to fully parse the
    // container at scan, hit `unexpected EOF during skip`, and drop
    // the candidate with `!!! Invalid binary package`, ending in
    // `there are no ebuilds to satisfy`. The scan must return the
    // index stanza verbatim -- stale `SIZE` included -- so the merge
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
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
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
    // Backlog #174 review: the index-vouched branch must match the
    // stanza by category + basename (`CPV` starting with
    // `{category}/`), not the first same-basename stanza. The same
    // `PF` can exist in two categories with the same basename
    // (`cat-a/same-1.0.tbz2` vs `cat-b/same-1.0.tbz2`); each file
    // must reuse its own category's stanza. The vouched path never
    // parses the container, so the files' bytes are irrelevant.
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
    let entries = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
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
    // Backlog #187 (the bed's F3a shape): the local `Packages`
    // still lists a binary whose file was removed, while a
    // sibling binpkg is still on disk. Real trusts the index
    // under its default `FEATURES=pkgdir-index-trusted`
    // (`_populate_local(reindex=False)` `cpv_inject`s every
    // stanza without stating any file, `bintree.py:1057-1064`),
    // so the removed package stays a selectable binary and only
    // fails at merge (`BinpkgVerifier`). Portuale's file-driven
    // scan used to drop the orphan outright -- and with any other
    // file present the non-empty scan shadowed the raw-index
    // fallback too, so the resolver silently fell back to the
    // ebuild (rc 0) where real aborts (rc 1). The orphan must
    // rejoin the pool verbatim (`PATH` as the index claims it),
    // while the surviving file keeps its fast-path entry.
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

    // The lone-orphan shape (no file on disk at all): the scan
    // still yields the stanza -- the pool never degrades to "no
    // binary".
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
    // Backlog #199: real `_populate_local(reindex=True)` (explicit
    // `FEATURES=-pkgdir-index-trusted`, `bintree.py:1110-1136`) only
    // reuses an index stanza whose `_mtime_` *and* `SIZE` still agree
    // with the live file's `lstat(2)` (and which carries real's
    // `minimum_keys`); a file whose stat fields changed is re-parsed
    // from disk (`bintree.py:1179-1351`). So a stale stanza (bogus
    // `_mtime_`, proven stale by a fabricated SLOT the real archive
    // does not carry) is trusted verbatim with `trust_index == true`
    // (backlog #174) but re-derived with `false`: the real SLOT,
    // SIZE and `_mtime_` come back.
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
    // Backlog #199 fix round 1: real's fast path reuses a stanza
    // only when the stat fields agree *and* every `minimum_keys`
    // entry is present (`bintree.py:1115-1126` -- `_mtime_`, `SIZE`,
    // then `if not minimum_keys.difference(d)`). A stanza whose
    // `_mtime_`/`SIZE` still match the live file but which lacks a
    // minimum key (no `EAPI` here) is therefore re-parsed under
    // `FEATURES=-pkgdir-index-trusted` (`trust_index == false`,
    // real `_populate_local(reindex=True)`): the bogus `SLOT` the
    // stanza claims gives way to the archive's real one. In trusted
    // mode the same stanza is still vouched verbatim -- the fast
    // path now misses, but the stale-vouch arm (backlog #174)
    // reuses it byte-identically, so the trusted default is
    // unchanged by the added check.
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

#[test]
fn populate_local_pkgdir_reindex_drops_a_removed_files_index_stanza() {
    // Backlog #199: real `_populate_local(reindex=True)` deletes
    // every index stanza with no live file from the pool
    // (`bintree.py:1353-1356` -- `del metadata[instance_key]` for an
    // `instance_key not in pkg_paths`). A stanza whose file was
    // removed is re-injected verbatim with `trust_index == true`
    // (backlog #187) but absent with `false`, while the surviving
    // file's own entry is identical either way. The survivor's
    // stanza carries every `minimum_keys` entry (with live
    // `_mtime_`/`SIZE`), so the fast path vouches it verbatim in
    // both modes -- the equality below pins that, not a re-parse
    // that merely happens to agree.
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

    let trusted = populate_local_pkgdir(pkgdir, true).expect("scan succeeds");
    assert_eq!(trusted.len(), 2, "{trusted:?}");

    let reindexed = populate_local_pkgdir(pkgdir, false).expect("scan succeeds");
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
    // Backlog #199: real `_populate_local(reindex=True)` parses every
    // file the fast path did not match (`bintree.py:1179-1351`), so a
    // file no stanza covers joins the pool -- and portuale's walk
    // runs in trusted mode too (unlike real's `reindex=False`, which
    // never walks; kept so a `Packages`-less `$PKGDIR` still
    // resolves), so an unindexed file parses identically either way.
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
        let entries = populate_local_pkgdir(pkgdir, trust).expect("scan succeeds");
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
