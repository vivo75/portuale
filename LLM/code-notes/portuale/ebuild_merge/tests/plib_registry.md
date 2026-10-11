# crates/portuale/src/ebuild_merge/tests/plib_registry.rs
Preserved-libs registry coverage: serialization, reload ordering, write suppression, soname bumps, and empty-contents unregistering.

## fn plib_registry_round_trips_through_json
- No moved context.

## fn read_plib_registry_degrades_gracefully_when_missing_or_corrupt
- Portage's `load()` degrades `ENOENT` to `{}`.

## fn plib_inode_map_skips_paths_that_no_longer_exist_on_disk
- No moved context.

## fn plib_registry_is_not_rewritten_when_nothing_is_preserved
- S0 for backlog #167(a): Portage's `PreservedLibsRegistry.store()` (`3rdparty/portage/lib/portage/util/_dyn_libs/PreservedLibsRegistry.py:107-108`) returns without writing when the registry is unchanged (`self._data == self._data_orig`) -- a merge/unmerge that preserves nothing must leave a packaged 0-byte registry at 0 bytes (l32 C1/C3/C4 cells), not rewrite it.

## fn plib_registry_with_an_unrelated_entry_is_not_rewritten_without_preserve_activity
- Fix round 1 for review item 2 (backlog #178): Portage's `store()` returns early when the registry equals what was read (`PreservedLibsRegistry.store`). Pre-fix, every `preserve_libs_on_unmerge` / `prune_unused_preserved_libs` call rewrote the file even when `register()` was a no-op.
- Seed: a live unrelated entry -- the file exists (so `pruneNonExisting` keeps it) and stays needed (an installed consumer links its soname).
- The seed mtime is set distinctly old (`touch -d 2001-02-03 04:05:06`) so any rewrite is observable even at coarse filesystem timestamp granularity.

## fn plib_registry_empty_serializes_to_exactly_empty_braces
- S0 for backlog #167(b): an empty registry Portage *does* write is exactly `{}` (2 bytes, no trailing newline -- Portage's `json.dumps({}, indent="\t", sort_keys=True)`).

## fn plib_registry_serialization_is_byte_identical_to_python_json_dumps
- S0 for backlog #167(c): expected bytes generated with `python3 -c 'import json; print(repr(json.dumps({"b:0": ("x-1.0", "1", []), "a:0": ("y-2.0", "3", ["/usr/lib/lib\u00e9.so.1", "a\"b\\\\c"])}, ensure_ascii=False, indent="\t", sort_keys=True)))'`.

## fn plib_registry_reload_orders_regular_files_before_symlinks
- S0 for backlog #167(e): Portage's `load()` calls `pruneNonExisting()` (`PreservedLibsRegistry.py:96-97,180-219`), which rebuilds each entry's paths as regular files first (in stored order), then symlinks whose target is one of those files -- so after a reload the hardlink `.so.1.0.0` comes before the soname symlink `.so.1` even though `register()` stored `sorted()` order, and gone paths are dropped.

## fn plib_registry_two_consecutive_soname_bumps_replace_the_record_under_the_merging_cpv
- Backlog #178 (S0 oracle + S2): two consecutive soname bumps (`sonamebumplib` 1.0 -> 2.0 -> 3.0, each dropping the previous soname while `consumesonamebump` still links `.so.1`). Portage's `PreservedLibsRegistry.register(cpv, slot, counter, paths)` (`3rdparty/portage/lib/portage/util/_dyn_libs/PreservedLibsRegistry.py:142-169`) keyed `cp:slot` **replaces** the record with the merging package's `(cpv, counter, paths)` -- Portage's `dblink.treewalk()` calls it as `register(self.mycpv, slot, counter, sorted(preserve_paths))` (`lib/portage/dbapi/vartree.py:5266-5272`), after copying the preserved entries into the new package's own `CONTENTS` (`_add_preserve_libs_to_contents`, `:3775-3826`).
- The S0 oracle (one `podman run ... localhost/test-portuale:latest` probe over the l32 C2 cell shape plus a `sonamelib-3.0` ebuild, portage 3.0.82.2) holds after each step: `{"l32/sonamelib:0": ["l32/sonamelib-<N>.0", "<counter>", ["/usr/lib64/libl32soname.so.1.0.0", "/usr/lib64/libl32soname.so.1"]]}`, the `.so.2` files gone from disk, and the new package's `CONTENTS` listing the preserved `.so.1` entries with their original digest/mtime (see `differential-test-bed/findings/l5.md` "## Group 2").
- Seed detail: a Gentoo ROOT's own `/etc/ld.so.conf` lists `/usr/lib64` (container-verified on the S0 probe image), so Portage's `getlibpaths()`'s own `defpath` covers the libdir and Portage's `findConsumers` matches the consumer. Portuale regenerates `etc/ld.so.conf` from env.d `LDPATH` on every merge (`env_update::run_env_update`), so the seed provides that input the way a toolchain env.d entry does -- otherwise this synthetic root's `defpath` is just `/usr/lib` + `/lib` and no `/usr/lib64` consumer is ever found, on either implementation.
- Bump 1: the record is owned by the merging cpv with its own vdb COUNTER, paths file-then-symlink (Portage `pruneNonExisting` order), byte-exact; the assertions compare against the S0 oracle shape.
- Bump 2: the SAME key is replaced -- owner flips to the new merging cpv/counter while the still-needed `.so.1` path list survives verbatim (Portage drops only the unneeded `.so.2` pair, which nobody links).

## fn preserve_libs_on_unmerge_with_empty_contents_still_unregisters
- Portage's `_prune_plib_registry`'s own unregister shape for a package that owns no files at all (empty `CONTENTS`): Portage's `instance_owns_files` gates only the linkmap rebuild and the preserve/prune scans -- `unregister()` still runs. So portuale's own `preserve_libs_on_unmerge` returns an empty preserved set but still drops a matching stale entry; with no entry at all nothing is written (write-only-on-change).
- Only the `unregister()` removes the entry below, not the load-time pruning.

## fn replacement_unmerge_keeps_a_preserved_lib_needed_by_the_replacing_consumer
- Fix round 1 for review item 3, threading half: the replace loop (`unmerge_pkgfiles` with `is_replacement`) must reach the prune with `unmerge_no_replacement=false` and the old instance excluded. The replacing instance owns the consumer too (its `CONTENTS` + `NEEDED.ELF.2` are live -- Portage `include_file`), which is what keeps the consumer visible past the old-instance exclusion.

## fn preserve_libs_registry_entry_excludes_the_collision_and_hands_ownership_over
- Portage's `_collision_protect`'s own preserve-libs exclusion: with `preservepkg-old`'s own already-merged file registered in `preserved_libs_registry` (hand-seeded here -- portuale has no registration/detection side yet, see this module's own doc comment), `preservepkg-new` colliding on that exact path is excluded from collision-protect's abort entirely (even with `collision_protect: true`) and takes over the file; afterwards the registry no longer lists the path (its only entry, so the whole `cp:slot` key is dropped) and `preservepkg-old`'s own vdb `CONTENTS` no longer claims it either.
