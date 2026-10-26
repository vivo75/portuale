# Task 4 report: verify `RMD160` and `WHIRLPOOL` on binpkgs (#582)

Date: 2026-10-26. Branch `fix/2026-10-26-575-residues` in both repos (no commits made).

## S0 (premise check)

`t4/s0-checksum-origins.txt`:
`3.0.83 False bundled-c hashlib ['BLAKE2B', 'BLAKE2S', 'MD5', 'RMD160', 'SHA1', 'SHA256', 'SHA3_256', 'SHA3_512', 'SHA512', 'WHIRLPOOL', 'size']`
Matches the plan premise exactly (`_whirlpool_unaccelerated` False,
`WHIRLPOOL` origin `bundled-c`, `RMD160` origin `hashlib`), so no §P stop.
Expected digests for the test bytes saved in `t4/s0-test-bytes-digests.txt`
(taken with `portage.checksum.perform_checksum` on the same bytes; the tuple's
second element, the size, is dropped by the tests).

Portage citations re-verified in `3rdparty/portage`:
`lib/_emerge/BinpkgVerifier.py:29-46` (remote guard, size check, filter order),
`:122-127` (`_display_success` prints `sorted(self._digests)`),
`lib/portage/dbapi/bintree.py:2831` (`_get_digests`: every
`get_valid_checksum_keys()` key with a non-empty value, plus `size` from
`int(SIZE)`), `lib/portage/checksum.py:159-168` (bundled-C vs pure-Python
WHIRLPOOL) and `:294-310` (`_filter_unaccelarated_hashes` drops WHIRLPOOL
only when unaccelerated *and* another hash is present).

## Files changed (portuale only; no fixture/pmtest half needed)

- `crates/portuale/Cargo.toml`: added `ripemd = "0.2"` and
  `whirlpool = "0.11"` (`cargo search` at task time: `ripemd 0.2.0`,
  `whirlpool 0.11.0`). No direct `digest` dependency added.
- `crates/portuale/src/binpkg/digest.rs`:
  `VERIFIED_DIGESTS` 8 → 10 (added `RMD160`, `WHIRLPOOL` in ASCII order);
  new `pub(crate) fn selected_digest_names(record, checksum_filter)`;
  `verify_binpkg_against_index` and the removed-name-list path both use it;
  `RMD160`/`WHIRLPOOL` dispatch arms in `bytes_hex_digest`
  (`ripemd::Ripemd160`, `whirlpool::Whirlpool`, same `Digest` trait family
  as the existing arms); `VALID_CHECKSUM_KEYS` deleted and
  `index_has_size_and_checksum` reads `VERIFIED_DIGESTS` (identical set, so
  one digest-name list is left in the crate). No WHIRLPOOL drop ported.
- `crates/portuale/src/emerge_getbinpkg.rs`:
  `verified_digest_names_filtered` now calls the shared
  `selected_digest_names` (second list deleted). `download_and_verify`
  needed no edit: it already verifies through `verify_binpkg_against_index`
  (`:2113-2116`).
- `crates/portuale/src/emerge_build/fetchonly.rs`: test only (it already
  calls `verified_digest_names_filtered`, `:137-140`).
- Tests: `binpkg/tests/digest.rs` (4 new verifier tests + 2 new
  known-answer rows), `emerge_getbinpkg.rs` tests (1), `fetchonly.rs`
  tests (1).
- Docs: `LLM/code-notes/portuale/binpkg/digest.md` (rewrote the
  `RMD160`/`WHIRLPOOL` gap claims; `const VERIFIED_DIGESTS` carries the
  why-no-drop note citing `checksum.py:159-168` and `:294-310`);
  `python3 LLM/tools/code_notes_index.py` re-ran clean.

## What each test pins

All expected digests come from host Portage 3.0.83
`perform_checksum` on the same bytes (see S0 file above).

- `rmd160_only_record_is_verified`: SIZE-correct + bad `RMD160` fails with
  name `RMD160` (`bintree._get_digests`, `BinpkgVerifier._start`).
- `whirlpool_only_record_is_verified`: same for `WHIRLPOOL`
  (`checksum._filter_unaccelarated_hashes`; S0 rules the drop out).
- `whirlpool_checked_next_to_another_hash`: bad `WHIRLPOOL` + good `SHA512`
  fails on `WHIRLPOOL` (`checksum.py:159-168, 303-308`).
- `checksum_filter_can_select_rmd160`: `PORTAGE_CHECKSUM_FILTER="RMD160"`
  refuses a bad `RMD160` and excuses a bad `SHA512`
  (`checksum._apply_hash_filter`).
- `known_answer_digests_are_unchanged`: + `RMD160`/`WHIRLPOOL` rows for
  `b"portuale"` and empty (`checksum.perform_checksum`).
- `success_line_lists_rmd160_and_whirlpool`: record with SIZE/RMD160/
  WHIRLPOOL/SHA512 yields `RMD160 SHA512 WHIRLPOOL size`; filter `RMD160`
  yields `RMD160 size` (`BinpkgVerifier._display_success`, `:122-127`).
- `fetchonly_success_line_uses_the_same_names`: the `--fetchonly` path
  prints the same names (it calls the shared function at
  `fetchonly.rs:137-140`).

## Commands and results

- RED run (7 new/changed tests): all 7 failed as required
  (`expect_err` got `Ok`, `bytes_hex_digest` returned `None`, success line
  was `SHA512 size`).
- Post-fix targeted run: 7 passed.
- `cargo fmt --check`: clean (after one `cargo fmt`; `git diff --stat`
  shows only the 7 task files).
- `cargo clippy --release --all-targets`: 0 warnings.
- `cargo test --release -p portuale --bin portuale`: 1339 passed,
  0 failed, 3 ignored.
- `cargo tree -d | grep -w digest`: empty; `Cargo.lock` shows only
  `digest 0.11.3` — the tree still carries one `digest` version.
- Intermediate note: a filtered run showed 17 merge-test failures, all
  `run cargo build --release first` (stale `target/release/portuale`
  guard, as the Task 0 baseline documents). After rebuilding, the full
  run above is green.

## Open doubts

- The plan's `grep -rn '"SHA3_512"' crates/portuale/src` "one hit" check
  yields 2 product-code hits after the fix (`VERIFIED_DIGESTS` literal and
  the `bytes_hex_digest` dispatch arm), plus 2 test-data hits. The second
  *name list* is gone (that was the defect); the dispatch arm must name
  the key to hash it, so a single literal hit is unreachable without
  contorting the match. Treating the interface's "no other list of digest
  names" wording as satisfied.
- No OWNER QUESTION: S0 matched the premise, no new dependency beyond the
  owner-approved two, no behaviour invented beyond Portage source.
