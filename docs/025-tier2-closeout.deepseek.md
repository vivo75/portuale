# Tier 2 close-out — #17 + #26 + #27 — agent plan (deepseek draft)

Status: executing. A0 (record correction), A1 (effective
installed-metadata helper + built-`:=` append, gated default-off) and A2
(scheduler closure + `GraphEntry::deps` read the same view) landed
2026-09-12; A3 (L0 re-baseline, byte-identical: 96/0.800, order x22) and
A4 (installed-parent pullers + the `need_rebuild` pin) followed; B0 (the
committed trace harness) is in. See §11 for the findings. Written
2026-09-12 against `main` @ `be76d0d` (post #24 S7). Companion detail for
#26/#27 lives in `docs/024-dynamic-deps_n_disagreement.deepseek.md`
(referred to below as **the 024 plan**); this document is the combined
execution plan that also covers **#17** (`docs/backlog-tasks.md:38`),
which had no plan doc until now.

**Read first:** `AGENTS.md` (step 8; step 9's commit rules), `docs/agent-context.md`,
`docs/024-slot-operator-plan.md` §2 (rules/invariants), `docs/024-oracle.md`
(the oracle method), `TEST/README.md`, `TEST/findings/l0.md` §I
(`:314-698`, the cluster-I record) and `:921-998` (the last three L0
runs), the memory note
`/home/vivo/.claude/projects/-home-vivo-repo-PORTUALE-portuale/memory/cluster-i-merge-order.md`,
and `rust/portage-repo/src/merge_order.rs`'s module doc (`:1-39`).

Real portage = `3.0.82.2`, running in `localhost/test-portuale:latest`
(present locally) for L0, and the host tree at
`/usr/lib/python3.14/site-packages/{_emerge,portage}` for source reading.
Every line number drifts; re-locate by function name first.

Model tiers, same convention as #22/#23/#24:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion — what "close" means for each, and whether it is doable

### 0.1 Verdict per item

| Item | Current state | What closing means | Confidence | Effort |
|---|---|---|---|---|
| **#26** | backlog line is **false** (verified: the CLI does switch the walk source; §0.1 of the 024 plan) | delete the false claim (S0); land the real gap as the installed-metadata overlay (A1/A2) | **high** — the claim is disproven, the overlay has a one-function oracle (`FakeVartree._apply_dynamic_deps`) | ½ h doc + 6-10 h overlay |
| **#27** | renderer landed, dormant; blocked on "the vdb walk" | overlay **plus** installed-parent `slot_pullers` recording (the actual blocker), then the fixture with all three reasons + `--usepkgonly` silence | **high** the trailer can be made reachable; **medium** all three reasons fit one fixture | 4-8 h |
| **#17** | 120 probes, 96 clean, parity 0.800; **22 `order` probes**; 12 slices shipped 2026-09-09 | either all 22 byte-identical, or every residue adjudicated against real with a `known-divergences.yaml` entry (Gate 0.1) | **doable as a bounded investigation; not promiseable as 22/22 in one pass** | 35-60 agent-hours across 4-6 sittings |

**All three together: doable, but #17 is a research item with a fixed
time-box, and its outcome is decided by what the trace finds.** The plan
below makes #26/#27 the foundation (they are narrow and high-confidence)
and #17 the second phase, with an explicit stop rule. Do not sell #17 as
"22 fixes in N days" — the last three deep dives each ended in a
reverted instrumentation patch and a root cause one level further down.
What is genuinely new this time is committing the trace harness (B0)
instead of re-deriving it per session, and re-baselining L0 after A2
because the overlay changes the very graph edges cluster I runs on.

### 0.2 The coupling that dictates the order

```
#26 overlay ──> merge_order::add_installed_dependency_closure  ──> #17 graph edges
     │                    (site 14: Effective vs Raw)
     └──> enqueue_dependencies ──> slot_pullers recording ──> #27 fixture
```

- `add_installed_dependency_closure` (`merge_order.rs:935`) reads the
  **raw vdb** today and compensates for `doebuild._inject_libc_dep`'s
  phantom `>=libc` atom with `is_injected_libc` (`:971-1000`, cluster I
  slice 10, commit `264163f`). Real's scheduler graph is the
  **overlaid** (`Effective`) metadata, where that atom does not exist —
  which is *why* the strip was needed. Landing #26's overlay before #17
  removes the phantom at the source; the strip should then become dead
  on the Effective path and stay live on Raw (Gate 0.3).
- Therefore: **run full L0 after A1/A2 and re-file the cluster-I probe
  list before B1.** The 22-probe baseline in `l0.md` is the pre-overlay
  baseline; A2 may move some positions (in either direction) and the
  B-phase acceptance must be computed against the post-A baseline.
- #27 needs *both* the overlay (so the default mode sees the vdb's
  built `:=` atoms) **and** puller recording (so an installed parent can
  appear in a slot-conflict notice at all). The sibling draft
  `024-dynamic-deps_n_disagreement.opus.md` assumes the overlay alone
  unblocks #27; it does not (024 plan §0.3.2).

### 0.3 Adjacent hygiene (do it in B6, do not expand the plan)

- `docs/backlog-tasks.md:39` (#18 `_FrontierDigraph` perf layer) is
  **stale**: `merge_order.rs::SerializeFrontier` shipped 2026-09-10
  (`scope-backlog.md:200-205`). Flip to DONE.
- `scope-backlog.md:724-726` says "L0 merge-order: ~19 probes" while
  the last run's report has 22 `order` probes. Reconcile the wording.
- `docs/what-this-proves.md:15973-15975` and
  `docs/scope-backlog.md:134-137` carry the false #26 claim; the former
  is history (append-only), the latter is live and must be corrected.

---

## 1. Ground truth

### 1.1 #26 / #27 (one page; detail in the 024 plan)

- `--dynamic-deps=n` **does** switch the `--deep` walk source in both
  languages (`lib.rs:19649-19664` / `emerge_pretend_reference.py:14394-14399`):
  verified live on `changeddepspkg` plus the `--debug` trace (samepkg
  queued, not newpkg). The unit test and CLI agree.
- Real's **default** is the union: live ebuild deps + the vdb's own
  `slot_operator_built` (`:=` with a sub-slot) atoms appended
  (`_emerge/FakeVartree.py:146-191`, `portage/dep/_slot_operator.py:24-38`;
  `--ignore-built-slot-operator-deps` suppresses only the append).
- Portuale's `GraphEntry::deps` for `AlreadyInstalled` is always the
  ebuild (`lib.rs:17125-17162`, assigned `:17302`) — the `--debug`
  `Depstring` can contradict the walked `Child` under `=n`.
- `enqueue_dependencies` records **no** `slot_pullers` (signature
  `:19589-19611`); the only writers are `:16531-16537` (Argument) and
  `:18432-18442` (merge-bound candidates). Python same: `12067`,
  `13518`. `slot_conflict_need_rebuild` (`pretend.rs:7368`) scans
  `c.parents`, so an installed parent can never appear. This — not the
  overlay — is why #27's trailer stays dormant.
- The upstream oracle for the overlay default is
  `test_changed_deps.py::testChangedDeps` cases 1-6 (024 plan §1.4,
  including the "only case 6 actually runs upstream" trap).

### 1.2 #17 — the 22 `order` probes and their levers

Authoritative list: `TEST/logs/l0-20260912T120635Z/l0-report.json`
(`category == "order"`); grouped narrative: `TEST/findings/l0.md:611-698`.

| Lever | Probes | What is known | Tier |
|---|---|---|---|
| **1. installed-chain drain timing** | gtk:4, gtk+:3, kdecore-meta, networkmanager, texlive-core, texlive-latex, libreoffice, gimp, wireshark, xorg-server, i3, ruby, rake, vlc (**14**) | The digraph around `pyproject-metadata` was verified **byte-identical** to real (nodes/edges/priorities) and `_create_graph` discovery order identical; real defers an installed python-build-tool chain a few picks longer, portuale drains it ~7 picks early. Needs per-node drain tracing across ~400 nodes. | **F** |
| **2. multi-slot interleave** | firefox, thunderbird (**2**) | Divergence at `#29`: real groups `clang-*-config:21` before `:22`; portuale interleaves. Slice 11 fixed the `||` phantom edges; this is the remaining multi-slot bias/discovery detail. | M + F review |
| **3. emptytree tie-break** | `_system`, `_world`, `MULTI_emptytree-system` (**3**) | First divergence `#6`/`#8`/`#13`: `virtual/os-headers` vs `sys-devel/patch` transposed + `findutils` slip; `mime-types`/`mpdecimal`/`gentoo-common` front-loaded before the `acct-group/*` run. `seed_toolchain_asap` gates were already fixed (slice 5). | M + F review |
| **4. superseded installed in-edges** | gedit, nautilus (**2**) | `nghttp2 → systemd (merge, buildtime)` edge survives where real's scheduler has `nghttp2 → virtual/pkgconfig` only: real drops an installed node's in-edges when a same-slot merge supersedes it, and draws no edge for an initially-satisfied dep — **walk-order dependent**. This is the recorded resolver-architecture gap, adjacent to #25. | **F** |
| **5. cluster-A bundle artefact** | gnome-shell (**1**) | `order #0` inside the recorded cluster-A bundle (real abandons at the samba `show_missing_use` failure, portuale completes). Adjudicate; almost certainly not #17's mechanism. | S |

Already proven and not to be re-litigated: the 12 shipped slices
(`TEST/findings/l0.md:338-609`), including the injected-libc fix
(`264163f`) that moved the whole class from `#7`-`#13` to `#22`-`#59`,
and the two slice-6 edge narrowings. The module doc's claim that
`SerializeFrontier` is behaviour-neutral is unit-pinned — do not re-open
it without a counter-example.

### 1.3 The instrumentation that keeps getting reverted

Three sessions independently prototyped and reverted the same tools:

- real side: a container patch injecting an `RT_SEL` stderr line after
  the per-iteration state reset in real's `_serialize_tasks`, plus
  `RT_STUCK` (`/out/real-trace.py`).
- portuale side: an env-gated `PORTUALE_MO_SEL` eprintln in
  `select_nodes` (`merge_order.rs:1973`) dumping retlist length, alive
  count, `asap` length, `drop_satisfied`, method, used `Ignore`, and the
  selected node(s).
- A watchdog set for specific nodes (live-children-of-a-watch-set) was
  used for `pyproject-metadata` and reverted.

**B0 commits these under `TEST/scripts/mo-trace/` and never reverts
them again.** They are the actual deliverable that makes #17 tractable.

### 1.4 L0 infrastructure (already working)

- `TEST/run/l0-resolver.sh` — 120 probes from
  `TEST/atomlists/l0-resolve.txt`, `emerge -pv` under real and portuale
  in a throwaway container, diffed by `TEST/compare/resolve-compare.py`.
  The image exists locally; a run is minutes-to-an-hour, not seconds.
- Baseline: `TEST/logs/l0-20260912T120635Z/` — 120 probes, 96 clean,
  parity 0.800, `order ×22`, `truncated ×2`, advice/error ×8, the
  gnome-shell cluster-A bundle, and the environmental
  `MULTI_emptytree-system` portage-version skew.
- `known-divergences.yaml` is **empty**; every divergence is currently
  an unexplained finding. Comparator categories and the order check:
  `TEST/compare/resolve-compare.py:308-330`.
- `PYTHONHASHSEED=0` is pinned for real in `in-container.sh` (commit
  `6a4bffe`) because real's `_serialize_tasks` iterates plain sets; do
  not unpin it.

---

## 2. Rules and invariants for every slice

Reuse `docs/024-slot-operator-plan.md` §2 verbatim. Added:

1. **Byte parity is the bar.** A comparator tweak may *classify* a
   finding (e.g. "adjacent transposition"), never explain one. Only
   `known-divergences.yaml` entries with a real-side oracle
   (nondeterminism, real bug with ticket, deliberate portuale cut) may
   turn a run green.
2. **Instrument once, commit it.** No slice may revert the trace
   harness to make a diff smaller.
3. **One lever per commit**, with a full L0 run and a
   `TEST/findings/l0.md` update after each. A lever that needs a second
   L0 to show its effect must say so in the commit body.
4. **Oracle from the real container**, not from fixtures: real's actual
   merge list / `RT_SEL` trace is the expected value; fixtures only
   guard regressions.
5. **No re-tuning `merge_order.rs` to move a probe.** Fix the
   mechanism; if the mechanism is a documented cut, say so at Gate 0
   rather than adding a heuristic.
6. **Dual-language, empirically diffed** (`merge_order.rs` +
   `emerge_pretend_reference.py`), per AGENTS.md step 4.
7. **Time-box each investigation lever at two rounds.** Round 1: trace
   to a specific node/edge/priority difference. Round 2: fix or
   adjudicate. Do not open a third round without the owner.
8. **Surface, don't chase.** Anything outside #17/#26/#27 becomes a
   finding with an oracle (e.g. the `gedit`/`nautilus` shape may be #25).
9. Only `git commit`/`push` when the user asks (AGENTS.md step 9).

---

## 3. Slices

### Phase A — #26 + #27 foundation

#### A0 — Correct the #26 record (S, ½ h)
As 024 plan S0: retitle `docs/backlog-tasks.md:47`, replace the
`scope-backlog.md:134-137` clause, leave `what-this-proves.md` history
alone. Acceptance: the §0.1 transcript from the 024 plan re-run; no
"both languages walk ebuild deps" sentence left in live docs.

#### A1 — Effective installed-metadata helper + built-`:=` append (M write, F review, 4-6 h)
Exactly 024 plan S3: `InstalledMetaLayer::{Raw, Effective}` +
`installed_dep_string(...)` in both languages; `Effective` == `Raw`
under `--dynamic-deps=n`; `Effective` = live md5-cache value + appended
sub-slot `:=` atoms (vdb USE, `--ignore-built-slot-operator-deps`
gate); memo per pass; not-applicable fallbacks (missing ebuild/EAPI,
global updates) per Gate 0.3. Convert `enqueue_dependencies` only.
Acceptance: the S2 append pin flips; `=n` null-transformation script
byte-identical; perf delta on the `-puD` baseline reported.

#### A2 — Order-affecting consumers: scheduler closure + entry deps (M write, F review, 3-5 h)
Two commits:

1. `add_installed_dependency_closure` (`merge_order.rs:935`) reads the
   `Effective` layer on the default path. Decide `is_injected_libc`'s
   fate explicitly (Gate 0.3): it should become dead on the Effective
   path (the atom is not in the ebuild) and remain for `Raw` callers.
2. `GraphEntry::deps` for `AlreadyInstalled` (`lib.rs:17120-17162`)
   built through the same helper, so `--debug`/`--tree`/`--json` match
   the walked child under `=n`. Display-only; pin a `--debug` check on
   `changeddepspkg --dynamic-deps=n`.

Acceptance: unit tests both languages; `=n` byte-identical; one full
L0 run archived (this is the new cluster-I baseline, A3).

#### A3 — L0 re-baseline and re-file (M, 2 h)
Run `TEST/run/l0-resolver.sh`; archive to `TEST/logs/`; update
`TEST/findings/l0.md` with the post-A `order` probe list and any moved
first-divergence positions. **This list, not the 2026-09-09 one, is
#17's starting line.** If A2 makes a probe clean, that is a valid #17
win; if it regresses one, it is an A2 bug — adjudicate before B1.

#### A4 — Pullers + `need_rebuild` fixture (#27 closure) (M write, F review, 2-3 h)
As 024 plan S5: record `slot_pullers` from `enqueue_dependencies`'
flattened atoms (mirror `18432-18442`), build the fixture with the
three reasons (`--exclude`, `--useoldpkg-atoms`, masked/unavailable)
and the `--usepkgonly` silence — or, if A3's graph already makes a
Reinstall-parent path fire, shrink to fixture + regression pin. If the
trailer still cannot fire, **stop and report why**; close #27 either
as DONE or as re-scoped with the finding. Diff every moved slot-conflict
pin against the clean-`main` baseline by name.

### Phase B — #17 merge-order

#### B0 — Commit the trace harness (M, 4-6 h)
Create `TEST/scripts/mo-trace/`:

- `real-trace.py` — inject the `RT_SEL`/`RT_STUCK` stderr lines into
  the container's real `_serialize_tasks` (reconstruct from the memory
  note; the previous version lives in the git history of
  `/out/`—rebuild from the description, do not trust `/out` surviving);
- `ptl-trace.sh` (or an env doc) — `PORTUALE_MO_SEL=1` handling; the
  Rust trace itself is added behind `cfg(debug_assertions)`-free
  env-gating in `select_nodes`;
- `align-traces.py` — aligns real and portuale traces by merge index
  and prints the first iteration where the leaf sets differ, with the
  watch-set live children at that iteration.

Acceptance: on a probe that is already byte-clean (`sys-apps/systemd`),
the aligned traces agree through the whole list; on `gtk:4` they diverge
exactly at the known first position. Commit the harness with its own
Rust unit test for the trace formatter (no behaviour change:
`PORTUALE_MO_SEL` unset must leave stdout/stderr byte-identical).

#### B1 — Lever 1: installed-chain drain timing (F, time-boxed 8-16 h)
Critical path, 14 of the 22 probes. Method:

1. Run the harness on `gtk:4`; find the first iteration where the
   `NORMAL`-range leaf sets differ.
2. For each differing node, dump its per-`Ignore` surviving-child count
   (`SerializeFrontier`'s own state) and the exact `DepPriority` of
   each blocking edge, and diff both sides.
3. Classify the difference: edge missing/spurious, priority class
   wrong, `satisfied` flag wrong, `optional` flag wrong, or a
   `_SerializeFrontier` query difference. Slice 10 proved
   `find_smallest_cycle`/`gather_deps` faithful; re-check only with a
   counter-example.
4. Reproduce in a minimal fixture if possible (installed chain +
   merge-bound set; the `dev-libs/slotorder*` family is the pattern);
   pin real's order from the container.
5. Fix, L0, update `l0.md`.

Round 2 only if round 1 names a mechanism. If after two rounds the
difference is a real-vs-portuale architecture gap (e.g. the
supersede/in-edge model of Lever 4), stop and route it there rather
than adding another special case.

#### B2 — Lever 2: multi-slot interleave (M write, F review, 4-6 h)
`firefox`/`thunderbird` `#29`. Compare real's `RT_SEL` around the
`clang-*-config:21`/`:22` picks with portuale's watch-set trace. Likely
in the bias/discovery ordering of multiple slots of one cp; extend the
`dev-libs/slotorder*` fixture or add a `slotconfig*` pair. Fix, L0.

#### B3 — Lever 3: emptytree tie-break (M write, F review, 3-5 h)
`_system`/`_world`/`MULTI_emptytree-system`. Trace `emerge -pe @system`
(skip the multi-environment probe) around the `virtual/os-headers` /
`sys-devel/patch` / `findutils` transposition. The slice-5
`seed_toolchain_asap` gates are already real-faithful; look at the
`_merge_order_bias` parent counts and the leaf-batch rule
(`select_nodes` `nodes.len() == 1 || (ig.is_none() && asap.is_empty())`,
`:2021`) before touching the seed. Note the `MULTI_emptytree-system`
probe also carries the known portage-version skew; adjudicate that half
first.

#### B4 — Lever 4: superseded installed in-edges (F, Gate 0.2, 8-14 h)
`gedit`/`nautilus`. Model real's two rules: an initially-satisfied dep
draws **no graph edge** (real `_select_package` picks the installed
package and adds it nomerge with no in-edge from the merge-bound
parent), and an installed node's in-edges are **dropped** when a
same-slot merge supersedes it. The walk-order dependence (`p11-kit` and
`at-spi2-core` keep their edges because they are walked after polkit)
means this must be modelled in the resolver's `slot_pullers`/edge
recording, not patched in `build_digraph`. If Gate 0.2 says this is
#25's item, file it there with this evidence and leave the two probes
as the named residue.

#### B5 — Lever 5: gnome-shell `#0` adjudication (S, 1 h)
Read the probe's full finding bundle: if the `order #0` is downstream
of the cluster-A truncation (real abandons, portuale completes), file it
under cluster A (`TEST/findings/l0.md:948-952`) and remove it from the
`order` count via `known-divergences.yaml` **only** with that real-side
evidence. Do not touch `merge_order.rs` for it.

#### B6 — Closure and hygiene (S, 2 h)
`what-this-proves.md` new paragraph (runnable L0 example + the overlay
fixture), `TEST/findings/l0.md` final table, `docs/backlog-tasks.md`
#17/#26/#27 (+#18 DONE), `docs/scope-backlog.md` §A wording and the
"~19 probes" figure, 024 plan docs status lines. Every
`known-divergences.yaml` entry added in B1-B5 carries `reason:`/`ticket:`
and its real-side evidence in `l0.md`.

---

## 4. Difficulty and routing

| Slice | Work | Tier | Why |
|---|---|---|---|
| A0 | record correction | **S** | mechanical |
| A1 | overlay helper + append | **M** + F review | spec'd by the 024 plan; the `=n` invariant is exact |
| A2 | scheduler closure + entry deps | **M** + F review | semantic per-site call; L0-bisected |
| A3 | L0 re-baseline | **M** | running + re-filing, not judgement-heavy if A2 is clean |
| A4 | pullers + #27 fixture | **M** + F review | conflict pins can move; fixture craft |
| B0 | trace harness | **M** | tooling; the trace format must be exact |
| B1 | Lever 1 — drain timing | **F** | hypothesis-driven debugging on a 400-node graph; the whole item |
| B2 | Lever 2 — multi-slot | **M** + F review | narrow once traced |
| B3 | Lever 3 — emptytree | **M** + F review | narrow once traced |
| B4 | Lever 4 — supersede model | **F** | resolver-architecture; may be #25 |
| B5 | gnome-shell adjudication | **S** | read + file |
| B6 | closure | **S** | |

**Can a cheaper model do it?** Phase A, yes — it is spec'd by the 024
plan (M with F review). Phase B is where cheap fails: B1 and B4 have no
local signal (the code compiles, the fixture suite passes, and a wrong
edge order only shows at real-tree scale), and an L0 delta looks like
noise until traced. B0/B2/B3/B5/B6 are safe for a mid model **after
B0's harness exists**. The one hard rule: do not start B1 before B0 is
committed.

Effort, frontier-hours of agent time: A0 ½, A1 4-6, A2 3-5, A3 2, A4
2-3; B0 4-6, B1 8-16 (two rounds), B2 4-6, B3 3-5, B4 8-14, B5 1, B6 2.
L0 runs are extra: assume 6-8 full runs × 1-2 h. **Combined ≈ 45-70 h
across 4-6 sittings.** If that is too much for one push, stop after A3
(#26/#27 close, #17 re-baselined) and file the B phase as its own plan.

---

## 5. Sequencing and gates

```
A0 ─ A1 ─ A2 ─ A3 ─ A4 ─┐
                        ├─ go? ─ B0 ─ B1 ─┬─ B2 ─┐
#27 closes at A4 ───────┘                  ├─ B3 ─┼─ B5 ─ B6
                                            └─ B4 ─┘
```

- A1 before A2 (A2's closure uses A1's helper). A3 before B1 (the
  baseline must be post-overlay). A4 is independent of A2/A3 and can
  run in parallel on another branch.
- "go?" = A3 archived and the post-A `order` list agreed; if A3 leaves
  `order 0`, close #17 there and skip Phase B.
- B1 before B2/B3/B4 only in the sense that its result may name their
  root cause; they are otherwise independent. B4's Gate 0.2 answer can
  move it to #25.
- Every B slice ends with L0 + `l0.md`; no exception.

---

## 6. Gate 0 — owner decisions (answer before A1)

| # | Question | Recommendation |
|---|---|---|
| G0.1 | **#17 acceptance bar.** (A) all 22 byte-identical, open-ended; (B) every remaining finding adjudicated with real-side evidence and an entry in `known-divergences.yaml`, time-boxed at two rounds per lever. | **B**, with A as the aspiration. #17 has consumed multiple sessions; a bounded, evidence-backed residue is a closer, a third "keep digging" round is not |
| G0.2 | Lever 4 (`gedit`/`nautilus` supersede model) in #17 or moved to #25 (`_complete_graph` installed nomerge nodes)? | Move to #25 with B4's evidence; #17 keeps only the order probes it can fix without the resolver-architecture change |
| G0.3 | `is_injected_libc` under the Effective layer: delete on that path and keep for Raw, or keep both paths identical for now? | Delete on Effective (it cannot fire: the ebuild never declared the atom); keep the Raw branch as-is. Pin both with unit tests |
| G0.4 | `Effective` for `slot_operator_eliminate_rebuilds` rule 8 (024 plan G0.4)? | Effective; real's `_eliminate_rebuilds` makes no `_raw_metadata` call |
| G0.5 | #27 puller recording in this plan or attached to #27 directly? | Either; the unit is the same and both must land for the trailer to fire |
| G0.6 | L0 pin stability: no container/image/atom-list/pin change between A3 and B6 without re-archiving the baseline? | Yes. Record the `fingerprint.tsv` hash in each run's commit body |
| G0.7 | Comparator policy: allow the report to *classify* adjacent transpositions, but never to explain them? | Yes; explanation only via `known-divergences.yaml` |

---

## 7. Definition of done

**#26** — false claim gone from live docs; overlay lands; `--dynamic-deps=n`
byte-identical to pre-slice on the fixture corpus (scripted); default
mode sees vdb built `:=` atoms (pinned); `--debug` no longer
self-contradicts.

**#27** — `need_rebuild` trailer fires end-to-end for the three reasons
plus the `--usepkgonly` silence, or the failure is filed with evidence
and #27 is re-scoped; every moved slot-conflict pin names its oracle;
`slot_pullers` from installed parents is pinned by a Rust unit test and
a contract case.

**#17** — Gate 0.1's bar met: every one of the 22 `order` probes is
either byte-identical or carries a `known-divergences.yaml` entry with
a real-side oracle; `TEST/findings/l0.md` has the final table and the
per-lever narrative; the trace harness is committed and documented;
the post-A baseline is archived; no `merge_order.rs` heuristic was
added without a fixture and an oracle.

**All** — full pass green (`cargo fmt --check`, clippy zero-warn,
`cargo test --release`, `pytest tests -q`), failing-test names vs the
clean-`main` baseline, L0 run archived with its fingerprint, docs
updated per AGENTS.md step 7, no commit/push unless asked.

---

## 8. Review checklist (attach to every slice PR)

- [ ] Gate: fmt / clippy zero-warn / `cargo test --release` / `pytest tests -q` green; failing-test names vs clean-`main`
- [ ] `--dynamic-deps=n` null-transformation script byte-identical (A1-A4)
- [ ] Rust == Python diffed empirically on `fixtures/` and every `_b1_root` case
- [ ] L0 archived (`fingerprint.tsv` unchanged) and `findings/l0.md` updated for the slice
- [ ] No comparator change explains a finding without a real-side oracle in `known-divergences.yaml`
- [ ] B0: trace harness has a unit test; with the env var unset, output is byte-identical
- [ ] B1-B4: the trace excerpt that names the mechanism is pasted into the commit body
- [ ] B1-B4: no heuristic added to `merge_order.rs` without a fixture whose expected order comes from the container
- [ ] A2: `GraphEntry::deps` and the walked child agree under `--debug`; `is_injected_libc` disposition recorded at its call site
- [ ] A4: installed-parent puller lines carry owner cpv + atom; conflict pins diffed by name
- [ ] Comments carrying real citations moved, not dropped
- [ ] New fixtures `git add`ed before any `git clean`
- [ ] Judgment calls surfaced, not defaulted
- [ ] No `git commit`/`push` unless the user asked

---

## 9. Risks

1. **#17 is open-ended.** The last three deep dives each ended one
   level lower. Gate 0.1's time-box is the mitigation; without it this
   plan can absorb arbitrarily many sessions.
2. **Overlay changes the #17 baseline.** A2 rewires exactly the edges
   cluster I runs on (`add_installed_dependency_closure`). A3 must run
   before B1; skipping it invalidates every B-phase comparison.
3. **`is_injected_libc` regression.** Removing it from the Effective
   path while leaving a Raw caller unfixed re-introduces the phantom
   `>=glibc` edge and the whole slice-9 front-load class (14 probes).
   Unit-test both layers.
4. **L0 environmental drift.** The image pin, `PYTHONHASHSEED=0`, and
   the atom list must stay fixed; `MULTI_emptytree-system` already
   carries a portage-version skew. Gate 0.6.
5. **Instrumentation reverted again.** B0's whole point. If a slice
   must shrink the trace to keep a diff small, gate it behind the env
   var, do not delete it.
6. **Comparator weakening.** An "adjacent transposition" classifier is
   triage only; if it ever explains a finding, real nondeterminism has
   returned and the correct action is re-pinning, not waiving.
7. **Conflict-pin churn from A4.** Recording installed pullers can add
   parent lines to existing pinned notices; every moved pin needs its
   oracle, or A4 lands behind a flag like #19's abort path.
8. **Python reference performance.** Complete-mode real-tree graphs
   already take minutes in Python; the overlay memo must cover the
   Python mirror too or the L0 Rust==Python diff becomes unusable.

---

## 10. Non-goals

- `_SerializeFrontier` behaviour (shipped, unit-pinned; #18 is stale).
- The cluster-A truncation/advice work (`podman`, `plasma-meta`,
  `gnome-shell` bundle) — separate item; only gnome-shell's `order #0`
  is adjudicated here.
- The `--solver=pubgrub`/`resolvo` real-tree bugs (#33/#34).
- EAPI/KEYWORDS overlay, real's parallel metadata preload, and the
  `Scheduler`-side FakeVartree (024 plan non-goals).
- Any new resolver architecture beyond B4's two rules, and any
  `merge_order.rs` heuristic without a container oracle.

---

## 11. Findings filed while executing

### F-A1 — the appended vdb-built atom is not reconciled by the resolver (2026-09-12)

**Status:** open; blocks flipping the `PORTUALE_DYNAMIC_DEPS_APPEND`
default from off to on (A1 landed behind that gate).

A1's overlay appends the vdb's `slot_operator_built` atoms (`:=` with a
sub-slot) to the live ebuild dep string, exactly like real
`FakeVartree._apply_dynamic_deps`. Two #24 oracle pins prove portuale's
resolver cannot yet consume that union when the ebuild carries the
*unbound* form of the same atom:

- `test_oracle_slotop_slotchange_case4_changedslot`
  (installed `libarchive-3.1.1:0/13`, vdb built binding `:0/0=`, ebuild
  `:=`): with the append on, the Rust side still matches the pinned real
  output (`[rR] libarchive-3.1.1` + `[rR] ark` + the rebuilds block),
  but the Python mirror folds the conflict as "solvable" in
  `_slot_conflict_mask_choices`' pre-check and converges on the
  `3.0.4-r1` downgrade. Root cause: real portage's `match_from_list`
  against a *subslot-less* candidate string treats the built `:0/0=` as
  slot-matching (`True` for `libarchive-3.1.1:0`), while the Rust
  `portage-dep` matcher is stricter; the two languages disagree on
  whether a single version satisfies both wants.
- `test_oracle_slotop_undo_cascade`: with the append on, **both**
  languages drop the cascade's second consumer (`soccascc`) from the
  merge list (the rebuilds block still names it), diverging from the
  pinned real output (`[rR] soccascc-0`). Real resolves the pair through
  the slot-operator rebuild probe before any "one version satisfies all"
  folding.

**Not chased here** (plan rule 2.8/5 "surface, don't chase"): the fix is
probe ordering in the slot-conflict reconciliation (real
`_process_slot_conflicts` -> `_slot_conflict_backtrack_abi` /
`_slot_operator_update_probe` before the generic solvable fold), i.e. a
#23/#24-family resolver slice, not the #26 overlay. Until it lands the
append is opt-in (`PORTUALE_DYNAMIC_DEPS_APPEND=1`), `--dynamic-deps=n`
is unaffected, and the default remains byte-identical to pre-A1. The
helper, the per-pass memo, the `--ignore-built-slot-operator-deps` gate,
the `installed_dep_string` unit tests and the `builtbindpkg` fixture all
land now.

### F-A2 — `need_rebuild` fires under the append gate; the masked-ebuild reason and Python parity wait (2026-09-12)

**Status:** trailer pinned (Rust) for two of the three reasons; #27 closes
as partial with the residue named here.

A4 landed the `slot_pullers` recording from `enqueue_dependencies` (both
languages) -- the missing prerequisite identified in the 024 plan
§0.3.2. With the A1 append gate on
(`PORTUALE_DYNAMIC_DEPS_APPEND=1`), `kde-base/ark`'s vdb binding
`app-arch/libarchive:0/0=` walks alongside the ebuild `:=`, the
`--changed-slot` shape produces a real slot conflict, and the installed
parent is now a recorded puller. The `#27` trailer then fires for both
flag reasons, byte-identical Rust == Python on the trailer itself, and
is pinned by
`test_need_rebuild_trailer_fires_for_an_installed_parent` (with the
gate-off negative control: no conflict, no trailer).

What remains:

1. **The `--usepkgonly` shape** exits 1 on this fixture (no binary
   candidates), so its "stay quiet" arm is unpinned.
2. **The "ebuild is masked or unavailable" reason** needs an installed
   parent whose ebuild is present-but-masked *and reached as a
   dependency* (a top-level masked atom aborts before the walk, and a
   missing ebuild makes `enqueue_dependencies` return early today --
   real's `_DynamicDepsNotApplicable` fallback is not ported there).
3. **The Python mirror drifts** on the conflict/merge output for every
   shape in this family (F-A1), so the new pin runs the Rust side against
   the real-pinned trailer and xfails the Python comparison. Flipping the
   append default (F-A1) is the precondition for both this reason family
   and any Rust==Python pin.




### F-B0 — the trace harness is live (2026-09-12)

`TEST/scripts/mo-trace/` is committed: `real-trace.py` (idempotent
`RT_SEL` injector for real's `_serialize_tasks`; `--unpatch` is
byte-identical; handles both 3.0.81.3's extra `_spinner_update()` loop
head and the 3.0.82.2 shape), `ptl-trace.sh` (`PORTUALE_MO_SEL=1`
wrapper), `align-traces.py` (field-by-field first divergence), README.
The Rust side is gated by `PORTUALE_MO_SEL` and pinned by the
`mo_sel_trace_line_pins_the_harness_format` unit test; unset, output is
byte-identical.

Validation (container, 2026-09-12): `app-misc/tmux` aligns through every
iteration; `gui-libs/gtk:4` diverges at iteration 1 with `alive=398`
(real) vs `alive=395` (portuale) -- the same 3-node graph-membership gap
the 2026-09-09 note recorded as "post-prune al=301 vs 298" -- before any
`retlist`/`ig` difference. The `pick=` field marks each selected node
`m:` (merge) / `n:` (nomerge), so which *kind* of leaf drains first is
visible at the first divergent iteration: that is B1's entry point.

### F-B1 — B1 round 1: the closure's `by_cp` drops every installed slot but one (2026-09-12)

**Status:** root cause identified with the harness; fix (round 2) not
landed.

With the `MO_NODES` snapshot the aligner now names the gtk:4 graph
difference exactly: portuale's post-prune graph is 395 nodes against
real's 398, and the three missing nodes are
`app-text/docbook-xml-dtd-{4.2-r3,4.4-r3,4.5-r2}` -- installed slots
whose parents real has:

| missing node | parent edge (real's dump) |
|---|---|
| `docbook-xml-dtd-4.2-r3` | `app-text/xmlto-0.0.28-r11` (installed, optional + runtime + buildtime) |
| `docbook-xml-dtd-4.4-r3` | `sys-apps/dbus-1.16.2` (installed, optional) |
| `docbook-xml-dtd-4.5-r2` | `sys-apps/systemd-260.1-r2` (installed, optional) |

`xmlto`'s live metadata really does name `app-text/docbook-xml-dtd:4.2`
in both DEPEND and RDEPEND, and portuale's closure *does* visit xmlto --
so the edge is not missing. The dedup is: `add_installed_dependency_closure`
builds `by_cp` as `cp -> one InstalledPackage` (last wins) and keys
`present`/`add_node` on `(category, package)` only. The first
`docbook-xml-dtd` node added (4.1.2-r7, via gtk's explicit `:4.1.2` dep)
marks the cp present, so the `:4.2=`, `:4.4=`, `:4.5=` edges of xmlto,
dbus and systemd all resolve to the *same* cp key and are skipped --
real's `_complete_graph` keeps every installed slot. This is the
"membership wrong, count nearly equal" residue from the 2026-09-09 note
(301 vs 298), now named and reproducible.

The A1 append gate is *not* the cause: with
`PORTUALE_DYNAMIC_DEPS_APPEND=1` the node set is unchanged (395); the
live-vs-raw layer is irrelevant because the parent edge is present in
both. B1 round 2: key the closure's `present`/`add_node` by
`(cat, pkg, slot)` (or cpv), selecting the installed version the edge's
atom actually matches, and let `build_digraph`'s existing per-slot
`edge_matches` narrow the edges (slice 6). Any such change moves the
scheduler graph, so it needs its own L0 run before the next probe's
order can be re-read.

**B1 round 2 (2026-09-12): landed.** `add_installed_dependency_closure`
now keeps every installed version (`by_cp: cp -> Vec<&InstalledPackage>`),
keys `present`/`add_node` by cpv, and selects the version each edge's own
atom names (highest matching, highest overall when the atom is
absent/unparseable). Re-run in the container: `gui-libs/gtk:4` node sets
now match **398 == 398** (previously 398 vs 395). The first remaining
trace difference is no longer membership but the *order inside the
iteration-1 greedy batch* (`sys-libs/zlib` at real position 9 vs
portuale 45; the batch is the same set), and portuale still runs 578
iterations against real's 290 -- the frontier-timing residue now starts
from an identical graph, which is the state B1/B2/B3 were meant to
reach. Full L0 (`TEST/logs/l0-20260912T181102Z`): **clean 96 -> 98,
parity 0.800 -> 0.817, order 22 -> 20**, no new divergence;
`app-text/texlive-core` and `dev-texlive/texlive-latex` flipped clean
(the docbook NS row moved to real's position), `net-misc/networkmanager`
moved one row (still divergent). Python reference mirrored; the full
contract suite stays green.

### F-B2 — B2: a bare multi-slot atom drew an edge to every slot (2026-09-12)

**Status:** fixed; L0 measurement below.

The firefox/thunderbird divergence was *not* the bias tie-break the plan
assumed. The `MO_SEL` trace (with the asap list as contents, not a
count) showed portuale holding **two** `asap_nodes` where real held one:
`llvm-runtimes/clang-runtime-21.1.8` was promoted (PDEPEND-asap, bug
180045) on portuale's side only. Root cause: `llvm-core/clang-common-22`'s
ebuild `PDEPEND` ends with the **unslotted** atom
`llvm-runtimes/clang-runtime[...]`; `build_digraph`'s forward-edge loop
edged that atom to *every* scheduled slot of the cp, giving
`clang-runtime-21` an extra `runtime_post` parent. Real resolves each
atom to a single package (`_select_pkg_highest_available`) before
`_add_pkg` records the edge.

Fix: the forward-edge loop now narrows a multi-match atom to a single
entry -- preferring a merge-bound entry (the node real's scheduler graph
edges to when the cp is being rebuilt), then the highest version, first
on ties -- mirrored in the Python reference. The first refinement
(highest version only) fixed firefox/thunderbird (divergence #29 -> #37)
but transposed `sys-apps/portage`/`app-portage/gentoolkit` in
`MULTI_deep-update-world`; the merge-bound preference restores that probe
and keeps the firefox win.

Final L0 (`TEST/logs/l0-20260912T205626Z`): clean 98, parity 0.817,
order 20 -- the B1 topline with no new divergence; firefox #29 -> #37 and
thunderbird #30 -> #38 (both now the `nasm`/`freetype` Lever-1 timing
family).

### F-B3 — B3: the emptytree tie-break is real's `_create_graph` insertion order (2026-09-12)

**Status:** adjudicated — blocked on a resolver-traversal item; not a
scheduler tweak.

The `-pe @system` trace (`MO_ORDER`, new: post-prune, pre-bias) shows
real's and portuale's graphs have identical node sets (368 == 368) and
identical bias keys (parent counts for `glibc`/`zstd`/`packaging`/
`libxml2` all match: 8/7/7/7), but the **pre-bias insertion order**
diverges at index 1:

```
real     : baselayout, findutils, patch, eselect, awk, bzip2, gzip, sh, tar ...
portuale : baselayout, awk, bzip2, gzip, sh, tar, ..., eselect ...
```

`_merge_order_bias` is a stable sort, so equal-count leaves keep this
order; the first merge-list divergence (`#13`, `acct-group/adm` vs
`app-misc/mime-types`) is downstream of it. Parent counts and node sets
matching rules out the bias tiers, edge narrowing (B2), the closure
(B1), and `asap` (the divergence is a single greedy batch with
`asap=[]`). The remaining difference is real's `_create_graph` LIFO
insertion order vs portuale's `build_digraph` DFS from the expanded
top-level atoms -- i.e. the same "port real's graph insertion order"
item that B4 (superseded in-edges) also needs.

Action: filed as a resolver-architecture follow-up; B3's probes
(`_system` #11, `_world` #14, `MULTI_emptytree-system` #13) stay in the
cluster-I residue. The `MO_ORDER` snapshot stays in the harness (both
sides; patcher round-trip re-verified).
