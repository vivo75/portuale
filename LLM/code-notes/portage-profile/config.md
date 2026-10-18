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
- `USE_EXPAND_UNPREFIXED` companion: Gentoo's own `profiles/arch/amd64/make.defaults` sets `USE_EXPAND_UNPREFIXED="ARCH"`, literally how `amd64`/`x86`/`arm64` exist as plain USE flags.
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
- Portage `config.py:2691-2699`, `reload`.

## struct Config
- field `use_tokens`: Historically this field also held the `make.conf` + `USE_EXPAND` values; the "Config depth" slice split those into [`Config::conf_use_tokens`] so profile-level `package.use` ([`Config::package_use`]) can be applied at its Portage `USE_ORDER` position *between* the two (after `defaults`, before `conf`).
- field `env_use_tokens`: The process-environment `USE="..."` value(s) -- Portage `configdict["env"]["USE"]`, the **highest** `USE_ORDER` tier. `effective_use_flags` replays these (via `apply_incremental`, `+`/`-` syntax preserved) *after* the user-level `/etc/portage/package.use`, so `USE="-X" emerge foo` overrides even a `package.use` flag for `foo`. Populated by `apply_env_layer` from `$USE`; empty otherwise. (The global [`Config::use_flags`] set is also env-last, which is correct for any package with no `package.use` entry; this field makes the per-package walk agree with it.)
- field `envd_use_tokens`: Portage `configdict["env.d"]["USE"]` -- the **lowest** `USE_ORDER` tier (`env:pkg:conf:defaults:pkginternal:features:repo:env.d`), from `<eroot>/etc/profile.env` (which `env-update` generates from `/etc/env.d/*`). Read `expand=False`, `export ` prefix stripped. `effective_use_flags` replays these first, before the `repo` tier, so everything else overrides them. Read from `<eroot>` (see `resolve_config`), not `config_root`. `/etc/env.d/*` practically never sets `USE`, so this is almost always empty.
- field `envd_higher_keys`: Every key a higher config layer assigned during `resolve_config`, whether or not env.d held it then (plus `"USE"` when the globals/profile `USE`-reset dropped env.d's scalar -- `resolve_config`'s own `scalars.remove("USE")`). `reload_env_d` never touches `other_vars` for these keys: the higher layer wins, exactly like Portage's layered `configdict` lookup, where replacing `configdict["env.d"]` cannot move a key a higher db owns.
- field `package_mask`: Raw atom or bounded-wildcard-atom strings (see `portage_dep::parse_wildcard_atom`) from `package.mask`, with `-atom` removal already applied within this source.
- field `package_accept_keywords`: (atom-or-wildcard string, extra accepted keyword tokens) pairs from `package.accept_keywords`. A `"**"` keyword token means "accept any keyword" for matching packages.
- field `package_use_repo`: (atom-or-wildcard string, raw USE tokens) pairs from **every configured repo's own** `profiles/package.use` -- Portage `configdict["repo"]` (`_repo_puse_dict`). Overlay entries are `::repo`-scoped (`scope_repo_package_use_lines`); the main repo's stay unscoped (it implicitly masters every overlay). Applied by `effective_use_flags` *first*, before the ebuild's own IUSE `+`/`-` defaults -- the weakest USE layer portuale models. Tokens use the same `-flag`/`flag`/`+flag` incremental syntax as `USE` itself -- see `apply_incremental`.
- field `repo_make_defaults_use`: `(repo_name, USE= value string(s))` pairs from each configured repo's `<repo>/profiles/make.defaults` -- Portage `config.py`'s `_repo_make_defaults`, folded into `configdict["repo"]` alongside repo `package.use`. Applied by `effective_use_flags` at the very start of the `repo` layer, **before** [`Config::package_use_repo`] (Portage `regenerate()` walks `configdict["repo"]["USE"]` first, then that tier's package-scoped USE). `${VAR}` in a value is expanded against the profile-chain + `make.conf` scalars already accumulated by the time each file is read.
- field `repo_make_defaults_use`: The **main repo's** entry has an empty `repo_name` -- it applies to every package (the main repo implicitly masters every overlay here). An **overlay's** entry is keyed by its Portage repo name and applies only to a candidate resolved from that overlay (`candidate_str`'s `::<repo>` suffix). **Narrowing:** the full masters-chain stacking order (main, then each master, then the repo) collapses to "main, then the candidate's own repo".
- field `features_use`: Portage `configdict["features"]["USE"]` (`config.py` ~2043): the USE flags implied by `FEATURES`. In practice only `test` -- appended when `FEATURES` contains `test`, so a package that declares `test` in IUSE resolves with it enabled (the make.conf/env equivalent of `--with-test-deps`). Applied by `effective_use_flags` between the `repo` and `pkginternal` tiers. `["test"]` or empty; portuale's `FEATURES` is a last-wins scalar (`other_vars["FEATURES"]`), a documented simplification of Portage's incremental stacking.
- field `package_use`: (atom-or-wildcard string, raw USE tokens) pairs from every **profile** level's own `package.use` (chain order) -- Portage `configdict["defaults"]` (`_pkgprofileuse`), as one flat group. **Superseded** for the actual `defaults`-tier walk by [`Config::profile_use_layers`] (which interleaves each level's `package.use` right after that level's `make.defaults`, matching Portage); `effective_use_flags` only falls back to this flat list when `profile_use_layers` is empty (a hand-built `Config`, e.g. a test helper). Still populated by `resolve_config` for inspection / that fallback.
- field `profile_use_layers`: The `defaults` `USE_ORDER` tier, resolved **per profile chain level** (weakest first) rather than as two flat groups. Portage `config.py::regenerate()` walks `configdict["defaults"]` one profile at a time, applying that profile's `make.defaults` `USE=` and then its own `package.use` before moving to the next -- so a child profile's `make.defaults USE="-foo"` can cancel a parent profile's `package.use foo` for a matching package. `effective_use_flags` iterates this in place of the flat [`Config::use_tokens`] + [`Config::package_use`] whenever it is non-empty.
- field `package_use_user`: (atom-or-wildcard string, raw USE tokens) pairs from the user-level `/etc/portage/package.use` -- Portage `configdict["pkg"]` (`_pusedict` / `getPUSE`). Applied by `effective_use_flags` after `make.conf`, the strongest USE layer before the final `use.force`/`use.mask` step. This is the only one of the three `package.use` sources that gets the `USE_EXPAND`-prefix shorthand (`VIDEO_CARDS: nvidia` lines) -- Portage's own user-only `extended_syntax`; see `parse_package_use_lines`.
- field `autounmask_use`: `--autounmask-use` USE changes the resolver has decided are needed to make the graph resolve -- Portage `_dynamic_config. _needed_use_config_changes`, the in-loop config-change feedback (`backtracking.py::_feedback_config`). `(atom, [+flag | -flag …])` pairs, exactly `package_use_user`-shaped -- so `effective_use_flags` replays them the same way, as the **top** USE tier (after `env`, before `use.force`/`use.mask`): a package under an autounmask flip is walked with the flipped flag, and its `flag?`-gated deps appear.
- field `autounmask_use`: Normally empty. `portage-repo`'s `'backtrack` loop rebuilds it from its own `autounmask_use_config` accumulator at the top of each retry iteration (it clones `Config` per iteration to vary this), so a hand-built `Config` literal never sets it and gets the pre-autounmask behaviour.
- field `use_context_base`: A frozen digest of every USE-context field **except** [`autounmask_use`](Self::autounmask_use), computed by [`resolve_config`] once the config is complete (`use_context_base_fingerprint`). The memo keys in `portage-repo` (`effective_use_flags`, `resolved_use_mask_or_force`, `metadata_key_accepted`) rebuild their fingerprint from this base plus the live `autounmask_use`, instead of re-hashing the big `HashSet`s and `Vec`s on every call (backlog `#105`: 23.77 % of the reference workload with children).
- field `is_visible_base`: #114: the visibility base -- the immutable `is_visible`-relevant config fields the USE-context base does not cover: `package.mask`/ `package.unmask`, the `LICENSE`/`PROPERTIES`/`RESTRICT` accept lists, `package.accept_keywords`, and `license_groups`. Frozen by [`resolve_config`] (`is_visible_base_fingerprint`); the `portage-repo` `is_visible` memo hashes it plus the live `autounmask_use` (which the check reaches through `use_flags_if_conditional`) instead of re-hashing those lists per call. Same `None`-means-fall-back contract as [`use_context_base`](Self::use_context_base).
- field `autounmask_backtrack`: `--autounmask-backtrack` (Portage `emerge` option, `choices: ("y", "n")`, **disabled by default** -- `man emerge`). When off (the default), Portage collects autounmask config changes but does **not** re-drive the whole graph afterwards (`need_config_change` -> break in `_backtrack_depgraph`): the flipped package's own USE line reflects the change, but its newly-`flag?`-gated deps do not appear. When on (or implied by `--autounmask-continue`), the `'backtrack` loop re-runs the walk on every accumulator growth so the cascade is fully realised. Like [`autounmask_use`], this is a CLI-option carrier the resolver reads, not profile state; a hand-built `Config` literal leaves it `false`.
- field `complete_seed_atoms`: The `@world ∪ @selected ∪ @system` atom list, populated by the CLI layer **only when complete-graph mode is active** (Portage `_complete_graph`'s deep re-walk of the required sets). The resolver uses it (`required_set_reachable_cps`) to gate the slot-operator-rebuild scan -- Portage only slot-op-rebuilds a consumer reachable from those sets. Empty when not in complete mode (and for any hand-built `Config`), which disables the scan entirely, matching Portage (`emerge -p <atom>` with no installed change triggers no slot-op rebuild).
- field `blocker_retry_seed_atoms`: The same `@world ∪ @selected ∪ @system` atom list as [`Config::complete_seed_atoms`], but populated by the CLI layer on **every** pass, complete or not (#68/#72 B2c). Portage's `_serialize_tasks` re-runs the whole serialization in complete mode when the first pass selected an uninstall task (`depgraph.py:10365-10383`), which walks the required sets -- so a block resolved by an uninstall sees those sets' installed closure even on a plain `emerge -p <atom>` run, where the surrounding resolution is *not* complete. The resolver uses it only for that blocker classification (`ResolveCtx::blocker_retry_closure`); it deliberately does not feed `complete_seed_atoms`, which gates the slot-operator-rebuild scan (filling it here would switch #24/#65 behaviour on for non-complete runs).
- field `complete_locked_merges`: `"category/package"` of every package the **first, non-complete** resolve pass would actually merge. Portage `_complete_graph` runs after the normal graph is built and swaps package selection to `_select_pkg_from_graph` -- graph-or-installed, **never a new merge** (`depgraph.py:8662`). Portuale re-resolves the whole graph in complete mode instead, so the CLI layer passes phase 1's merge set here: in the complete pass, an atom whose `cp` is not in this set may only resolve to `AlreadyInstalled`/`NoVisibleCandidate`, never a merge. Empty for any non-complete pass and every hand-built `Config` -- a strict no-op there.
- field `package_env`: (atom-or-wildcard string, ordered env-file names) pairs from `/etc/portage/package.env` -- Portage `config.py:894`'s `grabdict_package(.../package.env)`. Each named file lives under `/etc/portage/env/` and is a `make.conf`-style `KEY=value` snippet layered onto a matching package's config (Portage `_grab_pkg_env`, invoked from `setcpv`). An entry with no file names is dropped. The `USE=` half of each file is pre-resolved into [`Config::package_env_use`], the build-relevant scalar half into [`Config::package_env_vars`].
- field `package_env_use`: (atom-or-wildcard string, raw USE tokens) pairs derived from [`Config::package_env`]: for each entry, every referenced `/etc/portage/env/<name>` file's `USE=` value(s), concatenated in file order. Applied by `effective_use_flags` in the `pkg` `USE_ORDER` layer, immediately **before** [`Config::package_use_user`] -- Portage fills `pkg_configdict` from `package.env` first, then appends `package.use`'s own USE on top (`config.py:2042-2048`), so a user `package.use` flag wins over a `package.env` one. A referenced file that doesn't exist contributes nothing (silently -- portuale's standing "no warnings from deep in config resolution" precedent).
- field `system_packages`: `@system`'s Portage atom source: every profile level's own `packages` file, stacked in chain order and filtered to `*`-prefixed lines (the `*` stripped) -- see the module doc comment's `packages` bullet and `PackagesSystemSet.load`.
- field `profile_packages`: `@profile`'s Portage atom source (`ProfilePackageSet.load`, `portage/_sets/profiles.py:13-38`): the **unstarred** lines of the `packages` files of only those profile levels whose repo declares `profile-set` in `metadata/layout.conf`, stacked in chain order with the same `-atom` removal `@system` uses. Portage `@world` is `@profile @selected @system` (`_sets/__init__.py:97-98`), so this set is part of every `@world` expansion. A level not inside any configured repo contributes nothing (Portage gates on the level's own repo `profile_formats`; the user-config profile node is the same deliberate narrowing).
- field `package_provided`: `package.provided` -- Portage `config.py:970-1027`'s `pprovideddict`, flattened: every profile level's own `package.provided` file (chain order) + the user-level `/etc/portage/profile/package.provided`, stacked with the same `stack_lists(incremental=1)` `-atom` removal `package.mask`/ `packages` use, as a flat list of bare `cat/pkg-version` CPVs. A dependency atom (or a top-level target) that `match_from_list` against this list is treated as already satisfied and never pulled in (Portage `dep_check.py:1052` / `depgraph.py:5497-5615`; Portage cp-keys this into a dict, but `match_from_list` already filters by cp, so the flat list is equivalent). Each line should be a bare CPV -- Portage validates with `isvalidatom( "=" + line)` and drops invalid ones with a warning; portuale carries every stacked line through and lets `match_from_list` simply never match a malformed one (a documented simplification). Portage's EAPI 7+ gate (`allows_package_provided`, disallowed for EAPI 7+) is NOT ported: portuale tracks no per-profile-level EAPI, consistent with its "no EAPI parametrization within the 5+ floor" precedent (and EAPI 5 -- what every fixture profile is -- does allow it).
- field `use_expand`: `USE_EXPAND` itself (PMS 7.3.4 profiles doc; Portage `const.py`'s own `INCREMENTALS` list): the set of variable NAMES (e.g. `VIDEO_CARDS`, `PYTHON_TARGETS`) accumulated incrementally across every profile level's own `make.defaults` plus `make.conf`, same mechanism `USE`/ `ACCEPT_KEYWORDS` already use. Each named variable's own VALUE is expanded into lowercase-prefixed pseudo-USE-flags already folded into `use_flags` by the time `resolve_config` returns (e.g. `VIDEO_CARDS="nvidia"` contributes `video_cards_nvidia`) -- see the module doc comment's own `USE_EXPAND` bullet for the full scope writeup and its deliberate simplifications. Exposed here too (not just folded into `use_flags`) purely for documentation/testability, same reasoning `use_force`/`use_mask` already have for being separately visible.
- field `use_expand_unprefixed`: `USE_EXPAND_UNPREFIXED` (Portage `config.py`'s own companion to `USE_EXPAND`): the set of variable NAMES (e.g. `ARCH` -- Gentoo's own profile sets `USE_EXPAND_UNPREFIXED="ARCH"`, which is literally how `amd64`/`x86`/`arm64` etc. exist as plain USE flags at all) accumulated incrementally the same way `USE_EXPAND` is. Each named variable's own VALUE is folded into `use_flags` *without* any prefix at all (unlike `USE_EXPAND`'s own `lowercase(name)_` prefixing) -- see the module doc comment's own `USE_EXPAND_UNPREFIXED` bullet for the full scope writeup. Exposed here too, same reasoning `use_expand` already has.
- field `use_expand_implicit`: `USE_EXPAND_IMPLICIT` (Portage `config.py`, an INCREMENTAL): the set of `USE_EXPAND`/`USE_EXPAND_UNPREFIXED` variable NAMES whose expanded flags count as *implicitly*-valid IUSE for every package even when unlisted in its own `IUSE` -- Gentoo's own `profiles/base/make.defaults` sets `USE_EXPAND_IMPLICIT="ARCH ELIBC KERNEL USERLAND"`. Accumulated incrementally the same way `USE_EXPAND` is; drives `iuse_effective` below. Exposed for testability, same as `use_expand`.
- field `iuse_implicit`: `IUSE_IMPLICIT` (Portage `config.py`, an INCREMENTAL): literal flag names that are implicitly-valid IUSE for every package (Gentoo: `prefix prefix-guest ...`). Accumulated incrementally; folded straight into `iuse_effective` below.
- field `iuse_effective`: Portage EAPI 5+ `IUSE_EFFECTIVE` (`config.py::_calc_iuse_effective`): `iuse_implicit`, plus every value of each `USE_EXPAND_UNPREFIXED` variable that's also in `use_expand_implicit` (unprefixed), plus `lowercase(v)_<value>` for each `USE_EXPAND` variable `v` that's also in `use_expand_implicit` -- the values coming from `USE_EXPAND_VALUES_<v>`. This is exactly the extra domain Portage `pkg.iuse.is_valid_flag` grants an EAPI 5+ package on top of its own declared `IUSE`, so a USE-dep like `foo[elibc_glibc]` matches a `foo` that never lists `elibc_glibc` in `IUSE`. `USE_EXPAND_ HIDDEN` is deliberately NOT part of this -- for EAPI 5+ it is a pure `emerge --info`/`-pv` USE-grouping *display* concern (Portage `_get_implicit_iuse` is the pre-EAPI-5 path). See `use_expand_hidden` below, which `emerge --pretend -v`'s own `USE_EXPAND` grouping honours.
- field `use_expand_hidden`: `USE_EXPAND_HIDDEN` (Portage `config.py`, an INCREMENTAL): the subset of `USE_EXPAND` variable NAMES whose expanded flags Portage omits from `emerge --info`/`emerge -pv`'s own `USE_EXPAND` grouping (`output.py::map_to_use_expand`'s own `remove_hidden`). Accumulated incrementally the same way `USE_EXPAND` is. Display-only: never consulted by `iuse_effective`/visibility/dependency resolution -- only `portage_repo`'s own `-pv` `USE=`-line grouping reads it.
- field `use_stable_force`: `use.stable.force`: every profile level's own file (in chain order), stacked the same `-atom`-removal way `use_force` already is -- but, unlike `use_force`, deliberately NOT folded into `use_flags` here: Portage `getUseForce`'s own `pkg=None` (global) case never even looks at the stable variant at all (confirmed by reading it), since "stable" is inherently a per-candidate property (it depends on that candidate's own KEYWORDS) with no meaningful "global" value. `portage-repo`'s own `effective_use_flags` applies this conditionally, once it knows a specific candidate's own stability -- see that crate's own doc comment for the full `package.use.stable.mask`/`.force` scope writeup, including the deliberate simplification of not also adding the repo-level sourcing Portage per-package `getUseForce(pkg)` has for the *non-stable* global file (which portuale's own `use_force` never had either, profile-chain-only, so the stable variant stays consistent with it rather than gaining new capability the non-stable one lacks).
- field `archlist`: `PORTAGE_ARCHLIST`: every profile level's own `arch.list` file, stacked in chain order with the same `-entry` removal semantics `package.mask` uses (Portage `config.py`'s own `grabfile` + `stack_lists(archlist, incremental=1)`, confirmed by reading it directly). Portage `config.py`'s own `_get_implicit_iuse()` feeds this (plus the profile's own `ARCH`, `use.mask`/`use.force`, and literal `build`/`bootstrap`) into `IUSE_IMPLICIT`, which makes a flag like `x86` a *valid* (if not necessarily enabled) REQUIRED_USE reference even when a specific package's own `IUSE` never mentions it -- `portage-repo`'s own `resolve_pretend_graph` unions this into the `iuse_set` it hands `check_required_use`. See the module doc comment's own "implicit IUSE" bullet for the full scope writeup, including the deliberate simplification of not also modeling `USE_EXPAND_HIDDEN`-derived regex flags (`elibc_.*` etc.) -- a bigger, separate feature portuale doesn't otherwise model (no ELIBC/KERNEL/USERLAND support at all).
- field `use_mask_force_levels`: The same eight `use.*mask`/`use.*force` sources as the flat fields above, but kept **per profile level in chain order** (with the repo-level `package.use.*` entries as a synthetic leading level), so `portage-repo` can reproduce Portage `UseManager.getUseMask`/ `getUseForce`'s per-level *interleaved* `stack_lists(incremental=1)`: a global `use.mask` `-flag` line at a later profile level cancels an earlier level's `package.use.mask` entry (and vice-versa), a cross-source cancellation the flat-union fields above structurally cannot express. The flat fields stay for the readers that only need the global-only / approximate result (`emerge --info`'s global `USE=`, the REQUIRED_USE valid-IUSE domain, `--newuse`'s forced-flags filter). See `portage-repo`'s `resolved_use_mask`.
- field `license_groups`: `license_groups` (PMS-adjacent; Portage `LicenseManager. _read_license_groups`): every profile level's own file plus the user-level `/etc/portage/license_groups`, each `<name> <license or @group ...>` line's own value list *extended* (not stacked/ masked -- Portage code's own `setdefault(k, []).extend(v)`) onto whatever the same group name already has from an earlier source. Raw/unexpanded: a group's own value list may itself reference another `@group` recursively, resolved by `expand_license_token` (below) wherever a group is actually expanded -- once, here in `resolve_config`, for both `accept_license` and `package_license` (matching Portage's own `set_accept_license_str`/ `_read_user_config`, which both call `expandLicenseTokens` eagerly at config-read time too, not per-candidate), not pre-flattened into this map itself, matching Portage `_expandLicenseToken`'s own recursive-with-cycle-guard shape exactly.
- field `accept_license`: `ACCEPT_LICENSE`, already `@group`-expanded (see `license_groups`) but still symbolic -- `*`, `-*`, and `-license` tokens are kept literally, not resolved into a concrete license set here, since Portage's own `*`/`-*` resolution depends on a *specific candidate's own* LICENSE string (PMS 7.3.2), not anything known at config-resolution time (see `portage-repo`'s own per-candidate application). Deliberately last-level-wins (profile chain, then make.conf), not genuinely incremental across sources the way Portage's own `ACCEPT_LICENSE` is (`prune_incremental` over every source's own raw tokens) -- extends portuale's own pre-existing "any variable other than USE/ACCEPT_KEYWORDS is a plain last- level-wins scalar" cut (see the module doc comment) to this one too, rather than inventing a new, ACCEPT_LICENSE-specific incremental mechanism only this one variable would use. Portage's own default when `ACCEPT_LICENSE` is never set *at all* (not even as an explicit empty override) anywhere in the whole chain -- `"* -@EULA"` -- is replicated exactly (`config.py`'s own `accept_license_str = " ".join(mysplit) or "* -@EULA"`).
- field `package_properties`: (atom-or-wildcard string, raw tokens) pairs from `package.properties`, user-level only (Portage `LocationsManager.abs_user_config`, confirmed by reading `config.py`'s own `package.properties` read site -- no repo-level or profile-level source exists for this file at all).
- field `pkgdir`: `PKGDIR` (`--usepkg`/`--usepkgonly`'s own binary-package directory, Portage `lib/portage/dbapi/bintree.py`'s own `pkgdir`): an ordinary, non-incremental `make.conf` scalar, read the same "last-level-wins" way every other such variable already is (see the module doc comment). Portage default (`cnf/make.globals`, `PKGDIR="/var/cache/binpkgs"`) is replicated as a hardcoded fallback -- the same "Portage default, ported without modeling the file it technically comes from" precedent `accept_properties`'s own doc comment already established. `portage-repo`'s own `list_binary_candidates` reads `<pkgdir>/Packages` when `--usepkg`/`--usepkgonly` is given.
- field `binrepos`: Binary-package repositories (`--getbinpkg`/`--getbinpkgonly`), Portage `lib/portage/binrepo/config.py`'s own `BinRepoConfigLoader`: every `[section]` in `<config_root>/etc/portage/binrepos.conf` (`sync-uri = ...`, optional `priority = ...`), plus one implicit entry per whitespace-separated `PORTAGE_BINHOST` URI (Portage "Convert PORTAGE_BINHOST entries into implicit binrepos.conf ones", `name_fallback = md5(uri)`, incrementing priority). Sorted by `(priority or 0, name)`. Empty when neither is configured. `portage-repo`'s `list_remote_binary_candidates` reads each one's on-disk `Packages` index (see `BinRepo::packages_dir`).
- field `scanned_binpkgs`: The `$PKGDIR` directory-scan fallback (Portage `bintree._populate_local`): when `<pkgdir>/Packages` is *absent* but `$PKGDIR` holds binpkg *files*, the CLI layer walks them, reads each file's own embedded metadata (`portuale`'s `binpkg::scan_pkgdir` -- Portage `xpak`/`gpkg`), and synthesizes one `Packages`-style entry per file here. `None` means "no scan was done" -- either `Packages` exists (use it) or `--usepkg`/ `--usepkgonly` wasn't given (binary candidates aren't eligible anyway). `portage-repo` builds a `BinaryIndex` from this when set, or reads `<pkgdir>/Packages` when not. Not written back to disk -- Portage caches it as `Packages`; portuale recomputes each run, so `--pretend` still writes nothing.
- field `incremental_sources`: Per-source token lists for the `const.INCREMENTALS` variables Portage stacks in `config.regenerate()` and stores as `" ".join(sorted(myflags))`, narrowed to the four `emerge --info` displays: `CONFIG_PROTECT`, `CONFIG_PROTECT_MASK`, `ENV_UNSET`, `FEATURES`. One `Vec<String>` per assignment seen, appended in Portage's own db-stack order (`env.d` / `/etc/profile.env`, then `make.globals`, then the profile chain, then `make.conf`, then the process env). [`Config::resolved_incremental`] folds them with `-*` / `-tok` semantics and sorts. `ACCEPT_KEYWORDS` (also an incremental) keeps its own dedicated `accept_keywords` set.
- `resolved_incremental`: Portage `config.regenerate()`'s incremental fold for one `const.INCREMENTALS` variable: stack every source's tokens in db order, `-*` clears everything so far, `-tok` discards, a bare (or `+`-prefixed) token adds, then `sorted(myflags)` (Portage `self.configlist[-1][mykey] = " ".join(sorted(myflags))`). Returns `None` only when the variable was never assigned anywhere.
- `env_d_changed`: Whether `<eroot>/etc/profile.env` changed since this config was resolved (or last reloaded): the live file's text differs from the recorded [`Config::envd_raw`]. A missing file reads as `""`, so a deleted file counts as changed unless nothing was ever recorded. The per-package-task gate (`entry_build_env`): when this is false the caller keeps the old code path byte for byte.
- `reload_env_d`: Portage `config.reload()` (`config.py:2691-2699`): re-read `env.d` and replace that layer, leaving every higher layer alone. For every re-read key a higher layer owns ([`Config::envd_higher_keys`]) or the blacklist drops, `other_vars` is untouched; every other key is set, changed, or removed to match the new file. The env.d incremental layer per tracked key is replaced the same way, and `envd_sets_path`/`envd_use_tokens` are recomputed. Higher-layer expansions are deliberately not redone (they were computed against the old env.d at resolve time -- Portage's `expand=False` env.d read likewise never re-expands them). The USE-context fingerprint is refreshed so the `portage-repo` memos keyed on it see the new `envd_use_tokens`.
- field `envd_sets_path`: Portage `doebuild.py:365-368`, `_doebuild_path`.
- field `envd_raw`: Portage `config.py:2691-2699`, `reload`.
- field `envd_vars`: Portage `config.py:724-743`.
- field `envd_profile_only`: Portage `config.py:734-743` (the env.d blacklist is computed once at init and reused by `reload()`).

## field package_keywords_profile (#346 S2)
- (atom-or-wildcard, keyword tokens) from profile-node `package.keywords` (chain order, then the user node). Folded incrementally onto each candidate's own KEYWORDS by `portage-repo::effective_keywords`, never into the accept dict. Hashed into both config fingerprints (visibility and USE-context: `is_stable` reads it through effective USE).
