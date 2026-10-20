# crates/portage-repo/src/tests/installed.rs
Installed-db (vdb) read, snapshot, memo, and candidate-cache tests.

## fn all_installed_packages_cache_hit_shares_the_allocation
- Backlog #454: two consecutive scans of an unchanged vdb return `Arc::ptr_eq` pointers -- the cache hit clones the pointer, never the entries. Verified load-bearing by temporarily cloning into a fresh `Arc` and watching the test fail.

## fn all_installed_packages_skips_a_stale_merging_entry
- Backlog #183: a stale `-MERGING-<pf>` entry left by a killed merge is invisible to installed-package enumeration -- Portage's `vardbapi._excluded_dirs` (`vartree.py`), which `cp_list` consults before `_pkg_str` ever sees the name (without the skip, `split_pf` mis-splits `-MERGING-realpkg-2.0` into a bogus `-MERGING-realpkg`/`2.0` row).
- Ground truth: the `l32` C4 control cell (`findings/l5.md` Group 4) leaves exactly `-MERGING-slow-a-1.0` after the SIGKILL, and the resumed merge treats `slow-a` as not installed.

## fn read_vdb_string_collapses_multiline_values_to_one_space_separated_line
- Backlog #109 S1: Portage's `_aux_get` normalises every field that is not `CONTENTS`/`NEEDED.*` with `" ".join(myd.split())` (`vartree.py:1044-1046`), on the individual-file path too.

## fn read_vdb_string_drops_the_trailing_newline
- Backlog #109 S1: the trailing newline Portage's writer leaves in every per-field file must not survive the read.

## fn read_vdb_string_missing_file_is_still_empty
- Backlog #109 S1: an absent field file is still `""`, not a panic and not a normalised-away error.

## fn vdb_aux_get_reads_an_out_of_set_key_raw
- Backlog #125 (audit O11): a key outside Portage's 23-field set is served raw, never whitespace-collapsed -- a multi-line `CONTENTS` comes back line-for-line, even though a validated snapshot exists for the entry. (Before, the `debug_assert!`-only guard let it fall into the snapshot path and corrupt it in release.)

## fn read_vdb_string_decodes_invalid_utf8_lossy_like_portage
- Backlog #125 (audit O15): Portage decodes field files with `errors="replace"` -- a stray non-UTF-8 byte becomes `U+FFFD`, not a wholesale `""`.

## fn read_vdb_string_normalises_the_committed_multiline_fixture
- Backlog #109 S1: the committed `dev-libs/multilinevdb-1.0` fixture (pmtest) is the integration half of the same contract -- its `IUSE`, `DEFINED_PHASES` and `RDEPEND` files all carry embedded newlines. Keeps the fixture load-bearing for an isolating test, not only for the whitespace-insensitive `--info` pin.

## fn vdb_aux_get_serves_a_valid_snapshot_without_opening_the_field_file
- Backlog #109 S3 (a): proved by writing a different value into that file and getting the snapshot's.

## fn vdb_aux_get_serves_empty_for_a_field_absent_from_a_valid_snapshot
- Backlog #109 S3 (b): Portage's own rule (`vartree.py:1010-1017`: no `open()` that would fail).

## fn vdb_aux_get_rejects_a_wrong_format_version
- Backlog #109 S3 (c): Portage's reader abandons the file on the version line before parsing the rest.

## fn vdb_aux_get_falls_back_on_a_missing_stale_or_garbage_stamp
- Backlog #109 S3 (d,e,f): `#dir_mtime=` is Portage's freshness signal and the reason an interrupted write cannot be read as a short snapshot.

## fn vdb_aux_get_falls_back_without_a_metadata_file
- Backlog #109 S3 (g): no `metadata` file at all is the state of every existing fixture and of a pre-consolidation vdb entry.

## fn vdb_entry_slot_matches_vdb_aux_get_on_a_fixture_entry
- Backlog #116: the `pf`-addressed wrapper around `vdb_aux_get("SLOT")` used by the last two direct `SLOT` scans (`pretend.rs::installed_cp_versions` and `ebuild_merge.rs::blockers_from_flat_deps`). It returns the normalised `slot/sub_slot` exactly as `_aux_get` does -- no splitting, no defaulting; callers keep their own post-processing verbatim.

## fn vdb_entry_slot_collapses_a_multiline_slot_to_one_space_separated_line
- Backlog #116: routing the two scans through the seam is what makes a multi-line `SLOT` collapse (`" ".join(myd.split())`, Portage's `_aux_get` on the individual-file path); the bare `.trim()` the scans used before kept the embedded newline. No live vdb entry has one (host probe: 0 of 2090), so this is pinned explicitly here.
- Backlog #115 S1: the collapsed value (`"0/1.2 0/1.3"`) is itself invalid, so the seam translates it on to `"0"` -- the collapse is still what feeds the translation (without it the raw newline would fail differently), and the pre-S1 expectation is kept in git history.

## fn vdb_entry_slot_returns_empty_for_an_unsplittable_pf
- Backlog #116: the same value the seam serves for an absent `SLOT`, so a caller's existing `""` defaulting is untouched.

## fn vdb_entry_slot_translates_a_present_but_invalid_slot_to_0
- Backlog #115 S1: matching Portage `aux_get`'s `_get_slot_re` translation (`vartree.py:967-972`) -- the S0.2 probe value, which the seam passes through raw before this slice.

## fn vdb_entry_slot_keeps_a_valid_sub_slot
- Backlog #115 S1: every live EAPI has `slot_operator` (`eapi.py:319`), so this is the shape Portage itself accepts for the entry.

## fn vdb_entry_slot_translates_a_collapsed_multiline_slot_to_0
- Backlog #115 S1: the space the #109 normalisation introduces fails `_get_slot_re` -- this pins the interaction of the two slices.

## fn vdb_aux_get_snapshot_parse_ignores_comments_and_lets_the_last_key_win
- Backlog #109 S3: the snapshot parse is Portage's `_read_metadata_file` -- `split("=", 1)` with the last duplicate winning.

## fn vdb_aux_memo_serves_the_cached_snapshot_until_the_dir_mtime_moves
- Backlog #109 S4 (a): proved behaviourally -- an in-place rewrite of the `metadata` file does not move the dir mtime, so the memo keeps serving the validated map (Portage's own in-process hole, documented at the cache), while a dir mtime change invalidates it and a fresh snapshot is read.
- A fresh valid snapshot is picked up on the next lookup after the dir mtime moves again, the way the merge path replaces a whole entry (an in-place rewrite with the mtime held is exactly the documented stale hole above).

## fn vdb_aux_memo_caches_the_fallback_path_per_key
- Backlog #109 S4 (b,c): a second key on the same instance resolves without re-attempting the snapshot, and a resolved key is not re-read while the dir mtime holds.

## fn installed_candidates_caches_per_cp_and_invalidates_on_vdb_change
- Backlog #102 S1: the category dir mtime moves on every add/remove/replace of a package dir.

## fn candidate_use_flags_display_resolves_use_expand_flags_for_a_resume_entry
- `emerge --resume` rebuilds each `GraphEntry` from a bare `cat/pkg-ver`; this is how it re-derives `use_flags_display` so a source build gets a Portage `USE=`. `dev-libs/useexpandpkg`'s own `IUSE="video_cards_nvidia video_cards_amdgpu"` + the fixture's `VIDEO_CARDS="nvidia"` -> `video_cards_nvidia` on.

## fn candidate_effective_use_flags_treats_a_missing_iuse_key_as_empty_iuse
- Portage's md5-cache writer omits empty fields (`porttest/emptydirs` ships no `IUSE=` line at all), but a package with an empty IUSE still has the full effective USE -- the global profile flags. Bailing on a missing key would leave `USE=""` on exactly those packages (#37 S2, `TEST/findings/l2.md` "S0 recon").
- `aubreaktop`'s cache has no `IUSE=` line.

## fn running_root_satisfies_atom_checks_the_given_roots_own_real_vdb
- rootdepsprovider-1.0 exists only as a hand-seeded vdb entry under fixtures (no ebuild anywhere in the fixture repo tree at all) -- see this crate's own doc comment on `running_root_satisfies_atom` for why this is deliberately generic on which root it's pointed at.
