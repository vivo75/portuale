use super::*;

/// Backlog #86 S1: `live_metadata_for_installed` reads the
/// vdb-recorded repo's copy of the exact installed cpv, never the
/// priority winner's -- real `FakeVartree._aux_get_wrapper`
/// (`FakeVartree.py:136`, `myrepo=pkg.repo`).
#[test]
fn live_metadata_for_installed_prefers_the_recorded_repo() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portuale-live-meta").keep();
    // Two scratch repos, same cpv, different RDEPEND.
    for (repo, rdepend) in [("lowrepo", ""), ("highrepo", "dev-libs/blocked")] {
        let dir = base.join(repo).join("dev-libs").join("twounit");
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            "EAPI=8\nDESCRIPTION=\"scratch recorded-repo unit\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n"
                .to_string();
        if !rdepend.is_empty() {
            writeln!(body, "RDEPEND=\"{rdepend}\"").unwrap();
        }
        std::fs::write(dir.join("twounit-1.0.ebuild"), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry =
            "DEFINED_PHASES=-\nDESCRIPTION=scratch recorded-repo unit\nEAPI=8\n".to_string();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "KEYWORDS=amd64\nSLOT=0\n_md5_={md5}").unwrap();
        let cachedir = base.join(repo).join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join("twounit-1.0"), entry).unwrap();
    }
    let repo_config = |name: &str, priority: i32| RepoConfig {
        name: name.to_string(),
        location: base.join(name),
        priority,
        is_main: name == "lowrepo",
        masters: vec![],
        profile_formats: vec![],
        cache_formats: vec![],
        aliases: vec![],
        sync_type: None,
        sync_uri: None,
        volatile: false,
        module_specific_options: vec![],
    };
    let repos = vec![repo_config("lowrepo", -1000), repo_config("highrepo", 10)];

    // Recorded repo wins over priority: the vdb says `lowrepo`.
    let root = tmp_vdb(
        "dev-libs",
        "twounit-1.0",
        &[("repository", b"lowrepo\n"), ("USE", b"\n")],
    );
    let meta = live_metadata_for_installed(&repos, &root, "dev-libs", "twounit", "1.0")
        .expect("recorded repo carries the version");
    assert_eq!(meta.get("RDEPEND").map(String::as_str), None);
    let _ = std::fs::remove_dir_all(&root);

    // No `repository` file: the pre-#86 priority search is the
    // fallback, so minimal fixture vdbs keep their live view.
    let root = tmp_vdb("dev-libs", "twounit-1.0", &[("USE", b"\n")]);
    let meta = live_metadata_for_installed(&repos, &root, "dev-libs", "twounit", "1.0")
        .expect("fallback search finds the version");
    assert_eq!(
        meta.get("RDEPEND").map(String::as_str),
        Some("dev-libs/blocked")
    );
    let _ = std::fs::remove_dir_all(&root);

    // Recorded repo no longer configured: same fallback.
    let root = tmp_vdb(
        "dev-libs",
        "twounit-1.0",
        &[("repository", b"gone-overlay\n"), ("USE", b"\n")],
    );
    let meta = live_metadata_for_installed(&repos, &root, "dev-libs", "twounit", "1.0")
        .expect("fallback search finds the version");
    assert_eq!(
        meta.get("RDEPEND").map(String::as_str),
        Some("dev-libs/blocked")
    );
    let _ = std::fs::remove_dir_all(&root);

    // Gone from every repo: `None` (the caller's Raw fallback).
    assert!(live_metadata_for_installed(&repos, &root, "dev-libs", "twounit", "9.9").is_none());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn installed_dep_string_effective_is_the_live_ebuild_value() {
    // Backlog #25 S1b: the built-`:=` append is EAPI-gated exactly
    // like real (`FakeVartree._apply_dynamic_deps`). These two
    // fixtures' vdbs carry no `EAPI` file, so real reads the
    // installed EAPI as `"0"` (`vartree.py:1054-1055`) -- no
    // `slot_operator` support -- and the live value stands even
    // though the vdb carries a built binding. (Without the gate this
    // append broke both #24 pins, Phase 5 S1 S0.)
    // --dynamic-deps=n and Raw collapse to the vdb record;
    // the no-live-metadata arm is real's `_DynamicDepsNotApplicable`
    // fallback (raw + global moves, identity in fixtures).
    // Every live map below carries `EAPI`, like real's `portdb`
    // `aux_get` always does -- a missing live `EAPI` is unsupported
    // and takes the raw fallback (unreachable in real).
    let root = fixtures_root();
    let live_slotop: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/slotoptarget:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    // slotopdepspkg's vdb RDEPEND is `dev-libs/slotoptarget:2=` --
    // the live value stands.
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "slotopdepspkg",
        "1.0",
        Some(&live_slotop),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "dev-libs/slotoptarget:=");

    // revdepslotconsumer's vdb RDEPEND is
    // `dev-libs/revdepslottarget:0/1=` -- built, but its vdb has no
    // `EAPI` file: Effective is the live value (empty here), never
    // the vdb binding.
    let live_empty: HashMap<String, String> = [("EAPI".to_string(), "8".to_string())]
        .into_iter()
        .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "revdepslotconsumer",
        "1.0",
        Some(&live_empty),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "");
    assert!(memo.contains_key(&(
        "dev-libs".to_string(),
        "revdepslotconsumer".to_string(),
        "1.0".to_string(),
        "RDEPEND".to_string()
    )));

    // The live value stands alone even when the vdb carries a built
    // binding for the same cp.
    let live_target: HashMap<String, String> = [
        (
            "RDEPEND".to_string(),
            "dev-libs/revdepslottarget:=".to_string(),
        ),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "revdepslotconsumer",
        "1.0",
        Some(&live_target),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "dev-libs/revdepslottarget:=");

    // =n: the raw snapshot, byte-for-byte.
    let mut memo = HashMap::new();
    let raw = installed_dep_string(
        &root,
        false,
        false,
        "dev-libs",
        "revdepslotconsumer",
        "1.0",
        Some(&live_target),
        "RDEPEND",
        InstalledMetaLayer::Raw,
        &mut memo,
    );
    assert_eq!(raw, "dev-libs/revdepslottarget:0/1=");

    // Missing live metadata: raw record stands (global moves are
    // identity without `profiles/updates` commands).
    let mut memo = HashMap::new();
    let fallback = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "revdepslotconsumer",
        "1.0",
        None,
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(fallback, "dev-libs/revdepslottarget:0/1=");
}

#[test]
fn installed_dep_string_effective_appends_built_atoms_when_eapi_gated() {
    // Backlog #25 S1b: real `FakeVartree._apply_dynamic_deps`
    // (`lib/_emerge/FakeVartree.py:146-191`) on the hermetic r25
    // shape (findings/l0.md "## #25 S0 (2026-09-27)"): both vdb rows
    // carry `EAPI` files (`8`), so the recorded bound atoms append
    // to the live depstring -- `Depstring: dev-libs/r25lib:=
    // >=dev-libs/r25lib-1.0:0/1=` (`realC-debug.log:35-47`).
    let root = fixtures_root();
    // r25mid: live `dev-libs/r25lib:=`, vdb
    // `>=dev-libs/r25lib-1.0:0/1=` (real's recorded bound form).
    let live_mid: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "r25mid",
        "1.0",
        Some(&live_mid),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "dev-libs/r25lib:= >=dev-libs/r25lib-1.0:0/1=");

    // r25consumer: live `<dev-libs/r25lib-2.0:=`, vdb
    // `<dev-libs/r25lib-2.0:0/1=`.
    let live_consumer: HashMap<String, String> = [
        ("RDEPEND".to_string(), "<dev-libs/r25lib-2.0:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "r25consumer",
        "1.0",
        Some(&live_consumer),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "<dev-libs/r25lib-2.0:= <dev-libs/r25lib-2.0:0/1=");

    // `--ignore-built-slot-operator-deps`: append suppressed, live
    // stands (real `FakeVartree.py:166-168`).
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        true,
        "dev-libs",
        "r25mid",
        "1.0",
        Some(&live_mid),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, "dev-libs/r25lib:=");

    // Live EAPI without `:=` support voids the whole overlay, not
    // just the append (real `FakeVartree.py:176-178`): the raw
    // record stands.
    let live_old_eapi: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "4".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "r25mid",
        "1.0",
        Some(&live_old_eapi),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, ">=dev-libs/r25lib-1.0:0/1=");

    // Unsupported live EAPI: `_DynamicDepsNotApplicable`, raw stands
    // (real `FakeVartree.py:158-162`).
    let live_bad_eapi: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "99".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(
        &root,
        true,
        false,
        "dev-libs",
        "r25mid",
        "1.0",
        Some(&live_bad_eapi),
        "RDEPEND",
        InstalledMetaLayer::Effective,
        &mut memo,
    );
    assert_eq!(eff, ">=dev-libs/r25lib-1.0:0/1=");
}
