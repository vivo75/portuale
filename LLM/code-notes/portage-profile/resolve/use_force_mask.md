# crates/portage-profile/src/resolve/use_force_mask.rs
package.use.force/mask and their stable variants, repo and profile levels.

## fn fold_package_use_force_mask
- No user-level source at all, unlike package.use: confirmed by reading `UseManager.__init__`'s file/variable table (the "user config" section lists only "package.use -> _pusedict", nothing for mask/force).
- No masters merge at load: confirmed by reading `_parse_repository_files_to_dict_of_tuples`/`_of_dicts`, which parse each repo's file independently; the masters chain is consulted later, per-package, in `getUseMask`/`getUseForce` (`repos = masters + [pkg.repo]`). Which entry wins when several match is decided at application time; see portage-repo's doc comment on atom-specificity ordering.
- use.stable.mask/.force are PMS 5+ (Portage `eapi_supports_stable_use_forcing_and_masking`); portuale always recognizes them per its established "no EAPI parametrization" precedent (see the crate's module doc comment). Deliberately not folded into use_flags here; see `use_stable_force`'s doc comment for why.
- package.use.stable.mask/.force sourcing confirmed by the same `UseManager.__init__` table entries for `_repo_pusestablemask_dict`/`_repo_pusestableforce_dict`.
