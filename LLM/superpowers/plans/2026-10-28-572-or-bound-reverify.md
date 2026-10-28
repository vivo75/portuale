# #572 (from #567) — an installed new-style virtual's `||` must reach the graph

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** proposed 2026-10-28 (branch `slice/572-or-reverify`).

**Goal:** portuale lists the package a new-style virtual's `||` falls through to when a
co-scheduled upgrade replaces the arm it had bound to (`perl-core/Encode-3.210.0`,
`dev-perl/Sub-Name-0.280.0-r1` in the #558 bed rows), instead of merging neither arm.

**Architecture:** Portage resolves a consumer's dep string with `_expand_new_virtuals`
before `dep_zapdeps` (`3rdparty/portage/lib/portage/dep/dep_check.py:37`), so an atom
naming a new-style virtual carries that virtual's own `RDEPEND` into the consumer's
requirements. The virtual's `||` is then evaluated in the consumer's graph, where the
bug-531656 downgrade guard (already ported as `or_preference::alternative_downgrade_demoted`)
demotes the replaced arm and the fall-through arm becomes a normal `_add_dep` node.
portuale has no expansion step, so that `||` is reached only by the reverse-dependency
pin pass, which cannot add nodes. The fix is to inline a new-style virtual's dependency
tokens into the owning consumer's flattened dep list — not to widen the installed walk.

**Tech Stack:** Rust 2024 workspace (`crates/portage-repo/src/resolve/`), Portage 3.0.83
(`3rdparty/portage`), pmtest contract suite + L0 bed.

**Spec:** `LLM/backlog-tasks.md` #572 (and #567's S0, `LLM/evidence/2026-10-25-567/`);
mechanism and rejected candidate in `LLM/evidence/2026-10-28-572/REPORT.md`.

## Global Constraints

- Expected values come from Portage, never from a second portuale run: the
  `ResolverPlayground` captures in `LLM/evidence/2026-10-28-572/` for the new shape and
  the existing Portage-pinned cells for everything the change moves.
- A cell that moves away from its Portage-pinned expectation is a regression. Stop and
  capture Portage on that exact shape before touching the pin (the Q19 cell below is the
  known tripwire).
- The change must not alter any output for a graph with no `||`-bearing new-style
  virtual whose arm was replaced: the walk order, merge order and `--tree` parents of
  every untouched probe stay byte-identical.
- Determinism: no timing/hash-order in pins (`Dependency resolution took …` is cut,
  `Change USE:` compared as a set — existing convention).
- Pair branch `slice/572-or-reverify` in portuale and pmtest; commits paired, pmtest
  first; corpus bless only in pmtest after reviewing drift row by row.
- Merge-path safety gate: this slice touches `resolve/` only, not `ebuild_merge/`,
  `emerge_build/` ordering or `ebuild_unmerge.rs`, so no glibc/bash reinstall is
  required. Re-check the file list before claiming this at close-out.

## Review Focus

- A virtual whose installed version has **no tree ebuild** (`virtual/libc-1.0` in the
  fixtures) must keep its installed behaviour: no expansion, no upgrade, silent display
  — `update_without_deep_keeps_an_installed_virtual_provider_silent`.
- A virtual that is *not* pure (`||` mixed with a plain atom) is not expanded by Portage;
  expanding it changes the answer (`probe572_mixed.py`). Do not special-case it away
  silently: pin the non-expansion.
- The Q19 sibling-virtual leak cell must not move: expanding a virtual as an installed
  walk removes the `[skipped-updates]` parents
  (`sibling_virtuals_of_a_merge_bound_parent_share_the_atom_a_sibling_chose`).
- `--nodeps` and `--onlydeps` must not gain merges from the expansion.
- `--emptytree`/`--deep` already walked these virtuals; the expansion must not duplicate
  a child (`||` arm reached twice → one node, one parent edge).

---

### Task 1: Fixtures, RED pin, evidence (done in this repo's working tree)

**Files:**
- Create: `LLM/evidence/2026-10-28-572/{REPORT.md,probe572*.py}`
- Create (pmtest, through the `fixtures/` symlink): `fixtures/repo/dev-libs/p572prov/*`,
  `fixtures/repo/dev-libs/p572enc/*`, `fixtures/repo/dev-libs/p572leaf/*`,
  `fixtures/repo/virtual/p572virt/*`, the matching `fixtures/repo/metadata/md5-cache/*`
  entries, and `fixtures/var/db/pkg/{dev-libs/p572prov-5.42,virtual/p572virt-2.0}`
- Create: `crates/portage-repo/src/tests/tests_p572.rs` (+ `mod tests_p572;` in
  `crates/portage-repo/src/tests/mod.rs`)

**Interfaces:**
- Consumes: `tests_consumer_pin_conflict::{hermetic_root, request_in, complete_request_in}`.
- Produces: the fixture family and the RED test that every later task must turn green.

- [x] **Step 1: Build the fixture family** (see the table in the REPORT) mirroring
  `dev-lang/perl-5.42.2 → 5.44.0`, `virtual/perl-Encode-3.210.0-r2`, `perl-core/Encode-3.210.0`,
  `dev-perl/Encode-Locale`. The virtual's `RDEPEND` must be `||`-only (a plain atom beside
  it suppresses Portage's expansion).
- [x] **Step 2: Write the RED test** `tests::tests_p572::…`, asserting the p02-shape merge
  list equals Portage's playground result: `p572enc New`, `p572prov Upgrade`,
  `p572virt AlreadyInstalled`, `p572leaf New`.
- [ ] **Step 3: Confirm the RED**

Run: `cargo test --release -p portage-repo tests_p572`
Expected: FAIL — `p572enc` absent (that is the #567 `[missing]` row).

- [ ] **Step 4: Validate the fixture metadata**

Run: `python3 ../pmtest/scripts/md5_cache_audit.py`
Expected: no invalid `_md5_` entry for `p572*`.

### Task 2: Inline a new-style virtual's deps into the consumer (product change)

**Files:**
- Modify: `crates/portage-repo/src/resolve/pass/enqueue_deps.rs` (the flatten call sites:
  `flatten_deps_for_enqueue` / `enqueue_flat_deps` tokens) and
  `crates/portage-repo/src/resolve/enqueue.rs` (`flatten_deps_for_enqueue`, the
  installed-owner call path)
- Modify: `crates/portage-repo/src/resolve/pass/unresolved_dep.rs` only if the flatten
  hook needs the resolved outcome
- Test: `crates/portage-repo/src/tests/tests_p572.rs`

**Interfaces:**
- Consumes: the fixture family from Task 1; `gate.repo_candidates`, `live_metadata_for_installed`,
  `read_vdb_flag_set` (both already used by `enqueue_installed_deps`).
- Produces: `fn virtual_expansion_tokens(...) -> Vec<String>` (name and module the
  implementer's choice, `resolve/enqueue.rs` preferred) returning the reduced dependency
  tokens of a new-style virtual, or an empty `Vec` when the atom is not one.

- [ ] **Step 1: Add the failing assertions for the tripwires before the code**

Add to `tests_p572.rs`: the `virtual/libc-1.0` shape stays `AlreadyInstalled` with no
`fakeglibc` row (`graph_update("dev-libs/toolchainroot")` oracle already pinned in
`tests::graph`), and the Q19 cell stays as `tests_consumer_or_view` has it. Run both
before the change to see them green, so a later move is unambiguous.

- [ ] **Step 2: Implement the expansion on the reverse-dependency channel, not the queue**

Read the S0 REPORT's last three candidates first — the queue was tried three ways and
each inverts the backtracking preference (the virtual's arm becomes a hard dep, so the
consumer's upgrade wins and the arm is reported as a masked miss instead of a skip row).

1. Teach the reverse-dependency pass to hand its **non-installed** chosen arms to the
   next walk as queue items, attributed to the virtual. The shape to reuse is
   `complete_seed_atoms`/`complete_reuse` (`portage_profile::Config`,
   `send_through` in `pretend/resolve_phases.rs`): a feedback kind or a seed list that
   `resolve_phases` carries into the next pass, so the arm resolves with a graph that
   already holds it rather than being appended mid-walk.
2. Keep installed-satisfied arms as pins (no queue item): that is Portage's parent-atom
   rule (`depgraph.py:8803-8809`, already partly ported as #603), and the Q19 cell is
   the tripwire.
3. Single-tree-version virtuals only; the multi-version choice
   (`|| ( (deps =v-1) (deps =v-2) )`) stays a documented cut — file a backlog number for
   it at close-out.

- [ ] **Step 3: Run the slice tests**

Run: `cargo test --release -p portage-repo tests_p572`
Expected: PASS.

- [ ] **Step 4: Run the whole crate and settle every move**

Run: `cargo test --release -p portage-repo`
Expected: 1303+ passed, 0 failed. Any moved cell is reviewed against its cited Portage
oracle before the pin is touched; a move with no oracle stops the task and adds a backlog
entry instead.

- [ ] **Step 5: Commit the product change** (paired with Task 3's pmtest commit, which
  lands first)

```bash
git add crates/portage-repo/src/resolve crates/portage-repo/src/tests/tests_p572.rs \
        crates/portage-repo/src/tests/mod.rs LLM/evidence/2026-10-28-572
git commit -m "resolver: inline a new-style virtual's deps into its consumer (#572)"
```

### Task 3: Second shape (nested virtual) and the pmtest pin

**Files:**
- Modify: `crates/portage-repo/src/tests/tests_p572.rs`
- Create/Modify (pmtest): the `CASES` entry + `pytests-contract-suite/test_emerge_pretend_contract.py`
  pinned function for the new fixture request, in pmtest's own commit

**Interfaces:**
- Consumes: Task 2's expansion.
- Produces: the S0 p03 shape (`Try-Tiny`'s `|| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name )`
  whose first arm's own `||` is doubly dead) as a fixture pair `p572slu*`, with Portage's
  playground answer recorded in the test docstring.

- [ ] **Step 1: Capture Portage on the p03 shape** with `LLM/evidence/2026-10-28-572/probe572.py`
  modified for the Sub-Name graph; record the merge list in the new test's docstring.
- [ ] **Step 2: Add the fixture pair and the Rust test**; run it RED, then GREEN after the
  expansion (no second product change expected — if the nested arm needs one, stop and
  file it as a #572 residue).
- [ ] **Step 3: Add the pmtest CASES entry + pinned output test**, run
  `../pmtest/scripts/pytest-parallel.sh`, and bless corpus drift only if it appears, after
  reviewing each row.

### Task 4: Docs, close-out and the bed gate

**Files:**
- Modify: `LLM/what-this-proves.md` (append one paragraph, never rewrite prior ones)
- Modify: `LLM/backlog-tasks.md` (#572 → DONE, #567 → DONE with the S0 cited)
- Modify: this plan's `Status:` header

- [ ] **Step 1: Run the full verification pass**

Run: `cargo fmt --check`, `cargo clippy --release --all-targets` (zero warnings),
`cargo test --release`, then in `../pmtest` `scripts/pytest-parallel.sh`.
Expected: all green.

- [ ] **Step 2: Re-grade the bed rows**

Run: `../pmtest/differential-test-bed/run/l0-resolver.sh` and confirm the six
`perl-544-stabilization-version-selection-{encode,sub-name}` rows go clean, then move the
two yaml entries out. If the bed cannot run here (no container runtime), say so in the
close-out and leave the yaml entries in place — the rows stay explained until a bed run
proves them gone.

- [ ] **Step 3: Close #572 and #567** with the commit, the bed verdict (or the recorded
  blocker), and the residue list.
