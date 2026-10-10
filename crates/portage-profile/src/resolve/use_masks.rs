//! Profile `use.mask`, `use.force` and `arch.list`.

use crate::resolve::*;

/// Profile `use.mask`, `use.force` and `arch.list`.
pub(crate) fn fold_profile_use_masks(chain: &[PathBuf], config: &mut Config) -> Result<(), Error> {
    // use.mask/use.force: every profile level's own file (in chain
    // order), stacked with the same -atom removal semantics
    // package.mask uses (see stack_mask_lines) -- confirmed by reading
    // UseManager.getUseMask/getUseForce's own pkg=None (global) case,
    // which returns stack_lists(self._usemask_list/self._useforce_list,
    // incremental=True) directly, never consulting a repo-level or
    // per-package source at all (those only exist on the *per-package*
    // path, out of scope for portuale's flat/global USE model, same
    // as package.use's own repo/profile/user-only sourcing already is).
    // NOT folded into `use_flags` here -- real config.py's own
    // `regenerate()` applies `self.useforce`/`self.usemask` (which
    // `setcpv()` sets to the *per-package* `getUseForce(pkg)`/
    // `getUseMask(pkg)` -- global use.force/use.mask combined with the
    // atom-scoped package.use.force/.mask portuale already applies
    // per-candidate) as the literal *last* step of its own incremental
    // USE walk (`lib/portage/package/ebuild/config.py`, ~line 3024:
    // `myflags.update(self.useforce); ...;
    // myflags.difference_update(self.usemask)`), strictly *after* the
    // `pkg` (`package.use`) tier -- not folded in early alongside
    // `defaults`/`conf` the way portuale previously (incorrectly) did,
    // which let a `package.use` entry override a global force/mask
    // decision real portage never lets it override. See
    // `portage-repo`'s own `effective_use_flags` doc comment for where
    // `use_force`/`use_mask` actually get applied now -- alongside the
    // atom-scoped `package_use_force`/`package_use_mask` it already
    // positions correctly, force-add first then force-remove, so a flag
    // in both ends up masked, not forced, exactly like real portage.
    let mut usemask_sources: Vec<Vec<String>> = Vec::new();
    let mut useforce_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        usemask_sources.push(read_config_lines(&level.join("use.mask"))?);
        useforce_sources.push(read_config_lines(&level.join("use.force"))?);
    }
    config.use_force = stack_mask_lines(&useforce_sources).into_iter().collect();
    config.use_mask = stack_mask_lines(&usemask_sources).into_iter().collect();

    // PORTAGE_ARCHLIST: same chain, same stacking semantics as
    // use.mask/use.force just above -- see `archlist`'s own doc comment.
    let mut archlist_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        archlist_sources.push(read_config_lines(&level.join("arch.list"))?);
    }
    config.archlist = stack_mask_lines(&archlist_sources).into_iter().collect();
    Ok(())
}
