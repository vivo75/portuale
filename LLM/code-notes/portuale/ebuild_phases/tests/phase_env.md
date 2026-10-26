# crates/portuale/src/ebuild_phases/tests/phase_env.rs
Phase-environment unit tests (path vars, PATH composition, quoting, eclass locations, standalone package.env). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## fn eapi_path_var_strips_the_trailing_slash_from_eapi_7
- From #37 S4 (L2 live set, pv/htop `KERNEL_DIR="//usr/src/linux"`): path variables lose their trailing slash from EAPI 7 on, so `ROOT=/` is exported empty.

## fn phase_path_prefixes_helpers_onto_the_threaded_path
- From #37 S4: a threaded `PATH` pair (the resolved env.d `PATH`) is the base behind the helper dir, deduplicated like Portage's `_doebuild_path`, and never replaces the helper-prefixed value verbatim.

## fn phase_path_puts_the_xattr_wrapper_first_under_features_xattr
- Expected value from Portage's `_doebuild_path` (lib/portage/package/ebuild/doebuild.py:344-346), #329: `<bin>/ebuild-helpers/xattr` goes in front of `<bin>/ebuild-helpers` when `xattr` is in `FEATURES`, and only then.

## fn shell_single_quote_neutralises_every_shell_metacharacter
- From #37 S2 G6: every brush `export` value is single-quoted, so a config-derived value containing `$`, backticks, `\`, `"` or whitespace can neither word-split nor command-substitute. Before this gate the resolved config env reached `phase_setup_script` through Rust's `{:?}` Debug formatting, which does not protect against expansion. The last arm pins a `CFLAGS`-shaped value with a single quote and a newline.

## fn eclass_locations_value_quotes_the_containing_repo_root
- Canonicalized first, matching what `compute_environment` always hands `repo_root_for` (it always canonicalizes the ebuild's path before deriving `pkg_dir` from it).
- Portage's `bin/ebuild.sh` `eval "PORTAGE_ECLASS_LOCATIONS=(${...})"` expects single-quoted tokens -- confirmed by round-tripping through the exact same unmodified bash line here.

## fn eclass_locations_value_puts_the_own_repo_first_then_masters_in_declared_order
- Ports Portage's config.py:1256-1266 + eclass_cache.py:177-179: `eclass_locations = [master.location for master in repo.masters] + [repo.location]`, exported `reversed()` -- so the ebuild's own containing repo is searched first, its masters after, in declared order.

## fn eclass_locations_value_does_not_duplicate_the_own_repo_when_it_is_also_a_master
- Ports Portage's config.py:1264-1266 ("Only append the current repo to eclass_locations if it's not there already"), exercised via the main repo itself, whose own `masters` defaults to empty (config.py:1229-1260, "the main repo can never be its own master"), so this also doubles as a default-masters proof: no explicit `masters =` key at all for `main`.

## fn standalone_phase_env_layers_matching_package_env_build_vars
- Ports Portage's `_grab_pkg_env` for a standalone phase env: a matching `package.env` entry's env file is layered over the base flags, atom-matched against the ebuild's own md5-cache identity, with no resolved graph entry anywhere.
- `dev-libs/penvbuildpkg` (mapped to `penv-buildflags`) carries the env file's `CFLAGS`/`MAKEOPTS` plus the toolchain selectors `CC`/`CXX`/`AR`/`RUSTFLAGS` and the `ENV_UNSET` incremental (backlog #95); `dev-libs/newpkg` (unmapped) doesn't; the `depend` phase never does (its empty base is deliberate -- metadata extraction must not see config values).

## fn drop_ebuild_phase_pops_only_the_phase_var (#516)
- A `misc-functions.sh` spawn (`install_qa_check ...`) is not an ebuild phase — Portage's `MiscFunctionsProcess._spawn` pops `EBUILD_PHASE` from the child env — while an ebuild phase spawn keeps it. Portage: `_emerge/EbuildPhase.py`, `MiscFunctionsProcess._spawn`.
