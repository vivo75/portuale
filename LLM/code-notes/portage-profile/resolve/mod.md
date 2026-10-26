# crates/portage-profile/src/resolve/mod.rs
`resolve_config`: building the full `Config` from disk and the environment.

## fn resolve_config
- The `profile-formats = portage-2` gate on a profile `parent` line's `reponame:path`/`:path` cross-repo syntax: Portage `_config/LocationsManager.py:47`/`259`. Read here via a tiny dedicated layout.conf reader (`repo_profile_formats`) rather than threading a param through every call site.
- The env layer is Portage `config.regenerate()`'s `env` `USE_ORDER` layer; see `apply_env_layer` / `ENV_*_VARS`.
- PKGDIR default `/var/cache/binpkgs`; see `pkgdir`'s doc comment.
- `etc/portage/binrepos.conf` may be a directory of `*.conf` fragments: Portage `BinRepoConfigLoader._parse` runs `_recursive_file_list` over the path (the standard Portage "file or dir" pattern; how the release profile ships `gentoobinhost.conf`). See `binrepos`'s doc comment.
- `PORTAGE_CONFIGROOT` stamping and the `CBUILD`-defaults-to-`CHOST` fallback: Portage `config.py.__init__` (`if self.get("CBUILD") is None and self.get("CHOST")`).
- The two frozen digests: #105 (USE-context base; the `'backtrack` loop mutates only `autounmask_use` on clones, which the memo keys hash against this base) and #114 (visibility base: masks + accept lists).
- `virtuals` deprecation (#513): Portage's `VirtualsManager` reads every profile of the chain plus the user profile node, appended last.
- The local user-patch sets for the `--binpkg-respect-user-patches` comparison (#509): `None` when there is no patches directory, exactly like Portage's lazy `_user_patches`.

## fn resolve_config user node (#346 S1)
- The `user_node` resolved once in `load_scalar_layers` is threaded by reference into every profile fold (`fold_profile_use_masks`, `fold_package_masks_and_keywords`, `fold_package_use_force_mask`, `use_mask_force_levels`, `fold_package_sets`); `fold_package_use` needs no param since the user `package.use` now lives in the levels, not the flat fallback. Untouched: `package.provided` (keeps its unconditional user-profile read per the plan) and `pkgindex_profile` (`chain.last()`, the selected repo profile — Portage never reports the user node there).
