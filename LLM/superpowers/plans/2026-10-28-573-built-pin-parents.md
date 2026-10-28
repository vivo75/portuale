# #573 — the skipped-update block lists every consumer Portage lists

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** done 2026-10-28 (Tasks 1–3; branch `slice/573-built-pin-parents`
in both repos, portuale `55b8ec58`, pmtest `8ff08e5` +
the contract commit). Task 3's design moved while executing: the bed showed
that a locked complete retry changes the merge list (`p569pull-2` where
Portage keeps `-1`), so the missed-update parents come from a report-only scan
over the required sets' installed closure inside the plain pass
(`ctx.report_reachable`, `ReverseScan.report`) — same rows, no second resolve.
The evidence and the measurements are in
`LLM/evidence/2026-10-28-573/REPORT.md` §2b.

**Goal:** on the L0 `dev-lang/perl` probe — and on any withheld provider update — the
`WARNING: One or more updates/rebuilds have been skipped` block names **every**
installed consumer Portage names, not just the first in-graph consumer that happens
to share the pin atom.

**Architecture:** the skip block is derived from backtrack masks
(`runtime_pkg_mask` → `MaskReason::SlotConflict { parents }` → `backtrack_missed_updates`),
and the parents come from `RevDepPin`s built by the reverse-dependency scan. Two
defects shrink that set: (a) a mask is created once per `(cp, atom)` and every later
consumer recording the same atom loses its rows; (b) consumers Portage reaches only
through complete-mode's installed-vdb walk never produce a pin at all. (a) is fixed
in the mask bookkeeping; (b) is fixed with a **report-only** parent scan — never a new
mask and never a new `slot_constraint` — gated on the same complete-mode condition
Portage uses, so a consumer Portage does not list stays unreported.

**Tech Stack:** Rust 2024 workspace (`crates/portage-repo/src/resolve/`), Portage
3.0.83 (`3rdparty/portage`), pmtest contract suite + L0 bed.

**Spec:** `LLM/backlog-tasks.md` #573, corrected by
`LLM/evidence/2026-10-28-573/REPORT.md` §1 (the item's "27 real-only lines" half no
longer reproduces; only `dev-lang_perl` is left — 67 real-only rows today against
Portage's 69-row block, so 69 is the target and 67 is the gap).

## Global Constraints

- Expected values come from Portage, never from a second portuale run: the
  captures in `LLM/evidence/2026-10-28-573/captures/` for the hermetic cells,
  `../pmtest/differential-test-bed/logs/l0-20261028T083834Z` for the bed. Mind
  the streams: Portage writes the skip block to stderr, portuale to stdout
  (`portuale-p569prov.stderr` is empty), so a capture `diff` compares the same
  stream or a concatenation, and the Rust tests assert `result.skipped_updates`,
  never capture text.
- **The merge list does not move.** This slice changes what is *reported*, never
  what is resolved: `result.entries`, `slot_constraints`, the mask set's
  membership (`!=cpv` negatives), backtrack counts and rc stay byte-identical on
  every probe. If a step needs a new mask to make a row appear, the design is wrong.
- The nine probes that are already set-identical (`dev-perl_XML-Parser`,
  `virtual_perl-ExtUtils-MakeMaker`, `dev-lang_ruby`, `dev-ruby_rake`,
  `kde-apps_kdecore-meta`, `app-text_texlive`, `app-text_texlive-core`,
  `dev-texlive_texlive-latex`, `dev-db_mariadb`) stay set-identical: no new rows,
  no lost rows. `#569`'s demotion filter (`or_branch_vdb_deselected_set`) and
  `#567`'s leak filter must be reused, not bypassed — a naive vdb scan re-adds
  the `=5.38*`/`=5.40*` rows that slice removed.
- Row order: Portage's intra-block order is hash order (`_parent_atoms` is a
  `set`); portuale's is deterministic (sorted). The comparator compares
  skipped-update lists as sets, so order stays out of scope.
- Determinism: no hash iteration order may decide *which* consumer's rows appear
  (today it is sort order — keep it).
- Pair branch: open `slice/573-built-pin-parents` in portuale and pmtest from
  the current `slice/572-or-reverify` tip (the tree the probes were captured
  on; #572 has not landed, so branching from `main` would lose both its resolver
  work and the fixtures). Commits paired, pmtest first; corpus bless only in
  pmtest after reviewing drift row by row.
- Merge-path safety gate: `resolve/` only, so no glibc/bash reinstall. Re-check
  the file list before claiming this at close-out.
- `#572` (`slice/572-or-reverify`) touches `resolve/enqueue*`; this slice touches
  `resolve/rev_dep.rs`, `resolve/feedback/*`, `resolve/backtrack_params.rs`,
  `resolve/withhold.rs`, and `resolve/ctx.rs` (Task 3 Step 0, only if that step
  shows the gate false). Rebase onto `main` before Task 4 if #572 lands first.

## Review Focus

- A consumer Portage does **not** list must stay unlisted: the hermetic cell's
  `dev-libs/p573real-1` (out-of-graph, built atom only, no complete-mode trigger)
  is absent from both blocks today (`captures/*-p569prov`). A "scan all installed
  consumers" implementation adds it and is a regression — pin it.
- A second consumer with the *same* atom must add rows, not replace the first:
  `p573virt-1` + `p573world-1` must both appear, **four** rows (each consumer's
  built + menu atom). The S0 capture has three only because `p573virt-2` is
  visible: Portage's search then rebuilds `p573virt-1`, and a rebuilt parent's
  built slot-operator component is discarded (`depgraph.py:2509-2517`). Verified
  on host Portage 3.0.83 — with `=dev-libs/p573virt-2` masked the block prints
  four rows. Task 1 Step 1 drops `p573virt-2` so the oracle is the clean
  four-row set, and records the three-row shape in `LLM/portage-quirks.md`.
- The withhold renderer (`constraint_withheld_updates`) has the same
  `(cp, atom)` dedup as the mask renderer; fixing one and not the other leaves
  the settle-path shape broken.
- `--exclude` and consumers being removed/replaced in the same run must not gain
  rows: reuse `consumer_in_scope`'s exclusions, widen only the reachability half.
- A consumer whose only atom is satisfied by the kept installed instance (the
  `dev-lang/perl:0/5.42=` rows against `perl-5.42.2 R`) is still a parent — do
  not filter rows by "the merge breaks it".
- `--quiet` (without `--debug`) still suppresses the slot-conflict block
  entirely; the new parent set must not leak through another renderer (existing
  pinned contract tests cover this — re-run them, do not add a new cell).

---

### Task 1: S0 — fixtures, the complete-mode cell, the RED pin

**Files:**
- Keep (pmtest, through the `fixtures/` symlink): `fixtures/repo/dev-libs/p573real/`,
  `fixtures/repo/dev-libs/p573virt/p573virt-1.ebuild`,
  `fixtures/repo/dev-libs/p573world/` (+ their
  `fixtures/repo/metadata/md5-cache/` entries) — already in the working tree
  from the S0 probe.
- Delete (pmtest): `fixtures/repo/dev-libs/p573virt/p573virt-2.ebuild` and
  `fixtures/repo/metadata/md5-cache/dev-libs/p573virt-2`.
- Create (pmtest): `fixtures/repo/dev-libs/p573up/p573up-{1,2}.ebuild` (+
  md5-cache entries) — the complete-mode cell's world member: installed `-1`,
  tree `-2` requiring `=dev-libs/p569prov-5.44*`.
- Modify: `LLM/evidence/2026-10-28-573/{REPORT.md,probe-p573.sh}` (add the
  complete-mode cell and its captures; correct §2a's row note),
  `LLM/portage-quirks.md` (the rebuilt-parent row).
- Create: `crates/portage-repo/src/tests/tests_p573.rs` (+ `mod tests_p573;` in
  `crates/portage-repo/src/tests/mod.rs`).

**Interfaces:**
- Consumes: `tests_consumer_pin_conflict::{hermetic_root, resolve_in}`.
- Produces: the fixture family, the RED tests, the four-row oracle for the
  in-graph cell, `LLM/portage-quirks.md`'s rebuilt-parent entry, and a decision
  record (`LLM/evidence/2026-10-28-573/REPORT.md` §2b) naming the condition under
  which Portage lists an out-of-graph consumer.

- [ ] **Step 1: Keep the repo-side fixtures, drop `p573virt-2`, validate metadata**

`p573virt-2` (a tree upgrade to `=dev-libs/p569prov-5.44*`) makes Portage's
search rebuild `p573virt-1`; a parent the graph rebuilds loses its built
slot-operator component (`depgraph.py:2509-2517`), so the cell prints three rows
instead of four. The cell is about two *installed* consumers of the same atom,
so delete `p573virt-2.ebuild` and its md5-cache entry.

Verified on host Portage 3.0.83 against the S0 scratch ROOT: with `p573virt-2`
visible, three rows (virt menu only); with `=dev-libs/p573virt-2` in
`package.mask`, four rows (virt built + menu). Record the three-row shape in
`LLM/portage-quirks.md`: what it does, the source cite, why it looks unintended
(the block drops a row for a package the settled merge list never rebuilds), and
that portuale does not copy it.

Then run

```bash
python3 ../pmtest/scripts/md5_cache_audit.py
```

Expected: no invalid `_md5_` entry for `p573*` and no entry for the deleted
`p573virt-2`. The installed vdb entries stay **out** of the shared fixture tree
— the probe and the Rust test build them into a scratch ROOT
(`probe-p573.sh` already does; the test does it in Step 3).

- [ ] **Step 2: Add the complete-mode cell to the probe**

The bed's out-of-graph rows appear only when Portage walks the installed vdb.
The cell must satisfy both conditions at once: (i) the provider candidate is
*withheld* into a skip block (not a slot-conflict abort), and (ii) an installed
package changes version in the same graph, so `_complete_graph` walks the vdb.
`p573pull-3/4` satisfied (ii) but aborted (`REPORT.md` §3); the bare-provider
arg satisfies (i) but does not walk (`captures/*-p569prov`). Candidate: a world
member `p573up-1` (installed) with a tree `p573up-2` that requires
`=dev-libs/p569prov-5.44*`, while `p573virt-1`/`p573world-1` still pin 5.42 —
so the provider update is withheld and `p573up` upgrades in the same graph.
That upgrade must be a **settled** `Upgrade` entry, not a transient one
backtracked away: `portage_complete` (`ctx.rs:226-238`) reads `pass.entries`
through `complete_graph_auto_enable` (`complete_graph.rs:29-52`), so only a
surviving version change turns portuale's gate on. Accept the cell only when
Portage's `--debug` output shows the walk visiting the vdb (a `slot operator
dependencies:` block naming `dev-libs/p573real-1`, or
`Parent: (dev-libs/p573real-1 …, installed)`) **and** portuale's own `--debug`
shows `dev-libs/p573real-1` as a walk-reached installed node.

```bash
bash LLM/evidence/2026-10-28-573/probe-p573.sh
```

Expected: the new cell's `portage-*.stderr` lists `dev-libs/p573real-1`; record
the exact trigger in `REPORT.md` §2b (which merge node, which condition, and
whether portuale's `portage_complete()` sees it). If no shape hits both
conditions after two attempts, stop and say so: Task 3 is then **dropped**, not
half-shipped — the backlog entry stays OPEN with the trigger question and Task 4
is not run.

- [ ] **Step 3: Write the two RED tests**

In `tests_p573.rs`, build the ROOT without touching the shared vdb:

```rust
fn p573_root() -> TempDir {
    let base = hermetic_root(&["dev-libs/p573virt", "dev-libs/p573world"]);
    let vdb = |cpv: &str, rdepend: &str| { /* write CATEGORY/EAPI/SLOT/repository/RDEPEND
        under base.join("var/db/pkg").join(cpv) exactly as probe-p573.sh's vdb() */ };
    vdb("dev-libs/p573world-1", "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=");
    vdb("dev-libs/p573virt-1",  "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=");
    vdb("dev-libs/p573real-1",  "dev-libs/p569prov:0/5.42=");
    base
}
```

- `two_consumers_sharing_a_pin_atom_both_render`: `resolve_in(&p573_root(), &["dev-libs/p569prov"])`;
  assert the `(atom, consumer_cpv)` set of `result.skipped_updates` equals
  Portage's **four** rows for the `p573virt-2`-masked cell —
  `(dev-libs/p569prov:0/5.42=, p573world-1)`,
  `(=dev-libs/p569prov-5.42*, p573world-1)`,
  `(dev-libs/p569prov:0/5.42=, p573virt-1)`,
  `(=dev-libs/p569prov-5.42*, p573virt-1)` — and that
  `dev-libs/p573real-1` is absent. Re-capture
  `captures/portage-p569prov.*` with the masked config and paste the four-row
  block into `REPORT.md` §2a.
- `out_of_graph_consumer_is_reported_when_complete_mode_walks_the_vdb`: the
  Step-2 cell — `hermetic_root(&["dev-libs/p573up", "dev-libs/p573virt",
  "dev-libs/p573world"])`, the same three vdb entries plus
  `vdb("dev-libs/p573up-1", "=dev-libs/p569prov-5.44* dev-libs/p569prov:0/5.42=")`
  — asserting `p573real-1` **is** present and the merge list is unchanged from
  the pre-change run (capture the expected `entries` from the probe, not from a
  portuale run).

  It cannot use `resolve_in` unchanged: `ResolveRequest::new` leaves
  `config.complete_seed_atoms` empty (the CLI fills it from
  `@selected`/`@system`/world in `portuale/src/pretend/resolve_plan.rs:304`), so
  `world_reachable` is empty and Task 3's clause 4 can never fire. Build the
  request in the test (or add a `complete_request` helper beside `request_in`)
  with `complete_seed_atoms` naming those three world atoms.

- [ ] **Step 4: Confirm the RED**

```bash
cargo test --release -p portage-repo tests_p573
```

Expected: `two_consumers_…` FAILS with only `p573virt-1`'s two rows (the
`(cp, atom)` dedup) against the four-row oracle; the complete-mode test FAILS
with `p573real-1` absent.

### Task 2: A mask's parents are per consumer, not per `(cp, atom)`

**Files:**
- Modify: `crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs`
  (`feed_reverse_dependency_pins`: the mask-creation latch and the `MaskEntry`
  push)
- Modify: `crates/portage-repo/src/resolve/backtrack_params.rs` (the new
  per-consumer parent set)
- Modify: `crates/portage-repo/src/resolve/withhold.rs`
  (`constraint_withheld_updates`, the `seen: HashSet<((String, String), String)>`)
- Test: `crates/portage-repo/src/tests/tests_p573.rs`,
  `crates/portage-repo/src/tests/feedback.rs:328`,
  `crates/portage-repo/src/tests/backtracker.rs:274`

**Interfaces:**
- Consumes: the Task-1 fixtures and RED tests.
- Produces: no new public item; `ReverseScan` keeps its shape (`enforced`,
  `dropped`), masks keep their keys (`(cp, neg)`).
  `BacktrackParams.reverse_dep_masked` keeps `(cp, atom)`: it is both the
  mask-creation latch and `consumer_pin_conflict_may_feed_back`'s predicate
  (`reverse_dep_pins.rs:99-104`), and its doc comment promises loop
  convergence (`backtrack_params.rs:288-292`). A new
  `reverse_dep_conflict_parents: HashSet<(cp, atom, consumer)>` tracks which
  consumer's parent rows are already in the mask.

- [ ] **Step 1: Give each consumer its own parent rows, without touching the latch**

Two different jobs live in one set today, and only the second changes:

1. `grown.reverse_dep_masked.insert((pin.cp, pin.atom))`
   (`reverse_dep_pins.rs:303`) is the **latch**: it runs the mask-creation loop
   once per `(cp, atom)` and keeps `slot_constraints` from growing. It stays
   keyed `(cp, atom)`, so `consumer_pin_conflict_may_feed_back`'s
   `.contains(&(p.cp, p.atom))` (`:99-104`) keeps compiling and keeps its
   meaning.
2. **Parent bookkeeping** gets the consumer: add
   `reverse_dep_conflict_parents: HashSet<(cp, atom, consumer)>` to
   `BacktrackParams`. Inside the existing
   `if grown.reverse_dep_masked.insert(…)` block, when the pin's `(cp, neg)`
   `MaskEntry` already exists (`bucket.iter_mut().find(|m| m.neg == neg)`) and
   this consumer's triple is new, append its `(consumer, built atom)`/
   `(consumer, atom)` pairs to that entry's
   `MaskReason::SlotConflict { parents }` instead of skipping the pin. Re-sort
   and dedupe `parents` after the append: `MaskReason` is compared in tests, and
   `built_render` is only sorted within one pin (`rev_dep.rs:520`).

In `constraint_withheld_updates` (`withhold.rs:35-39`) the `seen` set is not a
latch — nothing else reads it — so its key becomes `(cp, atom, consumer)`.

- [ ] **Step 2: Run the pin tests**

```bash
cargo test --release -p portage-repo tests_p573
cargo test --release -p portage-repo tests_p563
cargo test --release -p portage-repo tests_consumer_pin_conflict
```

(One `TESTNAME` per invocation — `cargo test` takes a single positional
filter.)

Expected: `two_consumers_…` PASSES with the four rows; the `p569*`/`p563*` pins
and the `#580` consumer-pin cells are unchanged.

- [ ] **Step 3: Pin the same fix in the withhold renderer**

Add a unit test beside the existing `constraint_withheld_updates` tests: two
`RevDepPin`s with the same `(cp, atom)` and *different* consumers, one settled
`AlreadyInstalled` entry; assert two `SkippedUpdate` rows, one per consumer
(failing before this task's `seen`-key change). This is the settle-path shape the
bed's MULTI probes use, so it must not regress while the mask path is fixed.

- [ ] **Step 4: Re-run the hermetic probe and diff against the captures**

```bash
PORTUALE_BIN=target/release/portuale bash LLM/evidence/2026-10-28-573/probe-p573.sh
```

Expected: `portuale-p569prov` now prints Portage's four rows (compare its
stdout block against Portage's stderr block, or concatenate both streams);
`portuale-p569pull` is byte-identical to its pre-change capture up to the
deterministic row order.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "resolver: give every skipped-update consumer its own parent rows (#573)"
```

(pmtest half first if the fixtures changed.)

### Task 3: Report-only parents for the complete-mode consumers

**Files:**
- Modify: `crates/portage-repo/src/resolve/rev_dep.rs`
  (`ReverseDependencyConstraintsArgs`, `ConstraintsFromConsumerArgs`,
  `consumer_in_scope` split into hard exclusions + reachability, the extra-pin
  bucket)
- Modify: `crates/portage-repo/src/resolve/feedback/reverse_dep_pins.rs`
  (`ReverseScan`, `scan_reverse_dependencies`, `feed_reverse_dependency_pins`)
- Modify: `crates/portage-repo/src/resolve/ctx.rs` **only if Step 0 shows the
  gate false** (`portage_complete`)
- Test: `crates/portage-repo/src/tests/tests_p573.rs`

**Interfaces:**
- Consumes: Task 2's per-consumer parent lists; Task 1's decision record for the
  complete-mode condition.
- Produces: `ReverseScan.report: Vec<RevDepPin>` and a
  `report: &mut Vec<RevDepPin>` field on `ConstraintsFromConsumerArgs`.
  Report-only pins never enter `enforced`/`dropped`, `slot_constraints`, or a
  mask's *membership*. A pin enters `report` iff **all four** hold:
  1. it passes every **hard exclusion** — being replaced, being removed,
     `--exclude`, and the no-op-top-level rule (`consumer_in_scope`'s first,
     third and fourth arms, `rev_dep.rs:566-587`);
  2. it failed **only** the reachability arm (`!reachable && !graph_cps`);
  3. `portage_complete` is true for the pass;
  4. the consumer is in the required-set closure — `scope.world_reachable`
     contains its cp (`ctx.rs:285-290`, seeded from `@selected`/`@system`/world
     via `complete_seed_atoms`). This clause is what keeps `dev-libs/p573real-1`
     unlisted.

- [ ] **Step 0: Prove the complete-mode gate fires on the mask-creating pass**

`portage_complete` (`ctx.rs:226-238`) is evaluated over `pass.entries`, so it is
true only on a pass that still carries the withheld upgrade as an
`Upgrade`/`Downgrade`/slot-changing `Reinstall` (`complete_graph.rs:29-52`).
That is exactly the pass that creates the mask, so the design can hold — but it
is this task's load-bearing assumption, so prove it before writing code: on the
bed `dev-lang/perl` probe and on the Task-1 complete-mode cell, log
`portage_complete` and the report-pin count (a `--debug` line or a temporary
assertion) and paste both into `REPORT.md` §2b.

If the gate is false there, add Portage's second arm — `_resolve_conflicts`
forces complete mode on **any** slot conflict when backtracking is allowed
(`depgraph.py:9472-9480`), independent of the version-change arm — and add
`ctx.rs` to this task's edits.

- [ ] **Step 1: Split the scope gate and add the report bucket**

Split `consumer_in_scope` (`rev_dep.rs:564-587`) into
`consumer_hard_excluded` (being replaced, being removed, `--exclude`, no-op
top-level outside the world closure) and the reachability arms
(`reachable`/`graph_cps`). Then in `constraints_from_consumer`:

- `consumer_hard_excluded` → `ControlFlow::Break(())`, as today;
- `consumer_in_scope` → the existing enforced/dropped path;
- otherwise, if `scope.world_reachable` contains the consumer's cp **and**
  `portage_complete` → run the same per-atom analysis with `report_only = true`:
  push into `report` only, skip `rev_dep_pin_holdable`, and mark the pin so the
  `built_render` attachment loop covers it too.

Follow `consumer_or_demotions` exactly as the in-scope path does (that is what
keeps `#569`'s rows out). Read the `portage_complete` field for the gate, not
`or_scan.portage_complete`: the former is what Step 0 proved.

- [ ] **Step 2: Feed the bucket to the mask parents only**

In `feed_reverse_dependency_pins`, after a mask exists for `(cp, version)`,
append the `report` pins' `(consumer, atom)` pairs for the same cp and masked
version. Do **not** create a mask for a `report` pin, and do not push its atom
into `slot_constraints`.

Attach on the pass that creates the mask, never later: the latch accepts a
`(cp, atom)` once, and the settle pass returns early with an empty `upgrading`
(`rev_dep.rs:423-425`), so a report pin seen then has nowhere to go. Add a unit
test for the two-pass shape (mask created on pass 1, report pin seen on pass 1
only) so a later reorder cannot silently drop the rows.

Portage's rule, applied to the report set: `_conflict_missed_update[pkg]["slot
conflict"]` gets `(parent, atom)` for every parent atom that does not match the
skipped package (`depgraph.py:2107-2121`). The walk that makes those parents
graph nodes is `_complete_graph` (`:8605-8675`), self-enabled by
`complete_if_new_ver` (`:8628-8675`) and forced by `_resolve_conflicts`
(`:9472-9480`).

- [ ] **Step 3: Run the complete-mode test**

```bash
cargo test --release -p portage-repo tests_p573
```

Expected: both Task-1 tests PASS — the four-row cell and the complete-mode cell —
and `p573real-1` is present only in the complete-mode cell, whose Portage
capture lists it.

- [ ] **Step 4: Prove the resolver did not move**

```bash
cargo test --release -p portage-repo
```

Expected: every existing pin green; no test outside `tests_p573` changes its
`entries`, `slot_constraints`, backtrack count or rc.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "resolver: report the missed-update parents the complete walk reaches (#573)"
```

### Task 4: Bed re-grade, yaml tightening, docs

**Files:**
- Modify (pmtest): `differential-test-bed/compare/known-divergences.yaml`
  (delete `perl-544-stabilization-built-pin-residual`; drop its `see …`
  reference from `perl-544-stabilization-slot-conflict-a`'s `reason`, line ~262)
- Modify (pmtest): `pytests-contract-suite/test_emerge_pretend_contract.py`
  (a pinned skipped-update block over the `p573*` fixtures, modelled on
  `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`)
  and its `CASES` list
- Modify: `LLM/backlog-tasks.md` (#573 → DONE with commit SHAs),
  `LLM/what-this-proves.md` (new paragraph), `LLM/code-notes/…` for the touched
  files (`resolve/rev_dep.md`, `resolve/feedback/reverse_dep_pins.md`,
  `resolve/backtrack_params.md`, `resolve/withhold.md` — as applicable)
- Create: `LLM/evidence/2026-10-28-573/bed-after/` (report excerpt + the two
  `dev-lang_perl` captures)

**Interfaces:**
- Consumes: Tasks 2–3 merged into the branch.
- Produces: a green gate and a closed backlog item.

- [ ] **Step 1: Pin the block in the contract suite, then run the full gate**

Add the `p573*` `CASES` row and a pinned-output function for the four-row block
(AGENTS.md rule 6; the `orbttool` test shows the shape). Write it before this
slice lands: it fails on `main`'s binary and passes after Task 2.

```bash
cargo fmt --check
cargo clippy --release --all-targets
cargo test --release
```

Then in `../pmtest`: `scripts/pytest-parallel.sh`.

Expected: zero clippy warnings, whole workspace green, contract suite green. A
corpus row may drift on the new cell — review it against Portage, then bless
with `PORTUALE_CORPUS_BLESS=1` in the pmtest commit, never here.

- [ ] **Step 2: Re-run L0 and re-grade**

```bash
differential-test-bed/run/l0-resolver.sh
```

Expected: `dev-lang_perl`'s skip block set-equal to Portage's 69 rows (48
consumers: 21 virtuals with built + menu, 27 real packages with the built atom
only), the nine already-green probes unchanged, and the
`perl-544-stabilization-built-pin-residual` finding gone. If a *new* slug's
skipped-updates row moves, stop: that is the `#569` filter leaking. **If Task 1
found no complete-mode cell, this step cannot pass** — stop and re-file the
out-of-graph half with the trigger question instead of grading the in-graph
half as done.

- [ ] **Step 3: Delete the yaml entry and re-grade again**

Only after Step 2 is green: remove `perl-544-stabilization-built-pin-residual`
from `known-divergences.yaml` and drop the `see
perl-544-stabilization-built-pin-residual` clause from
`perl-544-stabilization-slot-conflict-a`'s `reason` (line ~262), then rerun
`compare/resolve-compare.py` on the Step-2 log. Expected: the finding stays gone
(it was masking a real match, not a real divergence); every other yaml id still
fires where it did.

- [ ] **Step 4: Docs and close-out**

Append the `what-this-proves.md` paragraph with a runnable example (the probe
command and the four-row block), flip #573 to DONE in `LLM/backlog-tasks.md`,
run `python3 LLM/tools/code_notes_index.py`, and update `REPORT.md` with the
final counts. If the plan file's own task list closes #573, flip this plan's
`Status:` header to `done <date>` in the same commit (AGENTS.md rule 11).

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "bed: #573 closed — the skipped-update block carries every consumer Portage lists"
```
