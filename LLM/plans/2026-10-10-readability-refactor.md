# #336 Readability refactor: implementation plan (v2)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development or
> superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

Status: in progress 2026-10-10 · branch `refactor/readability` (portuale + pmtest)
· v1 reviewed in `2026-10-10-readability-refactor.review.md` · owner-away
decisions in `2026-10-10-decisions.md` · renames recorded in `LLM/renames.tsv`.

**Commit grant (owner, 2026-10-10):** "commit locally, no push", one commit per
phase or task, paired across the two repos, made only once the gate is green.
At a checkpoint I decide, log the decision in `decisions.md` and continue.
Phase 6 waits for the owner. portuale-only commits say "standalone: no pmtest
counterpart" in the body.

**Goal:** make portuale readable for humans with behaviour frozen.
- Cargo runs from the repo root (`crates/`).
- LLM-only material lives in `LLM/`.
- Portage is called "Portage".
- Comments are short, and their context is kept in `LLM/code-notes/`.
- Call arguments are explicit.
- Duplicated code is folded.
- Giant files and functions are split.

**Spec, with the owner's decisions:** see v1's list, unchanged. In short:
- crates move to `crates/`;
- the real→Portage rename covers all of portuale in the Portage sense only;
- removed comment context goes to `LLM/code-notes/<crate>/<file>.md`, keyed by
  symbol;
- explicit arguments are typed (enums, params structs, lints);
- the `cargo dupes` fold;
- follow the rust skills and the `strategie-…` guide;
- the history squash comes after.

## Global constraints

- **Behaviour frozen.** Corpus and contract output stay byte-identical to the
  Phase 0 baseline. The only exception is portuale's *own* user-visible
  wording that says "real" for Portage. That drift is reviewed by hand and
  blessed in pmtest. (Portage itself prints none of these strings; the review
  checked it.)
- **FROZEN names**, never renamed:
  - `PortagePackage::Real` (external crate);
  - `real_quick_ratio` (Python difflib);
  - env value `PORTUALE_PYTHON_HELPERS=real` (read by the bed and `bin/`);
  - vendored Portage `bin/*` and upstream `man/*` pages;
  - pmtest `fixtures/` and `corpus/` content (e.g. hookoutputpkg's
    `DESCRIPTION` naming `docs/scope-backlog.md`).
- **Gate** after every commit. Run it serially, never alongside a bed run.
  1. In the repo root: `cargo fmt --check`.
  2. `cargo clippy --release --all-targets`, with zero warnings.
  3. `cargo test --release --no-fail-fast`.
  4. Then `git -C ../pmtest clean -fdq fixtures/ && (cd ../pmtest && python3 -m pytest pytests-contract-suite -q -rfE -p no:cacheprovider --basetemp=/var/tmp/pytest-readability)`.
  5. Compare failures with the Phase 0 baseline **by test name**. The
     test-name renames from `LLM/renames.tsv` apply.
- **Merge path** = `ebuild_merge.rs`, plus the merge-ordering code in
  `emerge_build.rs` and `emerge_getbinpkg.rs` (`agent-context.md:390-392`),
  plus `binpkg.rs`. Any phase whose commit changes *code tokens* there also
  runs:
  - bed L0 + L1;
  - the glibc + bash test merge.

  A comment-only change, proven with the token check in Phase 7, runs neither.
- **Bed:** `differential-test-bed/run/l0-resolver.sh` from pmtest, compared
  with Phase 0 on the UNEXPLAINED set. No `cargo build` anywhere while it runs.
- **No new runtime dependencies.** Never `pkill`; agent processes run through
  `helpers/oc.sh` scopes.
- **Serial work only** (decision 3). Helpers get file-scoped edit jobs; I build,
  run the gate and review every diff.

## Rules adopted from the skills and the guide (G8)

| Source | Rule | Here |
|---|---|---|
| rust-skills `proj-mod-rs-dir`, `proj-pub-use-reexport`, `proj-pub-crate-internal`, `test-cfg-test-module` | module layout, `pub use` to keep paths stable, `pub(crate)` | Phase 6 |
| rust-skills `api-*` / rust-patterns "illegal states unrepresentable" | enums over bool flags, params structs | Phase 5 |
| rust-refactor-pro DRY | fold duplicates | Phase 4 |
| guide §2.2 | functions ≤ ~50 lines, files ≤ ~500 lines *where natural* | Phase 6 (targets in 6.0) |
| guide §7.3 / owner | comments only for the non-obvious *why* | Phase 7 |
| guide §2.1 `rustfmt`, clippy | already enforced, kept | gate |
| guide `docs/adr/`, `CONTRIBUTING.md` | human docs | Phase 8 (short `docs/architecture.md`, `docs/CONTRIBUTING.md`) |
| rust-refactor-pro "comprehensive rustdoc everywhere", `#![deny(missing_docs)]` | **rejected**: contradicts "shorten comments"; rustdoc stays concise on public items | |
| `thiserror`/`anyhow`, "no unwrap outside tests" sweep | **deferred**: new deps / behaviour risk; not requested | |

---

## Phase 0: baseline (no product commit)

- [ ] Gate on `ebb1899d`. Keep the summaries in `LLM/plans/2026-10-10-baseline/`:
  - `cargo-test.txt` (`test result:` lines and the names of failing tests);
  - `contract-failures.txt`;
  - `clippy.txt`, `fmt.txt`.

  The full logs go to the scratchpad, not the repo.
- [ ] Re-run `cargo test --release --no-fail-fast`, so that one failing binary
  does not hide the crates after it. Re-run any failing test alone three times
  to label it flaky or broken.
- [ ] `dupes.json`. The stream holds **five** JSON values: the summary, exact
  functions, near functions, sub-exact and sub-near. Decode them in a loop.
- [ ] Run one bed L0 pass and record its UNEXPLAINED set as the parity reference.
- [ ] Commit `LLM/plans/` (plan, review, decisions, baseline summaries), so
  agents and worktrees can see them (P5). portuale standalone commit:
  `plan: #336 readability refactor`.

## Phase 1: Cargo workspace at the repo root (paired commit)

- [ ] `git mv rust/<crate> crates/<crate>` for all 16 crates. Move
  `rust/Cargo.toml`, `Cargo.lock` and `.cargo/` to the root. `rm -rf rust/target`.
- [ ] Root `Cargo.toml`: `members = ["crates/…"]`, and
  `exclude = ["3rdparty"]` (G7).
- [ ] Every literal `rust` path. The grep must also catch `join("rust")`:
  `grep -rnE '(^|[^a-z_-])rust/|"rust"|\.\./rust' --exclude-dir=target --exclude-dir=3rdparty --exclude-dir=.git . ../pmtest --exclude-dir=fixtures --exclude-dir=corpus`.
  Known hits:
  - `crates/portage-vdb/tests/no_hand_built_vdb_paths.rs:208`: change to
    `join("crates")`, and add `assert!(!files.is_empty())` (D1).
  - `musl/Containerfile`:
    - `COPY Cargo.toml Cargo.lock ./`, `COPY .cargo/ .cargo/`,
      `COPY crates/ crates/`;
    - `RUN cargo build …` with no `cd`;
    - every `/work/rust/target/` becomes `/work/target/`;
    - comments `:8,18,56` (D2).
  - `musl/smoke_test.sh:150,158`: tar `Cargo.toml Cargo.lock .cargo crates bin cnf`
    with `--exclude=./target`.
  - `scripts/real_world_spotcheck.sh:42-53`: `RUST_DIR="${REPO_DIR}"` (the name
    means real-world and stays, D11).
  - `.containerignore`: `/target`. `.gitignore`: `/target`, and update the comment.
  - `3rdparty/repos.toml:55`, `3rdparty/README.md:22`.
  - `README.md`, `bin/README.md`, `.claude/agents/fixture-smith.md:42`.
  - pmtest:
    - `managers/registry.py:181-186`: `rust_dir()` = repo root (the cargo dir).
    - Add `crates_dir()` = repo/`crates`.
    - `scripts/portage_repin_review.py:37,88` globs through `crates_dir()`, with
      an emptiness assert (D3).
    - `managers/managers.yaml:14-18`, `3rdparty/repos.toml:44`,
      `test_registry.py:136-150`.
    - Every `rust/target`, `rust/<crate>` path in `differential-test-bed/`,
      `scripts/`, `python-harness/`, `bench/` and test docstrings.
- [ ] `LLM/renames.tsv`: one row, `path	rust/<crate>/	crates/<crate>/	phase1`.
- [ ] History docs are not rewritten (decision 5). They get the header line in
  Phase 2.
- [ ] Update the auto-memory files that cite `rust/…` paths (17 files), and run
  `rm -rf graft/rust && graft build` (P7).
- [ ] Gate, then bed L0 + L1 (the bind mount of `target/release` moves). Commit
  pmtest, then portuale.

## Phase 2: `LLM/` directory (paired commit)

The move list (decisions 2, 11, 12):
- `docs/0[0-9].*.md`, `02.326-review-plan.md`
- `agent-context.md`, `backlog-tasks.md`, `backlog-tasks-2026-10.md`, `scope-backlog.md`
- `what-this-proves.md`, `lessons-of-backlog-ops-2026-10-03.md`, `recap-of-backlog-ops-2026-10-03.md`
- `feat-157-authoritative-vdb-database.md`, `glep-compliance-review.md`, `Paragone_solver_portage.md`
- `evidence/`, `superpowers/`

These all go to `LLM/` with the same relative names. `docs/` keeps:
- `README.md` (rewritten), `brush-pin.md`, `brush-pr/`;
- `diagrams/`, `images/`, `glep/`, `cleanroom/`;
- `on-disk-caches.md`, `performances-tuning.md`;
- `remote-merge.md`, `remote_emerge_examples.md`, `vdb_to_db.md`.

`helpers/` is untouched. `graft/` stays.

- [ ] `git mv` each file, and append a `path` row per file to `renames.tsv`.
- [ ] Inbound links, using a grep derived **from the move list**. Search
  portuale (crates, scripts, musl, man, bin, `.claude/agents`, AGENTS.md,
  README) and pmtest, excluding `fixtures/` and `corpus/` (G4). Then:
  - rewrite live entry points;
  - leave history docs alone;
  - give each history doc a header line after its title:
    `> Paths and identifiers before 2026-10-10: see `LLM/renames.tsv`.`
- [ ] `LLM/README.md`: the agent index, moved out of `docs/README.md`.
  `docs/README.md` becomes a human index.
- [ ] Link check with
  `python3 LLM/tools/check_links.py docs LLM AGENTS.md README.md .claude/agents`.
  The script is new in this task: it resolves each relative markdown link
  target and lists the missing ones, ignoring URLs and anchors. Expected: zero
  missing in live docs. History docs may list `rust/` targets, and those are
  covered by `renames.tsv`.
- [ ] File backlog #336 (this plan) and residues #337 (pmtest real→Portage pass),
  #338 (dupes groups skipped in Phase 4) and #339 (giant functions left after
  Phase 6) in `LLM/backlog-tasks.md`.
- [ ] Update AGENTS.md paths now, not in Phase 8, because its rules cite these
  files. Update the memories that cite moved docs.
- [ ] Gate, then commit pmtest, then portuale.

## Phase 3: "real" → "Portage" in identifiers, strings and live docs

### 3.1 Inventory, `LLM/plans/2026-10-10-real-inventory.tsv`

- [ ] Run two greps over `crates/`, excluding vendored code (D7, D10):
  - words: `grep -rnwiE 'real'`, for strings and docs;
  - identifiers: `grep -rnoE '\b\w*([Rr]eal_|_real|Real)\w*\b'`.
- [ ] Columns: `kind · file:line · token · class (PORTAGE|ACTUAL|OTHER|FROZEN) · replacement`.
  - Comment rows are **not** listed; Phase 7 handles them (decision 4).
  - The 241 `fn` names are all listed.
- [ ] Mapping: PORTAGE-sense `real` → `portage`. Examples:
  - `*_like_real*` → `*_like_portage*`;
  - `graph_result_real` → `graph_result_portage` only if it means "Portage's
    result", otherwise use the precise word.

  ACTUAL-sense renames only remove ambiguity:
  - `is_real_merge_command` → `is_merge_command`;
  - `graph_real*` → `graph_fixture*`;
  - `real_size`, `real_mtime` stay.
- [ ] Log the decision; no owner stop.

### 3.2 Identifiers (portuale standalone commit, or paired if pmtest is touched)

- [ ] Rename per TSV row, with
  `sed -E 's/\b<old>\b/<new>/g'` over `crates/`, then `cargo check --all-targets`.
- [ ] Append `ident` rows to `renames.tsv`.
- [ ] Gate. `ebuild_merge.rs` changes tokens, so also run bed L0+L1 and the
  glibc + bash merge.
- [ ] Commit `refactor: name Portage "Portage" in identifiers, not "real"`.

### 3.3 portuale's own user-visible strings (paired commit)

- [ ] Rows come from the inventory. Known:
  - `pretend.rs:3140,3184,3290` (pinned at
    `test_emerge_pretend_contract.py:13425,13572,17661,17677`);
  - `main.rs:110`;
  - `resolver_trace.rs:584` (13 corpus entries);
  - `ebuild.rs:96-99`, `binpkg.rs:1359` if PORTAGE-sense.

  `pretend.rs:3292` "which real shell" is ACTUAL and stays. Test-only strings
  (`portageq.rs:1084`) are optional.
- [ ] Gate. Expect `corpus drift` only on those strings: review it, then
  `PORTUALE_CORPUS_BLESS=1`, and update the pmtest pins. Commit pmtest, then
  portuale: `ui: say "Portage" instead of "real" in portuale's own messages`.

### 3.4 Live docs and agents (paired if pmtest is touched)

- [ ] Change "real" → "Portage" in README, `docs/*` (human), AGENTS.md,
  `LLM/agent-context.md`, `LLM/README.md`, `.claude/agents/*`,
  `scripts/*` and `musl/*` comments, and portuale-owned `man/` and `bin/`
  files. History docs are excluded.
- [ ] Rename the agent `real-portage-oracle` → `portage-oracle` (decision 7).
- [ ] End check: `python3 LLM/tools/real_check.py` (new). Every remaining
  `real` hit outside comments and history docs must be in the TSV as
  ACTUAL, OTHER or FROZEN. Expected: zero unclassified.
- [ ] Commit `docs: say "Portage", not "real", in live docs and agents`.

## Phase 4: fold duplicated code

- [ ] Re-run `cargo dupes` on the current tree (G10). Phase 0's file is used only
  for the before/after numbers.
- [ ] Task 4.1: the 11 function groups (5 exact, 6 near). Extract the shared body
  and pass the differences as typed parameters. Check bodies by reading them;
  "exact" means equal after normalisation, and literals may differ.
- [ ] Task 4.2: sub-function groups (D5). The reported spans are the *enclosing
  function's*, so ranking by size is useless.
  - Triage by enclosing function, production code first; test-only groups are
    folded only when they are helper-sized.
  - Fold a group only when the shared block has one meaning.
  - Record each group left alone, with its reason, in
    `LLM/code-notes/_dupes-skipped.md` (#338).
- [ ] Target: exact function groups = 0, and production sub-exact groups halved
  versus the re-run.
- [ ] Gate per crate. If merge-path tokens changed: bed L0+L1 and the glibc +
  bash merge. Commit per crate: `refactor(<crate>): fold duplicated <what>`.

## Phase 5: explicit arguments

- [ ] Task 5.1:
  - Add `[workspace.lints.clippy] too_many_arguments = "warn"` and
    `fn_params_excessive_bools = "warn"`.
  - Add `[lints] workspace = true` in each crate.
  - Root `clippy.toml`: `too-many-arguments-threshold = 5`,
    `max-fn-params-bools = 2`.
  - **Keep** the existing allows. Add allows to any newly flagged function, so
    clippy stays clean (P1).

  Commit.
- [ ] Task 5.2, per module:
  - Remove the allows in that module and fix the warnings: bool flags with a
    meaning become enums; more than 5 parameters become a params struct built
    with named fields; context passed everywhere (config, root, output) goes
    into one `&Ctx` introduced first, alone.
  - Clippy must be clean in every commit.
  - Commit `refactor(<module>): explicit call arguments`.
- [ ] Acceptance: `grep -rc 'allow(clippy::too_many_arguments\|allow(clippy::fn_params_excessive_bools' crates/`
  is 0, and clippy is clean.
- [ ] Gate. Then bed L0+L1 and the glibc + bash merge (the merge path is
  certainly touched).

## Phase 6: split giant files and functions (waits for the owner)

- [ ] 6.0: write `LLM/plans/2026-10-10-split-map.md`, covering:
  - every file over 1,500 lines and every function over 200 lines (from
    `graft skeleton`), including `pretend::run` (`pretend.rs:10081-16150`,
    about 6,070 lines), `emerge_build.rs` and `binpkg.rs`;
  - a proposed module map per file and an extraction map per function;
  - numeric targets.

  **Stop for the owner.**
- [ ] 6.1, after sign-off: move inline tests over ~300 lines into `<module>/tests.rs`.
  - Fix up relative paths (`vdb_rw.rs:1425,1447,1560-1564`, D12).
  - First teach `no_hand_built_vdb_paths.rs` to skip `tests.rs` files.
- [ ] 6.2: split per the signed map, with `pub use` for stable paths and
  `pub(crate)` where possible. Gate per file, and bed + merge gate per the
  merge-path rule.

## Phase 7: short comments + `LLM/code-notes/`

Can run before Phase 6 if the owner has not signed off yet. The notes are keyed
by symbol, so they survive the later split.

- [ ] Notes format: `LLM/code-notes/<crate>/<path-without-.rs>.md`, with
  `## <kind> <symbol>` sections holding the Portage citation, backlog #,
  traps and evidence paths. `LLM/code-notes/INDEX.md` has one line per file
  plus the comment ratio per crate (baseline 54.5k / 229k).
- [ ] What stays in the code:
  - `//!` module purpose, a few lines;
  - `///` on public items: summary, `# Errors`/`# Panics`, invariants;
  - `//` only for the non-obvious *why*, 1-2 lines;
  - one short `// Portage: <file>::<fn>` reference where the behaviour is
    surprising without it.

  Kept comments say "Portage", never "real" in that sense (decision 4).
- [ ] What moves to notes: backlog numbers, slice and batch history, dates,
  long Portage quotations, anecdotes, shas.
- [ ] What gets deleted: comments that restate the code.
- [ ] Proof per commit (D4), with `LLM/tools/strip_comments_eq.py` (new). It
  lexes each changed `.rs` file before and after (`git show HEAD:<f>`),
  dropping `//…`, `/*…*/` and doc comments outside string literals, and
  compares the token streams. Expected: equal for every file.
- [ ] Also check that the doctest count is unchanged (`cargo test --doc` summary).
- [ ] Batches are one crate or one big file per commit:
  `docs(<crate>): shorten comments, context to LLM/code-notes`. Token-equal
  commits need no bed run and no merge gate. Run `cargo test` per crate, and
  the full gate at the end of the phase.
- [ ] Target: the owner sets the final ratio. Working aim: comment lines ≤ 12 %
  of `.rs` lines (from 24 %).

## Phase 8: close-out

- [ ] `docs/architecture.md` (human: goals, crate map, the data flow of one
  `emerge`) and `docs/CONTRIBUTING.md` (build/test from the root, commit
  policy).
- [ ] AGENTS.md: the code-notes rule ("context goes to `LLM/code-notes`,
  comments stay short").
- [ ] `graft build`. `LLM/agent-context.md` current state. One
  `LLM/what-this-proves.md` paragraph (behaviour unchanged: corpus/bed numbers
  before and after).
- [ ] Full gate, bed L0 + L1, glibc + bash merge. Flip `Status:` to done, with
  the commit list; flip #336 to DONE. No push.
