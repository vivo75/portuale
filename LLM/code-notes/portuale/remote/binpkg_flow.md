# crates/portuale/src/remote/binpkg_flow.rs
Checking the binary plan and running the binpkg flow: bundle, stream, unpack, phases, merge, postinst.

## struct VdbShadow
- Vdb shadow plus pre-ship collision pre-check (slice 6). The shadow reasons about installed-file ownership before a unit's tarball ships (plan §7); the pull is the same `tar -c`/`tar -x` shape as the etc-portage pull.
- Same-package ownership refines to same-slot replace vs. collision. A passed pre-check is necessary but not sufficient; a failed one is final.

## impl VdbShadow::load
- No new dependencies: the vdb is exactly two levels deep.

## fn load_vdb_shadow
- Plan §7's pull-once-per-run. The tmp dir is left for forensics; it carries no secrets beyond file lists.

## fn run_binpkg_flow
- `install_mask` / `install_mask_prunes_usr_share` are the resolve's own `config_install_mask` values for the placed config; both paths resolve the client config now (backlog #170/#171b: the trial path's `run_bundle_stage` places and loads it like `run_remote_resolve` does).
- `regen_features` is the resolve's own `config_features_string` for the placed config (the merge-time `FEATURES` the regen'd vdb env carries, backlog #171); `regen_bzip2` is the resolve's own `config_bzip2_command` for the placed config (the merge-time `PORTAGE_BZIP2_COMMAND` the scrubbed vdb env records).

## fn run_binpkg_flow (phases)
- Slice-3 phases (pretend/setup/preinst, `DEFINED_PHASES`-gated at bundle time). Postinst waits for the slice-4 merge. Same degrade as the local merge; returning early on an empty phase list silently unmerged the hookless binpkg.

## fn run_binpkg_flow (merge)
- Slice-4 merge (copy+vdb+replace).

## fn finish_postinst_regen (regen postinst)
- Backlog #171: an undefined `pkg_postinst` is a no-op that still re-saves the env (Portage `vartree.py:5334` + `phase-functions.sh:1072-1082`). Any regen/pull/compress/install failure keeps the build-time env with one `!!!` warning and continues (Portage `_postinst_failure`: "It's stupid to bail out here").

## fn finish_postinst_regen (locale)
- Backlog #171b: Portage's `environ_whitelist` rule; the server process is the calling environment.

## fn run_bundle_stage
- Backlog #171b: the trial path places and loads the client config like `run_remote_resolve` does (same shape as `run_remote_resolve`), then loads the same repos + resolved config `pretend::run` merges with (`load_repos_and_config`: one shared resolver call, no duplicate logic). The placed config's `INSTALL_MASK` (+ the `no{man,info,doc}` fold), resolved `FEATURES`, and `PORTAGE_BZIP2_COMMAND` feed `build_bundle` / the env regen exactly as `run_remote_plan`'s own values do. There is no config-less mode left here; a pull or resolve failure is a hard error like the resolve path's own.
- The S8.2 install, like the resolve path.
