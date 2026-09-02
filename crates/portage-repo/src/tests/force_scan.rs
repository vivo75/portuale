use super::*;

#[test]
fn slot_operator_slot_change_probe_schedules_a_moved_installed_child() {
    // Bug 456208 shape: tree `RDEPEND=app-arch/libarchive:=` (unbuilt) against
    // installed `libarchive-3.1.1` (vdb `0`, tree `0/13`): Portage schedules it.
    let dir = slotundo_temp_dir("probe");
    let mk = |name: &str, slot: &str| {
        let d = dir.join("var/db/pkg/app-arch").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-arch\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
    };
    mk("libarchive-3.1.1", "0");
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let parent = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Reinstall {
            version: "4.10.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("kde-base", "ark", "4.10.0")
    };
    let child = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "3.1.1".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-arch", "libarchive", "3.1.1")
    };
    let child_cp = ("app-arch".to_string(), "libarchive".to_string());
    let empty = BTreeSet::new();

    let mut scheduled = BTreeSet::new();
    let mut masked: BTreeSet<(String, String, String)> = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        repos: &repos,
        entries: &[parent.clone(), child.clone()],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut masked,
        config: &test_config(),
    });
    assert_eq!(
        scheduled,
        BTreeSet::from([child_cp.clone()]),
        "the moved installed child joins the replace set (bug 456208)"
    );
    assert!(
        masked.is_empty(),
        "no binary entry is merged, so the #212 arm reports nothing"
    );

    // Resolved to a merge-bound entry (Portage's built dep): no probe.
    let mut merged_child = child.clone();
    merged_child.outcome = PretendOutcome::Reinstall {
        version: "3.1.1".into(),
        changed_flags: Vec::new(),
        deps_changed: false,
        slot_changed: false,
        rebuilt_binary: false,
        new_repo: false,
        slot_operator_rebuild: false,
    };
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        repos: &repos,
        entries: &[parent.clone(), merged_child],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
    });
    assert!(scheduled.is_empty());

    // The undo latch suppresses a re-add.
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        repos: &repos,
        entries: &[parent, child],
        undone: &BTreeSet::from([child_cp]),
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
    });
    assert!(scheduled.is_empty());

    // A child whose tree ebuild did not move is not scheduled.
    slotundo_vdb(SlotundoVdbArgs {
        dir: &dir,
        name: "souprov",
        version: "1.0",
        slot: "0/1",
        rdepend: "",
        installed_use: "",
    });
    let parent_souprov = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: true,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "souneedslot", "1.0")
    };
    let child_souprov = GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("1".into()),
        ..graph_entry("dev-libs", "souprov", "1.0")
    };
    let mut scheduled = BTreeSet::new();
    slot_operator_slot_change_probe(SlotOperatorSlotChangeProbeArgs {
        root: &dir,
        repos: &repos,
        entries: &[parent_souprov, child_souprov],
        undone: &empty,
        scheduled: &mut scheduled,
        excluded: &[],
        masked: &mut BTreeSet::new(),
        config: &test_config(),
    });
    assert!(scheduled.is_empty());

    let _ = fs::remove_dir_all(&dir);
}
