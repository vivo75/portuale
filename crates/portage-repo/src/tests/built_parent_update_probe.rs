use super::*;

/// An installed `dev-libs/mbabilib-1.0` whose vdb `SLOT` is `slot`
/// (`main/sub`); the fixture tree has `mbabilib-2.0` in `0/2`.
fn mbabilib_vdb(slot: &str) -> PathBuf {
    let dir = slotundo_temp_dir("built-parent");
    let d = dir.join("var/db/pkg/dev-libs/mbabilib-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
    fs::write(d.join("repository"), "testrepo\n").unwrap();
    dir
}

/// The fixture `dev-libs/mbabicons-1.0` binary, built against `mbabilib:0/1`.
fn mbabicons_binary() -> GraphEntry {
    GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::New {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        source: CandidateSource::Binary,
        ..graph_entry("dev-libs", "mbabicons", "1.0")
    }
}

#[test]
fn built_parent_update_probe_masks_a_binary_whose_installed_child_has_a_tree_update() {
    // Portage: _slot_operator_update_probe finds the tree mbabilib-2.0:0/2 that the
    // ebuild replacement mbabicons-1.0 accepts (`dev-libs/mbabilib:=`), so
    // _slot_operator_update_backtrack masks the binary and seeds the installed child.
    let root = mbabilib_vdb("0/1");
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let local = BinaryIndex::from_pkgdir(&fixtures_root().join("pkgdir"));
    let entries = [mbabicons_binary()];
    let (replace, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: &root,
        repos: &repos,
        config: &test_config(),
        local_binpkg: &local,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        masked: &HashMap::new(),
    });
    assert_eq!(
        replace,
        BTreeSet::from([("dev-libs".to_string(), "mbabilib".to_string())]),
        "the installed child joins the replace set"
    );
    assert_eq!(
        masks,
        BTreeSet::from([(
            "dev-libs".to_string(),
            "mbabicons".to_string(),
            "1.0".to_string()
        )]),
        "the stale binary parent is masked, not the tree ebuild"
    );

    // Control: the installed sub-slot already matches the tree, so no update.
    let root_same = mbabilib_vdb("0/2");
    let (replace, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: &root_same,
        repos: &repos,
        config: &test_config(),
        local_binpkg: &local,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        masked: &HashMap::new(),
    });
    assert!(
        replace.is_empty() && masks.is_empty(),
        "no sub-slot change, no mask"
    );

    // A child the graph merges itself is not the installed instance the binary bound.
    let mut merged = entries.to_vec();
    merged.push(GraphEntry {
        outcome: PretendOutcome::New {
            version: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "mbabilib", "2.0")
    });
    let (replace, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: &root,
        repos: &repos,
        config: &test_config(),
        local_binpkg: &local,
        entries: &merged,
        with_bdeps: false,
        excluded: &[],
        masked: &HashMap::new(),
    });
    assert!(
        replace.is_empty() && masks.is_empty(),
        "a merged child is left alone"
    );

    // An excluded installed child refuses the probe.
    let excluded = ["dev-libs/mbabilib".to_string()];
    let (replace, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: &root,
        repos: &repos,
        config: &test_config(),
        local_binpkg: &local,
        entries: &entries,
        with_bdeps: false,
        excluded: &excluded,
        masked: &HashMap::new(),
    });
    assert!(
        replace.is_empty() && masks.is_empty(),
        "an excluded child refuses the probe"
    );
}

#[test]
fn built_parent_update_probe_ignores_a_tree_update_the_search_has_masked() {
    // Portage: `_select_package` honours `runtime_pkg_mask`, so a masked
    // mbabilib-2.0 is no update and the binary stays.
    let root = mbabilib_vdb("0/1");
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let local = BinaryIndex::from_pkgdir(&fixtures_root().join("pkgdir"));
    let entries = [mbabicons_binary()];
    let masked = HashMap::from([(
        ("dev-libs".to_string(), "mbabilib".to_string()),
        vec![MaskEntry {
            neg: "!=dev-libs/mbabilib-2.0".to_string(),
            reason: MaskReason::SlotConflict { parents: vec![] },
        }],
    )]);
    let (replace, masks) = built_parent_update_probe(BuiltParentUpdateProbeArgs {
        root: &root,
        repos: &repos,
        config: &test_config(),
        local_binpkg: &local,
        entries: &entries,
        with_bdeps: false,
        excluded: &[],
        masked: &masked,
    });
    assert!(
        replace.is_empty() && masks.is_empty(),
        "a masked tree update does not mask the binary"
    );
}
