# crates/portuale/src/pretend/tests/unmerge.rs
Unmerge info-regen wiring, the end notice, and set-parent detection.

## fn execute_unmerge_forwards_quiet_and_noinfo_to_the_info_regen
- #176 review: `run_unmerge_pretend` -> `execute_unmerge` carry Portage's `--quiet` (`noiselimit < 0`) and the resolved-config `noinfo` (Portage's `"noinfo" not in settings.features`, `post_emerge.py:127`) into `post_merge_info_update`. Hermetic: an empty removal list touches no vdb package, and a stale `dir`-only info dir needs no `install-info` run (every name is skippable) yet still records the memo -- so neither case needs the host binary.
- `quiet=true` prints nothing here (a `dir`-only regen counts zero files), while `noinfo=false` lets the regen record the memo -- proving the params arrive, not just exist. (`ExitCode` exposes neither `PartialEq` nor its code, so success is asserted via its `Debug` rendering.)
- No repos and a default (news-less) config: the post-removal GLEP 42 notice (backlog #196 fix round 1) is gated off, so these info-regen assertions observe no notice output.
- `noinfo=true` (resolved-config `FEATURES=noinfo`) suppresses the whole block: the memo stays empty.

## fn unmerge_end_notice_skips_an_empty_removal_list
- Backlog #196 fix round 1: Portage's `post_emerge` after the uninstall actions prints the notice only when the vdb changed -- an empty selection removes nothing, so no notice (and no evaluation: no state dir appears). `noinfo=true` skips the info regen (nothing to regenerate under a scratch ROOT); the removal list is empty, so nothing else runs either -- the positive path (a real removal prints) is pinned by the contract suite instead.

## fn still_listed_parents_applies_the_higher_slot_refinement
- dev-libs/dualslotpkg is installed in slot 1 (1.0) and slot 2 (2.0); `dualslotset` lists the bare `dev-libs/dualslotpkg`.
- Removing the slot-1 version: slot 2 (higher, different slot) still matches the bare set atom -> not a parent. Removing the slot-2 version: nothing higher -> the set is a parent.

## fn still_listed_parents_is_empty_when_no_set_lists_the_cp
- No comment carried facts.
