# #305 S0.2 - inventory of installed-database access sites

Evidence for backlog #305 (VDB backends), step S0.2. Read-only analysis of
`rust/` at the S0.2 run date (2026-10-04). Nothing in this file changes
behaviour. Feeds S1.1 (method list) and the routing steps S1.2-S1.8.

Targets are the methods of `InstalledDb` / `WriteTxn` in
`docs/vdb_to_db.md` section 6.1, with the write order of section 9.
Read classes R1-R7 and write classes W1-W5 are the ones of section 3.3.
The D4 stores (`world`, `world_sets`, `preserved_libs_registry`,
`config` memory, `counter`) are not in section 3.3; where a write to one of
them needs a class I call it **W6 (store write)**. Items named `N<number>`
refer to section 5 below ("Needed beyond 6.1").

## 1. Method

Run, from `/home/vivo/repo/PORTUALE/portuale`:

```
python3 docs/evidence/305-s0-inventory.py          # production hits only
python3 docs/evidence/305-s0-inventory.py --all    # production + test hits
```

The script (not modified) greps every `rust/**/*.rs` (skipping `target/`)
for six patterns: `var/db/pkg` (also `"db").join("pkg"`, `VDB_DIR`,
`vdb_dir(`), `var/lib/portage/world`, `var/lib/portage/world_sets` (and the
string literals `"world")`, `"world_sets")`), `preserved_libs_registry`,
`var/lib/portage/config`, `cache/edb/counter`. Each line is one hit.

**Test-span rule** (from the script docstring): a hit is test code when it
lies inside an item annotated `#[cfg(test)]` (a `mod`, `fn`, `impl`, ...:
the span from the attribute to the item's matching closing brace, braces
counted outside strings/comments roughly), or anywhere under a `tests/`
directory. Everything else is production. A `#[cfg(test)] use ...;` or
`mod x;` ends at its `;`.

Per-file counts, copied from the script output:

```
# per file: prod test
#     5     3  rust/mrg-director/src/lib.rs
#    15   102  rust/portage-repo/src/lib.rs
#     0     2  rust/portage-repo/src/merge_order.rs
#     0     1  rust/portage-repo/src/solver_bridge.rs
#    30    57  rust/portuale/src/ebuild_merge.rs
#     1     1  rust/portuale/src/ebuild_package.rs
#     2     1  rust/portuale/src/ebuild_phases.rs
#     5     8  rust/portuale/src/ebuild_unmerge.rs
#     0     8  rust/portuale/src/emerge_build.rs
#     0    25  rust/portuale/src/emerge_getbinpkg.rs
#     1     1  rust/portuale/src/mrg.rs
#     2     2  rust/portuale/src/needed_elf.rs
#     1     0  rust/portuale/src/preserved_libs.rs
#    23    18  rust/portuale/src/pretend.rs
#     9    34  rust/portuale/src/remote.rs
#    94   263  TOTAL (357)
```

Totals: **94 production + 263 test = 357**. This file classifies all 94
production hits (section 2). Test hits are fixture setup and are not
routed (S1.9's guard test scans only production code).

Class totals of the 94 production rows: message-or-comment 45, read 29, read+write 3, remote-bash 5, write 12 (sum 94).

Reading notes:

- The script matches text, so 45 of the 94 rows are doc comments, header
  comments, help text or the string label `"world"`. Only the 29 `read`,
  12 `write`, 3 `read+write (path builder)` and 5 `remote-bash` rows are
  real accesses. The comment rows are still listed because S1.9's guard
  test must allowlist or reword them (the guard greps `var/db/pkg`).
- A hit is often a **path builder or a doc**, while the function that does the
  I/O has no literal. Those functions are in section 3 (via param).
- The "enclosing fn" column names the item that owns the hit. For a doc
  comment it is the documented item (a naive nearest-preceding-`fn` lookup is
  wrong for doc comments and for `pretend.rs` lines inside `run`).
- Two functions contain accesses that the script does not hit, because the
  literal is built differently: `run_deselect` writes both files at
  `pretend.rs:4121-4128` (`portage_dir.join(name)`), and `entries_not_merged`
  / `resolve_vdb_path_arg` do `.join("CONTENTS")` right after their hit.
  They are covered by their rows.

## 2. Production hits (94 rows)

Columns: file:line | enclosing fn | class | store | target method.
Class: `read` / `write` / `message-or-comment` / `remote-bash`; read and
write rows carry the R/W class in brackets.

| file:line | enclosing fn | class | store | target method |
|---|---|---|---|---|
| `rust/mrg-director/src/lib.rs:95` | doc of trait `PackagesDb` (section banner) | message-or-comment | vdb entry | none (message) |
| `rust/mrg-director/src/lib.rs:675` | doc of `VdbReader` | message-or-comment | vdb entry | none (message) |
| `rust/mrg-director/src/lib.rs:697` | doc of `VdbReader::new` | message-or-comment | vdb entry | none (message) |
| `rust/mrg-director/src/lib.rs:737` | doc of `MemoryDb` | message-or-comment | vdb entry | none (message) |
| `rust/mrg-director/src/lib.rs:1328` | doc of field `Director::packages_db` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:2469` | doc of `quickpkg_direct_index_entries` (the fn reads via `all_installed_packages` + `read_vdb_string`, see via-param) | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:6571` | doc of `installed_candidates` (separated from it by `dir_mtime_nanos`) | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:6620` | `installed_candidates` | read (R1 cache key: per-category dir mtimes) | vdb entry | generation (category-scoped variant, see N8) |
| `rust/portage-repo/src/lib.rs:6663` | `installed_candidates_uncached` | read (R1: category dir scan, SLOT per entry) | vdb entry | snapshot (R1 via cp lookup, see N8) |
| `rust/portage-repo/src/lib.rs:6893` | doc of `installed_versions` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7507` | doc of `read_vdb_flag_set` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7691` | `vdb_pkg_dir_meta` | read (R2: resolve entry dir incl. pre-move names + one statx for is_dir/mtime) | vdb entry | snapshot / read_file (FilesDb-internal; pre-move fallback stays in portage-repo via has_entry, see N13) |
| `rust/portage-repo/src/lib.rs:7709` | doc of `read_vdb_string` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7810` | doc of `vdb_fingerprint` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7812` | doc of `vdb_fingerprint` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7837` | doc of `all_installed_packages` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:7856` | `all_installed_packages` | read (R3; builds the cache key with `vdb_fingerprint`) | vdb entry | generation + snapshot |
| `rust/portage-repo/src/lib.rs:7876` | `all_installed_packages_uncached` | read (R3: full two-level scan, SLOT per entry) | vdb entry | snapshot |
| `rust/portage-repo/src/lib.rs:9761` | doc of `read_vdb_slot` | message-or-comment | vdb entry | none (message) |
| `rust/portage-repo/src/lib.rs:18636` | doc of `bind_slot_operator_deps` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/ebuild_merge.rs:96` | module header comment (plib collision exclusion) | message-or-comment | preserved_libs | none (message) |
| `rust/portuale/src/ebuild_merge.rs:902` | doc of `cfg_mem_path` | message-or-comment | config_memory | none (message) |
| `rust/portuale/src/ebuild_merge.rs:911` | `cfg_mem_path` (path builder for `read_cfgfiledict` / `write_cfgfiledict`) | read+write (path builder) | config_memory | config_memory / set_config_memory |
| `rust/portuale/src/ebuild_merge.rs:984` | `plib_registry_path` (path builder for `read_plib_registry` / `write_plib_registry`) | read+write (path builder) | preserved_libs | preserved_libs / set_preserved_libs |
| `rust/portuale/src/ebuild_merge.rs:1374` | `remove_from_contents` | write (W4: read CONTENTS, rewrite CONTENTS, then NEEDED.ELF.2) | vdb entry | read_file + replace_file x2 |
| `rust/portuale/src/ebuild_merge.rs:1576` | `replacement_needed_entries` | read (R5: NEEDED.ELF.2 of the pending `-MERGING-<pf>` entry) | vdb entry | read_file on pending entry (needs N2) |
| `rust/portuale/src/ebuild_merge.rs:1624` | `find_preserve_paths_for_merge` | read (R5: old same-slot instance CONTENTS) | vdb entry | read_file |
| `rust/portuale/src/ebuild_merge.rs:1743` | `register_merge_preserved_libs` | read (R7-ish: COUNTER of the just-published new entry) | vdb entry | read_file (or counter returned by begin_entry, N1) |
| `rust/portuale/src/ebuild_merge.rs:1826` | `preserve_libs_on_unmerge` | read (R7-ish: COUNTER of the entry being unmerged) | vdb entry | snapshot (COUNTER field) / read_file |
| `rust/portuale/src/ebuild_merge.rs:2089` | `prune_unused_preserved_libs` | read (entry-exists `is_dir()` test) | vdb entry | has_entry (N13) / snapshot membership |
| `rust/portuale/src/ebuild_merge.rs:2647` | doc of `next_counter` | message-or-comment | counter | none (message) |
| `rust/portuale/src/ebuild_merge.rs:2658` | `next_counter` | read+write (W5: read, +1, create_dir_all, write; missing/corrupt = -1) | counter | begin_write -> begin_entry (counter allocated there, N1) |
| `rust/portuale/src/ebuild_merge.rs:2684` | `create_vdb_tmp` | write (W1 start: wipe stale `-MERGING-<pf>`, mkdir) | vdb entry | begin_write -> begin_entry |
| `rust/portuale/src/ebuild_merge.rs:2720` | `populate_vdb_tmp` | write (W1: copy every build-info file, write CATEGORY/SLOT/repository/COUNTER) | vdb entry | begin_entry (EntryImage of named files, N3) |
| `rust/portuale/src/ebuild_merge.rs:2766` | `write_vdb_tmp_contents` | write (W1: CONTENTS, then consolidated `metadata`) | vdb entry | begin_entry / finish_entry (put_entry_file, N3) |
| `rust/portuale/src/ebuild_merge.rs:2797` | `publish_vdb_tmp` | write (W1/W3 end: rm -rf same-pf final dir, rename tmp into place) | vdb entry | finish_entry |
| `rust/portuale/src/ebuild_merge.rs:2907` | doc of `read_installed_slot` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/ebuild_merge.rs:2960` | `installed_instance_pf` | read (R7: highest COUNTER among same-slot versions) | vdb entry | snapshot (COUNTER field) |
| `rust/portuale/src/ebuild_merge.rs:2993` | doc of `owns_path` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/ebuild_merge.rs:3027` | `read_contents_pf` (live entry) | read (R5: CONTENTS) | vdb entry | read_file |
| `rust/portuale/src/ebuild_merge.rs:3035` | `read_contents_pf` (`-MERGING-<pf>` fallback) | read (R5: CONTENTS of pending entry) | vdb entry | read_file on pending entry (needs N2) |
| `rust/portuale/src/ebuild_merge.rs:3253` | `blockers_from_flat_deps` | read (R3: two-level scan + SLOT per entry) | vdb entry | snapshot |
| `rust/portuale/src/ebuild_merge.rs:3466` | `find_owners` | read (R6: two-level scan, CONTENTS of every entry via `VdbReader`) | vdb entry | owners |
| `rust/portuale/src/ebuild_merge.rs:4149` | `merge_after_install` | read (path hand-off: `PORTAGE_UPDATE_ENV=<entry>/environment.bz2`, postinst writes it) | vdb entry | entry_path, or scratch file + replace_file (N9) |
| `rust/portuale/src/ebuild_merge.rs:4240` | `unmerge_replaced_same_slot` | read (R1: category dir listing, `<package>-<digit>` prefix rule, then SLOT) | vdb entry | snapshot |
| `rust/portuale/src/ebuild_merge.rs:4385` | `unmerge_one_installed` | read (R5: DEFINED_PHASES + exists-tests of environment.bz2 and `<pf>.ebuild`) | vdb entry | read_file (Option = exists) |
| `rust/portuale/src/ebuild_merge.rs:4455` | `unmerge_one_installed` (call to `delete_vdb_dir`) | write (W2) | vdb entry | delete_entry |
| `rust/portuale/src/ebuild_merge.rs:4482` | `run_vdb_saved_env_phase` | read (R5: environment.bz2 + `<pf>.ebuild`, copied to scratch / path to bash) | vdb entry | read_file x2 (entry_path fast path, N9) |
| `rust/portuale/src/ebuild_merge.rs:4877` | comment inside `merge_binpkg` | message-or-comment | preserved_libs | none (message) |
| `rust/portuale/src/ebuild_merge.rs:4929` | `merge_binpkg` | read (path hand-off: environment.bz2 for postinst + `is_file` test) | vdb entry | entry_path, or scratch file + replace_file (N9) |
| `rust/portuale/src/ebuild_package.rs:1469` | `quickpkg_from_vdb` | read (R5: BUILD_TIME, CONTENTS, `<pf>.ebuild`) | vdb entry | read_file x3 |
| `rust/portuale/src/ebuild_phases.rs:1871` | doc of `bind_slot_operator` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/ebuild_phases.rs:1929` | stray doc paragraph above `build_phase_use` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/ebuild_unmerge.rs:603` | `unmerge_pkgfiles` | read (R5: CONTENTS) | vdb entry | read_file |
| `rust/portuale/src/ebuild_unmerge.rs:721` | `delete_vdb_dir` (signature line) | write (W2) | vdb entry | delete_entry |
| `rust/portuale/src/ebuild_unmerge.rs:722` | `delete_vdb_dir` | write (W2: rm -rf entry dir, best-effort rmdir of category) | vdb entry | delete_entry |
| `rust/portuale/src/ebuild_unmerge.rs:741` | `run_unmerge` | read (R5: CONTENTS exists-test) | vdb entry | read_file (Option) / has_entry |
| `rust/portuale/src/ebuild_unmerge.rs:792` | `run_unmerge` (call to `delete_vdb_dir`) | write (W2) | vdb entry | delete_entry |
| `rust/portuale/src/mrg.rs:1070` | help text of the `--remote-vdb` option in the option table | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/needed_elf.rs:24` | module header comment | message-or-comment | preserved_libs | none (message) |
| `rust/portuale/src/needed_elf.rs:493` | `read_all_needed_entries` | read (R5 bulk: NEEDED.ELF.2 of every entry, two-level scan) | vdb entry | snapshot + read_file per entry, or bulk (N11) |
| `rust/portuale/src/preserved_libs.rs:4` | module header comment | message-or-comment | preserved_libs | none (message) |
| `rust/portuale/src/pretend.rs:1416` | comment inside `print_entry_line` | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:3300` | doc of `read_world_atoms` | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:3324` | `read_world_atoms` | read | world | world |
| `rust/portuale/src/pretend.rs:3357` | doc of `installed_set_atoms` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/pretend.rs:3375` | doc of `preserved_rebuild_atoms` | message-or-comment | preserved_libs | none (message) |
| `rust/portuale/src/pretend.rs:3519` | doc of `update_world_file` | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:3566` | `update_world_file` | write | world | set_world (atoms half only, N5) |
| `rust/portuale/src/pretend.rs:3693` | doc of `update_world_sets_file` | message-or-comment | world_sets | none (message) |
| `rust/portuale/src/pretend.rs:3732` | `update_world_sets_file` | write | world_sets | set_world (sets half only, N5) |
| `rust/portuale/src/pretend.rs:3743` | doc of `read_world_sets` | message-or-comment | world_sets | none (message) |
| `rust/portuale/src/pretend.rs:3759` | `read_world_sets` | read | world_sets | world |
| `rust/portuale/src/pretend.rs:3873` | doc of `run_deselect` | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:3874` | doc of `run_deselect` | message-or-comment | world_sets | none (message) |
| `rust/portuale/src/pretend.rs:4044` | `run_deselect` (the string `"world"` is a label for the `>>> Removing` message and the half selector, not a path) | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:4050` | `run_deselect` (label `"world_sets"`, same use) | message-or-comment | world_sets | none (message) |
| `rust/portuale/src/pretend.rs:4087` | `run_deselect` (label compare `*f == "world"`) | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:4092` | `run_deselect` (label compare `*f == "world_sets"`) | message-or-comment | world_sets | none (message) |
| `rust/portuale/src/pretend.rs:4317` | `resolve_vdb_path_arg` | read (validates a user-given entry directory path under the vdb) | vdb entry | cpv_for_path + read_file (N10) |
| `rust/portuale/src/pretend.rs:5533` | `entries_not_merged` | read (R5: CONTENTS exists-test) | vdb entry | read_file (Option) / has_entry |
| `rust/portuale/src/pretend.rs:6064` | `deselect_from_world` | write | world | set_world (atoms half only, N5) |
| `rust/portuale/src/pretend.rs:6110` | `installed_cp_versions` | read (R3: two-level scan, SLOT via `vdb_entry_slot`) | vdb entry | snapshot |
| `rust/portuale/src/pretend.rs:13800` | comment inside `run` (pub fn at pretend.rs:10062) | message-or-comment | world | none (message) |
| `rust/portuale/src/pretend.rs:15849` | comment inside `run` (pub fn at pretend.rs:10062) | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/remote.rs:164` | doc of field `RemoteContext::vdb` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/remote.rs:279` | `check_remote` (default placement string `client:<root>/var/db/pkg`) | remote-bash (path handed to generated client bash) | vdb entry | stays files (client bash) |
| `rust/portuale/src/remote.rs:999` | `preflight_script` (bash `[ -d "$ROOT/var/db/pkg" ]`) | remote-bash | vdb entry | stays files (client bash) |
| `rust/portuale/src/remote.rs:1068` | `preflight_gates` (reads the `VDB_DIR` key the probe printed; pushes a warning) | message-or-comment | vdb entry | stays files (client bash) |
| `rust/portuale/src/remote.rs:3074` | `MERGE_FLOW` const (bash): max over `$VDBROOT/*/COUNTER` and the edb counter file | remote-bash (R7 scan + W5) | counter | stays files (client bash) |
| `rust/portuale/src/remote.rs:3081` | `MERGE_FLOW` const (bash): writes `$ROOT/var/cache/edb/counter` | remote-bash (W5) | counter | stays files (client bash) |
| `rust/portuale/src/remote.rs:3185` | comment inside `merge_script` | message-or-comment | vdb entry | none (message) |
| `rust/portuale/src/remote.rs:3255` | `client_vdb_placement` (stateless fallback path for the client) | remote-bash (path handed to generated client bash) | vdb entry | stays files (client bash) |
| `rust/portuale/src/remote.rs:3501` | doc of `scrub_bzip2_command` | message-or-comment | vdb entry | none (message) |

Per-file row count (matches the section 1 table): mrg-director/src/lib.rs 5, portage-repo/src/lib.rs 15, portuale/src/ebuild_merge.rs 30, portuale/src/ebuild_package.rs 1, portuale/src/ebuild_phases.rs 2, portuale/src/ebuild_unmerge.rs 5, portuale/src/mrg.rs 1, portuale/src/needed_elf.rs 2, portuale/src/preserved_libs.rs 1, portuale/src/pretend.rs 23, portuale/src/remote.rs 9; total 94.

## 3. Via param: functions that touch VDB data without the literal

These functions receive `root` (or a vdb directory) and reach the store through a
path builder or a helper from section 2. They are what S1.2-S1.8
actually rewrite. Call-site line numbers are production code only.
"Target" is the replacement method.

### 3A. portage-repo helpers (all take `root: &Path`)

Full production call-site lists for the high-fan-out helpers are in
Appendix A. Counts below come from the same scan.

| fn (file:line of definition) | param | what it reads or writes | class | target | callers |
|---|---|---|---|---|---|
| `vdb_aux_get` (`portage-repo/src/lib.rs:7345`) | `root` | one field of one entry; consolidated-`metadata` stamp check; per-entry memo keyed on entry-dir mtime; out-of-set keys (`CONTENTS`, `NEEDED.*`) read raw | R2 | snapshot fields / read_file; stamp check and memo stay inside FilesDb (N8) | `lib.rs:6695`, `6742`, `7525`, `7739`, `7901` |
| `vdb_pkg_dir` / `vdb_pkg_dir_meta` (`:7672` / `:7685`) | `root` | entry directory path incl. fallback to pre-`move` category/name (`installed_cp_sources`) | R2 helper | has_entry + FilesDb-internal path (N13); direct readers below | direct users: `lib.rs:5760` (`installed_parent_use_state`, `is_dir`), `6918` (`installed_contents_files`, CONTENTS), `11513` (`chain_node_usedep_suffix`, `is_dir`), `13091` (`read_vdb_env_vars`, environment.bz2), `7347` (`vdb_aux_get`, raw key) |
| `read_vdb_string` (`:7732`), `read_vdb_flag_set` (`:7518`), `read_vdb_slot` (`:9767`), `vdb_entry_slot` (`:6738`), `installed_pkg_repo` (`:6706`), `installed_pkg_iuse_and_use` (`:7538`) | `root` | thin wrappers on `vdb_aux_get` (USE, IUSE, *DEPEND, SLOT, repository, EAPI, BUILD_TIME, KEYWORDS, DEFINED_PHASES) | R2 | snapshot fields (lazy per-entry, N8) | 29 / 50 / 28 / 3 / 16 / 9 sites |
| `installed_candidates` (`:6603`) | `root` | (cat,pkg) -> (version,slot,sub_slot); thread-local memo validated by category-dir mtimes | R1 | snapshot + category-scoped generation (N8) | 46 sites: portage-repo `lib.rs` 33, `resolver_trace.rs` 1, portuale `pretend.rs` 10, `ebuild_phases.rs` 2 |
| `installed_versions` (`:6894`), `installed_refs` (`:6717`) | `root` | projections of `installed_candidates` | R1 | snapshot | 10 / 13 sites |
| `all_installed_packages` (`:7851`) | `root` | every cpv + main slot; memo validated by `vdb_fingerprint` | R3 | generation + snapshot | 25 sites: `lib.rs` 16, `solver_bridge.rs` 4, `merge_order.rs` 2, `pretend.rs` 3 |
| `vdb_fingerprint` (`:7817`) | `vdb: &Path` | vdb-dir mtime + every category-dir mtime, xor-folded | cache key | generation | `lib.rs:7857` |
| `dir_mtime_nanos` (`:6595`) | `p: &Path` | one directory mtime | cache key | generation_of (N8) | `lib.rs:6625` (in `installed_candidates`) |
| `installed_contents_files` (`:6912`) | `root` | whole `CONTENTS` of one entry, path field only, `/` stripped, six kinds | R5 | list_files / read_file | `mrg-director/src/lib.rs:714` (`VdbReader::contents_files`) |
| `installed_reverse_dependents` (`:6948`) | `root` | scan of every entry: `USE` + four `*DEPEND` keys, flattened, matched against the consumer's cpv + slot | R4 | reverse_dependents (N15) | `mrg-director/src/lib.rs:722` |
| `read_vdb_env_vars` (`:13082`) | `root` | `environment.bz2` of one entry, bzip2 in-process | R5 | read_file | `lib.rs:13298` (`resolve_installed_info`) |
| `libc_provider_cps` (`:7776`) | `root` | `virtual/libc` entries: USE + RDEPEND | R1+R2 | snapshot | 7 sites: `lib.rs` 5, `merge_order.rs:1194`, `ebuild_phases.rs:2193` |
| `expand_new_virt` (`:6766`), `info_pkgs_table` (`:6812`) | `root` | installed versions + `USE`/`RDEPEND` of `virtual/*` | R1+R2 | snapshot | `info_pkgs_table` called from `pretend.rs:8629` |
| `quickpkg_direct_index_entries` (`:2477`) | `source_root` | 11 metadata keys + `repository` of every entry of another root | R3+R2 | `for_root(source_root)` snapshot | `lib.rs:2442` (`build_local_binpkg_index`) |
| `merge_order.rs` (`installed_candidates_by_cp` `:1026`, `add_installed_dependency_closure` `:1116`), `solver_bridge.rs` (`closure_seeds`, `resolve_pubgrub`, `resolve_resolvo`, ...), `resolver_trace.rs` | `root` / `req.root` | call-through only: they call the helpers above; 0 production literal hits | R1 R2 R3 | S1.3 routes them | listed in Appendix A |

### 3B. portuale: merge, unmerge, preserve-libs

| fn (file:line) | param | what it does | class | target | callers (production) |
|---|---|---|---|---|---|
| `read_plib_registry` (`ebuild_merge.rs:1111`) | `root` | parse registry JSON, snapshot as `orig_entries`, then `prune_non_existing` (lstat-based) | R (preserved_libs) | preserved_libs (prune stays above, N6) | `ebuild_merge.rs:1644`, `1750`, `1833`, `1894`, `1951`, `2085`, `3947`, `4793` |
| `write_plib_registry` (`:1246`) | `root` | serialise byte-identically to Python `json.dumps`; skipped when `entries == orig_entries` | W6 | set_preserved_libs (N6) | `ebuild_merge.rs:1341`, `1761`, `1879`, `2106` |
| `read_cfgfiledict` / `write_cfgfiledict` (`:918` / `:931`) | `root` | config-memory `"path md5\n"` map; write is unconditional | R / W6 | config_memory / set_config_memory | `ebuild_merge.rs:4030`, `4072`, `4844`, `4873`; `ebuild_unmerge.rs:646`, `684` |
| `preserved_lib_paths` (`:1893`) | `root` | `read_plib_registry(...).preserved_libs()` | R | preserved_libs | `preserved_libs.rs:27`, `pretend.rs:3394` |
| `unregister_preserved_libs` (`:1316`) | `root` | drop taken-over paths from the registry, `remove_from_contents` on the previous owner, write registry | W4 + W6 | replace_file + set_preserved_libs | `ebuild_merge.rs:4077`, `4882` |
| `register_preserved_libs` (`:1434`, in-memory) + `register_merge_preserved_libs` (`:1731`) | `root` | one `cp:slot` record replaced after publish; reads COUNTER of the new entry | W6 (+R COUNTER) | set_preserved_libs | `ebuild_merge.rs:4120`, `4914` |
| `linkage_owner_entries` (`:1505`), `owner_entries_with_preserved_orphans` (`:1547`) | `root` | linkage-map input: every entry's NEEDED.ELF.2 (through `read_all_needed_entries` `:1513`) plus registry orphans plus the pending entry | R5 bulk | bulk NEEDED read (N11), preserved_libs | `ebuild_merge.rs:1653`, `1848`, `1957` |
| `find_unused_preserved_libs` (`:1943`), `prune_unused_preserved_libs` (`:2031`) | `root` | registry + all NEEDED.ELF.2; deletes files; `remove_from_contents`; rewrites registry | R5 + W4 + W6 | preserved_libs, replace_file, set_preserved_libs | `ebuild_unmerge.rs:703`, `ebuild_merge.rs:4184`, `4950` |
| `preserve_libs_on_unmerge` (`:1810`) | `root` | COUNTER read, registry read/write, linkage rebuild | R + W6 | read_file/snapshot, preserved_libs, set_preserved_libs | `ebuild_unmerge.rs:656` |
| `find_preserve_paths_for_merge` (`:1614`) | `root` | old instance CONTENTS, registry, pending NEEDED | R5 | read_file, preserved_libs, pending read (N2) | `ebuild_merge.rs:4059`, `4866` |
| `replacement_needed_entries` (`:1570`) | `root` | pending `-MERGING-` NEEDED.ELF.2 | R5 | pending read (N2) | `ebuild_merge.rs:1652`, `4271` |
| `installed_instance_pf` (`:2951`) | `root` | highest COUNTER among same-slot versions | R7 | snapshot | `ebuild_merge.rs:1622`, `3922`, `4697`, `4843` |
| `read_installed_slot` (`:2925`) | `root` | main slot via `vdb_entry_slot` | R2 | snapshot | `ebuild_merge.rs:2955`, `3359`, `4254`; `ebuild_unmerge.rs:612`, `620`; `pretend.rs:3424` |
| `read_contents_pf` (`:3025`) | `root` | CONTENTS, live entry then pending | R5 | read_file / pending read | used by `owns_path_pf`, `owned_node_type_pf`, `owned_node_value_pf` |
| `owns_path` (`:3005`) | `root` | ownership via `VdbReader::contents_files` | R6 | owners | `ebuild_merge.rs:3429` (`find_collisions`) |
| `owns_path_pf` (`:3056`), `owned_node_type_pf` (`:3076`), `owned_node_value_pf` (`:3102`) | `root` | one entry's CONTENTS lines: path, type, md5/link target | R5 | list_files (FileMeta with kind and md5/target) | `ebuild_merge.rs:1664`, `1853`, `3432`, `850` (`protect_decision`); `ebuild_unmerge.rs:258`, `276` |
| `find_collisions` (`:3345`) | `root` | installed versions, slots, ownership | R1 R2 R6 | snapshot, owners | `ebuild_merge.rs:3955`, `4802` |
| `collision_message` (`:3522`) | `root` | `find_owners` | R6 | owners | `ebuild_merge.rs:3989`, `4824` |
| `merge_after_install` (`:3834`), `merge_binpkg` (`:4558`) | `root` | the two merge drivers: call all of the above in the S0.1 order | W1 W3 W5 W6 | begin_write ... commit | `merge_binpkg` from `emerge_getbinpkg.rs:997` (`merge_one_binary_entry`) |
| `unmerge_replaced_same_slot` (`:4223`) | `root` | replace loop; category dir listing, then `unmerge_one_installed` per old pf | R1 + W2 | snapshot, delete_entry | `ebuild_merge.rs:4100`, `4899` |
| `unmerge_one_installed` (`:4371`) | `root` | prerm hook, `unmerge_pkgfiles`, postrm hook, `delete_vdb_dir` | R5 + W2 | read_file, delete_entry | `ebuild_merge.rs:4273`; `pretend.rs:5941` (`execute_unmerge`) |
| `run_vdb_saved_env_phase` (`:4472`) | `root` | runs one phase from the entry's own `environment.bz2` + `<pf>.ebuild` | R5 | read_file (N9) | `ebuild_merge.rs:4424`; `pretend.rs:9082` (`run_info`), `9292` (`run_config_action`) |
| `unmerge_pkgfiles` (`ebuild_unmerge.rs:592`) | `root` | CONTENTS, slot siblings, cfg memory, preserve-libs, `remove_contents` | R5 R1 | read_file, snapshot | `ebuild_merge.rs:4440`, `ebuild_unmerge.rs:763` |
| `remove_contents` (`ebuild_unmerge.rs:235`) | `root` | ownership checks on slot siblings via `owns_path_pf` / `owned_node_type_pf` | R5 | list_files | `ebuild_unmerge.rs:666` |
| `quickpkg_from_vdb` (`ebuild_package.rs:1457`) | `root` | (hit row 1469) | R5 | read_file | `ebuild_merge.rs:4396` |
| `bind_slot_operator` (`ebuild_phases.rs:1882`), `inject_libc_dep` (`:2190`) | `root` | `installed_candidates` / `libc_provider_cps` | R1 | snapshot | `ebuild_phases.rs:2029` (in `write_post_install_metadata`, called at `4368`) |
| `read_all_needed_entries` (`needed_elf.rs:491`) | `root` | (hit row 493) | R5 bulk | N11 | `ebuild_merge.rs:1513`; `preserved_libs.rs:53`; `pretend.rs:3400`, `6606` |
| `show_preserved_libs_notice` (`preserved_libs.rs:26`) | `root` | `preserved_lib_paths` | R | preserved_libs | `pretend.rs` post-emerge notice |
| `VdbReader::installed_versions`, `contents_files`, `reverse_dependents` (`mrg-director/src/lib.rs:704`, `713`, `716`) | `self.root` | thin adapters over the three portage-repo functions in 3A | R1 R5 R4 | `for_root(root)` | `ebuild_merge.rs:3007` (`owns_path`), `3465` (`find_owners`) build one |

### 3C. portuale: world, world_sets, listing (pretend.rs)

| fn (file:line) | param | what it does | class | target | callers (production) |
|---|---|---|---|---|---|
| `expand_selected` (`pretend.rs:3347`) | `root` | `read_world_atoms` + `read_world_sets` | R | world | `pretend.rs:4451`, `12911`, `13349` |
| `installed_set_atoms` (`:3362`) | `root` | `@installed`: `all_installed_packages` | R3 | snapshot | `pretend.rs:4460`, `12954` |
| `preserved_rebuild_atoms` (`:3393`) | `root` | `@preserved-rebuild`: registry + all NEEDED + `read_installed_slot` | R5 bulk | preserved_libs, N11 | `pretend.rs:4461`, `12958` |
| `collect_installed_sets` (`:3838`) | `root` | `read_world_sets` | R | world | `pretend.rs:4616`, `6488` |
| `run_deselect` (`:3957`) | `root` | reads both world files, `installed_candidates`; rewrites **both** files unconditionally at `4121-4128` | R + W6 + R1 | world, set_world | called from `run` |
| `deselect_from_world` (`:6041`) | `root` | `read_world_atoms`, `installed_candidates`, rewrite world | R + W6 | world, set_world | `pretend.rs:5969` (`execute_unmerge`) |
| `run_depclean_pretend` (`:6797`) | `root` | `read_world_atoms` / `read_world_sets` / `all_installed_packages` | R | world, snapshot | `pretend.rs:6902`, `6909`, `6980` |
| `run_info` (`:8688`) | `root` | `read_world_sets`; `installed_candidates`; phase from saved env | R | world, snapshot | `pretend.rs:8812`, `8717` |
| `run` (`:10062`) | `root` | `read_world_atoms` for display and favorites (`13802`, `14964`, `15616`); `read_world_sets` (`15625`); `update_world_file` (`16042`); `update_world_sets_file` (`16065`) | R + W6 | world, set_world | entry point |
| `run_resume` (`:5559`) | `root` | `entries_not_merged` (`5806`) then `update_world_file` (`5842`); `run` also calls `entries_not_merged` at `15928`, `16003` | R + W6 | read_file/has_entry, set_world | called from `run` |
| `installed_cp_versions` (`:6108`) callers | `root` | `resolve_cleanup_args` (`6157`), `run_unmerge_pretend` (`4513`), `run_config_action` (`9201`) | R3 | snapshot | |
| `still_listed_parents`, `run_unmerge_pretend`, `news_item_relevant`, `highest_installed_pvr`, `run_search`, `render_ambiguous_search_output`, `run_config_action`, `lib_consumer_scan`, `execute_unmerge` | `root` | read-only portage-repo helper calls (3A) | R1 R2 R3 R5 | snapshot, bulk NEEDED | see Appendix A |

### 3D. remote.rs (server-side, not the client bash)

| fn (file:line) | param | what it does | class | target | callers |
|---|---|---|---|---|---|
| `VdbShadow::load` (`remote.rs:1938`) | `dir: &Path` (a **vdb directory**, not a root) | two-level walk, reads `CONTENTS` of every non-`-MERGING-` entry, keeps `obj`/`sym` paths, first owner wins | R6 bulk, read-only | `FilesDb` opened on a vdb dir (N12), owners | `load_vdb_shadow` (`:2035`, rows `2040` and `2046`) from `run_remote_plan` (`:1346`) |
| `load_vdb_shadow` (`:2035`) | `ctx.vdb` | `server:` = read the placed dir directly; `client:` = `pull_dir` to a temp copy first | R | as above | `remote.rs:1346` |
| `shadow_precheck` (`:2010`) | `&VdbShadow` | ownership test per staged path | R6 | owners | `run_remote_plan` |
| `ship_old_hook_envs` (`:3682`) | `vdb: &str` | client bash probe of same-package versions + `pull_file` of each `environment.bz2` | remote-bash | stays files (client bash) | `remote.rs:2226` |
| `run_binpkg_flow` (`:2101`) | `vdb` | builds `{vdb}/{cat}/{pf}/environment.bz2` for the regen install (`2325`) | remote-bash | stays files | `remote.rs:1566`, `2400` |
| `postinst_regen_script` (`:2730`) | `$vdbdir` | bash reading `environment.bz2` | remote-bash | stays files | `remote.rs:3379` |
| `merge_script` (`:3174`), `client_vdb_placement` (`:3251`) | `vdb` | the generated client merge driver (`MERGE_FLOW`, `MERGE_HELPERS`) does the whole write on the client: tmp dir, COUNTER scan, metadata, rename | remote-bash | stays files (client bash) | `remote.rs:3292`, `3293`, `2218` |

## 4. Function-level summary, by routing step

One line per production function that touches the VDB. Class: R1 (cat,pkg)
-> versions+slot; R2 (cpv,key) -> field; R3 every cpv+slot; R4 reverse
dependents; R5 blobs; R6 path -> owners; R7 (cp,slot) -> max COUNTER; W1
insert entry; W2 delete entry; W3 replace = W1+W2; W4 rewrite CONTENTS /
NEEDED; W5 counter+1; W6 store write (world, world_sets, preserved libs,
config memory; not in 3.3).

### S1.2 portage-repo: snapshot and aux reads

| fn | does | class | target |
|---|---|---|---|
| `vdb_aux_get` (`lib.rs:7345`) | one field of one entry, stamp check + memo | R2 | snapshot fields; out-of-set keys read_file |
| `read_vdb_file`, `read_vdb_raw_file` (`:7444`, `:7501`) | per-key file read, normalised / raw | R2 / R5 | read_file |
| `vdb_pkg_dir`, `vdb_pkg_dir_meta` (`:7672`, `:7685`) | entry dir resolution with pre-move fallback, one statx | R2 | FilesDb-internal + has_entry |
| `read_vdb_string`, `read_vdb_flag_set`, `read_vdb_slot`, `vdb_entry_slot`, `installed_pkg_repo`, `installed_pkg_iuse_and_use`, `read_installed_slot` | wrappers on `vdb_aux_get` | R2 | snapshot fields |
| `installed_candidates`, `_uncached`, `dir_mtime_nanos` (`:6603`, `:6652`, `:6595`) | R1 scan + memo keyed on category mtimes | R1 | snapshot + generation_of (N8) |
| `installed_versions`, `installed_refs` (`:6894`, `:6717`) | projections | R1 | snapshot |
| `all_installed_packages`, `_uncached`, `vdb_fingerprint` (`:7851`, `:7874`, `:7817`) | R3 scan + memo keyed on fingerprint | R3 | snapshot + generation |

### S1.3 portage-repo: other reads

| fn | does | class | target |
|---|---|---|---|
| `installed_contents_files` (`:6912`) | CONTENTS path list | R5 | list_files |
| `installed_reverse_dependents` (`:6948`) | scan of USE + *DEPEND of every entry | R4 | reverse_dependents (N15) |
| `read_vdb_env_vars` (`:13082`) | environment.bz2 variables | R5 | read_file |
| `quickpkg_direct_index_entries` (`:2477`) | index records from another root's vdb | R3 R2 | for_root(source_root) snapshot |
| `libc_provider_cps` (`:7776`), `expand_new_virt` (`:6766`), `info_pkgs_table` (`:6812`) | virtual/libc and `info_pkgs` rows | R1 R2 | snapshot |
| `installed_parent_use_state` (`:5747`), `chain_node_usedep_suffix` (`:11402`) | `vdb_pkg_dir(..).is_dir()` entry-exists test | R1 | has_entry |
| `merge_order.rs`, `solver_bridge.rs`, `resolver_trace.rs` | call-through only (no literal hits) | R1 R2 R3 | snapshot (S1.3 text names the first two; `resolver_trace.rs` also calls the helpers) |

### S1.4 merge writes (`ebuild_merge.rs`)

| fn | does | class | target |
|---|---|---|---|
| `next_counter` (`:2657`) | read counter file (missing/corrupt = -1), +1, write | W5 | begin_entry returns the Counter (N1) |
| `create_vdb_tmp` (`:2682`) | wipe + mkdir `-MERGING-<pf>` | W1 | begin_write -> begin_entry |
| `populate_vdb_tmp` (`:2711`) | copy every build-info file; write CATEGORY, SLOT, repository, COUNTER | W1 | begin_entry(EntryImage) (N3) |
| `write_vdb_tmp_contents` (`:2759`) | write CONTENTS, then `write_consolidated_metadata_file` | W1 | put_entry_file (N3); consolidation inside FilesDb |
| `write_consolidated_metadata_file` (`:2865`) | 23-field `metadata` + `#dir_mtime=` stamp | W1 | FilesDb-internal (constants move to portage-vdb) |
| `publish_vdb_tmp` (`:2796`) | rm -rf same-pf final dir, rename tmp into place | W1/W3 | finish_entry |
| `read_cfgfiledict`, `write_cfgfiledict`, `cfg_mem_path` | config memory | R / W6 | config_memory / set_config_memory |
| `read_plib_registry`, `write_plib_registry`, `plib_registry_path`, `unregister_preserved_libs`, `register_merge_preserved_libs` | preserved-libs registry | R / W6 | preserved_libs / set_preserved_libs (N6) |
| `find_preserve_paths_for_merge`, `replacement_needed_entries` | old CONTENTS, pending NEEDED | R5 | read_file, pending read (N2) |
| `installed_instance_pf` | highest-COUNTER same-slot instance | R7 | snapshot |
| `read_contents_pf`, `owns_path`, `owns_path_pf`, `owned_node_type_pf`, `owned_node_value_pf` | CONTENTS ownership and node values | R5 R6 | list_files, owners, pending read |
| `find_owners` | scan of every entry's CONTENTS | R6 | owners |
| `blockers_from_flat_deps` | scan of every entry + SLOT | R3 | snapshot |
| `merge_after_install`, `merge_binpkg` | drivers; hand `PORTAGE_UPDATE_ENV=<entry>/environment.bz2` to postinst (`4149`, `4929`) | W1 W3 + path hand-off | begin_write ... commit; entry_path (N9) |

### S1.5 unmerge and W4

| fn | does | class | target |
|---|---|---|---|
| `delete_vdb_dir` (`ebuild_unmerge.rs:721`) | rm -rf entry, rmdir empty category | W2 | delete_entry (category rmdir inside FilesDb, N14) |
| `run_unmerge` (`:733`) | CONTENTS exists-test, phases, `delete_vdb_dir` | R5 W2 | read_file, delete_entry |
| `unmerge_pkgfiles` (`:592`) | CONTENTS read, slot siblings, cfg memory | R5 R1 | read_file, snapshot |
| `unmerge_one_installed` (`ebuild_merge.rs:4371`) | DEFINED_PHASES + env + ebuild exist-tests, hooks, delete | R5 W2 | read_file, delete_entry |
| `run_vdb_saved_env_phase` (`:4472`) | saved env + ebuild into scratch | R5 | read_file (N9) |
| `unmerge_replaced_same_slot` (`:4223`) | category dir listing for same-cp versions | R1 | snapshot |
| `remove_from_contents` (`:1370`) | rewrite CONTENTS, then NEEDED.ELF.2 | W4 | read_file + replace_file x2 |
| `preserve_libs_on_unmerge`, `find_unused_preserved_libs`, `prune_unused_preserved_libs`, `preserved_lib_paths` | registry + linkage | R / W6 / W4 | preserved_libs, set_preserved_libs, replace_file |
| `read_all_needed_entries` (`needed_elf.rs:491`) | NEEDED.ELF.2 of every entry | R5 bulk | N11 |
| `show_preserved_libs_notice` (`preserved_libs.rs:26`) | registry + NEEDED for the notice | R | preserved_libs |

### S1.6 other portuale sites

| fn | does | class | target |
|---|---|---|---|
| `quickpkg_from_vdb` (`ebuild_package.rs:1457`) | BUILD_TIME, CONTENTS, `<pf>.ebuild` | R5 | read_file |
| `bind_slot_operator`, `inject_libc_dep` (`ebuild_phases.rs:1882`, `:2190`) | installed candidates for `:=` binding / libc | R1 | snapshot |
| `read_world_atoms`, `read_world_sets` (`pretend.rs:3323`, `:3758`) | the two world files | R | world |
| `update_world_file`, `update_world_sets_file`, `deselect_from_world`, `run_deselect` | rewrite world / world_sets | W6 | set_world (N5) |
| `installed_cp_versions` (`:6108`), `installed_set_atoms` (`:3362`) | all cpv + slot | R3 | snapshot |
| `preserved_rebuild_atoms` (`:3393`) | registry + NEEDED + slot | R5 | preserved_libs, N11 |
| `entries_not_merged` (`:5523`) | CONTENTS exists-test per resumed entry | R5 | read_file / has_entry |
| `resolve_vdb_path_arg` (`:4296`) | user-given entry dir path | path | cpv_for_path (N10) |
| `mrg.rs:1070` | help text only | none | none (message) |
| `emerge_getbinpkg.rs`, `emerge_build.rs` | **no production hits**; only `merge_binpkg` call at `emerge_getbinpkg.rs:997` | none | nothing to route |

### S1.7 remote

| fn | does | class | target |
|---|---|---|---|
| `VdbShadow::load`, `load_vdb_shadow`, `shadow_precheck` | read-only ownership shadow of a vdb directory | R6 | FilesDb on a vdb dir (N12) |
| `merge_script`, `MERGE_FLOW`, `preflight_script`, `ship_old_hook_envs`, `client_vdb_placement`, `check_remote` | client bash and path strings | remote-bash | stays files (client bash) |

### S1.8 mrg-director

| fn | does | class | target |
|---|---|---|---|
| `VdbReader` (`lib.rs:692`) and its `PackagesDb` impl (`:703`) | adapter over `portage_repo::installed_versions`, `installed_contents_files`, `installed_reverse_dependents` | R1 R5 R4 | `for_root` |
| `MemoryDb` (`:756`) | in-memory test double, no VDB access | none | keep |

## 5. Needed beyond 6.1

Accesses the section 6.1 interface cannot express, with the smallest
addition for each. "Today" cites the code.

**N1 Counter allocation point.** Today `next_counter` (`ebuild_merge.rs:2657`)
runs inside `populate_vdb_tmp`, before any file is merged. The value is
written into the pending `COUNTER`, and read back by
`register_merge_preserved_libs` (`:1743`) and `installed_instance_pf`.
6.1's `finish_entry -> Counter` gives it too late. portuale has **no high-water
scan** of entry COUNTERs: the counter file alone drives it (missing or
corrupt reads as -1, so the first merge gets 0). Only the remote client bash
(`remote.rs:3074`) scans `*/COUNTER`. Add: `begin_entry(&mut self, image)
-> Result<(EntryId, Counter)>` and let `finish_entry` return `()`.
On files the counter file is read/+1/written inside `begin_entry` with no
lock, as today.

**N2 Pending (-MERGING-) entry reads.** `read_contents_pf` (`:3035`) falls back
to the `-MERGING-<pf>` CONTENTS, and `replacement_needed_entries` (`:1576`)
reads the pending `NEEDED.ELF.2`; both run while the new entry is not yet
published. Normal readers must keep skipping it. Add
`InstalledDb::read_file_or_pending(cpv, name)` (live first, then pending);
on db backends the pending row is visible only through the writing handle.

**N3 Staged writes into the pending entry.** `CONTENTS` is known only after
the file merge, long after `populate_vdb_tmp`, and `EntryImage` must carry
arbitrary named files (every regular file of `build-info`: `<PF>.ebuild`,
`environment.bz2`, `NEEDED.ELF.2`, `INSTALL_MASK`, ...), not just the 23
fields. Add `WriteTxn::put_entry_file(id, name, &[u8])`; `begin_entry` takes
the initial files. On files, `finish_entry` runs the consolidation
(`write_consolidated_metadata_file`) and the publish. Move
`METADATA_FILE_FIELDS`, `in_metadata_file` and the format version into
`portage-vdb` (writer and reader must agree).

**N4 Write order is part of the contract.** The merge writes, in order:
cfg memory (`:4072`, before CONTENTS), CONTENTS (`:2766`), registry
(`:4077`, before the replace loop), replace loop, publish, registry
again (`:4120`), then postinst. The world file is written **later and
separately** (`update_world_file`, after all merges, `pretend.rs:16042`),
and `deselect_from_world` runs after each unmerge. So design section 9's
single `set_world` inside the merge txn does not match the code: a txn
with only `set_world` must be legal, and FilesDb's txn must apply every call
eagerly in call order (`commit` is a no-op there). No new method; a rule for
S1.1/S1.4.

**N5 World has two files and write-if-added.** `update_world_file` writes only
`world` and only when an atom was added; `update_world_sets_file` only
`world_sets`; `run_deselect` rewrites both unconditionally; an empty list
writes an empty file. The caller sorts and dedups; comments and `@` lines
are dropped on read. A single `set_world(&World)` would rewrite the other file
and change its bytes. Replace by `set_world_atoms(&[String])` and
`set_world_sets(&[String])`; FilesDb writes `lines.join("\n") + "\n"`
(empty -> empty file) and creates `var/lib/portage`.

**N6 Preserved-libs write-if-changed.** `write_plib_registry` skips the write
when `entries == orig_entries` (parsed content, not bytes; a corrupt file
parses as empty and is then not rewritten). The JSON is byte-identical to
Python `json.dumps` with tabs. `prune_non_existing` is lstat-based and stays
above the trait. `PreservedLibs` must carry `entries` plus `loaded`
(the parsed state at read time); `set_preserved_libs` skips when equal.

**N7 Config memory** is a plain `BTreeMap<path, md5>`, rewritten
unconditionally on every merge (`write_cfgfiledict`, `:4072`, `:4873`) and by
unmerge only when `stale_confmem` is non-empty. No addition beyond the type.

**N8 Cache keys and lazy R2.** (a) `installed_candidates` validates its memo
with the mtime of only the 1-2 category dirs of that cp (12,684 calls on the
reference run, S1.2's `statx` budget), while `all_installed_packages` uses
the full `vdb_fingerprint` (vdb dir + every category dir, about 30 `stat`s).
One `generation()` forces the dearer check on every R1 call. Add
`InstalledDb::generation_of(&self, categories: &[&str]) -> Result<u64>`;
FilesDb's `generation()` stays the full fingerprint. (b) `vdb_aux_get` is lazy
and per entry (one statx, `metadata` `#dir_mtime=` check, per-entry memo). R2
through a whole-`snapshot()` would read ~2000 SLOT files. Add
`InstalledDb::entry(&self, cpv) -> Result<Option<Arc<EntryFields>>>`
(the 23 normalised fields of one entry) and `cp_entries(cat, pkg)` for R1.
The per-entry dir mtime and stamp stay inside FilesDb.

**N9 Entry directory path hand-off to phases.** Four places give bash or a
copier a path inside the entry: `PORTAGE_UPDATE_ENV=<entry>/environment.bz2`
(`ebuild_merge.rs:4149`, `:4929`; postinst writes it), `run_vdb_saved_env_phase`
(`:4482`: env path to `run_phase_from_saved_env`, ebuild copied to scratch),
`quickpkg_from_vdb` (`ebuild_package.rs:1469`: ebuild copied), and the
exists-tests in `unmerge_one_installed` (`:4385`). Read paths: engine spills
`read_file` into the scratch dir. The postinst write: engine passes a
scratch file and calls `replace_file(cpv, "environment.bz2", bytes)` after
postinst. To keep files byte- and `strace`-identical add
`InstalledDb::entry_path(&self, cpv) -> Option<PathBuf>` (`Some` only on
FilesDb); callers use it when present. There is no `PORTAGE_BUILDDIR`
involvement: the builddir is separate, only the vdb file paths matter.

**N10 User-given entry directory.** `emerge -C /var/db/pkg/cat/pf`
(`resolve_vdb_path_arg`, `pretend.rs:4296`) canonicalises a directory and
strips the vdb prefix. Add `InstalledDb::cpv_for_path(&self, &Path) ->
Option<Cpv>` (`Some` only on FilesDb); other backends reject path-shaped
args. CONTENTS existence becomes `read_file(...).is_some()`.

**N11 Bulk blob read.** `read_all_needed_entries` (`needed_elf.rs:491`),
`find_owners` (`:3466`) and `VdbShadow::load` (`remote.rs:1938`) read one
named file of every entry. N queries on a database backend. Add
`InstalledDb::read_file_all(&self, name) -> Result<Vec<(Cpv, Vec<u8>)>>`
(entries with no such file are absent from the list; callers that need an
empty row, like `read_all_needed_entries`, take the cpv list from
`snapshot()`).

**N12 FilesDb on a vdb directory.** `VdbShadow::load(dir)` is handed a placed
`--remote-vdb server:<path>` (already a vdb dir) or a `pull_dir` temp copy,
never `<root>/var/db/pkg` joined from a root. `for_root(root)` cannot express
it. Add `FilesDb::open_vdb_dir(path) -> FilesDb` (read-only, unregistered).
Also: nothing writes the `server:` shadow today (see surprises).

**N13 Presence test and pre-move names.** `vdb_pkg_dir(..).is_dir()`
(`lib.rs:5760`, `11513`), `prune_unused_preserved_libs` (`ebuild_merge.rs:2089`)
and the CONTENTS exists-tests (`entries_not_merged`, `run_unmerge`) ask only
"is this cpv installed". Add `InstalledDb::has_entry(&self, cpv) -> Result<bool>`.
Package moves (`installed_cp_sources`, `apply_updates_to_cp`,
`global_package_updates`) live in `portage-repo`, above `portage-vdb`; they
must stay there and call the trait with the on-disk (pre-move) names. The
trait is keyed by the stored cpv, never by a remapped one.

**N14 Category directory side effects.** `delete_vdb_dir` removes the empty
category dir best-effort (`ebuild_unmerge.rs:725`), and the category-dir mtime
is the `installed_candidates` cache key. `publish_vdb_tmp` removes a same-pf
final dir before the rename (same-cpv reinstall). Both are internal to
FilesDb's `delete_entry` / `finish_entry`. Category listing needs no
addition (R3 comes from the snapshot).

**N15 `reverse_dependents` signature.** The existing scan
(`installed_reverse_dependents`, `lib.rs:6948`) is keyed by the consumer
**cpv** and its slot, flattens each entry's `*DEPEND` against its own `USE`
(`portage_use_reduce`) and matches with `portage_dep::match_from_list`.
`portage-vdb` sits below `portage-repo` and may not depend on those crates.
Two options: (a) S1 keeps the scan in `portage-repo` on top of `entry()`
(files only, as S1.3 says "keep today's scans"); (b) the trait returns raw
edges `DepEdge { consumer: Cpv, key, atom }` for `cp` and the caller
version-matches. Pick (b) before S2, since an insert-time index needs the
flattening at write time. S1.1 should state which.

**N16 `FileMeta` content.** CONTENTS consumers need more than path and
kind: `owned_node_value_pf` needs the md5 (`obj`) or link target (`sym`),
`VdbReader::contents_files` needs six kinds (`obj sym dir dev fif bin`),
`VdbShadow` only `obj`/`sym`. `FileMeta` = `{ kind, path, md5_or_target, mtime }`
and `list_files` must keep CONTENTS line order (merge order).

**Not needed (checked):** slot locks (there is no lock, `flock` or lockfile
anywhere in the merge or unmerge path, so nothing to expose; design section 9
already says "no VDB lock"); an entry's directory mtime outside `vdb_aux_get`
(only the stamp check and memo, internal to FilesDb); category listing as a
separate call; the high-water COUNTER scan (not in portuale, N1).

## 6. Surprises

1. **S1.6 names `emerge_getbinpkg.rs` and `emerge_build.rs`, but they have 0
   production hits** (25 and 8 test hits only). Their only link is the call
   `ebuild_merge::merge_binpkg` at `emerge_getbinpkg.rs:997`. `merge_order.rs`
   and `solver_bridge.rs` (S1.3) also have 0 production hits; they call
   the helpers in 3A. `resolver_trace.rs` is not named in S1.3 but calls
   `installed_candidates`, `installed_pkg_repo`, `read_vdb_slot`.
2. Only 49 of the 94 production hits are real accesses; 45 are
   comments, docs, help text or the label `"world"`. The two world writes in
   `run_deselect` are not hits at all.
3. The world file is **not** written inside the merge; design section 9's
   `set_world` in the same commit does not match (N4).
4. The remote `server:<path>` shadow is **read-only** today (`VdbShadow::load`,
   `shadow_precheck`); nothing writes it. S1.7's "reads and writes" has no
   write to route.
5. portuale never scans entry COUNTERs (N1); the remote client bash does.
6. No VDB lock exists, so there is nothing for a slot lock to replace.
7. `unmerge_replaced_same_slot` finds same-cp versions by its own prefix
   rule (`<package>-<digit...>`) and does not apply package-move updates,
   unlike `installed_versions`. Routing it through R1 must keep the
   same-set result.
8. `find_owners` is a hand-walked directory scan that then builds a
   `VdbReader` and re-lists `CONTENTS` per entry; `owns_path_pf` stays a
   direct read on purpose (`ebuild_merge.rs:3050`). On `owners` both
   collapse into one call.
9. The script's total (357) is text hits; the plan's "307 `var/db/pkg`
   lines" counted only the first pattern.

## Appendix A. Production call sites of the portage-repo reader helpers

Generated with the script's own test-span rule: every non-test, non-comment call of a helper listed in section 3, row group A. Enclosing fn is the nearest preceding `fn` (so it can name a nested helper). Definitions are excluded.

**`read_vdb_flag_set`** (50 sites): `portage-repo/src/lib.rs:5769` (installed_parent_use_state), `portage-repo/src/lib.rs:5774` (installed_parent_use_state), `portage-repo/src/lib.rs:6782` (expand_new_virt), `portage-repo/src/lib.rs:6966` (installed_reverse_dependents), `portage-repo/src/lib.rs:7128` (running_root_satisfies_atom), `portage-repo/src/lib.rs:7130` (running_root_satisfies_atom), `portage-repo/src/lib.rs:7545` (installed_pkg_iuse_and_use), `portage-repo/src/lib.rs:7546` (installed_pkg_iuse_and_use), `portage-repo/src/lib.rs:7779` (libc_provider_cps), `portage-repo/src/lib.rs:8121` (topological_removal_order), `portage-repo/src/lib.rs:8340` (unresolved_runtime_deps), `portage-repo/src/lib.rs:8529` (depclean_cleanlist), `portage-repo/src/lib.rs:8760` (prune_cleanlist), `portage-repo/src/lib.rs:9281` (refresh_one_entry_display), `portage-repo/src/lib.rs:9282` (refresh_one_entry_display), `portage-repo/src/lib.rs:9698` (deps_changed), `portage-repo/src/lib.rs:10130` (reinstall_flags_for_use_change), `portage-repo/src/lib.rs:10131` (reinstall_flags_for_use_change), `portage-repo/src/lib.rs:11516` (chain_node_usedep_suffix), `portage-repo/src/lib.rs:11820` (best_installed_matching), `portage-repo/src/lib.rs:11821` (best_installed_matching), `portage-repo/src/lib.rs:13261` (resolve_installed_info), `portage-repo/src/lib.rs:13865` (dependency_avoid_update_candidate), `portage-repo/src/lib.rs:13867` (dependency_avoid_update_candidate), `portage-repo/src/lib.rs:14929` (resolve_pretend), `portage-repo/src/lib.rs:14931` (resolve_pretend), `portage-repo/src/lib.rs:16044` (merge_use_changed_vs_installed), `portage-repo/src/lib.rs:16045` (merge_use_changed_vs_installed), `portage-repo/src/lib.rs:16228` (reverse_dependency_constraints), `portage-repo/src/lib.rs:17951` (slot_operator_update_force_scan), `portage-repo/src/lib.rs:19260` (required_set_reachable_cps), `portage-repo/src/lib.rs:19362` (rebuild_if_entries), `portage-repo/src/lib.rs:21153` (collect_unwalked_installed_blockers), `portage-repo/src/lib.rs:21339` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:21436` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:21443` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:22845` (build_residual_slot_conflicts), `portage-repo/src/lib.rs:23279` (direct_solve_instance_use), `portage-repo/src/lib.rs:28591` (pkg_of), `portage-repo/src/lib.rs:28592` (pkg_of), `portage-repo/src/lib.rs:28824` (arg_pinned_installed_reuse), `portage-repo/src/lib.rs:28825` (arg_pinned_installed_reuse), `portage-repo/src/lib.rs:30409` (run_pass), `portage-repo/src/lib.rs:31789` (run_pass), `portage-repo/src/lib.rs:31790` (run_pass), `portage-repo/src/lib.rs:35953` (installed_dep_string), `portage-repo/src/lib.rs:36108` (enqueue_dependencies), `portage-repo/src/merge_order.rs:1243` (add_installed_dependency_closure), `portage-repo/src/resolver_trace.rs:295` (installed_use_str), `portage-repo/src/resolver_trace.rs:296` (installed_use_str)

**`installed_candidates`** (46 sites): `portage-repo/src/lib.rs:6718` (installed_refs), `portage-repo/src/lib.rs:6775` (expand_new_virt), `portage-repo/src/lib.rs:6791` (expand_new_virt), `portage-repo/src/lib.rs:6895` (installed_versions), `portage-repo/src/lib.rs:7007` (best_installed_for_atom), `portage-repo/src/lib.rs:7040` (complete_mode_withheld_version), `portage-repo/src/lib.rs:7098` (running_root_satisfies_atom), `portage-repo/src/lib.rs:11760` (atom_cp_installed), `portage-repo/src/lib.rs:11793` (best_installed_matching), `portage-repo/src/lib.rs:11864` (atom_installed_in_slot_of), `portage-repo/src/lib.rs:12225` (alternative_downgrade_demoted), `portage-repo/src/lib.rs:12481` (disjunction_preference), `portage-repo/src/lib.rs:13236` (resolve_installed_info), `portage-repo/src/lib.rs:14406` (resolve_pretend), `portage-repo/src/lib.rs:14709` (resolve_pretend), `portage-repo/src/lib.rs:16802` (slot_conflict_abi_probe), `portage-repo/src/lib.rs:18480` (slot_operator_unsatisfied_probe_full), `portage-repo/src/lib.rs:18666` (built_equals_binding), `portage-repo/src/lib.rs:18767` (bind_slot_operator_token), `portage-repo/src/lib.rs:19661` (cycle_restartable), `portage-repo/src/lib.rs:19702` (owner_self_loop), `portage-repo/src/lib.rs:21358` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:22706` (synthesize_surviving_conflict_entries), `portage-repo/src/lib.rs:23431` (direct_solve_slot_conflicts), `portage-repo/src/lib.rs:24050` (backtrack_missed_updates), `portage-repo/src/lib.rs:24113` (backtrack_missed_updates), `portage-repo/src/lib.rs:24317` (missing_dep_full_row), `portage-repo/src/lib.rs:28788` (arg_pinned_installed_reuse), `portage-repo/src/lib.rs:28800` (arg_pinned_installed_reuse), `portage-repo/src/lib.rs:30165` (run_pass), `portage-repo/src/lib.rs:31371` (run_pass), `portage-repo/src/lib.rs:32627` (run_pass), `portage-repo/src/lib.rs:32685` (run_pass), `portage-repo/src/resolver_trace.rs:253` (dump_atom_candidates), `portuale/src/ebuild_phases.rs:1889` (bind_slot_operator), `portuale/src/ebuild_phases.rs:2200` (inject_libc_dep), `portuale/src/pretend.rs:4003` (run_deselect), `portuale/src/pretend.rs:4355` (still_listed_parents), `portuale/src/pretend.rs:4540` (run_unmerge_pretend), `portuale/src/pretend.rs:4670` (run_unmerge_pretend), `portuale/src/pretend.rs:6052` (deselect_from_world), `portuale/src/pretend.rs:6189` (resolve_cleanup_args), `portuale/src/pretend.rs:8143` (news_item_relevant), `portuale/src/pretend.rs:8717` (run_info), `portuale/src/pretend.rs:9226` (run_config_action), `portuale/src/pretend.rs:13847` (jobs_count)

**`read_vdb_string`** (29 sites): `portage-repo/src/lib.rs:2485` (quickpkg_direct_index_entries), `portage-repo/src/lib.rs:6707` (installed_pkg_repo), `portage-repo/src/lib.rs:6783` (expand_new_virt), `portage-repo/src/lib.rs:6968` (installed_reverse_dependents), `portage-repo/src/lib.rs:7780` (libc_provider_cps), `portage-repo/src/lib.rs:8123` (topological_removal_order), `portage-repo/src/lib.rs:8343` (unresolved_runtime_deps), `portage-repo/src/lib.rs:8532` (depclean_cleanlist), `portage-repo/src/lib.rs:8763` (prune_cleanlist), `portage-repo/src/lib.rs:9723` (deps_changed), `portage-repo/src/lib.rs:9768` (read_vdb_slot), `portage-repo/src/lib.rs:9858` (new_repo_changed), `portage-repo/src/lib.rs:9916` (rebuilt_binary_changed), `portage-repo/src/lib.rs:11424` (chain_node_usedep_suffix), `portage-repo/src/lib.rs:13262` (resolve_installed_info), `portage-repo/src/lib.rs:13278` (resolve_installed_info), `portage-repo/src/lib.rs:13321` (resolve_installed_info), `portage-repo/src/lib.rs:16624` (collect_probe_parents), `portage-repo/src/lib.rs:16977` (slot_conflict_abi_display_pairs), `portage-repo/src/lib.rs:17540` (slot_operator_rebuild_scan), `portage-repo/src/lib.rs:17964` (slot_operator_update_force_scan), `portage-repo/src/lib.rs:19010` (slot_operator_eliminate_rebuilds), `portage-repo/src/lib.rs:19262` (required_set_reachable_cps), `portage-repo/src/lib.rs:19365` (rebuild_if_entries), `portage-repo/src/lib.rs:19417` (rebuild_if_entries), `portage-repo/src/lib.rs:21341` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:35801` (live_metadata_for_installed), `portage-repo/src/lib.rs:35899` (installed_dep_string), `portage-repo/src/lib.rs:35932` (installed_dep_string)

**`read_vdb_slot`** (28 sites): `portage-repo/src/lib.rs:6955` (installed_reverse_dependents), `portage-repo/src/lib.rs:8318` (unresolved_runtime_deps), `portage-repo/src/lib.rs:9807` (slot_changed), `portage-repo/src/lib.rs:15874` (rev_dep_pin_holdable), `portage-repo/src/lib.rs:17419` (slot_operator_rebuild_scan), `portage-repo/src/lib.rs:17995` (slot_operator_update_force_scan), `portage-repo/src/lib.rs:18003` (slot_operator_update_force_scan), `portage-repo/src/lib.rs:18973` (slot_operator_eliminate_rebuilds), `portage-repo/src/lib.rs:19114` (slot_operator_rebuild_entries), `portage-repo/src/lib.rs:19416` (rebuild_if_entries), `portage-repo/src/lib.rs:20907` (uninstall_entry), `portage-repo/src/lib.rs:21567` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:21646` (resolve_blockers_with_seeds), `portage-repo/src/lib.rs:23783` (constraint_withheld_updates), `portage-repo/src/lib.rs:24082` (backtrack_missed_updates), `portage-repo/src/lib.rs:24115` (backtrack_missed_updates), `portage-repo/src/lib.rs:24294` (missing_dep_full_row), `portage-repo/src/lib.rs:24305` (missing_dep_full_row), `portage-repo/src/lib.rs:24319` (missing_dep_full_row), `portage-repo/src/lib.rs:24321` (missing_dep_full_row), `portage-repo/src/lib.rs:28541` (pkg_of), `portage-repo/src/lib.rs:30207` (run_pass), `portage-repo/src/lib.rs:32546` (run_pass), `portage-repo/src/lib.rs:32554` (run_pass), `portage-repo/src/merge_order.rs:1029` (installed_candidates_by_cp), `portage-repo/src/merge_order.rs:1741` (digraph_prelude), `portage-repo/src/resolver_trace.rs:131` (node_label), `portage-repo/src/resolver_trace.rs:761` (dump_resolution_walk)

**`all_installed_packages`** (25 sites): `portage-repo/src/lib.rs:2479` (quickpkg_direct_index_entries), `portage-repo/src/lib.rs:6959` (installed_reverse_dependents), `portage-repo/src/lib.rs:8428` (depclean_cleanlist), `portage-repo/src/lib.rs:8666` (prune_cleanlist), `portage-repo/src/lib.rs:8883` (prune_nodeps_selection), `portage-repo/src/lib.rs:8940` (clean_selection), `portage-repo/src/lib.rs:15869` (rev_dep_pin_holdable), `portage-repo/src/lib.rs:16199` (reverse_dependency_constraints), `portage-repo/src/lib.rs:16593` (collect_probe_parents), `portage-repo/src/lib.rs:17436` (slot_operator_rebuild_scan), `portage-repo/src/lib.rs:18911` (slot_operator_eliminate_rebuilds), `portage-repo/src/lib.rs:19076` (slot_operator_rebuild_entries), `portage-repo/src/lib.rs:19232` (required_set_reachable_cps), `portage-repo/src/lib.rs:19350` (rebuild_if_entries), `portage-repo/src/lib.rs:21142` (collect_unwalked_installed_blockers), `portage-repo/src/lib.rs:21333` (resolve_blockers_with_seeds), `portage-repo/src/merge_order.rs:1028` (installed_candidates_by_cp), `portage-repo/src/merge_order.rs:1132` (add_installed_dependency_closure), `portage-repo/src/solver_bridge.rs:325` (closure_seeds), `portage-repo/src/solver_bridge.rs:372` (of), `portage-repo/src/solver_bridge.rs:1007` (resolve_pubgrub), `portage-repo/src/solver_bridge.rs:1168` (resolve_resolvo), `portuale/src/pretend.rs:3363` (installed_set_atoms), `portuale/src/pretend.rs:6980` (run_depclean_pretend), `portuale/src/pretend.rs:14306` (jobs_count)

**`installed_pkg_repo`** (16 sites): `portage-repo/src/lib.rs:6721` (installed_refs), `portage-repo/src/lib.rs:11565` (chain_node_line), `portage-repo/src/lib.rs:11804` (best_installed_matching), `portage-repo/src/lib.rs:13244` (resolve_installed_info), `portage-repo/src/lib.rs:13260` (resolve_installed_info), `portage-repo/src/lib.rs:15883` (rev_dep_pin_holdable), `portage-repo/src/lib.rs:19131` (slot_operator_rebuild_entries), `portage-repo/src/lib.rs:20908` (uninstall_entry), `portage-repo/src/lib.rs:24306` (missing_dep_full_row), `portage-repo/src/lib.rs:24322` (missing_dep_full_row), `portage-repo/src/lib.rs:25191` (autounmask_dep_chain), `portage-repo/src/lib.rs:28542` (pkg_of), `portage-repo/src/lib.rs:32632` (run_pass), `portage-repo/src/resolver_trace.rs:135` (node_label), `portage-repo/src/resolver_trace.rs:266` (dump_atom_candidates), `portage-repo/src/resolver_trace.rs:762` (dump_resolution_walk)

**`installed_refs`** (13 sites): `portage-repo/src/lib.rs:6853` (info_pkgs_table), `portage-repo/src/lib.rs:17671` (slot_operator_rebuild_scan), `portage-repo/src/lib.rs:22327` (build_slot_conflict), `portage-repo/src/lib.rs:22865` (build_residual_slot_conflicts), `portage-repo/src/lib.rs:23010` (build_residual_slot_conflicts), `portage-repo/src/lib.rs:23638` (direct_solve_slot_conflicts), `portage-repo/src/lib.rs:23869` (constraint_withheld_updates), `portage-repo/src/lib.rs:24176` (backtrack_missed_updates), `portage-repo/src/lib.rs:30919` (run_pass), `portage-repo/src/lib.rs:31243` (run_pass), `portage-repo/src/lib.rs:31420` (run_pass), `portage-repo/src/lib.rs:31425` (run_pass), `portage-repo/src/lib.rs:31426` (run_pass)

**`installed_versions`** (10 sites): `mrg-director/src/lib.rs:709` (installed_versions), `mrg-director/src/lib.rs:1366` (installed_versions), `portage-repo/src/lib.rs:7778` (libc_provider_cps), `portage-repo/src/lib.rs:19404` (rebuild_if_entries), `portuale/src/ebuild_merge.rs:2952` (installed_instance_pf), `portuale/src/ebuild_merge.rs:3356` (find_collisions), `portuale/src/ebuild_unmerge.rs:614` (unmerge_pkgfiles), `portuale/src/pretend.rs:7278` (run_search), `portuale/src/pretend.rs:7387` (render_ambiguous_search_output), `portuale/src/pretend.rs:8452` (highest_installed_pvr)

**`installed_pkg_iuse_and_use`** (9 sites): `portage-repo/src/lib.rs:22010` (skipped_update_installed_use_display_for), `portage-repo/src/lib.rs:22370` (build_slot_conflict), `portage-repo/src/lib.rs:22375` (build_slot_conflict), `portage-repo/src/lib.rs:22575` (merge_same_slot_conflicts), `portage-repo/src/lib.rs:22910` (build_residual_slot_conflicts), `portage-repo/src/lib.rs:32649` (run_pass), `portage-repo/src/solver_bridge.rs:1022` (resolve_pubgrub), `portuale/src/pretend.rs:8160` (news_item_relevant), `portuale/src/pretend.rs:14368` (jobs_count)

**`libc_provider_cps`** (7 sites): `portage-repo/src/lib.rs:8609` (depclean_cleanlist), `portage-repo/src/lib.rs:8834` (prune_cleanlist), `portage-repo/src/lib.rs:9651` (binary_deps_changed), `portage-repo/src/lib.rs:9715` (deps_changed), `portage-repo/src/lib.rs:18912` (slot_operator_eliminate_rebuilds), `portage-repo/src/merge_order.rs:1194` (add_installed_dependency_closure), `portuale/src/ebuild_phases.rs:2193` (inject_libc_dep)

**`vdb_pkg_dir`** (5 sites): `portage-repo/src/lib.rs:5760` (installed_parent_use_state), `portage-repo/src/lib.rs:6918` (installed_contents_files), `portage-repo/src/lib.rs:7347` (vdb_aux_get), `portage-repo/src/lib.rs:11513` (chain_node_usedep_suffix), `portage-repo/src/lib.rs:13091` (read_vdb_env_vars)

**`vdb_aux_get`** (5 sites): `portage-repo/src/lib.rs:6695` (installed_candidates_uncached), `portage-repo/src/lib.rs:6742` (vdb_entry_slot), `portage-repo/src/lib.rs:7525` (read_vdb_flag_set), `portage-repo/src/lib.rs:7739` (read_vdb_string), `portage-repo/src/lib.rs:7901` (all_installed_packages_uncached)

**`vdb_entry_slot`** (3 sites): `portuale/src/ebuild_merge.rs:2932` (read_installed_slot), `portuale/src/ebuild_merge.rs:3276` (blockers_from_flat_deps), `portuale/src/pretend.rs:6131` (installed_cp_versions)

**`installed_contents_files`** (1 sites): `mrg-director/src/lib.rs:714` (contents_files)

**`installed_reverse_dependents`** (1 sites): `mrg-director/src/lib.rs:722` (reverse_dependents)

**`read_vdb_env_vars`** (1 sites): `portage-repo/src/lib.rs:13298` (resolve_installed_info)

