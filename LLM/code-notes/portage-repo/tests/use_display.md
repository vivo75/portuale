# crates/portage-repo/src/tests/use_display.rs
`build_use_expand_display` grouping, diff-marker and force-wrap tests.

## fn alnum_sort_key_orders_digit_runs_numerically
- Portage: `output_helpers.py`, `_alnum_sort_key` (`re.split(r"(\d+)", "1a")` -> `["", "1", "a"]`).

## fn build_use_expand_display_diffs_against_the_installed_use_iuse
- Legend for the first block: alpha unchanged -> `alpha` (plain), beta newly enabled -> `beta*`, gamma brand-new IUSE and enabled -> `gamma%*`, delta brand-new IUSE and disabled -> `-delta%`, video_cards_nvidia now off (was on) -> `-nvidia*`. Enabled (alpha, beta, gamma) render before the disabled one (delta).
- The unchanged-Upgrade case mirrors Portage's `_create_use_string` returning `USE="alpha"`, non-empty.
- The dropped-from-IUSE case mirrors Portage's `removed_iuse`.

## fn build_use_expand_display_force_shows_reinstall_trigger_flags_at_plain_p
- Setup: installed with IUSE `gone keep`, `gone` enabled; the current ebuild dropped `gone` from IUSE -> trigger set `{gone}`.
- Plain `-p`: `keep` unchanged-disabled -> omitted, `gone` removed -> omitted.

## fn build_use_expand_display_wraps_forced_and_masked_flags
- Setup: `forcedon` force-enabled, `maskedoff` mask-disabled, `video_cards_nvidia` force-enabled.
- Portage: `if flag not in iuse_forced: += "%"`.
