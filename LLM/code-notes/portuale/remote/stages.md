# crates/portuale/src/remote/stages.rs
The merge, regen, old-hook and phase stages run against the client.

## fn client_vdb_placement
- Server-side vdb placement (plan §7). Shared by the merge driver and the #171 env-regen install.

## fn run_merge_stage
- The success gate is the driver's own `STATUS=merged` trailer (plan §11).

## section: merge-time vdb environment regeneration
- Backlog #171: Portage regenerates the vdb env from the live postinst environment of the merge (`vartree.py:5334` `PORTAGE_UPDATE_ENV=<dbpkgdir>/environment.bz2`; `bin/phase-functions.sh:1072-1082`), while `mrg` kept the binpkg's build-time env. No `bzip2` on the client (plan §6/§14.1).

## fn run_postinst_regen_stage
- The unit flow collects the locale pairs from the process environment (see `collect_server_locale`); tests inject them explicitly.

## fn pull_file
- `Err` is the pull failure (the #171 regen warn path).

## fn bzip2_compress
- The same level Portage's `PORTAGE_UPDATE_ENV` block uses (`-f9`).

## fn scrub_bzip2_command
- `bzip2_command` is the server-resolved client `PORTAGE_BZIP2_COMMAND` (`make.globals`' `bzip2` when nothing is configured). Portage's merge-time save records its live value (verified on this host's own `/var/db/pkg/*/environment.bz2`, which all carry `declare -x PORTAGE_BZIP2_COMMAND="bzip2"`), while ours ran with a per-unit workdir path that must not leak into the vdb (and would keep the L1 `environment` rows red: `normalize.py` compares the content verbatim). Any other value (Portage's own spelling, a foreign path, anything) is copied verbatim.
- `None` on paths that resolve no config: the `make.globals` default.

## fn install_regenerated_env
- `regen_bzip2` is `None` on paths that resolve no config (the `make.globals` default).

## fn install_file_atomic
- A failed `mv` best-effort removes the remote temp name (review #4: no client tmp litter).
- Backlog #262: `TempDir::new` creates a directory (backlog #260), so staging straight into its path fails every ssh install with `Is a directory`, and the vdb keeps the build-time env (with its stray `declare -- x=""`) instead of the regen'd one.

## fn regen_warn_message
- The #171 failure line (coordinator decision).

## fn parse_oldpf_probe
- The driver owns discovery; this only stages envs.

## fn ship_old_hook_envs
- Backlog #171. A version with no bz2 (older `mrg` entries carry only a plain `environment`) ships that file as-is; a version with neither warns with both pull errors and still hits the driver's own warn-and-skip (review #3: no silent staging failures).

## fn client_bin_abs
- The S8.1-resolved absolute client binary (#326 D5/S8.3).
