use super::*;

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
