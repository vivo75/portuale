use super::*;

/// Real `unregister` (`register(cpv, slot, counter, [])`): removes
/// the `cps` entry only when it still records the *same* `cpv` and
/// `counter` -- a different package (or a stale counter) sharing the
/// same `category/pn:slot` key must survive untouched.
#[test]
fn register_preserved_libs_unregister_only_matches_the_same_cpv_and_counter() {
    let mut registry = PlibRegistry {
        entries: BTreeMap::new(),
        orig_entries: BTreeMap::new(),
    };
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-1.0",
        "dev-libs",
        "foo",
        "0",
        "5",
        &["/usr/lib/libfoo.so.1".to_string()],
    );
    assert!(registry.entries.contains_key("dev-libs/foo:0"));

    // Wrong counter: real `unregister` must leave the entry alone.
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-1.0",
        "dev-libs",
        "foo",
        "0",
        "9",
        &[],
    );
    assert!(
        registry.entries.contains_key("dev-libs/foo:0"),
        "a stale counter must not unregister someone else's live entry"
    );

    // Wrong cpv (a different version currently holding this slot):
    // same real "leave it alone" rule.
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-2.0",
        "dev-libs",
        "foo",
        "0",
        "5",
        &[],
    );
    assert!(
        registry.entries.contains_key("dev-libs/foo:0"),
        "a different cpv must not unregister someone else's live entry"
    );

    // Matching cpv and counter: real `unregister` removes it.
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-1.0",
        "dev-libs",
        "foo",
        "0",
        "5",
        &[],
    );
    assert!(!registry.entries.contains_key("dev-libs/foo:0"));
}

/// Real `register` with non-empty `paths`: unconditionally overwrites
/// whatever the `cps` key already held, even a different package's
/// own entry -- no same-cpv/counter guard the way empty-`paths`
/// (`unregister`) has.
#[test]
fn register_preserved_libs_with_paths_unconditionally_overwrites() {
    let mut registry = PlibRegistry {
        entries: BTreeMap::new(),
        orig_entries: BTreeMap::new(),
    };
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-1.0",
        "dev-libs",
        "foo",
        "0",
        "5",
        &["/usr/lib/libfoo.so.1".to_string()],
    );
    register_preserved_libs(
        &mut registry,
        "dev-libs/foo-2.0",
        "dev-libs",
        "foo",
        "0",
        "6",
        &["/usr/lib/libfoo.so.2".to_string()],
    );

    let (cpv, counter, paths) = registry.entries.get("dev-libs/foo:0").unwrap();
    assert_eq!(cpv, "dev-libs/foo-2.0");
    assert_eq!(counter, "6");
    assert_eq!(paths, &["/usr/lib/libfoo.so.2".to_string()]);
}

/// Backlog #229 (merge-side half of #224): real `dblink.treewalk`
/// feeds the replacing package's build-info `NEEDED.ELF.2` into the
/// merge-side preserve computation explicitly
/// (`self._linkmap_rebuild(include_file=needed)` before
/// `_find_libs_to_preserve()`), because the new entry still sits in
/// its `-MERGING-<pf>` temporary and enumeration (`cpv_all()`)
/// skips it. A library owned by the replaced instance whose only
/// consumer is the replacing package itself must therefore be
/// preserved merge-side; without the feed the consumer is invisible
/// and the lib is never preserved (missed-preserve, so the file is
/// unmerged with the old instance).
///
/// Same hand-written-`NEEDED.ELF.2` seed shape as the #224 test,
/// except the provider is the replaced same-slot instance itself
/// (`oldapp-1.0`, live) and the only consumer (`/usr/bin/newtool`,
/// shipped by the new image) is known solely to the
/// `-MERGING-oldapp-2.0` temporary -- the real merge-side state
/// since #183 (`populate_vdb_tmp` runs before the preserve block).
/// The call goes through `find_preserve_paths_for_merge` itself
/// (new `new_pf` parameter), so this test compiles and fails
/// without the fix and passes with it.
#[test]
fn merge_side_preserve_sees_the_replacing_packages_merging_needed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/libold.so.1"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/newtool"), b"fake elf").unwrap();
    // The replaced same-slot instance, live: owns the library.
    let vdb_old = root.join("var/db/pkg/dev-libs/oldapp-1.0");
    std::fs::create_dir_all(&vdb_old).unwrap();
    std::fs::write(vdb_old.join("CONTENTS"), "obj /usr/lib/libold.so.1 def 1\n").unwrap();
    std::fs::write(
        vdb_old.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/libold.so.1;libold.so.1;;\n",
    )
    .unwrap();
    std::fs::write(vdb_old.join("SLOT"), "0\n").unwrap();
    std::fs::write(vdb_old.join("COUNTER"), "7\n").unwrap();
    // The replacing package, exactly as the merge-side preserve
    // block sees it: still `-MERGING-`, so only the explicit
    // include feed (never enumeration) can show its consumer to
    // the preserve computation.
    let vdb_new = root.join("var/db/pkg/dev-libs/-MERGING-oldapp-2.0");
    std::fs::create_dir_all(&vdb_new).unwrap();
    std::fs::write(
        vdb_new.join("NEEDED.ELF.2"),
        "X86_64;/usr/bin/newtool;;;libold.so.1\n",
    )
    .unwrap();
    // The new image ships the consumer but not the library.
    let new_image_paths: BTreeSet<String> = ["/usr/bin/newtool".to_string()].into_iter().collect();

    let (paths, _) = find_preserve_paths_for_merge(
        &root,
        "dev-libs",
        "oldapp",
        "oldapp-2.0",
        "0",
        &new_image_paths,
    )
    .expect("a same-slot instance owning files preserves");
    assert!(
        paths.contains("/usr/lib/libold.so.1"),
        "the old-owned lib consumed only by the replacing package must be preserved: {paths:?}"
    );
}

/// Real `removeFromContents`'s own `NEEDED`-line stripping
/// (`vartree.py:1279-1310`): a `NEEDED.ELF.2` entry for a path this
/// call actually removed from `CONTENTS` is dropped too, while an
/// entry for a path that's still owned survives untouched -- real
/// stale-linkage-data prevention for a *later* `LinkageMap.
/// rebuild()`'s own preserve-libs decision (see `remove_from_
/// contents`'s own doc comment).
#[test]
fn remove_from_contents_prunes_the_matching_needed_elf2_entry_too() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(
        vdb_dir.join("CONTENTS"),
        "obj /usr/lib/a.so abc123 100\nobj /usr/lib/b.so def456 100\n",
    )
    .unwrap();
    std::fs::write(
        vdb_dir.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/a.so;liba.so.1;;\nX86_64;/usr/lib/b.so;libb.so.1;;\n",
    )
    .unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/a.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    let contents = std::fs::read_to_string(vdb_dir.join("CONTENTS")).unwrap();
    assert!(!contents.contains("/usr/lib/a.so"));
    assert!(contents.contains("/usr/lib/b.so"));

    let needed = std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap();
    assert!(
        !needed.contains("/usr/lib/a.so"),
        "the entry for the removed path must be pruned: {needed}"
    );
    assert!(
        needed.contains("/usr/lib/b.so"),
        "the entry for the still-owned path must survive: {needed}"
    );
}

/// Real `if new_needed is not None:` (`writeContentsToContentsFile`):
/// when this package never had a `NEEDED.ELF.2` at all, nothing is
/// written -- no file is conjured into existence just because a
/// `CONTENTS` entry happened to be removed.
#[test]
fn remove_from_contents_does_not_create_a_needed_elf2_file_that_never_existed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/a.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    assert!(!vdb_dir.join("NEEDED.ELF.2").exists());
}

/// Real `if removed:` (`vartree.py:1279`): when none of the given
/// `paths` actually matched a real `CONTENTS` entry, `NEEDED.ELF.2`
/// isn't even read, let alone rewritten -- untouched, byte for byte.
#[test]
fn remove_from_contents_leaves_needed_elf2_untouched_when_nothing_was_removed() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb_dir = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb_dir).unwrap();
    std::fs::write(vdb_dir.join("CONTENTS"), "obj /usr/lib/a.so abc123 100\n").unwrap();
    let original_needed = "X86_64;/usr/lib/a.so;liba.so.1;;\n";
    std::fs::write(vdb_dir.join("NEEDED.ELF.2"), original_needed).unwrap();

    let mut paths = BTreeSet::new();
    paths.insert("/usr/lib/nonexistent.so".to_string());
    remove_from_contents(&root, "dev-libs/foo-1.0", &paths).expect("remove_from_contents succeeds");

    assert_eq!(
        std::fs::read_to_string(vdb_dir.join("NEEDED.ELF.2")).unwrap(),
        original_needed
    );
}
