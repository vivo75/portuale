# crates/portuale/src/ebuild_phases/tests/standalone_config.rs
Standalone-config resolver unit tests (RESTRICT gates, depend-phase reduction, tmpdir, binpkg scalars). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn restrict_mirror_from_restrict_evaluates_conditionals_against_the_empty_use_set
- The deprecated negative `nomirror` spelling still counts (Portage's fetch.py:880). A USE-conditional drops against the always-empty fetch-side USE set, so `foo? ( mirror )` is not a literal token match.

## fn restrict_fetch_from_restrict_matches_the_fetch_token_only
- A USE-conditional drops against the always-empty fetch-side USE set.

## fn depend_phase_reduces_restrict_on_the_config_use_set
- A conditional `RESTRICT` drops its gated tokens on the empty set but keeps them on the config `USE` set -- the `depend` phase's reduction (Portage's `doebuild(mydo="depend")` runs with the `setcpv` config `USE`, so ebuild.sh's own `contains_word … "${PORTAGE_RESTRICT}"` checks, e.g. the `strip`/`nostrip` `DEBUGBUILD` gate, see the configured tokens). No fixture change: a throwaway repo carries the conditional entry, while the config `USE` comes from the fixture tree (`make.conf` sets `confflag`), so the `depend` reduction keeps `strip` while still dropping the `other?` group.

## fn flat_field_on_dedups_and_sorts_like_portage_flatten
- Expected value from Portage's `config.py::_flatten` (`:1681-1688`): `" ".join(sorted(set(use_reduce(..., flat=True))))` -- the #45 ncurses case: `RESTRICT="!test? ( test ) test"` reduces to two `test` tokens with `test` off, and the set collapses them (Portage's saved value is `"test"`).

## fn depend_phase_standalone_base_env_stays_empty_with_a_real_repo
- From #37 S2 step 6: the `depend` phase (Portage's `doebuild(mydo="depend")`, the `--regen`/metadata path) never gets the standalone base env -- `phase_standalone_base_env` short-circuits before any config load, so resolved-config threading cannot leak into it. A genuine repo checkout with a resolvable package is used precisely so removing the guard would fail this test rather than silently pass. The `extra_env`-carries-USE guard is the same short-circuit.

## fn per_package_portage_tmpdir_feeds_the_build_directory
- Portage per-package `PORTAGE_TMPDIR` (backlog #99, S0 cells D/E): the matched env-file value re-derives the tmpdir when the calling environment does not carry the key itself; a matched directory that does not exist is Portage's `_check_temp_dir` failure byte for byte (lib/portage/package/ebuild/doebuild.py:1682); the process value wins otherwise (#101 precedence); an empty matched value blanks to absent. A later matching file wins (scalar semantics).

## fn resolve_standalone_binpkg_compress_matches_the_twin_fixture
- Backlog #147 S3: reads the twin fixture's committed `package.env` match (`dev-libs/penvcmppkg penv-compress`, `BINPKG_COMPRESS=gzip`) through the ebuild's own md5-cache identity -- `None` for an unmatched neighbour (`dev-libs/newpkg`).

## fn resolve_standalone_chain_scalar_reads_the_fixture_make_conf
- Backlog #180: serves every global binpkg scalar (`PKGDIR`, `BINPKG_COMPRESS*`, `PORTAGE_BZIP2_COMMAND`, `BINPKG_FORMAT`) below the calling env. The fixture `make.conf` sets `PKGDIR="${PORTAGE_CONFIGROOT}/pkgdir"`, so the file side must resolve to the fixture's own pkgdir; an unconfigured key resolves to `None` (the caller keeps Portage's `make.globals` default); and `BINPKG_FORMAT` agrees with the dedicated `resolve_standalone_binpkg_format` wrapper by construction.

## fn resolve_unmerge_config_reads_binpkg_format_from_make_conf
- Backlog #173 review: `execute_unmerge`'s `FEATURES=unmerge-backup` quickpkg resolves `BINPKG_FORMAT` through this chain (calling env over `make.conf`/profile/`make.globals`, Portage's `config` precedence), so a `make.conf` value reaches the backup artefact (a `.tbz2` for `xpak`). A full quickpkg run is too heavy for a unit test; this pins the resolver the backup path uses instead -- the scratch chain value plus the production lookup shape over it (the file-side lookup with no calling env names a `.tbz2`; the full production expression with calling env over the chain is only meaningful when the ambient process env is itself silent on the key, otherwise the ambient value would rightly win and the assertion would be testing the developer's shell). The calling-env-beats-file half lives in portage-profile's own `with_test_env` test (that hook is `cfg(test)`-local to that crate); here only the file side is asserted.
- No value anywhere in the chain: nothing resolves, so Portage's `make.globals` default (`gpkg`) applies. No `repos.conf` at all: no chain, `None` (the caller keeps the default) -- the same tolerance the standalone helpers have outside any repo checkout.
