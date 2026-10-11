# crates/portage-repo/src/tests/binpkg_use.rs
`binpkg_respect_use_ok` tests: ebuild comparison, memo keys, force/mask handling.

## fn binary_index_from_entries_feeds_list_binary_candidates_and_read_binary_metadata
- The `$PKGDIR` directory-scan path: `BinaryIndex::from_entries` (what `config.scanned_binpkgs` becomes) is consumed exactly like a parsed `<pkgdir>/Packages`. A missing pkgdir is an empty index (unchanged tolerance).

## fn binpkg_respect_use_checks_the_ebuild_at_the_same_version
- #69 S1, expected value from Portage: Portage compares a binary's baked IUSE/USE against the same-version ebuild's profile view, not against the binary's own selection.
- Each call gets a fresh repo dir because `repo_aux_metadata` memoises by path and the cells rewrite the same cpv's IUSE.
- The ebuild-gained-newflag cell is the regression pin for the IUSE-set difference.
- No same-version ebuild: Portage `depgraph.py:8265-8271` (`pkgsettings.setcpv(pkg)`) compares against the profile's selection for the binary cpv over the binary's own IUSE.
- The profile-dropped-foo cell is the #69 R0 n1 shape.

## fn binpkg_respect_use_memo_hits_on_repeat_and_separates_its_inputs
- #117: the memo is transparent (a repeated call runs the body once) and must distinguish every key axis: the ebuild's own metadata, the binary's IUSE and baked USE, the three flags, and the config.
- A differing binary IUSE misses where the baked USE enables newflag while the ebuild's effective USE does not (only the counter proves the miss, which is exactly why it exists).
- Backlog #122 (audit O4): two `repo_location`s with different md5-cache `IUSE` behind identical candidate `iuse` must not share; the uncached body reads the ebuild's md5-cache entry through `candidate_iuse_and_use`. The old key hashed only the candidate's own `iuse` and would have served the first call's `true` for the second.
- Backlog #124 E3: the build identity separates instances; the same CPV at a different BUILD_ID is a miss, and a repeat hits.

## fn binpkg_respect_use_subtracts_forced_and_masked_flags_like_portage
- #69 R2, expected value from Portage `depgraph.py:3148-3150` (`flags -= forced_flags`).
- The masked cell keeps the binary (the R0 n3a shape, regression pin for the subtraction); the forced cell rejects through the enabled-within-IUSE term instead (R0 n3b, exactly Portage's n3b cell).
- The no-ebuild arm passes the binary cpv's own force/mask set (Portage's `pkgsettings.useforce ∪ usemask`), though its presence term is empty by construction there (`cur_iuse = old_iuse`), so the outcome is the same keep either way.

## fn binpkg_respect_use_forgives_a_profile_masked_flag_the_ebuild_iuse_dropped
- Audit D1 (`LLM/02.68-74-review-through-B5.md`), expected value from Portage: a flag the profile masks that the new ebuild's IUSE dropped is forgiven on the presence term. Probed live against 3.0.82.2 (`_reinstall_for_flags` returns `None`; `depgraph.py:8264` chains `myeb.use.force/mask` unfiltered, and `config.setcpv`'s `getUseMask(cpv_slot)` never filters by IUSE).
- The old filtered set (R2's `forced_or_masked_flags`) lost the flag, so the presence difference rejected the binary.
- Baked IUSE carries the dropped flag; the baked USE does not enable it (a masked flag cannot be baked enabled; the live probe's `orig_use` is `{"foo"}`).
- Control (pre-D1 shape): without the profile masking the flag the presence difference does reject; the profile masking, not the filter, carries the keep.
