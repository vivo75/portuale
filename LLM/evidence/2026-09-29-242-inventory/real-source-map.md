# Real-source map for the root split (verified 2026-10-01)

Against `3rdparty/portage` 3.0.82.2 (`1d95fc2c5`, via
`./3rdparty/setup.sh portage`). Line numbers are that checkout's; find code
by symbol name.

## 1. `_add_pkg_deps` — `depend_root` selection (`lib/_emerge/depgraph.py:4218-4238`)

```python
if removal_action:
    depend_root = myroot
else:
    root_deps = self._frozen_config.myopts.get("--root-deps")

    if eapi_attrs.bdepend:
        depend_root = pkg.root_config.settings["ESYSROOT"]
    else:
        depend_root = self._frozen_config._running_root.root
        if root_deps == "rdeps":
            ignore_depend_deps = True

    if root_deps == True:
        edepend["RDEPEND"] += (" " + edepend["IDEPEND"] + " "
            + edepend["DEPEND"] + " " + edepend["BDEPEND"])
```

Native (non-cross) bed: `ESYSROOT=/`. Pre-EAPI-7 (`eapi_attrs.bdepend`
false, `lib/portage/eapi.py:292`) `DEPEND` resolves against the running
root; EAPI ≥ 7 against `ESYSROOT`. Only in the pre-EAPI-7 branch does
`root_deps == "rdeps"` set `ignore_depend_deps`; `root_deps == True`
folds `IDEPEND`+`DEPEND`+`BDEPEND` into `RDEPEND`. `--root-deps`'s own
fold/ignore branches stay as they are unless B's S0 proves otherwise (B8).

## 2. The five-group `deps` queue (`depgraph.py:4255-4291`)

```python
deps = (
    (myroot,                                    edepend["RDEPEND"], ... runtime ...),
    (self._frozen_config._running_root.root,    edepend["IDEPEND"], ... installtime+runtime ...),
    (myroot,                                    edepend["PDEPEND"], ... runtime_post ...),
    (depend_root,                               edepend["DEPEND"],  ... buildtime ...),
    (self._frozen_config._running_root.root,    edepend["BDEPEND"], ... buildtime ...),
)
```

Each group carries its root (`dep_root`) into `_dep_expand`; priority via
`self._priority(cross=self._cross(pkg.root), ...)` — build-time groups get
the `buildtime` priority class. Note `PDEPEND → myroot` (target root):
scope-backlog's permanent non-gap, not reopened.

## 3. `_dep_expand(root_config, ...)` (`:4928`) and `_cross` (`:4921`)

```python
def _cross(self, eroot):
    """Returns True if the ROOT for the given EROOT is not /,
    or EROOT is cross-prefix."""
    return eroot != self._frozen_config._running_root.root
```

`_dep_expand` takes the group's `root_config` (resolved from `dep_root`)
and expands the atom string against that root's trees/vdb — this is where
candidate selection becomes root-aware. The node's root identity downstream
is `Package.root_config` / `pkg.root`.

## 4. `create_trees` (`lib/portage/__init__.py:497-529`)

`trees._target_eroot = settings["EROOT"]`; when `ROOT == "/"` (and
non-cross `EPREFIX`) the running root *is* the target root. Otherwise a
second config is built from the host environment (`target_root="/"`),
`trees._running_eroot` points at it, and both roots' trees/vdb enter the
graph. The bed's `PORTAGE_RUNNING_ROOT=$FX` pin
(`layers/l0-fixture-oracle/in-container.sh:57-89`) collapses this back to
one tree — the bed-side approximation §2 of the README names.

## 5. Display sites

- `Package.__str__` (`lib/_emerge/Package.py:568-608`): merge-operation
  package with `ROOT != "/"` appends quoted `to '{ROOT}'`; installed
  package appends `in '{ROOT}'`; uninstall appends `scheduled for
  uninstall`. Nomerge/other operations: no suffix. Cycle-node text goes
  through this function.
- Merge-*list* rows (`lib/_emerge/resolver/output.py:462,475,861`):
  `darkgreen("to " + pkg.root)` (`pkg.root` = EROOT), **unquoted** —
  a different renderer from `__str__`.
- Cycle message (`lib/_emerge/resolver/circular_dependency.py:76-99`
  `_prepare_circular_dep_message`): `f"{pkg}"` per node of
  `shortest_cycle`, i.e. `__str__` — bare here because every printed
  cycle node sits in `/` (verified in `probes/default-debug-*.txt`:
  only the FX-rooted argument node carries `to '...'` and it is not a
  cycle member).
- Rotation source: `digraph.get_cycles`
  (`lib/portage/util/digraph.py:387`) emits both rotations;
  `_find_cycles` (`circular_dependency.py:48`) keeps the first minimal
  one as `shortest_cycle`; `shortest_cycle[0]` is the printed start node
  (backlog #278, DONE — portuale matches under both stagings).
- USE paint (`lib/_emerge/UseFlagDisplay.py:55` `pkg_use_display`):
  whole effective USE masked to the valid-IUSE domain, grouped per
  non-hidden `USE_EXPAND` var; read from the package's **own**
  `root_config` — hence the missed `blk0x` renders the `/` profile's
  `ABI_X86="(64)"` under default staging (capture-backed) and the
  fixture profile's `ELIBC="glibc"` as control.
- Missed-update block (`depgraph.py:1650-1700`
  `_show_missed_update_slot_conflicts`): one `<slot_atom>` header per
  missed upgrade (`_get_missed_updates`, `:1529`, keep-max per
  `(root, slot_atom)`), `(cpv, state) <USE displays> conflicts with`
  plus one `<atom> required by <parent> <USE displays>` + `^` pair per
  parent; arg-parent arm (`:1681-1686`) prints bare `str(parent)`.

## 6. Serializer abort (`depgraph.py:10262-10289`)

`_serialize_tasks`' leaf-selection `if not selected_nodes:` arm records
the residual `circular_dependency` map into `_backtrack_infos["config"]`
and sets `_need_restart`; `_backtrack_depgraph` (`:12192`,
`max_retries = myopts.get("--backtrack", 20)`) spends the run — hence
`backtrack: 1/20` on the cyc0z shapes under **both** stagings (fresh
captures), and `backtrack: 1/20` on the g216 default shape resolving to
`g216boot`.
