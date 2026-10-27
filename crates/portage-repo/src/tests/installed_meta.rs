use super::*;

/// `live_metadata_for_installed` reads the vdb-recorded repo's copy, never the
/// priority winner's (Portage's `FakeVartree`).
#[test]
fn live_metadata_for_installed_prefers_the_recorded_repo() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portuale-live-meta");
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
        let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
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

    // No `repository` file: the priority search is the fallback.
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
    // The built-`:=` append is EAPI-gated like Portage (`FakeVartree`); a vdb
    // without `EAPI` reads as `"0"`, without `:=` support.
    // --dynamic-deps=n and Raw collapse to the vdb record.
    let root = fixtures_root();
    let live_slotop: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/slotoptarget:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    // slotopdepspkg's vdb RDEPEND is the built `...:2=` form: the live value
    // stands.
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "slotopdepspkg",
        version: "1.0",
        live: Some(&live_slotop),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "dev-libs/slotoptarget:=");

    // revdepslotconsumer's vdb has no `EAPI` file: Effective is the live value
    // (empty here), never the vdb binding.
    let live_empty: HashMap<String, String> = [("EAPI".to_string(), "8".to_string())]
        .into_iter()
        .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "revdepslotconsumer",
        version: "1.0",
        live: Some(&live_empty),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "");
    assert!(memo.contains_key(&(
        "dev-libs".to_string(),
        "revdepslotconsumer".to_string(),
        "1.0".to_string(),
        "RDEPEND".to_string()
    )));

    // The live value stands alone over a same-cp vdb binding.
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
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "revdepslotconsumer",
        version: "1.0",
        live: Some(&live_target),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "dev-libs/revdepslottarget:=");

    // =n: the raw snapshot, byte-for-byte.
    let mut memo = HashMap::new();
    let raw = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: false,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "revdepslotconsumer",
        version: "1.0",
        live: Some(&live_target),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Raw,
        memo: &mut memo,
    });
    assert_eq!(raw, "dev-libs/revdepslottarget:0/1=");

    // Missing live metadata: the raw record stands.
    let mut memo = HashMap::new();
    let fallback = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "revdepslotconsumer",
        version: "1.0",
        live: None,
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(fallback, "dev-libs/revdepslottarget:0/1=");
}

#[test]
fn installed_dep_string_effective_appends_built_atoms_when_eapi_gated() {
    // Both vdb rows carry `EAPI` files (`8`), so the recorded bound atoms
    // append to the live depstring (Portage's `FakeVartree`).
    let root = fixtures_root();
    // r25mid: the recorded bound form appends to the live `:=`.
    let live_mid: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "r25mid",
        version: "1.0",
        live: Some(&live_mid),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "dev-libs/r25lib:= >=dev-libs/r25lib-1.0:0/1=");

    // r25consumer likewise appends its bound form.
    let live_consumer: HashMap<String, String> = [
        ("RDEPEND".to_string(), "<dev-libs/r25lib-2.0:=".to_string()),
        ("EAPI".to_string(), "8".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "r25consumer",
        version: "1.0",
        live: Some(&live_consumer),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "<dev-libs/r25lib-2.0:= <dev-libs/r25lib-2.0:0/1=");

    // `--ignore-built-slot-operator-deps` suppresses the append (Portage's
    // `FakeVartree`).
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: true,
        category: "dev-libs",
        package: "r25mid",
        version: "1.0",
        live: Some(&live_mid),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, "dev-libs/r25lib:=");

    // A live EAPI without `:=` support voids the whole overlay: the raw
    // record stands.
    let live_old_eapi: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "4".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "r25mid",
        version: "1.0",
        live: Some(&live_old_eapi),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, ">=dev-libs/r25lib-1.0:0/1=");

    // Unsupported live EAPI: raw stands (Portage's `_DynamicDepsNotApplicable`).
    let live_bad_eapi: HashMap<String, String> = [
        ("RDEPEND".to_string(), "dev-libs/r25lib:=".to_string()),
        ("EAPI".to_string(), "99".to_string()),
    ]
    .into_iter()
    .collect();
    let mut memo = HashMap::new();
    let eff = installed_dep_string(InstalledDepStringArgs {
        root: &root,
        dynamic_deps: true,
        ignore_built_slot_operator_deps: false,
        category: "dev-libs",
        package: "r25mid",
        version: "1.0",
        live: Some(&live_bad_eapi),
        key: "RDEPEND",
        layer: InstalledMetaLayer::Effective,
        memo: &mut memo,
    });
    assert_eq!(eff, ">=dev-libs/r25lib-1.0:0/1=");
}
