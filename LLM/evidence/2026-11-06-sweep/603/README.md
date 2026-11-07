# #603 captures (2026-11-07)

Host Portage 3.0.83 vs portuale after the #603 change. Config root `$FX` is a
copy of the pmtest fixture tree with the `w603` ebuilds (`w603-repo.json`, written
by the sweep's `mkfx.py`); `$R` is an ad-hoc ROOT holding only the vdb entries and
world file of `r603*.json` (`mkroot.py`). Same env and argv for both PMs (first
line of each file); repo-config warnings and the global-updates progress lines
are filtered.

- `plain`, `pu-world`: the world-only consumer's `<dev-libs/w603lib-2` matches no
  installed package (`w603lib-3` installed), so Portage files it as initially
  unsatisfied (`depgraph.py` 8797-8806): `U w603lib-3.1`, 0/20. portuale printed
  `UD w603lib-1 [3]` with the skip block, 1/20, before the change.
- `matching`: `w603lib-1` installed, the atom matches, the conflict forms
  (8807-8834). Unchanged.
- `nocomplete`, `beside-or`, `beside-or-nocomplete`: the backlog entry's
  "non-complete-mode half" (plain atoms beside a `||` Portage never evaluates).
  Both PMs already agree, before and after the change: nothing to fix.
- `side-N-vs-UD`: the clusterA side finding. Unrelated to #603 (the consumer is
  an argument and walked); filed as #669.
