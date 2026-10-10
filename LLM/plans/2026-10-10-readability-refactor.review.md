# Review: `LLM/plans/2026-10-10-readability-refactor.md`

**Nothing else was modified.** This review file is the only file written. No
product code, fixture, doc, pin, plan, memory file or commit was touched.

- Audited: portuale `refactor/readability` at `ebb1899df69aae97fd256fc253c05432823e121c`.
  Worktree: tracked files clean; untracked `LLM/` (the plan and its partial
  baseline) and `.claude/skills/rust-refactor-pro/`.
- pmtest `refactor/readability` at `45257de1df6a516d84e77a8faaa0b305dd8a34a1`, clean.
- Scope: the plan before execution. No commit range exists yet.
- Verification: I re-ran **no** gate pass and built nothing. A `cargo test --release`
  (Phase 0 baseline, pid 1495401) was running in `rust/` during the audit, and its
  capture `LLM/plans/2026-10-10-baseline/cargo-test.full.txt` was still partial
  (16 `test result:` lines, `portuale` main.rs suite still running). The only tool
  I ran was `cargo dupes --sub-function --min-lines 50 --format json`. It parses
  source and does not compile. Output: scratchpad, not the repo.
- Every finding is marked **proven** (a discriminating source read or command
  output is cited) or **likely** (reading only).

---

## What was checked and holds

- **Size claims.** 16 workspace members (`rust/Cargo.toml:3-20`). `.rs` lines
  = 229,006. Lines starting with `//` = 54,542. Giant-file sizes in Phase 6 are
  exact (`portage-repo/src/lib.rs` 77,226, `pretend.rs` 20,614, `ebuild_merge.rs`
  11,278, `ebuild_phases.rs` 9,233, `remote.rs` 8,730, `merge_order.rs` 8,179,
  `portage-profile/src/lib.rs` 7,285).
- **Dupes baseline numbers.** 5 exact and 6 near function groups, 905 exact and
  56 near sub-function groups, 1,020 + 833 = 1,853 duplicated function lines
  (≈1.9k). The `graph_real*` group sits at `portage-repo/src/lib.rs:49608-50059`
  as cited.
- **Lint baseline.** 127 `#[allow(clippy::too_many_arguments)]` attributes. 129
  is the raw token count, including two comments at `lib.rs:13935,35397`. 7
  `fn_params_excessive_bools`. No existing `[lints]`, `clippy.toml` or
  `rustfmt.toml`.
- **Phase 1 depth invariant.** It holds for build-time and compile-time paths:
  - `portuale/build.rs:19-27` (`manifest.join("../..")`, `rerun-if-changed=../../bin`).
  - Every `CARGO_MANIFEST_DIR` join (`../../fixtures*`, `../../3rdparty/portage`).
  - `ebuild_phases.rs:720-729` (`repo_root()` = `ancestors().nth(2)`).
  - `embedded_runtime.rs:94`.

  None of them changes under `rust/x` → `crates/x`. Exe-relative lookups
  (`main.rs:273`, `embedded_runtime.rs:243`, …) only pop `deps/`. They do not
  climb to the repo, so they are safe too.
- **No pmtest selection of cargo test names.** pmtest never runs `cargo test`
  with a filter, so renaming Rust test functions breaks no pmtest selection.
- **Workflows.** `.github/workflows/*.yml` already run `cargo clippy` from the
  root with no `working-directory: rust`. Today that step cannot work. After
  Phase 1 it will.
- **Portage citations used in the plan's examples exist** in 3.0.82.2:
  `_emerge/depgraph.py:2108` `_process_slot_conflicts`, `:3550` `_add_pkg`.
- **Portage itself prints none of the "real" strings** portuale emits.
  `grep` of `_emerge/` finds no "real emerge option", "LIFO" or "real's" text.
  So every PORTAGE-sense "real" in portuale's own output is portuale's wording
  and may change under Phase 3.3.
- The `PORTUALE_REAL_PORTAGEQ` rename is safe. pmtest and the bed never set
  it. It is read only by the optional test `portageq.rs:1082-1086`.
- `docs/0[0-9].[0-9][0-9][0-9]-*.md` matches all five tracked plan files.
  `docs/README.md` has the "For contributors / agents" section the plan cites
  (`docs/README.md:7`).

---

## D: defects

**D1 (high, proven). Phase 1 makes the VDB lint test pass vacuously.**
- **Evidence.** `rust/portage-vdb/tests/no_hand_built_vdb_paths.rs:208` builds
  its walk root as `workspace_root.join("rust")`. `collect_rs_files`
  (`:186-197`) swallows the `read_dir` error with `if let Ok(...)`.
- **Divergence.** After the move, `rust/` does not exist. The test scans zero
  files and passes, so the guard that keeps hand-built VDB paths out of the code
  is silently off. The plan's grep (`'"rust/\|/rust/\|\.\./rust'`,
  plan:116) does not match `join("rust")`.
- **Fix direction.** Walk `workspace_root.join("crates")`. Make an empty file
  list a test failure (`assert!(!rs_files.is_empty())`). Add `join("rust")` /
  `"rust"` to the Task 1.1 grep.

**D2 (high, proven). The plan's `Containerfile` rewrite would flatten the build context.**
- **Evidence.** Plan:126-127 proposes `COPY Cargo.toml Cargo.lock .cargo crates ./`.
  In Docker/Podman, a directory `COPY` source copies its *contents*, not the
  directory. `crates/portuale/...` would land as `./portuale/...` and `.cargo/`
  as `./config.toml`.
- **Misses.** The plan also omits `musl/Containerfile:42-45,52-54`
  (`COPY --from=builder /work/rust/target/...`) and the comment lines `:8,18,56`.
- **Fix direction.**
  - `COPY Cargo.toml Cargo.lock ./`, `COPY .cargo/ .cargo/`, `COPY crates/ crates/`.
  - `RUN cargo build ...` with no `cd`.
  - Every `--from=builder` path becomes `/work/target/x86_64-alpine-linux-musl/release/…`.
  - In `musl/smoke_test.sh:158`, use `--exclude=./target` (anchored), not a bare `target`.

**D3 (high, proven). pmtest's repin review silently scans nothing after Phase 1.**
- **Evidence.** `pmtest/scripts/portage_repin_review.py:37,88` does
  `RUST = registry.rust_dir(...)` and then `RUST.glob("*/src/**/*.rs")`. The
  plan changes `rust_dir()` to return the repo root (plan:133-135). The crates
  then sit at `crates/*/src`, so the glob matches nothing.
- **Fix direction.** Keep `rust_dir()` as the *cargo* dir (= repo root). Add a
  `crates_dir()` (repo/`crates`) for source globs, or change the glob to
  `crates/*/src/**/*.rs`. Add an emptiness assertion.

**D4 (high, proven). Task 7.2's "comment-only diff" check can never pass.**
- **Evidence.** Plan:413 pipes `... | grep -v '^[+-]{3}'`. Without `-E`, `{3}`
  is a literal string in BRE. Run on a pure comment edit, the pipeline prints
  `--- a/x.rs` and `+++ b/x.rs` (verified with `printf` input).
- **Also missed.** The check ignores `/* */` comments, and it would accept a
  changed `//`-led line inside a multi-line string literal (likely rare).
- **Fix direction.** Use `git diff -U0 | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ' | grep -vE '^[+-]\s*(//|$)'`.
  Better: compare token streams, e.g. `cargo expand`, or a syn-based strip of
  comments before and after. Also compare the doctest count, because 20 doc
  comments contain fenced code blocks.

**D5 (high, proven). Task 4.2's sort key is meaningless for sub-function groups.**
- **Evidence.** For sub-function units, `cargo dupes` reports the *enclosing
  function's* span. Example: three members "match arm 1/2/3", all
  `portuale/src/pretend.rs:10081-16150` (that is `pretend::run`). So
  `lines × members` ranks a small match arm as a 6,070-line block.
- **Stop rule.** By that same metric, 834 of 961 sub groups exceed the
  "3×50" stop rule (plan:288), so the rule trims almost nothing.
- **Fix direction.** Rank sub groups by the real block size. Re-locate each
  member by fingerprint with syn, or ask cargo-dupes for block spans if a flag
  exists. Otherwise drop the numeric rule and triage by file/function by hand.
  Restrict folding to production code first (311 groups are in
  `portage-repo/src/lib.rs`, most of them test code).

**D6 (medium, proven). The dupes JSON recipe reads only the first group array.**
- **Evidence.** Plan:85-86 says the report is "a summary object followed by a
  group array". The real stream holds five JSON values: the summary, exact
  functions (5), near functions (6), sub exact (905) and sub near (56). The
  given `raw_decode` recipe stops after the first array.
- **Fix direction.** Decode in a loop until the input is exhausted, and name the
  four arrays.

**D7 (medium, proven). The Phase 3 inventory grep misses every compound identifier.**
- **Evidence.** Plan:212 uses `grep -rnwiE 'real'`. `-w` treats `_` as a word
  character, so `is_real_merge_command`, `graph_result_real`,
  `PORTUALE_REAL_PORTAGEQ` and 241 distinct `fn` names containing a
  `real`/`_real` component are never listed.
- **Scale.** Scoping said "about 40 distinct identifiers" (plan:226). The
  tree has 241 such `fn` names, about 98 of them obviously Portage-sense
  (`*_like_real*`, `*matches_real*`, …).
- **Fix direction.** Grep twice. Once for words (comments, strings). Once
  `-oE '\b\w*(real_|_real|Real)\w*\b'` for identifiers. Update the
  owner-checkpoint estimate.

**D8 (medium, proven). Some identifiers must never be renamed, and the plan does not freeze them.**
- **`PortagePackage::Real`.** An external crate's enum variant
  (`portage-repo/src/solver_bridge.rs:1068-1077`, from `portage_atom_pubgrub`).
  "Real" here means non-virtual. It cannot be renamed.
- **`real_quick_ratio`.** Listed in the plan's identifier rows (plan:229). It is
  Python `difflib.SequenceMatcher.real_quick_ratio` (`/usr/lib/python3.14/difflib.py:651`),
  cited in `portuale/src/difflib.rs:4-6`. Renaming it breaks the citation.
- **`PORTUALE_PYTHON_HELPERS=real`.** A user-facing env *value*, read at
  `ebuild_phases.rs:865,3314` and `helpers/chmod_lite.rs:46`. It is set and
  tested by the bed (`pmtest/differential-test-bed/run/lib.sh:151`) and
  documented in `bin/chmod-lite:4` and `bin/README.md:22,40`, which are embedded
  into the binary by `build.rs`. The plan's env rule (plan:219-220) covers only
  `PORTUALE_REAL_*` names.
- **`PORTUALE_REAL_PYTHON`.** Set by 13 tests (to `/nonexistent`) but read
  nowhere. It is a dead guard; renaming it is harmless but pointless.
- **Fix direction.** Add a FROZEN class to the TSV: external-crate identifiers,
  Python/stdlib API names, and env names/values that the bed or `bin/` read.
  Keep the value `real` for `PORTUALE_PYTHON_HELPERS`, or add `portage` as an
  alias and keep `real` accepted. Any change needs the bed-side change in the
  same pair.

**D9 (medium, proven). Phase 3.3's "known rows" are wrong or incomplete.**
- **Wrong row.** `portageq.rs:1084` is inside `#[test] fn matches_real_portageq_on_the_fixture_root`
  (`:1081`). It is not user-visible output.
- **Missing user-visible rows** (all portuale's own wording):
  - `pretend.rs:3140` (`"emerge: {kind} {:?} is a real emerge {kind}, but is not yet …"`,
    pinned in pmtest at `test_emerge_pretend_contract.py:13425,17661,17677`).
  - `pretend.rs:3184`, `pretend.rs:3290` (`--help`: "not real emerge options",
    pinned at `:13572`).
  - `main.rs:110` (`mrg` description).
  - `portage-repo/src/resolver_trace.rs:584` (`--debug` trace "not real's LIFO
    `_create_graph` order", 13 corpus entries in `contract.json.xz`).
- **Fix direction.** Generate the rows from the inventory, not from memory.
  List the pmtest pins at those lines as the Task 3.3 pmtest edits.
  `pretend.rs:3292` ("which real shell") is ACTUAL-sense and stays.

**D10 (medium, proven). The inventory scope sweeps in vendored Portage files.**
- **Evidence.** Plan:212 greps `man/ bin/`. `bin/` is a verbatim copy of
  upstream Portage `bin/` (`bin/README.md:3-10`) except `chmod-lite`,
  `portuale-python` and `phase-functions.sh`'s one local change. It is embedded
  byte-for-byte into the binary (`portuale/build.rs:29-31`). `man/` mixes
  upstream pages (`make.conf.5`, `emerge.1`, `portage.5`, …) with portuale's
  own (`mrg.1`, `portuale-*.1`).
- **Why it matters.** Rewording vendored text breaks the re-sync contract and
  changes the embedded runtime.
- **Fix direction.** Exclude vendored files from the inventory. Only
  portuale-owned `bin/` and `man/` files are eligible.

**D11 (medium, proven). Phase 1 and Phase 3 contradict each other on `scripts/real_world_spotcheck.sh`.**
- **Evidence.** Plan:130-131 says the file "is renamed in Phase 3". Plan:262-263
  says "here real means real-world, so it stays".
- **Fix direction.** Drop the Phase 1 parenthetical.
- **Also missed.** Phase 1 does not list `scripts/real_world_spotcheck.sh:43,53`.
  `PILOT_BIN=${RUST_DIR}/target/release/portuale` stays correct once `RUST_DIR`
  is the repo root, but the line-42 edit alone is not the whole story.

**D12 (medium, proven). Task 6.1's "mechanical move with no edits" is false for at least one file.**
- **`vdb_rw.rs` relative paths.** Its tests use paths relative to the file:
  `include_str!("vdb_rw/testdata/…")` at `:1447,1560-1564` and
  `#[path = "vdb_rw/ops.rs"]` at `:1425`. Moved into `vdb_rw/tests.rs`, these
  resolve to `vdb_rw/vdb_rw/…`.
- **VDB lint test.** It recognises test code only by `#[cfg(test)]` spans
  inside a file (`find_test_spans`, `:25-55`) or a directory named `tests`
  (`:239-245`). An extracted `foo/tests.rs` is then scanned as production code.
  Likely outcome: false "hand-built VDB path" failures from test helpers.
- **Fix direction.** Allow path fix-ups in 6.1. Teach the lint test to skip files
  named `tests.rs`, or any file whose parent declares it `#[cfg(test)] mod`.

---

## G: gaps

**G1 (high, proven). `helpers/` is untracked, so Phase 2 cannot `git mv` it.**
- **Evidence.** `.gitignore:24` ignores `/helpers`, and `git ls-files helpers` is
  empty. Every `helpers/*` row in the table (plan:170) is local scratch, both
  "moves to `LLM/`" and "stays human". `LLM/helpers/` is *not* ignored, so a
  plain `mv` starts committing previously private files: `IDEE.md`,
  `lu_zero.md`, `migrate-to-new-server.sh`, `real_emerge/` with absolute host
  paths. Whether they hold secrets is likely, not checked.
- **"Stays human" files.** `strategie-…md`, `distfile-mirror.md`, `devmanual/`
  stay ignored and invisible. They are not "in `docs/`".
- **Missed.** The `oc*.sh` glob misses `oc.sh.orig-dir-flag`.
- **Close by.** An owner decision per file (track or keep ignored), stated in
  the checkpoint. Update `.gitignore` and the `oc.sh`/`scoperun.sh` header
  comments ("Lives in portuale/helpers/").

**G2 (high, proven). The parallel worktrees (plan:438-439) cannot run the gate as written.**
- **No fixtures.** `fixtures` is an untracked, ignored symlink
  (`fixtures -> ../pmtest/fixtures`, `.gitignore:44-45`). A new worktree has
  none, so every `../../fixtures` test fails. The relative target only resolves
  if the worktree is a sibling of `pmtest`.
- **Wrong binary.** The pmtest gate always builds and tests `../portuale`
  (`managers/managers.yaml:14-18`). The registry has no repo override
  (`registry.py:98,119,246` read only `PMTEST_PM`, `PMTEST_PROFILE`,
  `PMTEST_NO_BUILD`). So a worktree's contract run tests the *main checkout's*
  binary and triggers `cargo build` in the main `target/`. If a bed run is
  active there, that is exactly the "no rebuild during bed runs" hazard.
- **Shared fixtures.** All worktrees share one pmtest fixture tree, which the
  suite pollutes. The memory rule says `git clean -fdq fixtures/`, so parallel
  runs collide.
- **Branch pairing.** AGENTS.md wants paired same-name branches, and a
  per-crate worktree branch has no pmtest twin.
- **Parallelism is small anyway.** Most Phase 4/5/7 work is in `portage-repo`
  and `portuale`, and `portuale` depends on `portage-repo`. Phase 5's shared
  `&Ctx` is cross-cutting.
- **Close by.** Either drop parallel worktrees, or specify:
  - a per-worktree `managers.yaml` entry with `repo:` pointing at the worktree;
  - creating the `fixtures` symlink in the worktree;
  - serialising pmtest runs (one at a time, `git clean` before each);
  - running the bed only from the main checkout, with no cargo activity there
    during the run;
  - landing `Ctx` alone, before fanning out.

**G3 (high, proven). The plan does not address the single largest readability problem.**
`pretend::run` spans `portuale/src/pretend.rs:10081-16150`, about 6,070 lines
in one function. The guide's §2.2 asks for functions under 40-50 lines, and
Phase 6 splits files, not functions. Close by adding a task that breaks up
functions over N lines, at least `pretend::run`, with an owner-agreed N. Take
the list from `graft skeleton`, or from the function spans `cargo dupes` reports.

**G4 (high, proven). Inbound-link greps miss most references.**
- **pmtest.** The plan's pattern `portuale/docs/\|docs/agent-context`
  (plan:183) finds 23 references. The full set of LLM-bound docs referenced from
  pmtest, outside the corpus, is 106: `docs/what-this-proves`,
  `docs/scope-backlog`, `docs/07.56-…`/`07.058-…` in `scripts/gpkg_crafted_members.py:5,9`,
  `bench/*.py`, test docstrings.
- **Crates.** The pattern omits `feat-157` (`portage-vdb/src/lib.rs:4`),
  `superpowers/` (`portuale/src/vdb_rw.rs:4`), `lessons-*`, `recap-*`,
  `glep-compliance-review` and `backlog-tasks-2026-10`.
- **Must not edit.** `pmtest/fixtures/repo/dev-libs/hookoutputpkg/*.ebuild`
  and its md5-cache entry carry `docs/scope-backlog.md` inside `DESCRIPTION`.
  Changing it changes `-v` output and the md5.
- **Close by.** Derive the grep from the actual move list. Explicitly exclude
  `fixtures/` and `corpus/`.

**G5 (high, proven). No policy for historical documents that stop being runnable.**
- **Scale.** `docs/what-this-proves.md` has 240 lines with `rust/`, 53
  `cd rust` examples and 88 `cargo test` lines (at least one renamed test,
  `install_dies_like_real_when_s_is_missing`, `:18395`). 55 `docs/` files hold
  455 `rust/` references. 250 references to `real`-named functions sit in
  `docs/` (backlog, plans, evidence, cleanroom). pmtest findings (`l0.md`,
  `l2.md`, `l3.md`) cite them too.
- **Conflict.** AGENTS.md rule 7 forbids rewriting prior paragraphs, and the
  plan's "fix the links" (plan:139-140) would rewrite them.
- **Close by.**
  - A committed rename map (`LLM/renames.tsv`: old path or identifier → new).
  - A one-line header in `what-this-proves.md`, `backlog-tasks*.md` and
    `LLM/evidence/README`: "paths/identifiers before 2026-10-10: see renames.tsv".
  - Live entry points only (README, AGENTS.md, agent-context, `.claude/agents`,
    `bin/README.md`, `3rdparty/*`) are rewritten.

**G6 (medium, proven). Phase 1 misses some files that hold `rust/` paths.**
- `.containerignore:1` (`rust/target`, should become `/target`).
- `3rdparty/repos.toml:55` (`pinned_in = "rust/portuale/Cargo.toml"`) and
  `3rdparty/README.md:22`.
- pmtest `3rdparty/repos.toml:44`, and `managers/managers.yaml:16-18`. The
  `emerge:`/`ebuild:`/`mrg:` paths are real paths, not just the `:14` comment,
  though the plan's catch-all grep would find them.
- pmtest `test_registry.py:136-150` (literal path strings; stale, still pass).
- `README.md:60,85,88,96,119`. `bin/README.md:33,54,68,77`.
  `.claude/agents/fixture-smith.md:42` ("product code in `rust/`").
- The `.gitignore:22-23` comment.
- The auto-memory files. 17 of them contain `rust/`, but the plan updates
  memory only in Phase 2, and only for `helpers/oc.sh` and docs paths.

Close by extending the Task 1.2 list and running the memory update in Phase 1
as well.

**G7 (medium, likely). Repo-root `Cargo.toml` adopts nested crates in `3rdparty/`.**
`3rdparty/{findutils,diffutils,grep,util-linux}/Cargo.toml` have no
`[workspace]`. With a workspace manifest at the portuale root, `cargo` invoked
inside them fails with "current package believes it's in a workspace when it's
not". Nothing in the repo builds them today. Close by adding
`exclude = ["3rdparty"]` to `[workspace]`.

**G8 (medium, proven). Some owner requests have no task.**
- **"Follow the rust skills".** The plan gives no checklist of which rules apply.
  - It cites "rust-skills `mod-*`" (plan:348). No `mod-*` rules exist; the
    module rules are `proj-*` (`proj-mod-rs-dir`, `proj-pub-use-reexport`,
    `proj-pub-crate-internal`, `test-cfg-test-module`).
  - `.claude/skills/rust-refactor-pro/` holds `SKIL.md` (sic), so it does not
    load as a skill.
  - That skill asks for "comprehensive rustdoc" and `thiserror`/`anyhow`. The
    first conflicts with "shorten comments"; the second with "no new runtime
    dependencies".
- **The readability guide.** It covers `docs/adr/`, `CONTRIBUTING.md`,
  `rustfmt.toml`, `#![deny(missing_docs)]`, no `unwrap` outside tests, and
  functions under 50 lines. The plan does not say which items it adopts or
  rejects.
- **"Stop calling Portage real across all of portuale".** No task covers
  `AGENTS.md` (11 hits), `.claude/agents/*` (32, including the agent *name*
  `real-portage-oracle`, referenced by `.claude/agents/README.md:10,30`),
  `scripts/` (41), `musl/` (19), or the LLM docs moved to `LLM/` before Phase 3
  runs. If `LLM/` is out of scope, say so.
- **Human architecture doc.** Moving `docs/agent-context.md` wholesale strips
  `docs/` of its only goals and architecture document. Nothing replaces it.
- **Close by** adding a per-rule adopt/reject table, fixing `SKIL.md` →
  `SKILL.md` (owner's file), and widening or explicitly narrowing the Phase 3 scope.

**G9 (medium, proven). Several acceptance criteria are unverifiable or missing.**
- **Phase 0 (plan:91).** Asks for "three files" but lists six artefacts.
  It never says that the L0 parity reference is the UNEXPLAINED set.
- **Phase 2 link check (plan:187-189).** Only lists link targets. There is no
  resolver script; it skips `.claude/agents`, code comments and pmtest; and it
  includes URLs.
- **Phase 3.** No end check that every remaining `real` hit is an ACTUAL,
  OTHER or FROZEN row in the TSV.
- **Phase 4.** No after-target.
- **Phase 5 "bool flags become enums".** Not checkable. `fn_params_excessive_bools`
  fires only at 4+ bools (default `max-fn-params-bools = 3`), so the ~75
  bool-parameter functions never reach the clippy worklist.
- **Phase 6.** No size target and no list beyond seven files. `emerge_build.rs`
  (5,773) and `binpkg.rs` (5,292) are not in it.
- **Phase 7.** No comment-ratio target.
- **Close by.**
  - A `tsv-check` script for Phase 3.
  - `max-fn-params-bools = 1` in `clippy.toml` if "one obvious bool stays" is
    the rule.
  - Numeric targets the owner signs.

**G10 (low, proven). Phase 4 starts from a stale worklist.**
`dupes.json` is generated in Phase 0 against `rust/…` paths and pre-Phase-3
names. By Phase 4 the paths are `crates/…`, `graph_real*` is `graph_fixture*`,
and line numbers have moved. The example at plan:275-279 cites the old names
and lines. It also says the bodies are "identical"; they differ in a bool
literal (`lib.rs:49625` vs `:49881`), and cargo-dupes' "exact" means equal
after normalisation. Close by re-running `cargo dupes` at the start of Phase 4
and keeping the Phase 0 file only for the before/after numbers.

**G11 (low, proven). Merge-path gate triggers are narrower than agent-context's definition.**
`docs/agent-context.md:390-392` defines the merge path as `ebuild_merge.rs`
plus the ordering in `emerge_build.rs` and `emerge_getbinpkg.rs`.
- Phase 4 (plan:291-292) keys on `ebuild_merge.rs` and `binpkg.rs`.
- Phase 6 (plan:369) keys on `ebuild_merge` and `ebuild_phases`.
- Phases 3.2/3.3 and 7 edit `ebuild_merge.rs` (`is_real_merge_command`, comments)
  and schedule no bed or safety merge at all.

Close by using the agent-context file list as the trigger in every phase. For
Phase 7, get an explicit owner waiver backed by the token-stream proof (D4).

---

## P: process deviations

**P1 (medium, proven). The gate goes red between commits in Phase 5.**
Task 5.1 removes every allow and turns the lints on (plan:304-317). Task 5.2
then fixes the warnings module by module, with a commit per module. Every
intermediate commit therefore fails `clippy --all-targets` with zero warnings,
which breaks the plan's own rule (plan:16, "the verification gate is green at
every commit"). Fix: turn the lints on in 5.1 but keep the `#[allow]`s, then
remove each allow in the commit that fixes it, or land 5.1 last.

**P2 (medium, proven). Comments are rewritten twice.**
Phase 3.4 rewrites about 10k comment lines ("real" → "Portage"), and Phase 7
then rewrites or moves the same comments into `LLM/code-notes`. That doubles
the review work, which the plan reserves for the coordinator (plan:436-437).
Fix: Phase 3 keeps identifiers (3.2), strings (3.3) and human docs. The comment
part of 3.4 becomes a Phase 7 rule ("notes and kept comments say Portage").

**P3 (medium, proven). There is no backlog number and no plan-naming compliance.**
- **No entry.** The work has no entry in `docs/backlog-tasks*.md`. The next
  free number is #336 (highest filed: 335).
- **Unnumbered residues.** Deferred work has no number: "pmtest gets its own
  pass later" (plan:28), the skipped dupes groups (plan:288-289), and pmtest's
  `real` names. That includes CASES ids like
  `test_pretend_case_exit_code[real emerge option, …]` (corpus keys) and 384
  fixture files mentioning "real", which must stay frozen when that pass runs.
- **Location.** AGENTS.md rule 11 and "Numbering" expect
  `<tier>.<item>-<slug>.opus.md` and a numbered residue for every deferral.
  Putting the plan in `LLM/plans/` is consistent with the new layout, but
  AGENTS.md is updated only in Phase 8.
- **Fix.** File #336 (and residue numbers) in Phase 2, when the backlog moves.

**P4 (low, proven). The plan does not cite the commit grant.**
AGENTS.md rule 9 requires an explicit ask for each commit. The plan commits at
every phase (plan:144-146, 193-194, 240, …) and does not cite the owner's grant.
Memory shows grants are recorded per batch (`326-commit-grant`). Also, the
portuale-only commits in phases 3.2, 3.4 and 4-7 must say they have no pmtest
counterpart (AGENTS.md "Standalone commits stay single"). Fix: quote the grant
in the plan header and add the "standalone" line to those commit templates.

**P5 (low, proven). Untracked artefacts are invisible to worktrees and agents.**
The plan, its baseline (with a 341 KB test log), and Phase 3's TSV live in
untracked `LLM/` until the Phase 2 commit. Memory `sdd-reports-untracked`
records that worktrees cannot see untracked reports. Fix: commit `LLM/plans/`
(minus bulky logs, or with an owner OK on them) before any worktree or agent
is launched.

**P6 (low, likely). Two docs the plan keeps in `docs/` are Italian.**
`docs/Paragone_solver_portage.md` is an Italian LLM chat transcript (an LLM
artefact by the plan's own rule), and the guide in `helpers/` is in Italian.
The "English, always" rule binds agent artefacts, not the owner's notes, but
`Paragone_solver_portage.md`'s placement should be on the owner checkpoint
with that fact stated.

**P7 (low, proven). Two indexes go stale and the plan does not refresh them.**
- `graft/` is gitignored and holds `graft/rust/…` cards. `graft build` adds
  `graft/crates/…` but may leave the stale `graft/rust/` cards.
- The external codebase-memory index is named
  `home-vivo-repo-PORTUALE-portuale-rust`.

Fix: `rm -rf graft/rust` before `graft build`, and re-index the
codebase-memory project at the repo root.
