# crates/portuale/src/pretend/info.rs
`emerge --info`.

## fn use_expand_display_value
- The `emerge --info` half of this file ports `action_info` (`_emerge/actions.py::action_info`), narrowed to its deterministic config/repository block: the `Repositories:` list (Portage `repo.info_string()`), `Binary Repositories:`, `Installed sets:`, then the sorted `VAR="value"` dump (Portage `myvars`) and the `Unset:` line.
- Per-atom handling ports `action_info`'s `myfiles` loop: an atom whose `cat/pkg` has no ebuild anywhere aborts with `emerge: there are no ebuilds to satisfy "<atom>".` + `--misspell-suggestions` (exit 1, before the config block); otherwise, after the config block, a `Package Settings` section (Portage `mypkgs`): per atom, every installed vdb match wins (`<cpv>::<repo> was built with the following:` + its vdb `USE="…"` line + the `CHOST`/`CFLAGS`/`CXXFLAGS`/`FEATURES`/`LDFLAGS` that differ from the current config + an `Unset: …` line); otherwise the best visible ebuild candidate, but only if its ebuild defines `pkg_info()` (`<cpv>::<repo> would be built with the following:` + `USE="…"`); otherwise, under `--usepkg`/`--usepkgonly`, the highest local `$PKGDIR` binary build whose EAPI is 4+ and whose `DEFINED_PHASES` names `info` (`<cpv>::<repo> (non-installed binary) was built with the following:` + the binary's own baked `USE="…"`).
- For each selected package that defines `pkg_info()` portuale prints `>>> Attempting to run pkg_info() for '<cpv>'` and actually runs the phase (`ebuild_phases::run_single_phase` for an ebuild or an extracted binary, `ebuild_merge::run_vdb_saved_env_phase` for an installed match) -- Portage's `writemsg_stdout(...)` + `doebuild(..., "info", ...)` step. The phase's own output is whatever the ebuild `einfo`s; the contract suite pins the deterministic message and the phase run is covered in `test_portuale.py`.
- Large, deliberate cut: Portage `action_info`'s output is dominated by host state a fixture-driven test cannot verify -- the `Portage <ver> (python …, <profile>, <chost>, <gcc>, <libc>, <kernel>)` header, `System uname`, `KiB Mem`, the `sh:`/`gcc:`/`ld:`/`binutils`/`ccache`/`distcc` version probes, the `info_pkgs` version table, repository timestamps / head commits. None of that is reproduced. Narrower than Portage for the installed block: the `CHOST`/`CFLAGS`/… values come only from the individual vdb `build-info` files (Portage `_aux_env_search` falls back to the package's `environment.bz2`). The `USE` line's `( )` force/mask wrap and per-flag ANSI colour (Portage `pkg_use_display`'s `UseFlagDisplay`) are shipped -- see `resolve_installed_info` / `resolve_info_candidate` and `colorize_use_token`.
- An installed match's `>>> Attempting` step follows Portage's own `if metadata["DEFINED_PHASES"]: if "info" not in ...: continue` (`actions.py:2350`): it fires when the vdb `DEFINED_PHASES` names `info`, and also when `DEFINED_PHASES` is entirely empty (Portage's falsy-check quirk -- an old / hand-written vdb entry with no `DEFINED_PHASES` file). `"-"` (the modern "no phases" marker) is truthy and does not fire.
- The binary's ebuild is extracted from the archive and laid out as `<cat>/<pn>/<pf>.ebuild` so the phase driver derives `CATEGORY`/`PN` from the path (Portage `doebuild(tree="bintree")` gets them from the binpkg metadata). `--info` runs its own `$PKGDIR` scan before dispatching (the ordinary `--usepkg` resolve path's scan runs later in the CLI flow).
- `_hide_url_passwd` is also cut.
- `use_expand_display_value` ports `config.regenerate()`'s "Generate global USE_EXPAND variables settings that are consistent with USE" tail (`config.py`, guarded on `self.mycpv is None`). This resolves `-*` / `-flag` / `VIDEO_CARDS="-* intel …"`. Portage appends the `VAR="…"` token to the `USE=` line only `if myval`, so an empty value is simply omitted.

## fn resolved_global_use
- Ports `config.regenerate()`'s last two lines before the USE_EXPAND display pass: `myflags.update(self.useforce)` then `myflags.difference_update(self.usemask)` (the `mycpv is None` / global `getUseForce()` / `getUseMask()` case). `ARCH` is already in `use_flags` via `USE_EXPAND_UNPREFIXED`.
- Portuale keeps `use_force`/`use_mask` *out* of `Config::use_flags` on purpose (they are applied per-candidate in `effective_use_flags`); this reunites them for the one global display that needs it.

## fn info_scalar_value
- Ports `action_info`'s `settings.get(k)` (`_emerge/actions.py:2240`), which walks `lookuplist` env-first (`config.py:3178-3182`; `USE_ORDER` `"env:pkg:conf:defaults:…"` at `:1031-1035`): this is backlog #180's `env_over_config_scalar` chain (backlog #219), not `other_vars` with an env fallback. For keys `resolve_config` already env-folds (`ENV_SCALAR_VARS`, e.g. `PKGDIR`) the two reads agree; for the rest (`PORTAGE_BZIP2_COMMAND`, `PORTAGE_BUNZIP2_COMMAND`, any profile `info_vars` extra outside the allowlists) the env now wins, as Portage prints it. A curated `info_vars` entry that is only ever an environment variable (`SHELL`, `LC_ALL`, …) still shows via the same chain.

## fn tool_first_line
- `args` is usually `["--version"]`. Portage guards with `subprocess.Popen` + `os.EX_OK`.

## fn highest_installed_pvr
- `catpkgsplit`-style `<pv>-<pr>`; ports Portage `get_libc_version` / the `info_pkgs` join.

## fn print_info_header
- Ports the header (`actions.py:1946-2165`): the `Portage <ver> (…)` line, the 65-char rule, `System uname:` (Portage `platform.platform(aliased=1)`), `KiB Mem:`/`KiB Swap:` (Portage `get_vm_info`, i.e. `free`), per-repo `Timestamp`/`Head commit of repository`, the `sh:`/`coreutils:`/`ld:` probes and the `info_pkgs` version table.
- `System Settings` title: Portage `if myfiles:`.
- Portage bakes `retrieve_head`'s trailing newline into the head-commit line.
- --- `Portage <ver> (python …, <profile>, <gcc>, <libc>, <kernel>)` ---

## fn info_profile_version
- If `<config_root>/etc/portage/make.profile` points straight into the main repo, that relative path; else its first `parent` entry (a bare relative path or a `reponame:path`) resolved that way. Portuale's `info_pkgs` list only ever needs the main repo here.
- `reponame:path` -> `<main_repo>/profiles/<path>` (portuale's `info_pkgs` list only ever needs the main repo here).

## fn run_info
- The local binary index: backlog #123 -- threaded as an `Arc`, never re-derived from `&Config` per atom.
- The existence probe (`list_candidates`, all versions masked included, + the vdb) is portuale's `cp_exists` proxy.
- The variable dump: `<PORTDIR>/profiles/info_vars` (`actions.py:2221`); skipped: `PORTAGE_REPOSITORIES`, `PORTDIR`, `PORTDIR_OVERLAY`, `SYNC`. The `const.INCREMENTALS` members (`CONFIG_PROTECT`/`CONFIG_PROTECT_MASK`/`ENV_UNSET`/`FEATURES`, plus `ACCEPT_KEYWORDS`) are shown as Portage stores them: `-*`/`-tok` resolved, then `" ".join(sorted(...))`.
- `USE=` line: Portage `config.regenerate()`'s global USE.
- The buffer end: Portage ends with two `append("")` so `"\n".join` yields a single trailing blank line (`…\n\n`).
- Installed lookup short-circuits the ebuild lookup (`actions.py:1869-1875`); the ebuild half needs `defines_pkg_info`; the binary half is Portage `(bindb, "binary")` (`actions.py:1876`). The header math is `header_title.rjust(int(header_width / 2 + len(title) / 2))` (`header_width = 65`, `actions.py:1956`).
- Per-package block: Portage `actions.py:2314-2410` -- header + `pkg_use_display` line (+ the installed-only `mydesiredvars` diff). `run_phase` is Portage's `if metadata["DEFINED_PHASES"]: if "info" not in ...: continue`.
- The `>>> Attempting` line is Portage `writemsg_stdout(f">>> Attempting to run pkg_info() for '{pkg.cpv}'\n")` then `portage.doebuild(ebuildpath, "info", ...)`. The Python contract reference stops here (no phase machinery, like `--config`/`--regen`); the phase's own output is covered Rust-only in `test_portuale.py`.
- The binary arm ports `bindb.bintree.getname(cpv)` + `doebuild(..., "info", tree="bintree")` (Portage `doebuild` gets `CATEGORY`/`PN` from the binpkg metadata).
- Portage `action_info` also checks `bindb.match(x.cp)` when `--usepkg` is on (`actions.py:1877-1885`): a package only ever available as a binary isn't a "no ebuilds" error.
- Portage `RepoConfig.info_string()` field order (the subset portuale models): name, location, sync-type, sync-uri, masters, priority, aliases, volatile, then any set module-specific opts.
- Portage `BinRepoConfig.info_string()` field order: name, location (if set), priority, sync-uri, verify-signature.
- Installed sets (Portage `root_config.sets["selected"].getNonAtoms()` -> the `@name` entries of `world_sets`).
- Portage `settings.get(k)` walks `lookuplist` env-first (`actions.py:2240`; `config.py:3178-3182`, `USE_ORDER` `"env:pkg:conf:…"` at `:1031-1035`), so the calling environment wins over `make.conf`/profile/`make.globals` -- including for keys `resolve_config` never env-folds itself (backlog #219: `PORTAGE_BZIP2_COMMAND`, `PORTAGE_BUNZIP2_COMMAND`).
- Portage `pkg_use_display`'s own `UseFlagDisplay.__str__`: each flag token is coloured (the same `colorize_use_token` scheme the merge-list `USE="…"` column already uses), independent of `--quiet`/`--verbose` -- `--info`'s block always shows every flag.
