# #305 S0.1 — Real Portage's installed-database write order, mapped to portuale

Evidence for step S0.1 of `docs/02.305-vdb-backends.opus.md`. Read on 2026-10-04.
Every `file:line` below was read in this session. Paths:

- `V` = `3rdparty/portage/lib/portage/dbapi/vartree.py` (real).
- `PLR` = `3rdparty/portage/lib/portage/util/_dyn_libs/PreservedLibsRegistry.py`.
- `PU` = `3rdparty/portage/lib/portage/util/__init__.py`.
- `FS` = `3rdparty/portage/lib/portage/_sets/files.py`.
- `LK` = `3rdparty/portage/lib/portage/locks.py`.
- `EM` = `rust/portuale/src/ebuild_merge.rs`.
- `EU` = `rust/portuale/src/ebuild_unmerge.rs`.
- `PR` = `rust/portuale/src/pretend.rs`.
- `RP` = `rust/portage-repo/src/lib.rs`.

Real paths (`3rdparty/portage/lib/portage/const.py:48-53`): `VDB_PATH = var/db/pkg`,
`CACHE_PATH = var/cache/edb`, `PRIVATE_PATH = var/lib/portage`, `world`, `world_sets`,
`config` (config memory), all under `PRIVATE_PATH`. The counter file is
`${EROOT}var/cache/edb/counter` (`V:420`).

## 0. Names used in this file

- **Entry dir**: `var/db/pkg/<cat>/<pf>/`.
- **Temp dir**: `var/db/pkg/<cat>/-MERGING-<pf>/` (`V:2032`,
  `const.py:112` `MERGING_IDENTIFIER = "-MERGING-"`; portuale
  `portage-util/src/lib.rs:68`).
- **W1..W5** are the write classes of `docs/vdb_to_db.md` section 3.3.

Note: `write_vdb_entry_from_dir`, named in the plan, does not exist in portuale
(`grep -rn write_vdb_entry_from_dir rust/` finds nothing). The code that does the
work is four functions in `EM`: `create_vdb_tmp` (:2682), `populate_vdb_tmp`
(:2711), `write_vdb_tmp_contents` (:2759) and `publish_vdb_tmp` (:2796). Both
`merge_after_install` (:3834) and `merge_binpkg` (:4558) call them.

## 1. Ordered table: every write in one merge with a same-slot replace

Real entry point: `dblink.merge` (`V:6123-6124`) wraps `dblink.treewalk` (`V:4352`).
Default case, no `FEATURES=parallel-install`.

| # | Real write (what) | Real `V:` line | portuale line | Order matches? |
|---|---|---|---|---|
| 1 | `vardbapi.lock()`: `lockdir(var/db/pkg)`, lock file `${EROOT}var/db/.pkg.portage_lockfile`; held until `:6209` | 6144-6145 (`lockdb` 2082-2083, `lock` 466-482) | none | No equivalent (portuale takes no VDB lock) |
| 2 | `_bump_mtime`: `utime` on category dir, then on VDB root | 6146 (def 570-586) | none | No equivalent |
| 3 | Stale `-MERGING-<pf>` removed (`self.dbdir = self.dbtmpdir; self.delete()` = `rmtree`, then `rmdir` of an empty cat dir), then the temp dir is created (`ensure_dirs`) | 5027-5029 (`delete` 2148-2183, `rmtree` 2169, `rmdir` 2172) | `create_vdb_tmp` EM:2682-2693 (called EM:4006, EM:4836) | Yes (portuale does not `rmdir` the cat dir) |
| 4 | `pkg_preinst` runs (it may write nothing into the VDB) | 5053-5060 | EM:4007-4018 | Yes |
| 5 | Info files (`build-info/*`) copied into the temp dir, `shutil.copyfile` (non-atomic) | 5072-5073 (`copyfile` 6228-6229) | `populate_vdb_tmp` EM:2729 (`std::fs::copy`; called EM:4021, EM:4841) | Yes |
| 6 | `counter_tick()`: under `vardbapi.lock()`, `write_atomic(var/cache/edb/counter, N)` (temp file + rename), then `COUNTER` written into the temp dir (plain `open(..., "w")`) | tick 1301-1302 and 1372-1397, write 1392/1395; `COUNTER` 5076-5084 | `next_counter` EM:2657-2671 (non-atomic `std::fs::write`, no lock, no max scan); called EM:2734, `COUNTER` file EM:2743 | Same position; differs in mechanism (see W5) |
| 7 | Preserved-libs registry pre-prune: `_fs_lock` (lock on `config`), `plib_registry.lock/load/store/unlock` (load runs `pruneNonExisting`, `PLR:97`) | 5089-5103 | `read_plib_registry` EM:1111-1121 prunes in memory; persisted only by the next `write_plib_registry` | Partly: no separate pre-merge `store` |
| 8 | Config memory read: `grabdict(var/lib/portage/config)` | 5108 | `read_cfgfiledict` EM:918, called EM:4033 | Yes |
| 9 | `_merge_contents`: `CONTENTS` written to the temp dir through `atomic_ofstream` (temp file + rename); files copied to `${ROOT}` | 5403-5418, 5432-5474 | `merge_tree` + string `contents`; written later at row 12 | Order differs: portuale writes `CONTENTS` once, later |
| 10 | Config memory written: `writedict(cfgfiledict, var/lib/portage/config)` = `write_atomic`, only if the dict changed | 5478-5486 (call 5482/5485; `writedict` `PU:682-692`) | `write_cfgfiledict` EM:931-944 (`std::fs::write`), called EM:4072 (source), EM:4873 (binpkg) | Yes, one step later (after the preserve computation; no VDB write between) |
| 11 | Preserved libs: `_add_preserve_libs_to_contents` rewrites the temp dir `CONTENTS` (atomic) | 5171-5172 (def 3775-3826, write 3823) | `inject_preserved_libs_into_contents` result appended to `contents` (EM:4058-4071) | Merged into row 12 (one write instead of two; same bytes by design) |
| 12 | (portuale only) `CONTENTS` written to the temp dir | (real wrote it at rows 9 and 11) | `write_vdb_tmp_contents` EM:2759-2782 (`std::fs::write` at EM:2778; called EM:4073, EM:4874) | Equivalent end state |
| 13 | Same-slot replace loop: `dblnk.unmerge(...)`: `_bump_mtime` (2556), prerm, remove files, postrm, `_prune_plib_registry(unmerge=True)` (2655, W4 and registry), `env_update`, `_bump_mtime` (2717) | 5190-5208 (unmerge 2481-2839) | `unmerge_replaced_same_slot` EM:4223 (called EM:4100, EM:4899), `unmerge_one_installed` EM:4371, `unmerge_pkgfiles` EU:592, `preserve_libs_on_unmerge` EM:1810 | Yes, except no `_bump_mtime` |
| 14 | **Old entry deleted**: `dblnk.delete()` under `lockdb` (`rmtree` of the old entry dir; `rmdir` of an empty cat dir). The new entry is still `-MERGING-`. | 5216-5221 (def 2148-2183) | `delete_vdb_dir` EU:721-728 (called EM:4455) | Yes |
| 15 | `_consolidate_to_metadata_file(self.dbtmpdir)`: body written (`write_atomic`), then stamp appended, **last write into the temp dir** | 5223-5231 (def 232-283, `_write_metadata_file` 208-229, `_stamp_metadata_file` 188-205) | `write_consolidated_metadata_file` EM:2865-2903 (called EM:2780, so before the replace loop; same bytes, see EM:2750-2757 comment) | Position differs (consolidate before unmerge), result identical |
| 16 | **New entry published**: `self.delete()` of any live same-`pf` entry, then `_movefile(dbtmpdir, dbpkgdir)` (rename), under `lockdb` | 5234-5241 (`delete` 5237, `_movefile` 5238) | `publish_vdb_tmp` EM:2796-2805 (`remove_dir_all` EM:2801, `rename` EM:2803; called EM:4119, EM:4910) | Yes |
| 17 | Blockers: `removeFromContents(blocker, contents)` under `lockdb` (W4 on blockers) | 5251-5259 | no equivalent (portuale never builds `dblink._blockers`; see EM:3128-3131 comments) | No equivalent |
| 18 | Preserved-libs registry: `register(self.mycpv, ...)`, plib-collision handling (`removeFromContents(cpv, paths)` on the previous owner = W4, `register(...remaining)`), then `plib_registry.store()` (atomic JSON) under `_fs_lock` + `plib_registry.lock` | 5261-5328 (register 5270, collisions 5276-5323, `removeFromContents` 5295, store 5325) | Registration: `register_merge_preserved_libs` EM:1731-1762 (called EM:4120, EM:4914) after publish: matches. Collisions: `unregister_preserved_libs` EM:1316-1341 (called EM:4077, EM:4882) **before** the replace loop and publish | Registration yes; collision handling **no** (portuale does it at row 12.5, before rows 13-16) |
| 19 | `vardbapi._add(self)`: in-memory cache invalidation only (no disk write) | 5330, 829-835 | none | n/a |
| 20 | `pkg_postinst` with `PORTAGE_UPDATE_ENV=<entry>/environment.bz2`: bash rewrites `environment.bz2` **in place** (`> file`), so no directory-entry change | 5334-5346; `bin/phase-functions.sh:1072-1082` | postinst EM:4166 (and EM:4934); `PORTAGE_UPDATE_ENV` EM:4136-4152 (env var set up at the same place) | Yes |
| 21 | `env_update` (writes under `${ROOT}etc`, not the VDB), then `_prune_plib_registry()` (W4 on owners of removed preserved libs + registry `store`), `_post_merge_sync` | 5366-5380 (prune def 2390-2478, `removeFromContents` 2472, store 2476) | `run_env_update` EM:4177; `prune_unused_preserved_libs` EM:2031 (call EM:4184); `remove_from_contents` EM:2092; `write_plib_registry` EM:2106 | Yes |
| 22 | `merge()` finally: `_bump_mtime`, `unlockdb` | 6205-6209 | none | No equivalent |
| 23 | World file `emerge`-side, not in the merge subprocess: `Scheduler._world_atom` runs after each package merge (`EbuildMerge._merge_exit` calls it) | `_emerge/EbuildMerge.py:68`; `_emerge/Scheduler.py:2549-2603` | `update_world_file` PR:3537-3575; calls PR:5842, PR:15659, **PR:16042** (once after all merges) | **No**: real is per package, portuale once at the end of the run |

Row 12.5: in portuale, `unregister_preserved_libs` runs between `write_vdb_tmp_contents`
and `unmerge_replaced_same_slot` (EM:4072-4078). Real does the same work at
`V:5276-5325`, after `publish`. In real, the `COUNTER` and registry `counter` the
collision code reads (`V:5290`, `cpv_counter`) come from an installed entry; in
portuale the replaced entry is still live at the time. This is the only
reordering of writes found.

## 2. Write classes

### W1 — insert a whole entry

Real: build in the temp dir, then rename (rows 3, 5, 6, 9, 15, 16).

- Temp dir created: `V:5027-5029`.
- Files copied in: `V:5072-5073`; `COUNTER` `V:5078-5084`; `CONTENTS` `V:5412-5418`.
- Consolidated `metadata` written and stamped last: `V:5231` (see section 3).
- Rename into place: `V:5237-5238`. The comment at `V:5224-5230` says: "Renaming
  dbtmpdir into place below does not alter its own mtime, so the recorded value
  survives the move."
- A crash before the rename leaves `-MERGING-<pf>`. Readers skip it:
  `_excluded_dirs` regex, `V:352`.

portuale: `create_vdb_tmp` EM:2682, `populate_vdb_tmp` EM:2711,
`write_vdb_tmp_contents` EM:2759 (plus the `metadata` file, `EM:2865`),
`publish_vdb_tmp` EM:2796. Readers skip the temp dir through
`portage_util::is_merging_vdb_entry` (`rust/portage-util/src/lib.rs:74`).
The files are written with `std::fs::write` and `std::fs::copy` (in place), not
temp-file-plus-rename. The step order matches real.

### W2 — delete an entry

Real: `dblink.delete` (`V:2148-2183`): `lstat` the dir, check `dbdir.startswith(dbroot)`
(2161), `shutil.rmtree` (2169), `os.rmdir(<cat dir>)` ignoring failure (2172),
`vardbapi._remove` (2175, cache invalidation), `_post_merge_sync` (2183).
Standalone unmerge: `unmerge()` calls `mylink.unmerge`, then `mylink.delete()` under
`lockdb`, only on success (`V:6478-6487`).

portuale: `delete_vdb_dir` (EU:721-728): `remove_dir_all` then `remove_dir(cat)`.
Standalone `run_unmerge` calls it at EU:792; the replace path at EM:4455.
Equivalent. No `_bump_mtime`, no `_post_merge_sync`, no lock.

### W3 — replace in slot (= W1 + W2)

Real order (`V:5190-5241`): unmerge old files (5203), **delete the old entry (5219)**,
consolidate `metadata` in the temp dir (5231), delete any same-`pf` live entry (5237),
rename the temp dir into place (5238). The old entry is gone **before** the new
one is visible, so there is a window in which the slot has no installed entry.
The new entry stays `-MERGING-` (invisible) until the rename, so `has_version` sees
the old entry until row 14, then nothing, then the new one after row 16.

portuale: same order. `unmerge_replaced_same_slot` EM:4223 (loop at EM:4272,
`unmerge_one_installed` → `delete_vdb_dir` EM:4455), then `publish_vdb_tmp` EM:2796
(which also `remove_dir_all`s a live same-`pf` entry, EM:2800-2802, the reinstall case).
The `publish_vdb_tmp` doc comment (EM:2787-2795) acknowledges the delete-then-move
exposure on same-`pf` reinstall.

### W4 — rewrite CONTENTS / NEEDED (preserved libs)

Real, three sites:

1. **In the temp dir**, before the move: `_add_preserve_libs_to_contents`
   (`V:3775-3826`), `atomic_ofstream` at 3823.
2. **In an installed entry**, `vardbapi.removeFromContents` (`V:1414-1480`):
   - It computes `new_contents` (1429-1447). If anything was removed (`if removed:`, 1449), it also
     filters `NEEDED.ELF.2` (1449-1478).
   - It writes through `writeContentsToContentsFile` (`V:1480`, def 1482-1506):
     `_bump_mtime` (1496), `NEEDED.ELF.2` via `atomic_ofstream` (1498-1501) if
     `new_needed is not None`, `CONTENTS` via `atomic_ofstream` (1502-1504),
     `_bump_mtime` (1505), clear the contents cache (1506).
   - Callers: 2457 (unmerge-time preserve, on `self`), 2472 (unused preserved libs),
     5255 (blockers), 5295 (plib collisions).
3. `aux_update` (`V:1127-1157`): `_bump_mtime`, `setfile` per key (`write_atomic`,
   6241-6250), `_consolidate_to_metadata_file(pkgdir)` if a `metadata` file exists
   (1151-1155), `_bump_mtime`. Not part of a merge.

Every real in-place edit is temp file + `rename`, so it **changes the entry dir's
mtime** and so makes any earlier `#dir_mtime=` stamp stale. `writeContentsToContentsFile`
does not re-stamp. This is harmless: `CONTENTS`, `NEEDED` and `NEEDED.ELF.2` are not in
`_METADATA_FILE_FIELDS` (`V:69-104`; the comment at `V:70-77` says line-oriented
fields are absent), so the reader just falls back to per-field reads for the 23
fields.

portuale: `remove_from_contents` (EM:1370-1417):

- Reads `CONTENTS` (EM:1376), filters lines, and **always** rewrites it with
  `std::fs::write` (EM:1399), even when nothing was removed (real writes only if
  `removed`, `V:1449` gate).
- When `removed`, filters `NEEDED.ELF.2` and rewrites it in place (EM:1402-1411).
- No `_bump_mtime`. `std::fs::write` truncates in place: no dirent change, so the
  entry dir mtime is **unchanged** and an existing `#dir_mtime=` stamp stays valid.
- Callers: EM:1338 (`unregister_preserved_libs`), EM:2092 (`prune_unused_preserved_libs`).
- Not mirrored: real `removeFromContents(self, unmerge_preserve)` at `V:2457`; portuale
  passes the preserved set to `remove_contents` instead (EU:656-678), and the entry is
  deleted right after anyway.
- `preserved_libs.rs` and `needed_elf.rs` compute only: their `fs::write` calls are
  all in `#[cfg(test)]` code (`needed_elf.rs:1542,1570,1730,2013`). No production
  writes there.

### W5 — counter

Real: `counter_tick` (`V:1301-1302`) → `counter_tick_core` (`V:1372-1397`):

```python
self.lock()
try:
    counter = self.get_counter_tick_core() - 1          # V:1386
    if incrementing:
        counter += 1
        write_atomic(self._counter_path, str(counter))   # V:1392 (V:1395 retry)
    self._cached_counter = counter                      # V:1396
finally:
    self.unlock()
```

`get_counter_tick_core` (`V:1304-1370`) reads the file (-1 on missing or corrupt),
then, unless `self._cached_counter == counter` (1351), scans every installed
package's `COUNTER` (1363-1368) and returns `max(file, all entries) + 1` (1370).
So real never goes backwards past an installed entry's counter.
`COUNTER` in the entry is the same value (`V:5076-5084`).
`cpv_counter` (`V:592-608`) reads the entry's `COUNTER` for the registry.

portuale: `next_counter` (EM:2657-2671): read file (`-1` on missing or unparsable,
EM:2659-2662), `+1`, `create_dir_all`, **non-atomic** `std::fs::write` (EM:2667).
No lock, no max scan (the doc comment EM:2650-2656 says so).
It is called from `populate_vdb_tmp` (EM:2734). Then the `COUNTER` file in the temp
dir is written (EM:2743). `register_merge_preserved_libs` reads the new counter back
from the entry (EM:1742-1748).

## 3. The `metadata` file: how real writes it and the exact acceptance rule

### 3.1 Format and fields (`V:105-107`, `V:69-104`)

```
_METADATA_FILE_FORMAT_VERSION = 1            # V:105
_METADATA_FORMAT_PREFIX = "#format="         # V:106
_METADATA_DIR_MTIME_PREFIX = "#dir_mtime="   # V:107
```

23 fields in `_METADATA_FILE_FIELDS` (`V:78-104`): BDEPEND, BUILD_ID, BUILD_TIME, CHOST,
COUNTER, DEFINED_PHASES, DEPEND, DESCRIPTION, EAPI, HOMEPAGE, IDEPEND, IUSE, KEYWORDS,
LICENSE, PDEPEND, PROPERTIES, PROVIDES, RDEPEND, REQUIRES, RESTRICT, SLOT, USE,
repository. (Same set as portuale `portage_repo::METADATA_FILE_FIELDS`, `RP:7234-7257`.)

### 3.2 Writer: body, `stat`, stamp last

`_write_metadata_file` (`V:208-229`):

```python
path = os.path.join(dbdir, _METADATA_FILE)                                    # 224
content = f"{_METADATA_FORMAT_PREFIX}{_METADATA_FILE_FORMAT_VERSION}\n"       # 225
content += "".join(f"{k}={' '.join(v.split())}\n" for k, v in sorted(data.items()))  # 226
write_atomic(path, content, mode="w", encoding=_encodings["repo.content"])    # 227
if stamp:                                                                     # 228
    _stamp_metadata_file(dbdir)                                               # 229
```

`write_atomic` = temp file in the same dir + rename (`PU:1519-1535`,
`atomic_ofstream` `PU:1372-...`, `tempfile.mkstemp(prefix=basename, dir=parent)`), so it
**bumps the entry dir's mtime** (new dirent). `_stamp_metadata_file` (`V:188-205`):

```python
path = os.path.join(dbdir, _METADATA_FILE)                                        # 203
with open(path, mode="a", encoding=_encodings["repo.content"]) as f:              # 204
    f.write(f"{_METADATA_DIR_MTIME_PREFIX}{os.stat(dbdir).st_mtime_ns}\n")        # 205
```

An append creates no dirent, so it leaves `dbdir`'s mtime unchanged and the recorded
value stays true (docstring `V:189-199`). `_consolidate_to_metadata_file`
(`V:232-283`) reads every file in `dbdir` that `_in_metadata_file` accepts
(`V:262-274`), whitespace-normalised (`" ".join(f.read().split())`, `V:272`), and calls
`_write_metadata_file(dbdir, data, stamp=not delete_individual)` (`V:276`). It is a
no-op if a currently valid `metadata` file exists (`V:253-259`). Merge calls it as the last
write into the temp dir (`V:5231`, rationale in the comment `V:5223-5230`).

portuale writer: `write_consolidated_metadata_file` (EM:2865-2903): `std::fs::write` of
the body (EM:2887; in place, not temp+rename, but a new `metadata` file still adds a
dirent the first time), then `std::fs::metadata(dbdir)` (EM:2894), compute
`st_mtime_ns` (EM:2895), reopen with `.append(true)` (EM:2897) and write
`#dir_mtime=` (EM:2901). Same order as real. No-op when no field file exists (EM:2877-2879).

### 3.3 Reader: the exact acceptance rule

`_read_metadata_file(path, dir_st=None)` (`V:115-185`):

```python
if version is None or dir_mtime is None:          # V:176
    return None
if dir_st is None:                                # V:178
    try:
        dir_st = os.stat(os.path.dirname(path))   # V:180
    except OSError:
        return None
if dir_mtime != dir_st.st_mtime_ns:               # V:183
    return None
return result                                     # V:185
```

Also: a `#format=` line that does not parse, or is not `1`, returns `None`
immediately (`V:157-165`); an unparsable `#dir_mtime=` returns `None` (`V:166-170`);
other `#` lines are ignored (`V:171`); lines without `=` are skipped (`V:172-173`); `k, v = line.split("=", 1)`,
last duplicate wins (`V:174-175`). The rule is therefore: **the `metadata` file is
accepted iff `#format=1` is present AND `#dir_mtime=<int>` is present AND that
integer equals the entry directory's `st_mtime_ns` (nanoseconds, equality, not
`>=`)**. A truncated write lacks `#dir_mtime=` and is rejected. When accepted, the
dict is a *complete* snapshot: a field missing from it is served as `""` without
reading the field file (`V:1012-1019`, caller `_aux_get` `V:975-1053`: `dir_st=st`
at `V:994-996`). When rejected, `_aux_get` falls back to the per-field files.

portuale reader: `read_metadata_file` (`RP:7282-7309`): same rule
(`RP:7303`, `version.is_none() || dir_mtime.is_none() || dir_mtime != Some(dir_mtime_ns)`),
called from `vdb_aux_get` (`RP:7345`, stat at `RP:7356`, read at `RP:7412`).
`dir_mtime_ns = st.mtime() * 1e9 + st.mtime_nsec()` (`RP:7356`).
`vdb_aux_get` also memoises per instance, keyed on `dir_mtime_ns` (`RP:7370-7433`).

### 3.4 `_bump_mtime` rules (bug #290428)

`V:570-586`:

```python
def _bump_mtime(self, cpv):
    """
    This is called before an after any modifications, so that consumers
    can use directory mtimes to validate caches. See bug #290428.
    """
    base = self._eroot + VDB_PATH
    cat = catsplit(cpv)[0]
    catdir = base + os.sep + cat
    t = time.time()
    t = (t, t)
    try:
        for x in (catdir, base):
            os.utime(x, t)
    except OSError:
        ensure_dirs(catdir)
```

- It sets `atime` = `mtime` = now on the **category dir and the VDB root**
  (`var/db/pkg`). It does **not** touch the entry dir (the task text said "category
  dir + entry dir"; the code says `(catdir, base)`).
- If `utime(catdir)` fails, `ensure_dirs(catdir)` creates it and the loop stops, so
  the root is not bumped in that call.
- Called before **and** after every change: `merge` 6146 / 6207; `unmerge` 2556 / 2717;
  `aux_update` 1135 / 1156; `writeContentsToContentsFile` 1496 / 1505.
- Not called by `delete()` itself, nor around the rename (the rename and `rmtree`
  change the cat dir mtime on their own).
- portuale: no `_bump_mtime` equivalent (`grep` in `EM`, `EU`: none). Its caches rely
  on the category dir's own mtime changing through `rename`/`rmdir`/`remove_dir_all`
  (`RP:6586-6595`, `dir_mtime_nanos` at `RP:6595`, used by `installed_candidates`,
  `RP:6625`; the doc comment above it says the same). An in-place W4 rewrite
  changes no directory mtime in portuale, whereas real bumps the cat dir and root.

## 4. Locking

| Lock | Real | portuale |
|---|---|---|
| `vardbapi.lock()` / `lockdb` | `lockdir(${EROOT}var/db/pkg)` = `lockfile(..., wantnewlockfile=1)` (`V:466-482`, `LK:167-168`), lock file `<dirname>/.<basename>.portage_lockfile` = `${EROOT}var/db/.pkg.portage_lockfile` (`LK:247-250`). Reentrant (`V:471-480`). Held for the whole `merge` unless `parallel-install` (`V:6143-6145`, `6208-6209`), and `unmerge()` (`V:6477-6478`, `6496-6497`). Also taken around `delete` (5216, 5235), blockers (5252), counter (`V:1382`) | none (`grep` for `flock`/`lockfile` in `EM` returns nothing; `PortageLockfile` in `portage_lock.rs:1-40` is used only by `ebuild_package.rs:793` and `fetch.rs:92`) |
| `_slot_lock` | `vardbapi._slot_lock(slot_atom)` (`V:533-552`): `lock_path = getpath("<cp>:<slot>")` = `${EROOT}var/db/pkg/<cat>/<pn>:<slot>`; `lockfile(lock_path, wantnewlockfile=True)` (lock file `${EROOT}var/db/pkg/<cat>/.<pn>:<slot>.portage_lockfile`). Taken only with `FEATURES=parallel-install` by the `_slot_locked` decorator (`V:2088-2104`), on `cp:slot` of the package and each blocker, sorted (`V:2107-2131`); on `merge` (6123) and `unmerge` (2481) | none |
| Config memory | `vardbapi._fs_lock()` = `lockfile(${EROOT}var/lib/portage/config)` (locks the file itself, no `wantnewlockfile`) (`V:510-518`); held at 5089-5118, 5140-5169, 5263-5328, and in `_prune_plib_registry` (2398-2477) | none |
| Registry | `PreservedLibsRegistry.lock()` = `lockfile(<registry file>)` (`PLR:39-43`); `store()` replaces the inode atomically, which defeats the lock (docstring `PLR:99-106`: "must not call until the lock is ready to be released") | none |
| World | `WorldSelectedPackagesSet.lock()` = `lockfile(world, wantnewlockfile=1)` (`FS:324-328`) → `var/lib/portage/.world.portage_lockfile`; `world_sets` likewise (`FS:413-416`) | none |

## 5. D4 stores

### 5.1 `world`

- Real writer: `WorldSelectedPackagesSet.write` (`FS:287-290`): `write_atomic(world,
  sorted "<atom>\n")`; `world_sets` file: `WorldSelectedSetsSet.write` (`FS:382-385`),
  same. Callers: per package, `Scheduler._world_atom` (`_emerge/Scheduler.py:2549-2603`:
  `world_set.lock()`, `load()`, `cleanPackage` on uninstall or `world_set.add(atom)`,
  `unlock()`); `add`→`update`→`write` (`_sets/base.py:160-190`, write at 183, 190, 197,
  204). Called after each merge (`_emerge/EbuildMerge.py:68`) and uninstall
  (`_emerge/PackageUninstall.py:122`). `depgraph.saveNomergeFavorites` (`_emerge/depgraph.py:11305`,
  lock 11322-11323, load 11326-11327, `update` 11399, `unlock` 11402), called from
  `_emerge/actions.py:674`, writes the "nomerge" favorites and `@sets`.
  `emerge --deselect`: `_emerge/actions.py:1743-1834` (lock 1754-1755, `replace`, unlock 1834).
  `cleanPackage` (`FS:336-361`).
- portuale: plain `std::fs::write` (non-atomic, no lock), file contents sorted and
  deduplicated. `update_world_file` PR:3537 (write at PR:3572); calls PR:5842
  (resume), PR:15659 (ask path), **PR:16042** (after all merges). Uninstall:
  `deselect_from_world` PR:6041 (write PR:6073), called PR:5969. Deselect: `run_deselect`
  PR:3957 (writes both files, loop PR:4116-4126, `fs::write` PR:4122).

### 5.2 `world_sets`

- Real: as above (`FS:369-424`), `filename = "world_sets"` messages at `depgraph.py:11391`.
- portuale: `update_world_sets_file` PR:3708 (write PR:3738; calls PR:15671, PR:16065);
  read side `read_world_sets` PR:3731 region (`PR:3732`); `run_deselect` rewrites it at PR:4122.

### 5.3 `preserved_libs_registry`

- Real: `${EROOT}var/lib/portage/preserved_libs_registry` (`V:422-425`). `store()`
  (`PLR:99-125`): `atomic_ofstream(file, "wb")`, JSON; **skipped** when
  `SANDBOX_ON == "1"` or `_data == _data_orig` (`PLR:107`). Callers: `V:5101` (pre-merge prune),
  `V:5325` (post-merge register + collisions), `V:2476` (in `_prune_plib_registry`, unmerge
  and post-merge prune). `register`/`unregister` `PLR:142-176`.
- portuale: `write_plib_registry` EM:1246-1279: skipped when `entries == orig_entries`
  (EM:1247); JSON hand-formatted with tabs; `std::fs::write` (EM:1279). Writers:
  EM:1341 (`unregister_preserved_libs`), EM:1761 (`register_merge_preserved_libs`),
  EM:1879 (`preserve_libs_on_unmerge`, unmerge-time register), EM:2106
  (`prune_unused_preserved_libs`). Registry path `plib_registry_path` EM:983-985.

### 5.4 Config memory (`/var/lib/portage/config`)

- Real: `CONFIG_MEMORY_FILE` (`const.py:53`), `vardbapi._conf_mem_file` (`V:407`). Writes:
  `writedict(cfgfiledict, ...)` = `write_atomic`: merge `V:5478-5486`; unmerge (stale
  entries) `V:3267-3271`. Merge writes only if the dict changed (`V:5479`
  `if cfgfiledict != cfgfiledict_orig`, `cfgfiledict.pop("IGNORE")` first).
- portuale: `cfg_mem_path` EM:910-912; `read_cfgfiledict` EM:918; `write_cfgfiledict`
  EM:931-944 (`std::fs::write`; not conditional on a change inside the function; the merge
  call at EM:4072 / EM:4873 is unconditional). Unmerge: `EU:679-684` (only if
  `stale_confmem` is non-empty).

### 5.5 `counter` (`/var/cache/edb/counter`)

See W5. Real `V:420` path, `V:1372-1397`; portuale `next_counter` EM:2657-2671.

## 6. Implications for `FilesDb` / converters

1. **Write order** for a `FilesDb` entry: temp dir `-MERGING-<pf>` → per-field files →
   `COUNTER` → `CONTENTS` (once, final bytes) → `metadata` body → **stat the entry dir →
   append `#dir_mtime=`** (`V:188-229`, `V:5231`) → [replace loop: delete old] → rename
   the temp dir over `<pf>` (`V:5237-5238`). The rename is last and must stay within
   the same category dir (the stamp survives only because the dir's own mtime does not
   change when it is renamed inside the same parent, `V:5224-5230`).
2. **Never copy a stored `#dir_mtime=`**; always compute it from the new directory
   (design section 11 already says so). The stamp is an equality test on nanoseconds.
3. **Converters that restore directory mtimes must do it before stamping**, not after:
   `vdb_to_db.md` section 11 says "Directory mtimes are applied last"; but `utimensat` on the
   entry dir after the stamp changes `st_mtime_ns` and invalidates the `metadata` file
   (falls back to per-field reads: correct but slow). Order: write all files, set the
   entry dir mtime, `stat`, append the stamp, and do not add or remove entries afterwards.
   (Whether the restored mtime should be kept at all is a design choice; `_bump_mtime`
   itself uses "now".)
4. **Anything that adds or removes a dirent after stamping makes the snapshot stale.**
   Real W4 (`atomic_ofstream` temp+rename, `V:1498-1504`) does this and tolerates it
   (fallback). `FilesDb` may match real (temp+rename, snapshot goes stale) or keep
   today's portuale behaviour (in-place `write`, stamp stays valid, EM:1399); both
   read correctly because the `metadata` field set excludes `CONTENTS`/`NEEDED*`.
5. **`_bump_mtime` parity**: if other consumers (real `emerge`, eix) share the host
   VDB, `FilesDb` must `utime(catdir)` and `utime(var/db/pkg)` before and after each
   write batch (`V:570-586`). portuale does not today. In-place edits like W4 need it
   most, because they change no directory mtime.
6. **Delete before publish** is the real replace order (`V:5219` before `V:5238`).
   The database backends can make replace atomic (one commit), which is an improvement,
   not parity; `FilesDb` must keep the order (and the no-entry window), as the design
   already says.
7. **Counter**: real takes `max(counter file, every entry's COUNTER) + 1` under the VDB
   lock and writes with temp+rename. `FilesDb::next_counter` today (`EM:2657`) trusts the
   file only; a converter must copy the source's `counter` and keep every entry's
   `COUNTER` unchanged (design section 11: "Counters are preserved"), and a new backend's
   counter must be `>= max(entry COUNTER)`.
8. **Registry, config memory, world, world_sets are plain files** (`world*` and `config`
   are small text; the registry is JSON). Real writes them atomically, each under its own
   lock; portuale writes them in place without locks. `FilesDb::set_*` should use
   temp+rename (compatible with both) and may take the same `.portage_lockfile`
   names when real Portage can be running at the same time.
9. **World timing**: real writes `world` per package after each merge (and after each
   uninstall); portuale writes it once at the end of the run (PR:16042). A `WriteTxn`
   that folds `set_world` into the merge commit (design section 9) changes this timing;
   keep an explicit decision about whether `emerge` parity (per package) or one write per
   run is wanted for `mrg`.
10. **Plib collisions**: real registers/unregisters collisions after the entry is
    published (`V:5261-5328`); portuale does it before the replace loop (EM:4072-4078).
    The sqlite/redb backends commit everything together so the order is irrelevant
    there; `FilesDb` must pick one (portuale's current order is the tested one).
11. **Crash states**: only `-MERGING-<pf>` leftovers; the `files` backend must keep
    skipping them in readers (`V:352`, `is_merging_vdb_entry`) and the next merge
    wipes it (`V:5027-5029`, EM:2682).
12. **Locks**: none in portuale today; a `FilesDb` that wants to coexist with real
    `emerge` should take `${EROOT}var/db/.pkg.portage_lockfile` around each write
    batch (section 4).

## 7. Discrepancies found between design/plan and code

1. The plan (S0.1, `02.305-vdb-backends.opus.md:140-155`) names `write_vdb_entry_from_dir`;
   the function does not exist. Real write steps are `create_vdb_tmp`,
   `populate_vdb_tmp`, `write_vdb_tmp_contents` (which calls
   `write_consolidated_metadata_file`) and `publish_vdb_tmp` in `EM`.
2. The task text says `_bump_mtime` bumps "category dir + entry dir"; the code bumps
   the **category dir and the VDB root** (`V:578-583`), not the entry dir.
3. `vdb_to_db.md` section 9 sequence shows `finish_entry · delete_entry(old)` and the table says
   "Replace = write the new entry, then a separate `rm -rf` of the old one". Real (and portuale) is
   the reverse: **delete the old entry first (`V:5219`), then rename the new one in (`V:5238`)**, so
   there is a window with neither entry visible.
4. `vdb_to_db.md` section 11 says "Directory mtimes are applied last". With the stamp rule
   (`V:183`), applying an mtime after the stamp invalidates the `metadata` file
   (section 6, item 3).
5. `vdb_to_db.md` section 3.3 (W5): "counter + 1 (unlocked today)". True for portuale; real takes
   `vardbapi.lock()` and a max scan (`V:1372-1397`, `V:1351-1370`).
6. portuale's world write is once per run (PR:16042), real is per package (`EbuildMerge.py:68`).
7. portuale reorders plib collision handling before the replace loop (EM:4072-4078);
   real does it after publish (`V:5261-5328`).
8. portuale `remove_from_contents` rewrites `CONTENTS` even when nothing was removed
   (EM:1399); real only when `removed` (`V:1449`, `writeContentsToContentsFile`).
9. `RP` comment cites `vartree.py:105` for the format constant and `:115-187` for the
   reader: the constant is at `V:105`, `_read_metadata_file` spans `V:115-185`
   (minor, off by two lines at the end).
