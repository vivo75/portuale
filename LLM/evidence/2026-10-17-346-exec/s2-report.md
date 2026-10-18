# S2 (346-KW) execution report — 2026-10-18

Branch `backlog/346-rest` (both repos). No commit, no push (per hard rules).
Oracle: Portage 3.0.82.2 captures in `LLM/evidence/2026-10-15-346-s0/k/`
(k01..k05, `scripts/k.sh` recipe: `emerge -p --package-moves=n
dev-libs/bareacceptkeywordspkg`, `KEYWORDS=~amd64`). Say "Portage", never
"real". Plan default taken: Q3 message-only deprecation warning.

## Status: DONE, no open questions

All five K cells were re-run live against the post-fix release binary in a
`/var/tmp` scratch replicating `scripts/k.sh` (repos.conf with all fixture
repos + `repo-341`, scratch removed afterwards). All match Portage on rc,
merge row / mask reason, `~` marker, and warning presence. No pin moved, no
corpus drift, nothing blessed. Container bed not run (per instructions).

## Files changed (portuale repo, tracked only)

Mechanism (user `package.keywords` feeds the accept dict first; profile
nodes extend each candidate's own KEYWORDS):
- `crates/portage-profile/src/resolve/package_masks.rs` —
  `fold_package_masks_and_keywords` reads chain + user-node
  `package.keywords` into new `Config::package_keywords_profile`
  (chain: node rules incl. build-ids per formats; user node: node EAPI, no
  wildcards/build-ids), and reads user `package.keywords` first into the
  accept-dict lines (user rules: wildcards/build-ids on, use-deps dropped)
  with the message-only deprecation warning on a non-empty valid read.
- `crates/portage-profile/src/atom_lines.rs` — new
  `UserNodeFile::PackageKeywords` (same rules as `AcceptKeywords`);
  doc comment updated.
- `crates/portage-profile/src/config.rs` — new
  `package_keywords_profile` field.
- `crates/portage-profile/src/fingerprints.rs` — the field joins both the
  USE-context and the visibility digests (stability reads it through
  effective USE).
- `crates/portage-profile/src/lib.rs` — scope-cut comment updated.
- `crates/portage-repo/src/visibility.rs` — new `effective_keywords`
  helper (own KEYWORDS minus `-*`, then matching profile entries folded in
  order; build-less match, the #341 S2 rule, as Portage builds a build-less
  `_pkg_str` in `getKeywords`); all 5 `keywords_accepted` sites pass it.
- `crates/portage-repo/src/{masked_deps,provenance}.rs` — mask reason and
  `~`/`#` marker plus keyword provenance read the effective KEYWORDS.
- `crates/portage-repo/src/{use_flags,dep_chain,use_unsat,
  visibility_use_flip}.rs`, `resolve/pass/graph_entry.rs` — every direct
  `is_stable`/`keywords_accepted` input is the effective list (all
  `EffectiveUseFlagsArgs`/`ForcedOrMaskedFlagsArgs` callers inherit it
  through the two central folds). `suggested_keyword` stays raw (no cell
  covers that shape; noted in the code note).
- Tests: `portage-profile/tests/resolve.rs` (+2), `portage-repo/
  tests/visibility.rs` (+2).
- Docs (append-only): `LLM/what-this-proves.md` (+1 paragraph);
  `LLM/code-notes/portage-profile/{atom_lines,config,fingerprints,
  resolve/package_masks}.md`, `LLM/code-notes/portage-repo/{visibility,
  provenance,masked_deps,use_flags,dep_chain,use_unsat,visibility_use_flip,
  resolve/pass/graph_entry}.md` (atom_lines "Residues" rewritten: the
  `package.keywords` residue is now shipped). `code_notes_index.py` ran
  clean, twice.

pmtest repo (tracked): `pytests-contract-suite/
test_emerge_pretend_contract.py` only — 1 CASES row + 1 helper + 5 pinned
tests (below). The untracked `fixtures/*pg359*` files were not touched.

## Tests added

Rust (failing-first: pre-fix tree fails to compile the new tests —
`no field package_keywords_profile`, `effective_keywords` missing —
verified via stash; all pass post-fix):
- `portage-profile user_package_keywords_feeds_the_accept_dict_first_with_a_deprecation_warning`
  (k01/k05 mechanism: user keywords lines precede accept lines in the
  dict + the deprecation text).
- `portage-profile profile_package_keywords_stack_chain_then_user_node`
  (k02/k03 mechanism: chain-then-node order in the new field).
- `portage-repo profile_package_keywords_adds_a_stable_keyword_to_the_package`
  (k02/k03 mechanism: `~amd64` + profile `amd64` is visible, marker `None`).
- `portage-repo profile_package_keywords_removal_empties_the_package_keywords`
  (k04 mechanism: `-~amd64` stays masked with `missing keyword`).

pmtest (`test_emerge_pretend_contract.py`, `--pretend` convention — no
resolution preamble on stdout, per the #351 S7 Q6 default; each docstring
cites its capture file + full Portage argv):
- `_t346_user_file_root` helper (user-level `etc/portage/<name>` files).
- CASES row `#346 S2: revoked keyword without profile keywords earns
  Portage's rc 1` (`--pretend -v dev-libs/keywordrevokedpkg`, shared-root
  guard over the touched reason path).
- `test_s2_user_package_keywords_accepts_like_portage` (k01: rc 0,
  `[ebuild N ~]`, exact deprecation text on stderr).
- `test_s2_user_profile_package_keywords_adds_a_stable_keyword_like_portage`
  (k02: rc 0, `[ebuild N ]`, stderr empty).
- `test_s2_chain_package_keywords_adds_a_stable_keyword_like_portage`
  (k03: scratch chain, same expectations).
- `test_s2_profile_minus_empties_the_keywords_like_portage` (k04: rc 1,
  `missing keyword` block, stdout empty).
- `test_s2_cross_file_minus_cancels_like_portage` (k05: rc 1, `~amd64
  keyword` block + warning first on stderr).

No existing pin moved. No fixture added (per-test configroots only).

## Gates (in order, never concurrently)

- `cargo fmt --check`: clean (one self-made long line folded via `cargo fmt`).
- `cargo clippy --release --all-targets`: 0 warnings.
- `cargo build --release`: ok.
- `cargo test --release` (workspace): 31 suites, 2795 passed, 0 failed
  (S3 baseline 2783 + S4/S5/S6 landings + 4 new here).
- pmtest `python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt346b -p no:cacheprovider`: 4006 passed, 37 skipped,
  2 xfailed, 0 failed (failures compared by name: none). Scratch removed
  afterwards (sudo for root-owned merge leftovers).
- Corpus drift: none — no drift section in the full run, plus a
  `PORTUALE_CORPUS_STRICT=1` pass over 187 keyword/mask/stable/s1/s2/s3
  tests: 187 passed. Nothing to bless; nothing blessed.
- Container bed: not run (per instructions).

## Notes for the reviewer

- `--pretend` stdout on the masked cells (k04/k05) is empty where Portage
  prints the resolution preamble: portuale's long-standing `--pretend`
  convention, owned by #351 S7 (Q6 default), not re-litigated here. The
  pins document it in their docstrings.
- `effective_keywords` matches with no build-id (Portage `getKeywords`
  has no build context); an `=`+build-id profile line matches nothing,
  the #341 S2 rule. Deliberate; in the code note.
- `/var/tmp` filled to 100% mid-slice (two full-suite basetemps); both
  removed, back to 68%. No `/dev/shm` use, no rebuild-during-run, no
  `pgrep`/`pkill`.
