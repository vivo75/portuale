# 04 — Package model and resolution helpers

## 4.1 `Package(Task)` (`lib/_emerge/Package.py:26`)

Comparable candidate node. Identity:
`_gen_hash_key(type,root,cpv,operation,repo,build_id,size,time,mtime)`
(`Package.py:261`).

| Group | Symbols | Behavior |
|---|---|---|
| Construction | `__init__(**kwargs)` (:99) | Binds `root_config/root`, wraps metadata, validates `SLOT/IUSE/EAPI`, sets `operation=merge\|nomerge`, `slot_atom=cp:slot`. |
| Lazy metadata | `eapi/:175`, `build_id/:179`, `build_time/:183`, `defined_phases/:189`, `properties/:193`, `provided_cps/:201`, `restrict/:205`, `metadata/:205(deprecated)`, `invalid/:216`, `masks/:224`, `visible/:230`, `validated_atoms/:236`, `stable/:247`, `provides/:251`, `requires/:256` | Each computes on first access from wrapped metadata / mask evaluation. |
| Validation | `_validate_deps()` (:312) | Parses all `*DEPEND` without USE expansion; records `invalid` categories. |
| Copy | `copy()` (:415) | Re-constructs same `cpv/root/type/operation`. |
| Masks | `_eval_masks()` (:428) | Collects `invalid/CHOST/EAPI/KEYWORDS/PROPERTIES/RESTRICT/package.mask/LICENSE`, else `False`. |
| Visibility | `_eval_visibility(masks)` (:483) | `False` for unsupported-EAPI/invalid, uninstalled + `CHOST/deprecated-EAPI/KEYWORDS/PROPERTIES/RESTRICT`, or `package.mask\|LICENSE`. |
| Keyword detail | `get_keyword_mask()` (:505), `isHardMasked()` (:528) | `None\|missing\|unstable`; hard-mask boolean. |
| Errors | `_metadata_exception` (:534), `_invalid_metadata` (:559) | Classify dep syntax errors (installed pkgs show the `vdb` path). |
| Display/select | `__str__` (:568), `syspkg_wanted` (:611), `binpkg_wanted(exclude)` (:621) | String form; binary-vs-source preference predicates. |
| USE | `_use` (:645), `use` (:709), `_get_pkgsettings` (:714), `_init_use` (:719), `_iuse` (:763), `with_use(use)` (:870) | Effective USE computation + clone-with-overridden-USE. |
| Compare/iter | `__len__` (:820), `__iter__` (:823), `__lt__/__le__/__gt__/__ge__` (:830) | Version comparison passthrough. |
| Metadata view | `class _PackageMetadataWrapper` (:892): `__init__/:906`, `__getitem__/:916`, `__setitem__/:942`, `_set_inherited/_set_counter/_set_use/_set__mtime_/:947`, `properties/restrict/defined_phases/:981` | Lazy metadata view syncing `USE/_mtime_/counter`. |

## 4.2 `MergeListItem(CompositeTask)` (`MergeListItem.py:16`)

Scheduler wrapper `pkg + pkg_to_replace`.

- `_start()` (:40): installed → `NOOP`; else launch `EbuildBuild` or
  `Binpkg` with progress/log; honors `fetchonly/pretend`.
- `create_install_task()` (:127): `PackageUninstall` for replaces,
  `NOOP` for `fetchonly/buildpkgonly/pretend`, else the inner build's
  install task.

## 4.3 `RootConfig` (`RootConfig.py:5`)

Per-`$EROOT` context. Slots: `mtimedb, root, setconfig, sets, settings,
trees`. Maps: `pkg_tree_map={ebuild:porttree, binary:bintree,
installed:vartree}` + inverse.

- `__init__(settings,trees,setconfig)` (:17): `root=settings[EROOT]`,
  `sets=setconfig.getSets() or {}`.
- `update(other)` (:27): shallow-copy all slots.

## 4.4 `FakeVartree` (`FakeVartree.py:41`)

In-memory unlocked copy of `vartree` used during resolution.

- `FakeVardbGetPath.__init__/__call__(cpv,filename)` (:21): synthesizes
  `$EROOT/var/db/pkg/cpv[/file]`.
- `__init__(root_config,pkg_cache,pkg_root_config,dynamic_deps,ignore_built_slot_operator_deps,soname_deps)` (:53):
  snapshots aux keys, wraps `dbapi` in `PackageVirtualDbapi`
  (+`ProvidesIndex` when sonames enabled), hooks `aux_get/match` for
  dynamic deps.
- `root(prop)` (:99, deprecated): `settings[ROOT]`.
- `_match_wrapper` (:110) / `_aux_get_wrapper` (:124): on first touch,
  refresh `Package` metadata from the live ebuild (`dynamic_deps`) else
  apply global `updates/`.
- `_apply_dynamic_deps(pkg,live_metadata)` (:146): prefer live `*DEPEND`
  when both EAPIs are supported; preserve built `:=` slot-op atoms.
- `dynamic_deps_applied` (:193) / `dynamic_deps_preload` (:204) /
  `cpv_discard` (:210): idempotence / eager apply / evict + cache purge.
- `sync/__sync__/_sync` (:220-287) / `_pkg(cpv)` (:287): reload
  `cpv_all`, drop stale, validate `COUNTER/_mtime_`, keep highest
  counter per slot, inject `Package(installed)`.
- `grab_global_updates(portdb)` (:308) /
  `perform_global_updates(cpv,aux,mydb,updates)` (:336): load
  `profiles/updates/*` and rewrite `*DEPEND` in memory.

## 4.5 `PackageVirtualDbapi` (`PackageVirtualDbapi.py:8`)

Mutable installed-state model keyed by `cpv/cp`.

- `__init__(settings)` (:17), `clear` (:24), `copy` (:33),
  `__bool__` (:42), `__iter__` (:45), `__contains__` (:48), `get` (:54).
- `match_pkgs` (:66), `match` (:75), `cpv_exists` (:85), `cp_list` (:88),
  `cp_all` (:104), `cpv_all` (:107): `dep_expand`-aware matching, sorted
  `cp_list`.
- `cpv_inject(pkg)` (:110): insert, evicting same-`cpv` or same-slot
  occupant. `cpv_remove(pkg)` (:130): strict remove (`KeyError` on
  mismatch). `aux_get` (:138) / `aux_update` (:142): direct `_metadata`
  read/write + cache clear.

## 4.6 Frontier, deep deps, world atom

- `_serialize_frontier.py`: incremental leaf frontier replacing `O(V)`
  `leaf_nodes()` scans. `_build_levels()` (:36) unions
  `Normal+Satisfied.ignore_priority` (+`None`), dedup by identity.
  `_SerializeFrontier.__init__(graph)` (:61): per-node surviving-child
  counts + per-level ready heaps seeded in `graph.order`.
  `_compute_mask(priorities)` (:101): bitmask of levels an edge survives
  in. `level_of` (:118), `is_leaf` (:122, count `0`), `ready_nodes(level)`
  (:126, heap drain with lazy deletion in `order`).
  `remove(node)` (:156) decrements parents; `difference_update` (:190),
  `_assign_index` (:194), `add_edge` (:207) increments on new surviving
  levels. `_FrontierDigraph(digraph)` (:237: `remove/difference_update/
  add`) keeps the attached frontier in sync.
- `_find_deep_system_runtime_deps.py:8`:
  `_find_deep_system_runtime_deps(graph)` seeds with `@system` merge
  nodes, DFS only `runtime|runtime_post` children; returns the transitive
  runtime closure (for `--deep` system handling).
- `create_world_atom.py:9` — see doc 02 §2.2.

```mermaid
flowchart LR
    P["porttree/bintree/vartree"] --> F["FakeVartree (in-memory + dynamic deps)"]
    F --> V["PackageVirtualDbapi (cp/cpv maps)"]
    V --> T["PackageTracker (to-merge + installed + provides)"]
    T --> G["digraph (Dependency edges)"]
    G --> FR["SerializeFrontier (ready heaps per priority band)"]
    FR --> ML["mergelist: Package + Blocker + MergeListItem"]
```
