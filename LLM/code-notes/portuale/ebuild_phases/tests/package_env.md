# crates/portuale/src/ebuild_phases/tests/package_env.rs
`package.env` matching unit tests. Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn package_env_accepts_real_scalars_and_rejects_non_user_variables
- Expected value from Portage's `_grab_pkg_env` acceptance gate (lib/portage/package/ebuild/config.py:2269-2300): every non-`USE` key is accepted unless it is in `env_blacklist` (`SLOT`, `DEPEND`, `RDEPEND`, `EAPI`, `ROOT`, `PKGUSE`), `global_only_vars` + `environ_filter` (`CONFIG_PROTECT`), `environ_filter` (accepted into Portage's config but never exported to a phase: `ACCEPT_KEYWORDS`, `CONFIG_PROTECT_MASK`, `USE_ORDER`), the profile's dynamic `PROFILE_ONLY_VARIABLES` (`ARCH`, `ELIBC`, `IUSE_IMPLICIT`, `USE_EXPAND`), or portuale's own `PORTUALE_COMPUTED` set (residues #98/#99: `FEATURES`/`PORTAGE_TMPDIR` are its recomputed residues; the rest are portuale-owned: `DISTDIR`, `PATH`, `D`). `USE` has its own `package_env_use` path.
- The live host capture behind this matrix is pmtest's `findings/l2.md` "#95 S0".

## fn package_env_incrementals_fold_onto_the_base_like_regenerate
- Expected value from Portage's three-layer incremental semantics: `_grab_pkg_env` appends within the container (`container[k] += " " + v`), then `regenerate()` folds `[lower layers, pkg, calling-env]` with `-*`/`-tok` pruning and a sorted union (lib/portage/package/ebuild/config.py:2735, `:2778-2825`) -- the calling env folds last, so its `-tok` prunes a package.env token (S0 cell B: `FEATURES="-probe-feature"` beats the env file) but not vice versa (a pkg-layer `-tok` does not prune a calling-env token; the calling env folds last and re-adds it -- S0 cell B's mirror).
- The three host captures in pmtest's `findings/l2.md` "#95 S0" are the expected values for the first three arms (empty calling env). `-*` clears every base token; several matching files of one entry append in order.

## fn package_env_scalars_last_wins_and_empty_blanks
- Scalars are set, not appended: a later file replaces, and an empty value blanks the variable (Portage's `CC=""` in an env file leaves the phase's `CC` empty, S0 capture). A non-matching atom contributes nothing.

## fn package_env_scalar_loses_to_the_calling_environment
- Portage's `env`-over-`pkg` scalar precedence (`USE_ORDER`, lib/portage/package/ebuild/config.py:1031-1035): a package.env scalar is dropped when the calling environment carries the same key, so the process value the caller already layered wins (backlog #101, S0 cell A). Keys the calling env does not carry still arrive.

## fn package_env_raw_matchers_follow_the_container_semantics
- The raw single-key matchers behind the `PORTUALE_COMPUTED` recomputes (#98/#99: the `FEATURES`/`PORTAGE_TMPDIR` residues): scalars replace (last matching file wins), incrementals append across matching files in order, non-matching atoms and other keys contribute nothing -- Portage's `_grab_pkg_env` container behaviour without the acceptance gate or the fold.
