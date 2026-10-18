# S3 (346-BID) execution report — 2026-10-17/18

Branch `backlog/346-rest` (both repos). No commit, no push (per hard rules).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/b/`
(b01..b06, `scripts/b.sh` recipe: two-instance `Packages`,
`emerge -pv --package-moves=n --usepkgonly=y [--binpkg-respect-use=y]
dev-libs/t341-dep`). Say "Portage", never "real".

## Status: DONE with one open question (b03 stderr wording, §4)

All six B cells were re-run against the post-fix release binary in a
`/var/tmp` scratch replicating `scripts/b.sh` (repos.conf with all fixture
repos, scratch removed afterwards). 5 of 6 match Portage exactly (rc,
selection, `~` marker, USE). The 6th (b03) matches on rc (now 1, was 0)
but not on the stderr wording — pre-existing general gap, question below.
Per the STOP rule the divergent text was NOT pinned.

## Files changed (portuale repo, tracked only)

Mechanism (build-id threads into matching; memo keys carry it):
- `crates/portage-repo/src/use_flags.rs` — `EffectiveUseFlagsArgs` /
  `EffectiveUseFlagsUncachedArgs` gain `build_id`; EUF memo key gains it;
  every `package.use` tier matches via
  `config_entries_matching_with_build_id`; `specificity_ordered_flags`
  gains `build_id`; `ResolvedUseMaskOrForceArgs(+Uncached)` gain it and the
  `RUMF_CACHE` key carries it; `ForcedOrMaskedFlagsArgs` and
  `forced_or_masked_flags_unfiltered` gain it. Two
  `#[allow(clippy::too_many_arguments)]` (crate precedent: 9 existing).
- `crates/portage-repo/src/visibility.rs` — `KeywordsAcceptedArgs` /
  `IsStableArgs` gain `build_id`; 5 `keywords_accepted` call sites pass
  `candidate.build_id`.
- `crates/portage-repo/src/provenance.rs` — `KeywordProvenanceArgs` gains
  `build_id` (`keyword_provenance`, `keyword_mask_marker` pass it through).
- `crates/portage-repo/src/binpkg_use.rs` — no-ebuild arm computes the
  profile selection and forced/masked set with `candidate.build_id`.
- `crates/portage-repo/src/reinstall.rs` — `candidate_iuse_and_use` passes
  `candidate.build_id` through (ebuild arm; None in practice).
- Thread-through at every other `EffectiveUseFlagsArgs` /
  `ForcedOrMaskedFlagsArgs` / `ResolvedUseMaskOrForceArgs` /
  `KeywordsAcceptedArgs` / `IsStableArgs` /
  `specificity_ordered_flags` call site (resolve/*, solver_bridge,
  slot_operator/abi_probe, installed, fetch_restrict, use_display,
  info_pkgs, ebuild_merge/collisions): `*.build_id.as_deref()` where a
  `Candidate` is in scope, else `None` (ebuild-equivalent, no behaviour
  change). `package.license` matching deliberately untouched (stale per
  plan: Portage drops build-id lines there).
- `crates/portage-repo/src/tests/{mod,use_flags}.rs` — `euf_flat` shim
  gains/passes `build_id`; ebuild-shaped pins use `None`; positional
  `specificity_ordered_flags` pins pass `None`.
- Docs (append-only): `LLM/what-this-proves.md` (+1 paragraph);
  `LLM/code-notes/portage-repo/{config_match,use_flags,visibility,
  provenance,binpkg_use}.md` (+9 bullets; corrected the S9 bullet that
  said every non-mask list keeps `None`). No new notes files, so
  `code_notes_index.py` has nothing to index (ran, no output).

pmtest repo (tracked): `pytests-contract-suite/test_emerge_pretend_contract.py`
only — 1 CASES row + 1 helper + 5 pinned tests (below). The untracked
`fixtures/*pg359*` files were not touched.

## Tests added

Rust (failing-first: both failed pre-fix, pass post-fix):
- `tests::visibility::user_accept_keywords_build_id_line_accepts_only_the_equal_instance`
  (b01/b02 mechanism: `=…-1-2 ~amd64` accepts only build 2).
- `tests::binpkg_use::user_package_use_build_id_line_applies_only_to_the_equal_instance`
  (b03 mechanism: `=…-1-2 x` with bake `{x}/{}` rejects both builds under
  `--binpkg-respect-use`; also guards the EUF memo-key fix — both builds
  share one `candidate_str`).

pmtest (`test_emerge_pretend_contract.py`, `--pretend` short-line
convention, each docstring cites its capture file + full Portage argv):
- `_t341_bid_cell_configroot` helper (mirrors S0 `scripts/b.sh` `B`/`B_USE`).
- CASES row `#346 S3: build-id user-config cell earns Portage's rc 1`
  (shared-root plain miss, S9-entry precedent).
- `test_s3_bid_accept_keywords_bid2_accepts_only_build_2` (b01).
- `test_s3_bid_accept_keywords_bid1_accepts_only_build_1` (b02).
- `test_s3_bid_package_use_bid1_leaves_the_newer_build_mergeable` (b04).
- `test_s3_bid_range_atom_ignores_the_build_id` (b05).
- `test_s3_bid_package_use_without_respect_use_leaves_display_use_alone` (b06).
- No pin for b03 (see §4).

No existing pin moved. No fixture added (per-test configroots only).

## Gates (in order, never concurrently)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: 0 warnings.
- `cargo build --release`: ok.
- `cargo test --release` (workspace): 2783 passed, 0 failed.
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt346`: 3997 passed, 37 skipped, 2 xfailed,
  0 failed (baseline in plan: 3758 passed on older `main`; failures
  compared by name: none). Scratch removed with sudo (merge tests leave
  root-owned files).
- Corpus drift: none. No `=`+build-id line exists in any shared-fixture
  `package.use`/`package.accept_keywords`/profile file, so every suite
  binary match ran with `build_id=None` (identical to pre-fix behaviour);
  plus a `PORTUALE_CORPUS_STRICT=1` pass over 198 tests matching
  `bid or binpkg or package_use or accept_keywords or use_mask or
  use_force or usemask or useforce or respect`: 198 passed. Nothing to
  bless; nothing blessed.
- Container bed: not run (per instructions).

## §4 OPEN QUESTION — b03 stderr wording (STOP-rule item)

S0 `b03-use-bid2-respect` (user `package.use` `=t341-dep-1-2 x`, build 1
baked `USE: x`, build 2 baked empty, `--binpkg-respect-use=y`):
Portage rc 1 with `!!! All ebuilds that could satisfy "dev-libs/t341-dep"
… have been masked.` + `- dev-libs/t341-dep-1::t341 (masked by: use flag
configuration mismatch)`; post-fix portuale rc 1 with `emerge: there are
no binary packages to satisfy "dev-libs/t341-dep".`

The S3 mechanism works (both builds correctly rejected; pre-fix it merged
build 2, rc 0). The wording gap is a pre-existing GENERAL
respect-use reporting divergence, not build-id-specific: the same setup
with a build-id-free user line (`dev-libs/t341-dep x`) produces the same
miss text on current `main` (verified live). Root cause:
`binary_candidates.retain(binpkg_respect_use_ok)` in
`resolve/pretend.rs` drops mismatched binaries before visibility/masked
reporting, so the top-level all-masked path (`all_binary_masked_report`
← `masked_rows_for_pool`, which bails when any pool member `is_visible`)
never sees them; Portage models the mismatch as a mask reason. Matching
the text means threading respect-use (flags + ebuild lookup) into
`candidate_masking_reasons`/the top-level abort report — a reporting-layer
change outside S3's file list, affecting all use-mismatched binaries.

Options for the owner: (a) accept S3 with the rc fix only (b03 covered at
Rust level, no pmtest pin for its stderr); (b) extend S3 to the reporting
path; (c) file it as a new backlog item. Recommended: (a) or (c) — (b)
risks churn in existing use-mismatch pins for a message, and the
`masked by: use flag configuration mismatch` row shape (cpv-deduped
single row for both builds) deserves its own cell design.
