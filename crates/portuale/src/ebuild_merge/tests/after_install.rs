use super::*;

/// Same-version reinstall: the replace loop names the live same-`pf`
/// instance replaced (its `pkg_prerm`/`pkg_postrm` run from its own
/// saved env in the real merge; here the entry carries none, so the
/// hook runner skips both without spawning a shell), while the
/// just-merged files survive: `also_keep=[new_pf]` still owns every
/// path, so `remove_contents` deletes nothing.
/// Portage: vartree.py, dblink.treewalk (the replaced same-cpv dblink
/// is unmerged; `others_in_slot` keeps its files).
#[test]
fn replace_loop_unmerges_the_live_same_pf_instance_on_reinstall() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/share/sameapp")).unwrap();
    std::fs::write(root.join("usr/share/sameapp/payload.txt"), b"new bytes").unwrap();
    let vdb = root.join("var/db/pkg/app-misc/sameapp-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(
        vdb.join("CONTENTS"),
        "obj /usr/share/sameapp/payload.txt abc 1\n",
    )
    .unwrap();
    std::fs::write(vdb.join("SLOT"), "0\n").unwrap();
    std::fs::write(vdb.join("COUNTER"), "3\n").unwrap();

    let scratch = tmp.join("scratch");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let replaced = unmerge_replaced_same_slot(UnmergeReplacedSameSlotArgs {
        root: &root,
        category: "app-misc",
        package: "sameapp",
        new_pf: "sameapp-1.0",
        main_slot: "0",
        scratch_dir: &scratch,
        portage_tmpdir: &portage_tmpdir,
        options: &MergeOptions::default(),
        replacement_preserved: &BTreeMap::new(),
    })
    .expect("the replace loop succeeds");
    assert_eq!(
        replaced,
        vec!["sameapp-1.0".to_string()],
        "a reinstall must unmerge the same-PF instance"
    );
    assert!(
        root.join("usr/share/sameapp/payload.txt").is_file(),
        "the just-merged files must survive the replace loop"
    );
}

/// Replace-loop prune sees the replacing package's `-MERGING-` NEEDED feed,
/// so a preserved lib consumed only by the replacing package survives.
/// Portage: `dblink.py`, `unmerge`.
#[test]
fn replace_loop_prune_sees_the_replacing_packages_merging_needed() {
    let tmp = tempdir();
    let (root, _) = seed_old_owned_consumer_root(&tmp);
    // The replacing entry is still `-MERGING-`, invisible to enumeration:
    // only the explicit include feed shows its consumer to the prune.
    let vdb_new = root.join("var/db/pkg/dev-libs/-MERGING-newapp-1.0");
    std::fs::create_dir_all(&vdb_new).unwrap();
    std::fs::write(vdb_new.join("CONTENTS"), "obj /usr/bin/cprog abc 1\n").unwrap();
    std::fs::write(
        vdb_new.join("NEEDED.ELF.2"),
        "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
    )
    .unwrap();
    std::fs::write(root.join("var/db/pkg/dev-libs/oldapp-1.0/SLOT"), "0\n").unwrap();

    let scratch = tmp.join("scratch");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();
    let replaced = unmerge_replaced_same_slot(UnmergeReplacedSameSlotArgs {
        root: &root,
        category: "dev-libs",
        package: "oldapp",
        new_pf: "newapp-1.0",
        main_slot: "0",
        scratch_dir: &scratch,
        portage_tmpdir: &portage_tmpdir,
        options: &MergeOptions::default(),
        replacement_preserved: &BTreeMap::new(),
    })
    .expect("the replace loop succeeds");
    assert_eq!(
        replaced,
        vec!["oldapp-1.0".to_string()],
        "the loop must have unmerged the old instance"
    );

    assert!(
        root.join("usr/lib/keepA.so").is_file(),
        "the preserved lib consumed only by the replacing package must survive the replace-loop prune"
    );
    assert!(
        root.join("usr/bin/cprog").is_file(),
        "the consumer owned by the replacing package must survive"
    );
    let registry = read_plib_registry(&root);
    assert_eq!(
        registry.entries.get("dev-libs/provider:0"),
        Some(&(
            "dev-libs/provider-1.0".to_string(),
            "4".to_string(),
            vec!["/usr/lib/keepA.so".to_string()],
        )),
        "the registry entry must survive the replace-loop prune"
    );
    assert!(
        vdb_new.join("NEEDED.ELF.2").is_file(),
        "the replace loop must leave the `-MERGING-` entry alone"
    );
}
