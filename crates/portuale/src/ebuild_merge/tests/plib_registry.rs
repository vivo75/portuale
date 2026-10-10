use super::*;

#[test]
fn plib_registry_round_trips_through_json() {
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/preservepkg-old:0".to_string(),
        (
            "dev-libs/preservepkg-old-1.0".to_string(),
            "5".to_string(),
            vec![
                "/usr/lib/preservedtest/libfoo.so.1".to_string(),
                "/usr/lib/preservedtest/libfoo.so".to_string(),
            ],
        ),
    );
    let tmp = tempdir();
    write_plib_registry(
        &tmp,
        &PlibRegistry {
            entries: entries.clone(),
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("write succeeds");

    let text = std::fs::read_to_string(plib_registry_path(&tmp)).unwrap();
    let parsed = parse_plib_registry(&text).expect("real json.dumps-shaped output parses back");
    assert_eq!(parsed, entries);
}

#[test]
fn read_plib_registry_degrades_gracefully_when_missing_or_corrupt() {
    let tmp = tempdir();
    // No file at all -- real load()'s own ENOENT -> {} degrade.
    assert!(read_plib_registry(&tmp).entries.is_empty());

    std::fs::create_dir_all(plib_registry_path(&tmp).parent().unwrap()).unwrap();
    std::fs::write(plib_registry_path(&tmp), b"not json at all").unwrap();
    assert!(read_plib_registry(&tmp).entries.is_empty());
}

#[test]
fn plib_inode_map_skips_paths_that_no_longer_exist_on_disk() {
    let tmp = tempdir();
    std::fs::create_dir_all(tmp.join("usr/lib")).unwrap();
    std::fs::write(tmp.join("usr/lib/real.so"), b"x").unwrap();

    let mut preserved = BTreeMap::new();
    preserved.insert(
        "dev-libs/foo-1.0".to_string(),
        vec![
            "/usr/lib/real.so".to_string(),
            "/usr/lib/gone.so".to_string(),
        ],
    );
    let map = plib_inode_map(&tmp, &preserved);
    assert_eq!(
        map.len(),
        1,
        "only the still-existing path gets an inode entry"
    );
    let meta = std::fs::symlink_metadata(tmp.join("usr/lib/real.so")).unwrap();
    assert_eq!(
        map.get(&(meta.dev(), meta.ino())),
        Some(&vec![(
            "dev-libs/foo-1.0".to_string(),
            "/usr/lib/real.so".to_string()
        )])
    );
}

/// S0 for backlog #167(a): real `PreservedLibsRegistry.store()`
/// (`3rdparty/portage/lib/portage/util/_dyn_libs/
/// PreservedLibsRegistry.py:107-108`) returns without writing when the
/// registry is unchanged (`self._data == self._data_orig`) -- a
/// merge/unmerge that preserves nothing must leave a packaged 0-byte
/// registry at 0 bytes (l32 C1/C3/C4 cells), not rewrite it.
#[test]
fn plib_registry_is_not_rewritten_when_nothing_is_preserved() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let reg_path = plib_registry_path(&root);
    std::fs::create_dir_all(reg_path.parent().unwrap()).unwrap();
    std::fs::write(&reg_path, b"").unwrap();
    let before = std::fs::metadata(&reg_path).unwrap();
    let before_mtime = before.modified().unwrap();

    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::write(root.join("usr/lib/plain.so"), b"x").unwrap();
    let vdb = root.join("var/db/pkg/dev-libs/plain-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("COUNTER"), "3\n").unwrap();
    let preserved = preserve_libs_on_unmerge(
        &root,
        "dev-libs",
        "plain",
        "plain-1.0",
        "0",
        "obj /usr/lib/plain.so abc 1\n",
        false,
    )
    .unwrap();
    assert!(preserved.is_empty());

    let registry = read_plib_registry(&root);
    unregister_preserved_libs(&root, "dev-libs/plain-1.0", registry, &BTreeMap::new()).unwrap();

    let removed =
        prune_unused_preserved_libs(&root, false, &|_| false, &[], &BTreeMap::new(), &[]).unwrap();
    assert!(removed.is_empty());

    let after = std::fs::metadata(&reg_path).unwrap();
    assert_eq!(after.len(), 0, "a 0-byte registry must stay 0 bytes");
    assert_eq!(
        after.modified().unwrap(),
        before_mtime,
        "the registry file must not be rewritten when nothing changed"
    );
}

/// Fix round 1 for review item 2 (backlog #178): real `store()`
/// returns early when the registry equals what was read
/// (`PreservedLibsRegistry.store`) -- a merge/unmerge with no
/// preserved-lib activity must leave an unrelated non-empty entry's
/// bytes **and** mtime untouched, not just the empty-file case
/// above. Pre-fix, every `preserve_libs_on_unmerge` /
/// `prune_unused_preserved_libs` call rewrote the file even when
/// `register()` was a no-op.
#[test]
fn plib_registry_with_an_unrelated_entry_is_not_rewritten_without_preserve_activity() {
    let tmp = tempdir();
    let root = tmp.join("root");
    // A live unrelated entry: the file exists (so `pruneNonExisting`
    // keeps it) and stays needed (an installed consumer links its
    // soname), so every step below is a genuine registry no-op.
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/lib/other.so"), b"fake elf").unwrap();
    std::fs::write(root.join("usr/bin/cprog"), b"fake elf").unwrap();
    let vdb_consumer = root.join("var/db/pkg/dev-libs/consumer-1.0");
    std::fs::create_dir_all(&vdb_consumer).unwrap();
    std::fs::write(vdb_consumer.join("COUNTER"), "1\n").unwrap();
    std::fs::write(
        vdb_consumer.join("CONTENTS"),
        "obj /usr/bin/cprog abc 1\nobj /usr/lib/other.so def 1\n",
    )
    .unwrap();
    std::fs::write(
        vdb_consumer.join("NEEDED.ELF.2"),
        "X86_64;/usr/lib/other.so;libother.so.1;;\nX86_64;/usr/bin/cprog;;;libother.so.1\n",
    )
    .unwrap();
    let seed = "{\n\t\"dev-libs/other:0\": [\n\t\t\"dev-libs/other-1.0\",\n\t\t\"9\",\n\t\t[\n\t\t\t\"/usr/lib/other.so\"\n\t\t]\n\t]\n}";
    std::fs::create_dir_all(plib_registry_path(&root).parent().unwrap()).unwrap();
    std::fs::write(plib_registry_path(&root), seed).unwrap();
    // A distinctly old mtime, so any rewrite is observable even at
    // coarse filesystem timestamp granularity.
    let touch = std::process::Command::new("touch")
        .args([
            "-d",
            "2001-02-03 04:05:06",
            &plib_registry_path(&root).display().to_string(),
        ])
        .status()
        .expect("touch sets the seed mtime");
    assert!(touch.success());
    let before_bytes = std::fs::read(plib_registry_path(&root)).unwrap();
    let before_mtime = std::fs::metadata(plib_registry_path(&root))
        .unwrap()
        .modified()
        .unwrap();

    // A package with no preserved-lib activity at all.
    let vdb = root.join("var/db/pkg/dev-libs/plain-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("COUNTER"), "3\n").unwrap();
    let preserved = preserve_libs_on_unmerge(
        &root,
        "dev-libs",
        "plain",
        "plain-1.0",
        "0",
        "obj /usr/lib/plain.so abc 1\n",
        false,
    )
    .unwrap();
    assert!(preserved.is_empty());
    let being: BTreeSet<String> = ["/usr/lib/plain.so".to_string()].into_iter().collect();
    let removed = prune_unused_preserved_libs(
        &root,
        true,
        &|p| being.contains(p),
        &[],
        &BTreeMap::new(),
        &[],
    )
    .unwrap();
    assert!(
        removed.is_empty(),
        "the still-needed unrelated entry must survive the prune"
    );

    assert_eq!(
        std::fs::read(plib_registry_path(&root)).unwrap(),
        before_bytes,
        "the registry bytes must not change without preserve activity"
    );
    assert_eq!(
        std::fs::metadata(plib_registry_path(&root))
            .unwrap()
            .modified()
            .unwrap(),
        before_mtime,
        "the registry mtime must not change without preserve activity"
    );
}

/// S0 for backlog #167(b): an empty registry real *does* write is
/// exactly `{}` (2 bytes, no trailing newline -- real
/// `json.dumps({}, indent="\t", sort_keys=True)`).
#[test]
fn plib_registry_empty_serializes_to_exactly_empty_braces() {
    let tmp = tempdir();
    // A loaded non-empty registry whose entries were all
    // unregistered: snapshot `orig_entries` the way real `load()`
    // snapshots `_data_orig`, so the now-empty `entries` compare
    // changed and real `store()` rewrites.
    let mut registry = PlibRegistry {
        entries: BTreeMap::new(),
        orig_entries: BTreeMap::new(),
    };
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &["/usr/lib/libfoo.so.1".to_string()],
    });
    registry.orig_entries = registry.entries.clone();
    register_preserved_libs(RegisterPreservedLibsArgs {
        registry: &mut registry,
        cpv: "dev-libs/foo-1.0",
        category: "dev-libs",
        pn: "foo",
        slot: "0",
        counter: "5",
        paths: &[],
    });
    assert!(registry.entries.is_empty());
    write_plib_registry(&tmp, &registry).expect("write succeeds");
    let bytes = std::fs::read(plib_registry_path(&tmp)).unwrap();
    assert_eq!(
        bytes, b"{}",
        "an empty registry is exactly `{{}}`, no trailing newline"
    );
}

/// S0 for backlog #167(c): a non-empty registry is serialized
/// byte-identical to real `json.dumps(data, ensure_ascii=False,
/// indent="\t", sort_keys=True)` (expected bytes below generated with
/// `python3 -c 'import json; print(repr(json.dumps({"b:0":
/// ("x-1.0", "1", []), "a:0": ("y-2.0", "3",
/// ["/usr/lib/lib\u00e9.so.1", "a\"b\\\\c"])}, ensure_ascii=False,
/// indent="\t", sort_keys=True)))'`).
#[test]
fn plib_registry_serialization_is_byte_identical_to_python_json_dumps() {
    let tmp = tempdir();
    let mut entries = BTreeMap::new();
    entries.insert(
        "b:0".to_string(),
        ("x-1.0".to_string(), "1".to_string(), Vec::new()),
    );
    entries.insert(
        "a:0".to_string(),
        (
            "y-2.0".to_string(),
            "3".to_string(),
            vec!["/usr/lib/lib\u{e9}.so.1".to_string(), "a\"b\\c".to_string()],
        ),
    );
    write_plib_registry(
        &tmp,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("write succeeds");
    let bytes = std::fs::read(plib_registry_path(&tmp)).unwrap();
    let expected = "{\n\t\"a:0\": [\n\t\t\"y-2.0\",\n\t\t\"3\",\n\t\t[\n\t\t\t\"/usr/lib/lib\u{e9}.so.1\",\n\t\t\t\"a\\\"b\\\\c\"\n\t\t]\n\t],\n\t\"b:0\": [\n\t\t\"x-1.0\",\n\t\t\"1\",\n\t\t[]\n\t]\n}";
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        expected,
        "registry bytes must match Python json.dumps exactly (no trailing newline)"
    );
}

/// S0 for backlog #167(e): real `load()` calls `pruneNonExisting()`
/// (`PreservedLibsRegistry.py:96-97,180-219`), which rebuilds each
/// entry's paths as regular files first (in stored order), then
/// symlinks whose target is one of those files -- so after a reload
/// the hardlink `.so.1.0.0` comes before the soname symlink `.so.1`
/// even though `register()` stored `sorted()` order, and gone paths
/// are dropped.
#[test]
fn plib_registry_reload_orders_regular_files_before_symlinks() {
    let tmp = tempdir();
    let root = tmp.join("root");
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::write(root.join("usr/lib/libl32soname.so.1.0.0"), b"fake elf").unwrap();
    std::os::unix::fs::symlink(
        "libl32soname.so.1.0.0",
        root.join("usr/lib/libl32soname.so.1"),
    )
    .unwrap();
    let seed = "{\n\t\"l32/sonamelib:0\": [\n\t\t\"l32/sonamelib-1.0\",\n\t\t\"1\",\n\t\t[\n\t\t\t\"/usr/lib/libl32soname.so.1\",\n\t\t\t\"/usr/lib/libl32soname.so.1.0.0\",\n\t\t\t\"/usr/lib/libl32soname.so.gone\"\n\t\t]\n\t]\n}";
    std::fs::create_dir_all(plib_registry_path(&root).parent().unwrap()).unwrap();
    std::fs::write(plib_registry_path(&root), seed).unwrap();

    let registry = read_plib_registry(&root);
    let (_, _, paths) = registry
        .entries
        .get("l32/sonamelib:0")
        .expect("the entry survives reload");
    assert_eq!(
        paths,
        &vec![
            "/usr/lib/libl32soname.so.1.0.0".to_string(),
            "/usr/lib/libl32soname.so.1".to_string(),
        ],
        "regular files first, then symlinks; gone paths dropped"
    );
}

/// Backlog #178 (S0 oracle + S2): two consecutive soname bumps
/// (`sonamebumplib` 1.0 -> 2.0 -> 3.0, each dropping the previous
/// soname while `consumesonamebump` still links `.so.1`).
/// Real `PreservedLibsRegistry.register(cpv, slot, counter, paths)`
/// (`3rdparty/portage/lib/portage/util/_dyn_libs/
/// PreservedLibsRegistry.py:142-169`) keyed `cp:slot` **replaces**
/// the record with the merging package's `(cpv, counter, paths)` --
/// real `dblink.treewalk()` calls it as `register(self.mycpv, slot,
/// counter, sorted(preserve_paths))` (`lib/portage/dbapi/vartree.py:
/// 5266-5272`), after copying the preserved entries into the new
/// package's own `CONTENTS` (`_add_preserve_libs_to_contents`,
/// `:3775-3826`). The S0 oracle (one `podman run ...
/// localhost/test-portuale:latest` probe over the l32 C2 cell shape
/// plus a `sonamelib-3.0` ebuild, real portage 3.0.82.2) holds after
/// each step: `{"l32/sonamelib:0": ["l32/sonamelib-<N>.0",
/// "<counter>", ["/usr/lib64/libl32soname.so.1.0.0",
/// "/usr/lib64/libl32soname.so.1"]]}`, the `.so.2` files gone from
/// disk, and the new package's `CONTENTS` listing the preserved
/// `.so.1` entries with their original digest/mtime (see
/// `differential-test-bed/findings/l5.md` "## Group 2").
#[test]
fn plib_registry_two_consecutive_soname_bumps_replace_the_record_under_the_merging_cpv() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    // A real Gentoo ROOT's own `/etc/ld.so.conf` lists `/usr/lib64`
    // (container-verified on the S0 probe image), so real
    // `getlibpaths()`'s own `defpath` covers the libdir and real
    // `findConsumers` matches the consumer. Portuale regenerates
    // `etc/ld.so.conf` from env.d `LDPATH` on every merge
    // (`env_update::run_env_update`), so seed that input the way a
    // real toolchain env.d entry does -- otherwise this synthetic
    // root's `defpath` is just `/usr/lib` + `/lib` and no
    // `/usr/lib64` consumer is ever found, on either implementation.
    std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
    std::fs::write(
        root.join("etc/env.d/99sonamebump"),
        "LDPATH=\"/usr/lib64\"\n",
    )
    .unwrap();

    for (name, version) in [
        ("sonamebumplib", "1.0"),
        ("consumesonamebump", "1.0"),
        ("sonamebumplib", "2.0"),
    ] {
        let status = run_merge(
            &versioned_fixture(name, version),
            &root,
            &portage_tmpdir,
            &MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(status, 0, "{name}-{version} merges cleanly");
    }

    // Bump 1: the record is owned by the merging cpv with its own
    // vdb COUNTER, paths file-then-symlink (real `pruneNonExisting`
    // order), byte-exact.
    let counter_2 =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-2.0/COUNTER"))
            .unwrap();
    let expected_bump1 = format!(
        "{{\n\t\"dev-libs/sonamebumplib:0\": [\n\t\t\"dev-libs/sonamebumplib-2.0\",\n\t\t\"{}\",\n\t\t[\n\t\t\t\"/usr/lib64/libsonamebump.so.1.0.0\",\n\t\t\t\"/usr/lib64/libsonamebump.so.1\"\n\t\t]\n\t]\n}}",
        counter_2.trim(),
    );
    assert_eq!(
        std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
        expected_bump1,
        "after bump 1 the registry must match the S0 oracle shape"
    );
    let contents_2 =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-2.0/CONTENTS"))
            .unwrap();
    assert!(
        contents_2.contains("obj /usr/lib64/libsonamebump.so.1.0.0"),
        "the new package owns the preserved hardlink: {contents_2}"
    );
    assert!(
        contents_2.contains("sym /usr/lib64/libsonamebump.so.1 "),
        "the new package owns the preserved soname symlink: {contents_2}"
    );

    let status = run_merge(
        &versioned_fixture("sonamebumplib", "3.0"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("run_merge succeeds");
    assert_eq!(status, 0, "sonamebumplib-3.0 merges cleanly");

    // Bump 2: the SAME key is replaced -- owner flips to the new
    // merging cpv/counter while the still-needed `.so.1` path list
    // survives verbatim (real drops only the unneeded `.so.2` pair,
    // which nobody links).
    let counter_3 =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-3.0/COUNTER"))
            .unwrap();
    let expected_bump2 = format!(
        "{{\n\t\"dev-libs/sonamebumplib:0\": [\n\t\t\"dev-libs/sonamebumplib-3.0\",\n\t\t\"{}\",\n\t\t[\n\t\t\t\"/usr/lib64/libsonamebump.so.1.0.0\",\n\t\t\t\"/usr/lib64/libsonamebump.so.1\"\n\t\t]\n\t]\n}}",
        counter_3.trim(),
    );
    assert_eq!(
        std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
        expected_bump2,
        "after bump 2 the registry must match the S0 oracle shape"
    );
    assert!(
        root.join("usr/lib64/libsonamebump.so.1.0.0").is_file(),
        "the still-needed hardlink survives the second bump"
    );
    assert!(
        !root.join("usr/lib64/libsonamebump.so.2.0.0").exists(),
        "the unneeded .so.2 hardlink is unmerged at the second bump"
    );
    assert!(
        !root.join("usr/lib64/libsonamebump.so.2").exists(),
        "the unneeded .so.2 symlink is unmerged at the second bump"
    );
    let contents_3 =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/sonamebumplib-3.0/CONTENTS"))
            .unwrap();
    assert!(
        contents_3.contains("obj /usr/lib64/libsonamebump.so.1.0.0"),
        "the newest package owns the preserved hardlink: {contents_3}"
    );
    assert!(
        !contents_3.contains("libsonamebump.so.2"),
        "the newest package must not claim the dropped soname: {contents_3}"
    );
}

/// Real `_prune_plib_registry`'s own unregister shape for a package
/// that owns no files at all (empty `CONTENTS`): real
/// `instance_owns_files` gates only the linkmap rebuild and the
/// preserve/prune scans -- `unregister()` still runs. So portuale's
/// own `preserve_libs_on_unmerge` returns an empty preserved set but
/// still drops a matching stale entry; with no entry at all nothing
/// is written (write-only-on-change).
#[test]
fn preserve_libs_on_unmerge_with_empty_contents_still_unregisters() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let vdb = root.join("var/db/pkg/dev-libs/foo-1.0");
    std::fs::create_dir_all(&vdb).unwrap();
    std::fs::write(vdb.join("COUNTER"), "5\n").unwrap();
    // The stale path exists, so the load-time `pruneNonExisting`
    // keeps the entry and only the `unregister()` below removes it.
    std::fs::create_dir_all(root.join("usr/lib")).unwrap();
    std::fs::write(root.join("usr/lib/stale.so"), b"fake elf").unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/foo:0".to_string(),
        (
            "dev-libs/foo-1.0".to_string(),
            "5".to_string(),
            vec!["/usr/lib/stale.so".to_string()],
        ),
    );
    write_plib_registry(
        &root,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("seeding the registry succeeds");

    let preserved =
        preserve_libs_on_unmerge(&root, "dev-libs", "foo", "foo-1.0", "0", "", false).unwrap();
    assert!(preserved.is_empty());
    assert_eq!(
        std::fs::read_to_string(plib_registry_path(&root)).unwrap(),
        "{}",
        "the matching stale entry must be unregistered even with empty CONTENTS"
    );

    // No entry at all: nothing to unregister, nothing written.
    let tmp2 = tempdir();
    let preserved =
        preserve_libs_on_unmerge(&tmp2, "dev-libs", "foo", "foo-1.0", "0", "", false).unwrap();
    assert!(preserved.is_empty());
    assert!(!plib_registry_path(&tmp2).exists());
}

/// Fix round 1 for review item 3, threading half: the replace loop
/// (`unmerge_pkgfiles` with `is_replacement`) must reach the prune
/// with `unmerge_no_replacement=false` and the old instance
/// excluded, so the still-needed lib survives a real replacement
/// unmerge on disk and in the registry. The replacing instance owns
/// the consumer too (its `CONTENTS` + `NEEDED.ELF.2` are live --
/// real `include_file`), which is what keeps the consumer visible
/// past the old-instance exclusion.
#[test]
fn replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer() {
    let tmp = tempdir();
    let (root, _) = seed_old_owned_consumer_root(&tmp);
    let vdb_new = root.join("var/db/pkg/dev-libs/newapp-1.0");
    std::fs::create_dir_all(&vdb_new).unwrap();
    std::fs::write(vdb_new.join("CONTENTS"), "obj /usr/bin/cprog abc 1\n").unwrap();
    std::fs::write(
        vdb_new.join("NEEDED.ELF.2"),
        "X86_64;/usr/bin/cprog;;;libkeep.so.1\n",
    )
    .unwrap();

    crate::ebuild_unmerge::unmerge_pkgfiles(
        &root,
        "dev-libs",
        "oldapp",
        "oldapp-1.0",
        &["newapp-1.0".to_string()],
        &crate::ebuild_unmerge::UnmergeOptions::default(),
        true,
        &BTreeMap::new(),
        &[],
    )
    .expect("replacement unmerge succeeds");

    assert!(
        root.join("usr/lib/keepA.so").is_file(),
        "the still-needed preserved lib must survive the replacement unmerge"
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
        "the registry entry must survive the replacement unmerge"
    );
}

/// Real `_collision_protect`'s own preserve-libs exclusion: with
/// `preservepkg-old`'s own already-merged file registered in
/// `preserved_libs_registry` (hand-seeded here -- portuale has no
/// registration/detection side yet, see this module's own doc
/// comment), `preservepkg-new` colliding on that exact path is
/// excluded from collision-protect's abort entirely (even with
/// `collision_protect: true`) and takes over the file; afterwards
/// the registry no longer lists the path (its only entry, so the
/// whole `cp:slot` key is dropped) and `preservepkg-old`'s own vdb
/// `CONTENTS` no longer claims it either.
#[test]
fn preserve_libs_registry_entry_excludes_the_collision_and_hands_ownership_over() {
    let tmp = tempdir();
    let root = tmp.join("root");
    let portage_tmpdir = tmp.join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&portage_tmpdir).unwrap();

    run_merge(
        &collision_fixture("preservepkg-old"),
        &root,
        &portage_tmpdir,
        &MergeOptions::default(),
        None,
    )
    .expect("preservepkg-old merges cleanly");

    let mut entries = BTreeMap::new();
    entries.insert(
        "dev-libs/preservepkg-old:0".to_string(),
        (
            "dev-libs/preservepkg-old-1.0".to_string(),
            "0".to_string(),
            vec!["/usr/lib/preservedtest/libfoo.so.1".to_string()],
        ),
    );
    write_plib_registry(
        &root,
        &PlibRegistry {
            entries,
            orig_entries: BTreeMap::new(),
        },
    )
    .expect("seeding the registry succeeds");

    let options = MergeOptions {
        collision_protect: true,
        ..MergeOptions::default()
    };
    let status = run_merge(
        &collision_fixture("preservepkg-new"),
        &root,
        &portage_tmpdir,
        &options,
        None,
    )
    .expect("a preserved-lib collision is excluded, not aborted");
    assert_eq!(status, 0);

    assert_eq!(
        std::fs::read_to_string(root.join("usr/lib/preservedtest/libfoo.so.1")).unwrap(),
        "new library content\n"
    );

    // preservepkg-old's own CONTENTS no longer claims the path
    // preservepkg-new just took over.
    let old_contents =
        std::fs::read_to_string(root.join("var/db/pkg/dev-libs/preservepkg-old-1.0/CONTENTS"))
            .unwrap();
    assert!(!old_contents.contains("/usr/lib/preservedtest/libfoo.so.1"));

    // The registry entry is gone entirely -- it had exactly one
    // path, and that path is no longer preserved.
    let registry_text = std::fs::read_to_string(plib_registry_path(&root)).unwrap();
    let registry = parse_plib_registry(&registry_text).unwrap();
    assert!(registry.is_empty());
}
