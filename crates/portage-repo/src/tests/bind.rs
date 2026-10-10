use super::*;

/// `apply_prune_rebuilds` latches the flag, clears the replace set, and drops
/// only the built steers; other masks and search state survive.
#[test]
fn slot_operator_prune_rebuilds_clears_replace_and_built_masks() {
    let mut params = BacktrackParams::default();
    params
        .slot_operator_replace_installed
        .insert(("app-misc".to_string(), "pcons".to_string()));
    params
        .slot_operator_undone
        .insert(("app-misc".to_string(), "oldcons".to_string()));
    params.runtime_pkg_mask.insert(
        ("dev-libs".to_string(), "bttarget".to_string()),
        vec![MaskEntry {
            neg: "!=dev-libs/bttarget-2.0".to_string(),
            reason: MaskReason::SlotConflict { parents: vec![] },
        }],
    );
    params.runtime_pkg_mask.insert(
        ("dev-libs".to_string(), "somaskchild".to_string()),
        vec![MaskEntry {
            neg: "!=dev-libs/somaskchild-1.0[binary]".to_string(),
            reason: MaskReason::SlotOperatorBuilt,
        }],
    );
    params.mask_order = vec![
        (
            ("dev-libs".to_string(), "bttarget".to_string()),
            "!=dev-libs/bttarget-2.0".to_string(),
        ),
        (
            ("dev-libs".to_string(), "somaskchild".to_string()),
            "!=dev-libs/somaskchild-1.0[binary]".to_string(),
        ),
    ];
    apply_prune_rebuilds(&mut params);
    assert!(params.prune_rebuilds);
    assert!(params.slot_operator_replace_installed.is_empty());
    // The undone latch is not real state -- the prune does not touch it.
    assert!(
        params
            .slot_operator_undone
            .contains(&("app-misc".to_string(), "oldcons".to_string()))
    );
    // The conflict mask survives; the built steer is gone with its order slot.
    assert_eq!(params.runtime_pkg_mask.len(), 1);
    assert!(
        params
            .runtime_pkg_mask
            .contains_key(&("dev-libs".to_string(), "bttarget".to_string()))
    );
    assert_eq!(
        params.mask_order,
        vec![(
            ("dev-libs".to_string(), "bttarget".to_string()),
            "!=dev-libs/bttarget-2.0".to_string()
        )]
    );
}

/// One token of the S4 binder: another package never binds, graph entries beat
/// the installed fallback, and the greatest same-cp version wins.
#[test]
fn bind_slot_operator_token_prefers_graph_entries_then_installed() {
    let dir = slotundo_temp_dir("161-bind");
    let d = dir.join("var/db/pkg/dev-libs/bar-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0/3\n").unwrap();
    let new_entry = |package: &str, version: &str, slot: &str, sub: &str| GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: package.into(),
        outcome: PretendOutcome::New {
            version: version.into(),
        },
        slot: Some(slot.into()),
        sub_slot: Some(sub.into()),
        ..graph_entry("dev-libs", package, version)
    };
    assert_eq!(
        bind_slot_operator_token("dev-libs/bar:=", &[new_entry("baz", "2.0", "9", "9")], &dir),
        "dev-libs/bar:0/3="
    );
    assert_eq!(
        bind_slot_operator_token("dev-libs/bar:=", &[new_entry("bar", "2.0", "5", "5")], &dir),
        "dev-libs/bar:5/5="
    );
    assert_eq!(
        bind_slot_operator_token(
            "dev-libs/bar:=",
            &[
                new_entry("bar", "2.0", "2", "2"),
                new_entry("bar", "1.0", "0", "1")
            ],
            &dir
        ),
        "dev-libs/bar:2/2="
    );
    let reinstall = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Reinstall {
            version: "2.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("5".into()),
        sub_slot: Some("5".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    assert_eq!(
        bind_slot_operator_token("dev-libs/bar:=", &[reinstall], &dir),
        "dev-libs/bar:5/5="
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bind_slot_operator_deps_binds_against_entries_then_installed() {
    let dir = slotundo_temp_dir("bind");
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souprov",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    let provider = slotundo_provider_entry();

    // Merge-bound entry wins (Portage models the post-merge vdb); a stale
    // `:0/1=` falls back to installed (Portage enforces the built sub-slot).
    assert_eq!(
        bind_slot_operator_deps(
            "dev-libs/souprov:= dev-libs/souprov:0/1=",
            std::slice::from_ref(&provider),
            &dir,
        ),
        "dev-libs/souprov:0/2= dev-libs/souprov:0/1="
    );
    assert_eq!(
        bind_slot_operator_deps("dev-libs/souprov:=", &[], &dir),
        "dev-libs/souprov:0/1="
    );
    // Left as-is when nothing satisfies the constraint (keeps the vdb information).
    assert_eq!(
        bind_slot_operator_deps(
            ">=dev-libs/souprov-3:=",
            std::slice::from_ref(&provider),
            &dir,
        ),
        ">=dev-libs/souprov-3:="
    );
    assert_eq!(
        bind_slot_operator_deps("dev-libs/souprov ( || )", &[], &dir),
        "dev-libs/souprov ( || )"
    );
    let _ = fs::remove_dir_all(&dir);
}
