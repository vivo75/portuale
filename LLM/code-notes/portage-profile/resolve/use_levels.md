# crates/portage-profile/src/resolve/use_levels.rs
The per-level use.mask/use.force stack the USE-flip suggestions read.

## fn use_mask_force_levels
- Replays Portage `UseManager.getUseMask`/`getUseForce`'s interleaved per-level stacking (`for i, _ in enumerate(...): append usemask[i], usestablemask[i], pusemask[i][cp], pusestablemask[i][cp]` into one `stack_lists(incremental=1)`).
- Level 0 is synthetic: the repo-level `package.use.*` (main repo, then each overlay `::repo`-scoped) that Portage prepends before the profile loop.
- The global `use.mask`/`use.force` files are re-read per level here (the flat fields stacked them into a single set); a re-read of a handful of usually-absent small files, once per (memoised) `resolve_config`.
