# crates/portage-profile/src/resolve/package_sets.rs
The profile's `packages` (system and @profile sets) and `package.provided`.

## fn fold_package_sets
- @system: Portage `PackagesSystemSet.load` calls the identical `stack_lists(incremental=1)`. Only after stacking are the `"*"`-prefixed lines kept (with the `"*"` stripped) as the @system atom list; see the module doc comment's `packages` bullet for why every other stacked line is read/stacked but never contributes an atom.
- @profile: only profile levels whose own repo declares `profile-set` in metadata/layout.conf are a source at all, and the unstarred stacked lines are the atoms -- Portage `ProfilePackageSet.load` (`_sets/profiles.py:13-41`): `for y in self._profiles if "profile-set" in y.profile_formats ... if str(x)[:1] != "*"`. @system deliberately has neither filter. A level outside every configured repo (a bare synthetic profile dir in a unit test, or the user-config profile node) does not qualify; see `Config::profile_packages`'s doc comment.
- package.provided: Portage `config.py:970-1027`. See `Config::package_provided`.
