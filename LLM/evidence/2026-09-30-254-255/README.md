# #254 + #255 — the conflict-fired ABI probe family (batch-2026-09-28 Track R)

Real's own `ResolverPlayground` (`3rdparty/portage`, Portage 3.0.82.2
tree), run from `3rdparty/portage` with `PATH=$PWD/bin:$PATH` and a copy
of `lib/portage/tests/.gnupg` as `PORTAGE_GNUPGHOME`; `u254_pg.py <shape>`
prints the final display (`depgraph.display(altlist())`). Captured
2026-09-30. The shape is #214's g214 cell: installed world member
`app-misc/abicons-1` (built with `cflag`, recorded
`app-misc/abiprov:0/1=`), world member `abiforce` needing
`>=abiprov-2`, `abiprov-1`/`-2` in slot 0 with sub-slots `0/1`/`0/2`,
`-uD --backtrack 4 @world`. (The pmtest pin for the conditional shape
uses a separate consumer name, `abicondcons`, so the shared vdb's
`abicons` stays untouched.)

- `u254-plain.txt`: the tree ebuild carries `app-misc/abiprov:=` — the
  conflict-fired ABI probe rebuilds the consumer: `N abiprov-2`, `rR
  abicons-1`, `N abiforce-1`, **with** the "causing rebuilds" block.
- `u254-cond.txt`: the tree ebuild carries `cflag? ( app-misc/abiprov:= )`
  with `cflag` off. Real's probe evaluates the replacement's USE
  (`_select_atoms_probe`) and refuses; real masks `abiprov-1` for the
  conflict, then the unsatisfied probe (`validated_atoms`, every
  conditional branch) reinstalls the consumer: the same three rows,
  **without** the block (`_compute_abi_rebuild_info` pairs only a
  provider that is a child of the replacement consumer).
- `u255-otherslot.txt`: the plain shape plus an installed `abiprov-0.5`
  in slot `1/1`: real prints `[ebuild  NS    ] app-misc/abiprov-2 ...
  [0.5:1::test_repo]` — no `r`, because `_get_installed_best` is
  slot-keyed and a provider installed only in another slot is `new`.
