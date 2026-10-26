# Task 3 report: every hasher moved to `digest 0.11`

## Files changed

Versions (four `Cargo.toml` files):

- `crates/portuale/Cargo.toml`: `md-5 0.10`→`0.11`, `sha1 0.10`→`0.11`,
  `sha2 0.10`→`0.11`, `blake2 0.10`→`0.11`, `sha3 0.10`→`0.12`
- `crates/portage-fetch/Cargo.toml`: `blake2`/`sha2`→`0.11`, `md-5`/`sha1`→`0.11`
- `crates/portage-repo/Cargo.toml`: `md-5`→`0.11`
- `crates/portage-profile/Cargo.toml`: `sha2`→`0.11`
- `Cargo.lock`: only the hasher family changes — `generic-array 0.14.7`
  and `subtle` out; `hybrid-array 0.4.15`, `ctutils` (+`cmov`),
  `const-oid`, `sponge-cursor` (via `sha3 0.12`) in. Side-effect dedup:
  the lone `cpufeatures 0.2.17` lock entry is gone (everything now uses
  the `0.3.0` already in the tree via brush); `core-foundation-sys`
  only moved position in the file.

API (no behaviour change; every digest value byte-identical):

- `crates/portage-util/src/lib.rs`: new `pub fn hex_lower(bytes: &[u8]) -> String`,
  the single shared lowercase-hex spelling (no shared helper existed; S0
  grep found only private per-file copies).
- `{:x}`-on-digest-output sites converted to `portage_util::hex_lower`
  (all other call sites — `.iter()` folds, `&[u8]`-taking closures,
  `.to_vec()` — compile unchanged under the new output type):
  `portage-profile/src/user_patches.rs` + its test,
  `portuale/src/{regen.rs, remote_bundle.rs, remote/client_binary.rs}`,
  `remote/tests/{mod.rs, preflight.rs}`,
  `emerge_build/tests/buildpkgonly.rs`,
  `portage-repo/src/{md5_dict.rs, solver_bridge.rs}` and 31 identical
  one-liners under `portage-repo/src/tests/` (one `sed` over that exact
  literal, verified by `git diff`).
- `elog.rs` (named in the plan's file list) has no digest use — the grep
  hit was the unrelated `mail_summary_finalize` function name. Untouched,
  as are `portage-fetch/src/{layout,lib}.rs`, `ebuild_merge/tree.rs`,
  `ebuild_package.rs`, `helpers/{doins,gpkg,xattr}.rs` and
  `binpkg/digest.rs` itself.

Tests:

- New `known_answer_digests_are_unchanged` in
  `crates/portuale/src/binpkg/tests/digest.rs`: for `b"portuale"` and the
  empty string, asserts `bytes_hex_digest` (the verifier's own function)
  for all eight `VERIFIED_DIGESTS` algorithms. Expected values from host
  Portage 3.0.83 via `python3 -c 'import portage.checksum as c;
  print(c.perform_checksum(<file>, algo))'` (cited in the docstring with
  `lib/portage/checksum.py, perform_checksum`).
- S0 evidence: `t3/api.md` (output type `GenericArray`→`hybrid-array`
  `Array`, `{:x}` loss with `hybrid-array 0.4.15`/`digest 0.11.3` source
  refs, unchanged type names `Blake2b512`/`Blake2s256`/`Sha3_256`/`Sha3_512`,
  full `{:x}` site list).
- Code note: `## fn hex_lower` appended to
  `LLM/code-notes/portage-util/lib.md`; ran `code_notes_index.py`.

## Commands run with results

- `cargo test --release -p portuale --bin portuale known_answer_digests_are_unchanged`
  on the pre-bump tree: **PASS** (safety net proven before the move).
- `cargo build --release --all-targets` after the bump: only the predicted
  `LowerHex is not satisfied` errors on the `{:x}` sites; clean after conversion.
- `cargo tree -d | grep -w digest`: empty. `grep -A1 'name = "digest"' Cargo.lock`:
  only `0.11.3`.
- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`: 0 warnings
  (one `format!` useless-use I introduced was fixed).
- `cargo test --release -p portage-util -p portage-fetch -p portage-profile`:
  all ok (49+135+8+1). `cargo test --release -p portage-repo`: 1239 passed.
- `cargo test --release -p portuale --bin portuale`: **1333 passed, 0 failed**
  (3 ignored). Includes the new test plus all digest/regen/remote/binpkg suites.
- One transient failure seen mid-task
  (`package_after_install_stanza_omits_default_slot_and_writes_both_digests`):
  the `self_exe.rs` stale-binary guard — I had edited a source file after the
  last build. Rebuilt, reran: green. Not a product issue.

## Open doubts / notes for the manager

- Skipped per brief: full workspace `cargo test --release`, pmtest suite,
  merge-path gate (`ebuild_merge/tree.rs` computes CONTENTS MD5s), commit,
  and marking #591 DONE — all yours.
- OWNER QUESTION: none. Every expectation (hybrid-array output, `{:x}` loss,
  unchanged type names, byte-identical digests) was settled from the crate
  sources in `~/.cargo/registry` and host Portage 3.0.83 output.
- Self-review note: one mistaken no-op edit briefly collapsed a newline in
  `portage-util/src/lib.rs`; reverted immediately and verified via
  `git diff` that the file then differed only by the `hex_lower` addition.
  Final `git diff --stat`: 33 files, all within the task's file list plus
  the lock, the test, and the code note.
