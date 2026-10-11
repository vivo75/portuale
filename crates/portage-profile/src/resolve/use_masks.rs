//! Profile `use.mask`, `use.force` and `arch.list`.

use crate::resolve::*;

/// Profile `use.mask`, `use.force` and `arch.list`.
pub(crate) fn fold_profile_use_masks(chain: &[PathBuf], config: &mut Config) -> Result<(), Error> {
    // use.mask/use.force: every level's file stacked with `-atom` removal.
    // Not folded into `use_flags` here: Portage applies force/mask as the
    // literal last step of its USE walk, strictly after the `pkg` tier, so
    // a package.use entry can never override them. Applied per-candidate in
    // `portage-repo::effective_use_flags` instead (force-add first, then
    // force-remove, so a flag in both ends masked).
    // Portage: UseManager.getUseMask/getUseForce; config.py regenerate()
    let mut usemask_sources: Vec<Vec<String>> = Vec::new();
    let mut useforce_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        usemask_sources.push(read_config_lines(&level.join("use.mask"))?);
        useforce_sources.push(read_config_lines(&level.join("use.force"))?);
    }
    config.use_force = stack_mask_lines(&useforce_sources).into_iter().collect();
    config.use_mask = stack_mask_lines(&usemask_sources).into_iter().collect();

    // PORTAGE_ARCHLIST: same chain, same stacking. See `archlist`.
    let mut archlist_sources: Vec<Vec<String>> = Vec::new();
    for level in chain {
        archlist_sources.push(read_config_lines(&level.join("arch.list"))?);
    }
    config.archlist = stack_mask_lines(&archlist_sources).into_iter().collect();
    Ok(())
}
