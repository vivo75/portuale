# #602 captures (2026-11-07)

Host Portage 3.0.83 vs portuale after the #602 change; same setup as `../603/`
(`$FX` config root = fixture copy plus `w602-repo.json`, `$R` = ad-hoc ROOT from
`r602.json` / `r602b.json`, scripts in `../603/`).

- `three-atoms-switches-off`: the slot conflict on `w602s` while backtracking
  turns complete mode on (`depgraph.py` 9473-9480), so the world consumer
  `w602cons-1` is rebuilt: `r U w602prov-2`, `rR w602cons-1`. Rows equal; the
  count is 3/20 for Portage, 1/20 for portuale (#670). Before the change
  portuale printed `U w602prov-2` and no rebuild.
- `three-atoms-switches-off-noworld`, `one-atom-switches-off`,
  `three-atoms-switches-off-bt0`: no world consumer, no conflict, or
  `--backtrack=0`: no complete mode, identical output.
- `three-atoms-default`, `one-atom-default`: switches on (complete mode by
  version change anyway). Rows equal; the counts already differed before #602
  (5/20 vs 3/20, 2/20 vs 1/20): #670.
