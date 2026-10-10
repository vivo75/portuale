# crates/portage-profile/src/resolve/use_masks.rs
Profile `use.mask`, `use.force` and `arch.list`.

## fn fold_profile_use_masks
- Sourcing confirmed by reading `UseManager.getUseMask`/`getUseForce`'s `pkg=None` (global) case, which returns `stack_lists(self._usemask_list/self._useforce_list, incremental=True)` directly, never consulting a repo-level or per-package source (those exist only on the per-package path, out of scope for portuale's flat/global USE model, same as package.use's repo/profile/user-only sourcing).
- Ordering history: portuale previously folded force/mask early alongside `defaults`/`conf`, letting a `package.use` entry override a global force/mask decision Portage never lets it override. Portage's own order is `lib/portage/package/ebuild/config.py` ~line 3024: `myflags.update(self.useforce); ...; myflags.difference_update(self.usemask)`. `setcpv()` sets those to the per-package `getUseForce(pkg)`/`getUseMask(pkg)` -- global files combined with the atom-scoped package.use.force/.mask.
- `arch.list` stacks the same way (Portage `stack_lists(archlist, incremental=1)` in config.py).
