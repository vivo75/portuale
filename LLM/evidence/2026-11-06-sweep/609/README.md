# #609 captures (2026-11-06)

Host Portage 3.0.83 vs portuale after the #609 change, on a copy of the pmtest
fixture tree (`cells/c609`, built by the sweep's `cell.sh`). Argv in each file's
first line; `$FX` is the cell root, blank lines and repo-config warnings
filtered. Remaining differences after #609: the ` for $FX/` / `to $FX/` root
suffix (the cell's ROOT has no trailing slash; the contract env pins it), the
`package.mask` file + comment under the `maskeddep` row (#611(c)), and the
USE-change block printing after the masked block on `q`/`q0` (#611 display
order, done in the #611(e) commit).
