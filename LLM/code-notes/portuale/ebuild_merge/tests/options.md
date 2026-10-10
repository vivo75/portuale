# crates/portuale/src/ebuild_merge/tests/options.rs
`MergeOptions` resolution coverage: config-protect folds and FEATURES re-derivation.

## fn apply_config_protect_takes_the_resolved_fold
- Collision-protect background, kept with the test it was written for: with both `FEATURES=collision-protect` **and** `protect-owned` off, an ordinary file collision is merged over (`collisionpkg-c` overwrites `collisionpkg-a`'s own `shared.txt`). Portage's `protect-owned` *is* one of Portage's `make.globals`'s own default `FEATURES` tokens (`cnf/make.globals:77-84`, confirmed by reading it directly), unlike `collision-protect` -- so Portage's own actual default behavior for this exact scenario (an identifiable owner for the collision, `collisionpkg-a`) is to *abort*, not merge over; `MergeOptions::default()` alone (`protect_owned: true`) reproduces that Portage default correctly. The related case therefore sets `protect_owned: false` explicitly rather than relying on `MergeOptions::default()` -- see `protect_owned_alone_aborts_when_an_owner_is_identified` for the Portage-default (`protect_owned: true`) case.
- #333: the merge's protect lists are the resolved `CONFIG_PROTECT[_MASK]` fold (Portage's `dblink._get_protect_obj`, `vartree.py:2063-2072`, reading `settings`), not the raw process env `from_env` read. A fold that resolves to nothing (`-*`) protects nothing; a key no layer assigned keeps the `from_env` value.

## fn set_resolved_features_rederives_the_merge_tokens
- #37 S3: the resolved list is what makes a make.conf `collision-protect` / `-protect-owned` / `config-protect-if-modified` take effect; the GPG verify policy follows the same list. Defaults-on tokens present in make.globals stay on.
