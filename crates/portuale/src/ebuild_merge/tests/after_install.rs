use super::*;

/// Backlog #224: real `dblink.unmerge(needed=...)` feeds the
/// replacing package's `NEEDED.ELF.2` lines into the replace-loop
/// prune explicitly (`_prune_plib_registry` ->
/// `_linkmap_rebuild(include_file=needed, ...)`), because the new
/// entry still sits in its `-MERGING-<pf>` temporary and
/// enumeration (`cpv_all()`) skips it. A preserved library whose
/// only remaining consumer is the replacing package itself must
/// therefore survive the replace-loop prune; without the feed it
/// looks orphaned and is deleted + unregistered.
///
/// Same seed shape as
/// `replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer`
/// (hand-written `NEEDED.ELF.2` lines, per the brief's "or a
/// hand-written NEEDED line"), except the replacing entry is
/// `-MERGING-newapp-1.0` -- the real replace-loop state since #183,
/// invisible to `read_all_needed_entries` -- and the call goes
/// through the replace loop itself (`unmerge_replaced_same_slot`,
/// whose signature is unchanged: it reads the feed from that
/// temporary entry), so this test compiles and fails without the
/// fix and passes with it.
#[test]
fn replace_loop_prune_sees_the_replacing_packages_merging_needed() {
    let tmp = tempdir();
    let (root, _) = seed_old_owned_consumer_root(&tmp);
    // The replacing package's vdb entry, exactly as the replace
    // loop sees it: still `-MERGING-`, so only the explicit
    // include feed (never enumeration) can show its consumer to
    // the prune. The `CONTENTS` entry is what the `also_keep`
    // ownership check sees (via the `-MERGING-` fallback).
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
