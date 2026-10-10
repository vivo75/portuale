# crates/portuale/src/remote/tests/mod.rs
Shared remote-test fixtures: contexts, synthetic units, server roots, S8 trees.

## const S8_DIGEST
- The preflight values carry hex text, so any 64-hex string decides the same way.

## fn bare_portuale_hits
- Only `$PORTUALE_BIN`, an absolute path, or a non-command token is allowed: `portuale-python`, `portuale-<hash>`, `portuale-remote:`, `.portuale.` mktemp names, the ping token (`portuale-helper-ping-1`), `(portuale #nn)` code comments.

## fn synthetic_unit
- The unit is staged by hand; `filemeta` via the portuale collector.

## fn synthetic_unit_bytes
- A genuine executable in the image, for the atomic-replacement pins.

## fn local_ctx
- The fixture context carries the built `portuale` binary next to `deps/`: a genuine binary, never the libtest harness (#326 D8: the phase shims exec `$PORTUALE_BIN`, and a harness would answer its test banner). `cargo build -p portuale` before `cargo test`.

## fn build_time_environment
- The stray build-host local (`declare -- x=""`, as the bed's `setuid-1.0-1.gpkg.tar` build-time env carries). Portage binpkg envs also carry the hook functions themselves (the phase runs from the saved env, never re-sourcing the ebuild); `with_postinst` adds the test's `pkg_postinst`, without which the regen run is a no-op like Portage's undefined-function case.

## fn regen_unit
- Both `run_merge_stage` and the regen phase run against it under local bash. Like a Portage binpkg, `build-info/` carries the build-time `environment.bz2` (the `MERGE_FLOW` vdb fallback the regen later overwrites). Mirrors `build_bundle`: the regen run's `${PORTAGE_BZIP2_COMMAND}` stand-in rides `bin/`.
- The saved env carries the hooks the phase runs (Portage `__save_ebuild_env` keeps `pkg_postinst`).

## fn l171e_server_root
- Backlog #171 follow-up 4 (owner Q10 = b) scaffolding: the two server-side inputs `place_config_root` reads before repointing `PORTAGE_CONFIGROOT`. Repos are added per-test with `l171e_server_repo`.

## fn l171e_resolve_with_server_repos
- Kept for the l171e remap tests, which pin the rewritten link against hand-picked server repos; see `remote_client_without_repos_conf_resolves_on_the_production_path` for the production path.

## fn farm_path_without_bzip2
- Plan §6 tool floor.

## fn run_old_hook_snippet
- #326 S8.3: the header of a genuine merge driver exports the resolved client binary. Never the libtest harness (#326 D8).

## section: client-binary install e2e
- #326 S8 e2e (D5). The shipped file is the built `portuale` binary itself (via `--remote-portuale-binary`), so the install's `__helper ping` answers the D8 token.

## fn s8_portuale_bin
- The same layout `pretend.rs`'s remote test relies on; `cargo build --release -p portuale` before `cargo test --release`.

## struct S8Tree
- S8 end-to-end tree. A main repo over an empty dir, like `placed_config_root_covers_both_placements`.

## fn s8_run
- Local transport, the built binary as the shipped file, one fixture binpkg.
