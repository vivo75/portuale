# crates/portage-repo/src/tests/conflict_use_display.rs
Skipped-update and slot-conflict USE display tests: vdb sourcing, expand groups.

## fn enforced_pin_miss_renders_the_installed_enforcing_consumer
- Backlog #205 R3, expected value from Portage: the enforced pin's rejection is recorded as a slot-conflict-style mask on the chosen version (Portage "Record missed updates", `lib/_emerge/depgraph.py:2085-2106`), so the existing `backtrack_missed_updates` derivation renders the `WARNING` row.
- The run settles `[abk0c-1, abk0a-2, abk0d-1]` (`backtrack: 2/2`, rc 1 in the contract pin); the rejected `abk0a-3` rides out as exactly one skipped-update row naming installed `abk0b-1`, rendered vdb-based instead of the merge-scheduled tree form a graph puller keeps.
- The fixture vdb entry carries no `repository` file, like Portage's `portage.versions._unknown_repo`; the empty-valued USE group renders bare `USE=""`, as pinned and as Portage's probe consumer line shows.
- Post-#230 the display is Portage's full `pkg_use_display` (`skipped_update_installed_use_display_for`), which always emits the USE group instead of the old empty vec.

## fn enforced_pin_miss_carries_the_vdb_use_and_repo
- Backlog #205 R3, vdb legs: `IUSE`/`USE`/`repository` staged on a scratch copy of the `abk0*` vdb entries. Repos and config stay on the fixtures (like the contract pin's test-local ROOT, whose `PORTAGE_CONFIGROOT` stays shared); only the vdb comes from the scratch root.

## fn skipped_use_display_masks_to_the_valid_domain_and_groups_expands
- Backlog #230, expected value from Portage: EAPI 8 selects the `IUSE_EFFECTIVE` domain (profile-global `foo` and forced-but-undeclared `globalforceflag` mask out; implicit `elibc_glibc` kept and grouped). Pre-EAPI-5 instead keeps-then-discards ARCH (`other_vars["ARCH"]` is the profile ARCH scalar), hides hidden-expand groups, and lets per-package force/mask keep an undeclared flag (wrapped).
- Pure-function level: masking plus assembly, no repos needed. The assembled shape matches Portage's `emerge --info dev-libs/pkginfopkg` output.
- The skipped-update block renders Portage's full `pkg_use_display`, not the IUSE-only approximation -- the effective USE masked to the valid-IUSE domain, one group per non-hidden `USE_EXPAND` var, `( )`-wrapped force/mask flags.

## fn skipped_use_display_orders_enabled_first_then_byte_sorted
- Backlog #230: Portage's default `sort_separated` (plain string order); groups USE-first then byte-sorted var names.

## fn skipped_update_use_display_for_renders_the_fixture_blk0_shape
- Backlog #230, fix round 1 (post-#220), expected value from Portage: the blk0 ebuilds are EAPI 8 now, so the `eapi_has_iuse_effective` gate selects the `IUSE_EFFECTIVE` domain and the profile globals mask out. Byte-identical to Portage's own parent lines in the fix-round-1 probe (`<dev-libs/blk0x-2 required by (dev-libs/blk0b-1:0/0::testrepo, ebuild scheduled for merge to '<root>') USE="" ELIBC="glibc"`).

## fn slot_conflict_use_display_renders_use_expand_groups
- Backlog #264, expected value from Portage: Portage's `slot_collision.py` header/parent lines go through the same `pkg_use_display` (`_emerge/UseFlagDisplay.py:55`) as the skipped-update block.
- Grounded in Portage `/usr/sbin/emerge` 3.0.82.2 `--pretend --ignore-default-opts --autounmask-backtrack=y dev-libs/aub0a dev-libs/aub0b dev-libs/aub0c` on the staged fixture: headers `USE="foo" ELIBC="glibc"` (aub0d-0) / `USE="-bar" ELIBC="glibc"` (aub0d-1, `bar` incrementally removed by `profiles/default/make.defaults`), parents `USE="" ELIBC="glibc"`.
- The slot-conflict `USE="…"` display carries Portage's USE_EXPAND groups: the slot-conflict producers must render the same groups as the skipped-update block.
