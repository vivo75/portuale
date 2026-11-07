# #611(c) captures (2026-11-07)

Host Portage 3.0.83 (`*.portage`) vs portuale with the shared
`show_masked_packages` (`*.portuale`), `emerge -p <atom>` on a copy of the pmtest
fixture tree; output from the first `!!! All ebuilds` line on, `CFG/` = the cell
root (Portage's config root is a sibling `.cfg` copy), ` for <root>/` removed.
Every pair is equal except one trailing blank line at the very end of Portage's
output on `maskneedpkg`, `kwneedpkg` and `abort-masked-mid` (present before this
change too; the contract suite strips it). `eulapkg` is license-masked but the
fixture tree has no `licenses/` directory, so neither side prints a license line.
These captures are the source of `_MASK_NOTES` in pmtest's contract suite.
