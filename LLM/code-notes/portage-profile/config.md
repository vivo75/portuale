# crates/portage-profile/src/config.rs
The resolved configuration (`Config`): every USE layer, visibility list, and fingerprint base.

## struct ProfileUseLayer
- `make_defaults_use` holds `USE=` value strings in file order, then that level's own `package.use` entries.

## struct UseMaskForceLevel
- `use_*` fields hold raw `flag`/`-flag` line tokens (global `use.mask`/`use.force`); the `package_use_*` fields hold `(atom, flag-tokens)` pairs (`package.use.mask` etc.).

## field use_tokens
- Portage `configdict["defaults"]`'s own `make_defaults_use`.
- A flat union of the already-flattened `use_flags` on top of IUSE defaults could only ever add a flag, never let `defaults`/`conf` cancel an IUSE `+default` the way Portage's single continuous incremental walk (`repo` -> `pkginternal` -> `defaults` -> `conf` -> `pkg`) does; hence the replay model (see `effective_use_flags` in `portage-repo`).
- Historically this field also held the `make.conf` + `USE_EXPAND` values; the "Config depth" slice split those into `conf_use_tokens` so profile-level `package.use` can sit at its Portage `USE_ORDER` position between the two (after `defaults`, before `conf`).

## field conf_use_tokens
- Split out of `use_tokens` by the "Config depth" slice (SCOPE_BACKLOG Part 2.C); see that field's doc.

## field env_use_tokens
- The global `use_flags` set is also env-last, correct for any package with no `package.use` entry; this field makes the per-package walk agree with it.

## field envd_use_tokens
- From `<eroot>/etc/profile.env` (which `env-update` generates from `/etc/env.d/*`); read `expand=False`, `export ` prefix stripped.

## field envd_sets_path
- Portage `_doebuild_path` (`doebuild.py:365-368`): drops the calling env's `PATH` so the phase `PATH` is the config-file stack's value (`other_vars["PATH"]`), "this allows packages to update our PATH as they get installed".

## field envd_raw / envd_eroot
- Per-package-task reloads (`reload_env_d`, Portage `config.reload()`, `config.py:2691-2699`) only pay for a re-read when `env-update` actually rewrote the file mid-run.
- `entry_build_env` re-reads the same file without a root parameter.

## field envd_vars
- Keys a higher layer (`make.globals`, the profile chain, `make.conf`, the calling env) also assigned stay listed here when that higher value is what `other_vars` holds.

## field envd_higher_keys
- Includes `"USE"` when the globals/profile `USE`-reset dropped env.d's scalar (see `resolve_config`).

## field envd_profile_only
- The profile half of Portage `_env_d_blacklist` (`profile_only_variables + _env_blacklist`, `config.py:734-743`).

## field package_use_repo
- Tokens use the same `-flag`/`flag`/`+flag` incremental syntax as `USE` itself (see `apply_incremental`).
- Overlay entries `::repo`-scoped via `scope_repo_package_use_lines`; the main repo's stay unscoped (it implicitly masters every overlay).

## field conf_use_tokens
- Composition: `make.conf` (plus sourced files) `USE=` values, then every `USE_EXPAND` variable's value (`varname_`-prefixed), then every `USE_EXPAND_UNPREFIXED` variable's raw value: Portage `configdict["conf"]` plus the `USE_EXPAND` folding `regenerate()` does last. Replayed immediately after `use_tokens` and the profile-level `package.use`, immediately before user-level `package.use`.

## field repo_make_defaults_use
- Narrowing: the full masters-chain stacking order (main, then each master, then the repo) collapses to "main, then the candidate's own repo".

## field features_use
- Portage `config.py` ~2043 (`configdict["features"]["USE"]`); in practice only `test`, appended when `FEATURES` contains it (the make.conf/env equivalent of `--with-test-deps`).
- Applied by `effective_use_flags` between the `repo` and `pkginternal` tiers.

## field package_use
- The `defaults` tier is walked one profile at a time, each level's `make.defaults` USE then that level's own `package.use`, before the next level (Portage `regenerate()`), so a child profile's `make.defaults` can cancel a parent's `package.use`.
- Still populated by `resolve_config` for inspection and the hand-built-`Config` fallback.

## field package_use_user
- Portage appends `self.puse` on top of the `_grab_pkg_env` result (`config.py`), the strongest USE layer before `use.force`/`use.mask`.

## field autounmask_use
- Portage `_dynamic_config._needed_use_config_changes`, the in-loop config-change feedback (`backtracking.py::_feedback_config`).
- A hand-built `Config` literal never sets it and gets the pre-autounmask behaviour.

## field use_context_base
- Frozen digest computed by `use_context_base_fingerprint`. Backlog #105's memo-key optimization (23.77% of the reference workload with children).

## field is_visible_base
- Backlog #114. The `is_visible` memo hashes it plus the live `autounmask_use` (which the check reaches through `use_flags_if_conditional`).

## field autounmask_backtrack
- Portage `emerge` option, `choices: ("y", "n")`. Off: Portage collects autounmask config changes but does not re-drive the graph (`need_config_change` -> break in `_backtrack_depgraph`): the flipped package's own USE line reflects the change, but its newly-`flag?`-gated deps do not appear. On (or implied by `--autounmask-continue`): the `'backtrack` loop re-runs the walk on every accumulator growth.

## field complete_seed_atoms
- Portage `_complete_graph`'s deep re-walk of the required sets. The resolver gates the slot-operator-rebuild scan on it: only slot-op-rebuilds a consumer reachable from those sets. `emerge -p <atom>` with no installed change triggers no slot-op rebuild.
- Backlogs #68/#72 context for the sibling field below.

## field blocker_retry_seed_atoms
- Backlogs #68/#72 B2c. Portage `_serialize_tasks` re-runs the whole serialization in complete mode when the first pass selected an uninstall task (`depgraph.py:10365-10383`), which walks the required sets, so a block resolved by an uninstall sees those sets' installed closure even on a plain `emerge -p <atom>` run. Filling `complete_seed_atoms` here instead would switch #24/#65 behaviour on for non-complete runs.

## field complete_locked_merges
- Portage never resolves a new merge in the complete pass (graph-or-installed only).

## field package_env
- Portage `grabdict_package(.../package.env)`. Each named file lives under `/etc/portage/env/` and is a `make.conf`-style snippet layered onto a matching package's config (Portage `_grab_pkg_env`, invoked from `setcpv`).

## field package_env_use
- Portage fills `pkg_configdict` from `package.env` first, then appends `package.use`'s own USE on top (`config.py:2042-2048`), so a user `package.use` flag wins over a `package.env` one.
- Portuale's standing "no warnings from deep in config resolution" precedent for the silent missing-file case.

## field package_env_vars
- Portage `_grab_pkg_env` folds every key into `configdict["pkg"]`; `ebuild_phases::match_package_env_vars` applies Portage's acceptance gate and layers survivors onto the build-phase env (incrementals folded, not replaced). `FEATURES`/`PORTAGE_TMPDIR` are residues #98/#99.

## field system_packages
- Only after stacking does Portage keep the subset starting with `*` (stripping it) as the `@system` atom list; every other stacked line is a "known to the profile but not base system" hint. Confirmed by reading `PackagesSystemSet.load`, which calls the same `stack_lists` function `MaskManager` does, on the raw lines (`*foo` and plain `foo` alike: `-foo` only ever removes an earlier exact-text `foo`, never a `*foo`, same plain-string equality `stack_mask_lines` uses).
- No repo-level or user-level source exists in Portage at all (only `PackagesSystemSet.__init__`'s `profiles` list, never `config_root`), unlike `package.mask`.

## field profile_packages
- A level not inside any configured repo contributes nothing (Portage gates on the level's own repo `profile_formats`; the user-config profile node is the same deliberate narrowing).
- Portage `@world` is `@profile @selected @system` (`_sets/__init__.py:97-98`).

## field package_provided
- Portage `pprovideddict`, flattened: Portage cp-keys it into a dict, but `match_from_list` already filters by cp, so the flat list is equivalent.
- Portage validates each line with `isvalidatom("=" + line)` and drops invalid ones with a warning; portuale carries every stacked line and lets `match_from_list` never match a malformed one (documented simplification).
- The EAPI 7+ gate (`allows_package_provided`, disallowed for EAPI 7+) is not ported, consistent with the "no EAPI parametrization within the 5+ floor" precedent (EAPI 5, what every fixture profile is, allows it).

## field use_force / use_mask
- Previously portuale folded these into `use_flags`, which was wrong: Portage `regenerate()` applies `self.useforce`/`self.usemask` (set by `setcpv()` to the per-package `getUseForce(pkg)`/`getUseMask(pkg)`, global `use.force`/`use.mask` combined with atom-scoped `package.use.force`/`.force`) as the last step of its incremental USE walk (`myflags.update(useforce)` then `myflags.difference_update(usemask)`), strictly after the `pkg` tier: force-add first, then force-remove, so a flag in both ends up masked.
- Portage's `forced_flags` (consumed by `--newuse`'s `reinstall_flags_for_newuse` in `portage-repo`, previously always empty) is `use.force ∪ use.mask`, not either alone.

## field package_use_force / package_use_mask
- Repo-level (every repo, `::repo`-scoped the same way `package.mask` scopes overlay entries, but with no `masters`-merge step: Portage `UseManager.py` never combines an overlay's file with its master's at load time, only later per-package in `getUseMask`/`getUseForce`) plus every profile level's own file, in chain order. Confirmed by reading `UseManager.__init__`'s file/variable table that no user-level source exists for either file.
- Deliberate simplification made applying these per package: comparison-operator atoms lose Portage `best_match_to_list`'s "closest version wins a tie" refinement, since `package.use.mask`/`.force` files essentially never use `>`/`<`/`>=`/`<=` atoms in practice. The stable-vs-`~arch` `KEYWORDS` distinction named as a cut when this was written is now stale (closed by the `use.stable.mask`/`.force` follow-up).
- The full scope writeup, including the atom-specificity algorithm (`ordered_by_atom_specificity`, ported), lives with `effective_use_flags` in `portage-repo`.

## field use_expand
- Portage is genuinely per-variable-incremental here, but portuale's blanket "no incremental merge outside USE/ACCEPT_KEYWORDS" cut already covers every other such variable, so each named variable's value uses the last-level-wins scalar mechanism rather than inventing a new one.
- `package.use`'s `USE_EXPAND`-prefix shorthand (`VIDEO_CARDS: nvidia` lines) is user-level-only, confirmed by reading `UseManager.__init__` (repo/profile parsers go through a different function that never applies it); hence repo/profile lines parse separately from user-level ones.
- `USE_EXPAND_UNPREFIXED` companion: Portage Gentoo's own `profiles/arch/amd64/make.defaults` sets `USE_EXPAND_UNPREFIXED="ARCH"`, literally how `amd64`/`x86`/`arm64` exist as plain USE flags.
- `USE_EXPAND_IMPLICIT` + `IUSE_IMPLICIT` + `USE_EXPAND_VALUES_<v>` feed the EAPI 5+ `IUSE_EFFECTIVE` computation. IUSE-aware `_*` wildcard expansion (`linguas_*`) is done in `portage_repo::effective_use_flags` (it needs a specific candidate's IUSE).
- `USE_EXPAND_HIDDEN` is genuinely display-only for EAPI 5+, honoured only by `emerge --pretend -v`'s `USE_EXPAND` grouping (`portage_repo`).
- ARCH-based KEYWORDS-format validation is out of scope. Wildcard `_*` IUSE-aware expansion (e.g. `linguas_*`) lives in `effective_use_flags`, not this global layer (it needs a specific candidate's IUSE).

## field iuse_effective
- Exactly the extra domain Portage `pkg.iuse.is_valid_flag` grants an EAPI 5+ package on top of declared `IUSE`, so a USE-dep like `foo[elibc_glibc]` matches a `foo` that never lists it. `USE_EXPAND_HIDDEN` is deliberately excluded: for EAPI 5+ it is a pure display concern (Portage `_get_implicit_iuse` is the pre-EAPI-5 path).

## field use_stable_force / use_stable_mask
- PMS 5+, always recognized here per the "no EAPI parametrization" precedent. Ported as `portage-repo`'s `is_stable`, grounded against Portage `KeywordsManager.isStable`, which is subtler than a raw "no `~` prefix" check: a candidate counts as stable if replacing every one of its own KEYWORDS with its `~`-prefixed form would make it invisible under the current `ACCEPT_KEYWORDS`/`package.accept_keywords`.
- `use.stable.mask`/`.force` read from the profile chain only (matching the established profile-only sourcing for the non-stable global files, not the additional repo-level source in per-package `getUseMask`/`getUseForce`); `package.use.stable.mask`/`.force` read repo-level (every repo, `::repo`-scoped) plus profile-chain, mirroring `package.use.mask`/`.force`, no user-level source (same `UseManager.__init__` table confirmation).

## field archlist
- Portage `config.py`'s `grabfile` + `stack_lists(archlist, incremental=1)`, confirmed by reading it directly. Feeds Portage `_get_implicit_iuse()` (plus the profile's `ARCH`, `use.mask`/`use.force`, and literal `build`/`bootstrap`), making e.g. `x86` a valid (if not enabled) `REQUIRED_USE` reference when a package's own `IUSE` never mentions it.
- Deliberate simplification: not modeling `USE_EXPAND_HIDDEN`-derived regex flags (`elibc_.*` etc.), a bigger separate feature (no ELIBC/KERNEL/USERLAND support at all).

## field use_mask_force_levels
- The flat fields stay for global-only / approximate readers (`emerge --info`'s global `USE=`, the REQUIRED_USE valid-IUSE domain, `--newuse`'s forced-flags filter). See `resolved_use_mask` in `portage-repo`.
- Interleaving example the flat union cannot express: a global `use.mask -flag` line at a later profile level cancels an earlier level's `package.use.mask` entry (and vice-versa).

## field license_groups
- Portage's own `setdefault(k, []).extend(v)` (extended, not stacked/masked). A group's value list may itself reference another `@group` recursively, resolved by `expand_license_token` once, here in `resolve_config`, for both `accept_license` and `package_license` (matching Portage's `set_accept_license_str`/`_read_user_config`, which both call `expandLicenseTokens` eagerly at config-read time, not per-candidate), not pre-flattened into the map itself, matching Portage `_expandLicenseToken`'s recursive-with-cycle-guard shape exactly.

## field accept_license
- Portage's own default when unset anywhere in the whole chain is `"* -@EULA"` (`config.py`'s `accept_license_str = " ".join(mysplit) or "* -@EULA"`).
- Deliberately last-level-wins, not genuinely incremental across sources the way Portage's is (`prune_incremental` over every source's raw tokens): extends the pre-existing "any variable other than USE/ACCEPT_KEYWORDS is a plain last-level-wins scalar" cut to this variable rather than inventing an ACCEPT_LICENSE-specific mechanism.

## field package_license
- A rare, opt-in newer Portage sourcing variant (profile-level `package.license` via the `profile-license` profile-format marker) deliberately not replicated, same "narrow, rare sourcing variant" cut made elsewhere.
- Portage's `*/*`-line "extract into the global `ACCEPT_LICENSE`" quirk (`extract_global_changes`) deliberately not replicated: a `*/*` line is an ordinary per-package entry matched via the wildcard-atom machinery.

## field accept_properties
- Portage `config.py`'s own comment: "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without groups"; no `@group` expansion (confirmed by reading its handling, which never touches `LicenseManager`).
- Default `"*"` comes from `cnf/make.globals` (a config layer portuale doesn't model as a read file), replicated as a hardcoded fallback. `accept_license`'s `"* -@EULA"` is instead a genuine Python-level hardcoded fallback even in Portage itself.

## field pkgdir
- Read by `portage-repo`'s `list_binary_candidates` when `--usepkg`/`--usepkgonly` is given.

## field binrepos
- Portage "Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones" (`name_fallback = md5(uri)`, incrementing priority). Read by `list_remote_binary_candidates` (see `BinRepo::packages_dir`).

## field scanned_binpkgs
- Portage caches the scan as `Packages`; portuale recomputes each run, so `--pretend` still writes nothing. Read as a `BinaryIndex` by `portage-repo` when set, else `<pkgdir>/Packages`.

## field other_vars
- E.g. `CHOST`, `CFLAGS`, `GENTOO_MIRRORS`, `CONFIG_PROTECT`, `PORTAGE_TMPDIR`. Any variable other than USE/ACCEPT_KEYWORDS is a plain scalar for `${VAR}` substitution (last-value-wins, no incremental merge), matching real profile usage (e.g. `ARCH` feeding `ACCEPT_KEYWORDS="${ARCH}"`). No line continuation / multi-line quoted values, and no trailing `#` comment after an assignment (a `#` is a comment only at line start); real files in practice rely on neither.

## field incremental_sources
- Portage stacks in `config.regenerate()` and stores `" ".join(sorted(myflags))`.

## fn resolved_incremental
- Portage `self.configlist[-1][mykey] = " ".join(sorted(myflags))`.

## fn env_d_changed
- The per-package-task gate (`entry_build_env`).

## fn reload_env_d
- Portage `config.reload()` (`config.py:2691-2699`). Expansions computed against the old env.d at resolve time are never re-expanded (Portage's `expand=False` env.d read likewise never re-expands them).
- Nothing post-resolve mutates the incremental source lists, so a recorded env.d layer is still index 0.
- The "no recorded layer (or its trace is gone)" branch is unreachable (see above): the new env.d layer goes first regardless.
