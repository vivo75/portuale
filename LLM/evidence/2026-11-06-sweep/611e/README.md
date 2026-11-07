# #611(e) captures (2026-11-07)

Host Portage 3.0.83 vs portuale, `emerge -p --backtrack=0 @world` and `@selected`
on copies of the pmtest fixture tree (`$FX`):

- `c611e`: `dev-libs/maskneedpkg` appended to the world file (masked dependency
  `maskeddep`): the chain ends `"@selected" [set]`, `"@world" [argument]`.
- `c611f`: `dev-libs/r594qa` appended to the world file (USE change on
  `r594vdep`): `# required by @selected`, `# required by @world (argument)`.
- `c611s`: `*dev-libs/maskneedpkg` added to the leaf profile's `packages` (an
  `@system` member): the chain ends `"@system" [set]`, `"@world" [argument]`.

The display order (USE-change block before the masked block) is in
`../609/q.portage.txt` and `q0.portage.txt`. Remaining differences: the
` for $FX/` / `to $FX/` root suffixes of the cell setup only.
