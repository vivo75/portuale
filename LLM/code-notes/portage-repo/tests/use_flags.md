# crates/portage-repo/src/tests/use_flags.rs
Effective-USE layer-precedence, stable-flag and memo tests.

## fn use_expand_star_wildcard_strips_the_pseudo_flag_from_the_effective_set
- Exercised directly with the fixture's own `linguas_*` package.use entry against wildexpandpkg's declared IUSE.

## fn effective_use_flags_lets_use_tokens_explicitly_cancel_a_plus_iuse_default
- The gap portuale's own IUSE-defaults slice originally left open, now closed: Portage's `regenerate()` runs one continuous incremental walk (pkginternal -> defaults -> conf -> pkg). `use_tokens` here is two separate raw USE= values ("foo" then "-foo bar"), exactly what `resolve_config` would produce from two profile levels -- replayed via `apply_incremental`, not unioned as a pre-flattened set, so the "-foo" genuinely reaches back and cancels IUSE's own "+foo".

## fn effective_use_flags_applies_a_use_prefix_wildcard
- Backlog #67 S2: conf-tier USE_EXPAND folding shares Portage's USE loop with the IUSE defaults; the same level's plain `video_cards_intel` token still lands.

## fn effective_use_flags_defaults_walk_is_per_profile_level
- Two chain levels: the parent's package.use enables `foo` for this package; the child's make.defaults disables it. Portage walks make.defaults-then-package.use one profile at a time, so the child's `-foo` (applied after the parent's package.use) wins.

## fn effective_use_flags_repo_package_use_is_weaker_than_an_iuse_default
- The divergence the old flat model had: it applied every `package.use` source last, strongest.

## fn effective_use_flags_profile_package_use_loses_to_make_conf
- Profile `package.use` is Portage's `configdict["defaults"]`, applied before `conf`. The old flat model applied profile `package.use` after make.conf, so `foo` won.

## fn effective_use_flags_package_env_use_is_pkg_layer_and_loses_to_user_package_use
- Package.env's own USE= is applied just before user `/etc/portage/package.use` (Portage `_grab_pkg_env` fills `pkg_configdict`, then `self.puse` is appended after).

## fn effective_use_flags_env_use_is_the_highest_tier_and_beats_user_package_use
- Portage's `configdict["env"]` is above `pkg`: `USE="-foo"` in the process env overrides a user `/etc/portage/package.use foo`.

## fn effective_use_flags_envd_is_the_lowest_tier
- Portage's `configdict["env.d"]` (from `/etc/profile.env`) is the bottom of `USE_ORDER`.

## fn resolved_use_mask_or_force_memo_shares_the_allocation
- Backlog #104 S3.
- Backlog #122: the same candidate string with a different package is a different entry with its own value.

## fn effective_use_flags_package_use_mask_wins_over_force_on_conflict
- See `effective_use_flags`'s own doc comment for the force-then-mask order.

## fn effective_use_flags_global_use_force_wins_over_a_package_use_disable
- Portage's `regenerate()` applies `self.useforce` (global use.force combined with per-package `getUseForce(pkg)`) as the last step of its incremental USE walk, strictly after the "pkg" tier. History: an earlier portuale folded `use_force` into `base` too early, letting package.use incorrectly win.

## fn effective_use_flags_global_use_mask_wins_over_a_package_use_enable
- Global use.mask is folded into Portage's `self.usemask`, applied via `difference_update` as the last operation of all.

## fn effective_use_flags_applies_stable_force_and_mask_only_when_stable
- `is_stable`'s "would masking every keyword make this invisible" check: converting "amd64" to "~amd64" falls outside accept_keywords={"amd64"}, so true.

## fn effective_use_flags_skips_stable_force_and_mask_when_not_stable
- Replacing "~amd64" with its own already-"~"-prefixed form changes nothing, so the candidate stays visible either way and `is_stable` is false.

## fn specificity_ordered_flags_processes_entries_in_source_order
- This is Portage's `stack_lists(incremental=True)` over `[src0, src1]`, and the `targets/*` un-mask pattern (a broad child `cat/pkg -flag` overriding a narrow parent `>=cat/pkg-N flag`).

## fn resolved_use_mask_interleaves_per_level_across_sources
- The flat union (global stack {} union per-package {xmc}) would keep it masked; the interleave must not.

## fn effective_use_flags_memo_does_not_leak_between_differing_configs
- Same candidate / iuse / keywords, so only the config content can produce a different answer.

## fn use_context_fingerprint_uses_the_frozen_base_and_separates_autounmask_use
- Backlog #105: the immutable part is frozen by `resolve_config` into `use_context_base`, so the production path only hashes that `u64` plus the live `autounmask_use` -- the one field the backtracking loop mutates (each tier / `flag_is_settable` probe adds an entry), hashed in full so a single added entry always changes the key. A hand-built config (tests, `Config::default()`) has no base and falls back to the full content hash, `portage_profile::use_context_base_fingerprint`.
- No base (hand-built config): full content hash, content-sensitive.

## fn use_context_fingerprint_debug_asserts_on_post_freeze_mutation
- Backlog #121 (audit O6): mutating a USE-context field after `resolve_config` froze its digest trips the frozen-base `debug_assert_eq!`.
