# Backlog #26 — `--dynamic-deps=n` "walk-source disagreement" — agent plan

Status: proposed. Written 2026-09-12 against `main` @ `be76d0d` (post
#24 S7). Owner decisions required at Gate 0 (§6) before S2 lands.
Filename keeps the number the user asked for; the backlog item is
**#26** (`docs/backlog-tasks.md`), not #24.

**Read first:** `AGENTS.md` (the rhythm; step 8 is the verification
pass), `docs/agent-context.md`, `docs/024-slot-operator-plan.md` §2
(rules and invariants — they apply verbatim here), `docs/024-oracle.md`
(the oracle-pinning method), and `docs/what-this-proves.md`'s
`emerge --dynamic-deps / --dynamic-deps=n (2026-09-02)` entry (what
shipped).

Real portage = the vendored `3rdparty/portage/` checkout unless stated.
Every line number below drifts; **re-locate by function name before
trusting one** (AGENTS.md step 1).

Model tiers, same convention as the #22/#23/#24 plans:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion — the backlog line is wrong twice, and the real gap is bigger

The backlog line (`docs/backlog-tasks.md` #26):

> **`--dynamic-deps=n` walk source** — the
> `dynamic_deps_picks_ebuild_vs_vdb_deps` unit test disagrees with the
> CLI path; both languages currently walk ebuild deps. Unexplained —
> needs owner investigation.

and `docs/scope-backlog.md:134-137` / `what-this-proves.md` ~15974
("no vdb-recorded bound-`:=` fixture reachable until `--dynamic-deps=n`
switches the walk source via the CLI — open mystery, both languages
agree").

### 0.1 There is no disagreement. Verified live, 2026-09-12

```
$ FX="$(realpath fixtures)"
$ run() { PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
      rust/target/release/portuale emerge "$@"; }

$ run --pretend -D --noreplace dev-libs/changeddepspkg
[ebuild  N     ] dev-libs/newpkg-1.0
                          ^-- the CURRENT ebuild's RDEPEND. correct.

$ run --pretend -D --noreplace --dynamic-deps=n dev-libs/changeddepspkg
                          (nothing: the vdb RDEPEND names samepkg,
                           already installed.) correct.
```

The CLI switches the walk source exactly as the unit test
(`rust/portage-repo/src/lib.rs`
`dynamic_deps_picks_ebuild_vs_vdb_deps_for_an_already_installed_deep_walk`,
~24715) and the contract test
(`tests/test_emerge_pretend_contract.py`
`test_dynamic_deps_chooses_ebuild_vs_vdb_deps_for_an_installed_deep_dep`,
~13249, plus four `CASES` entries at ~270-288) already assert. **The
first half of the backlog line describes a bug that does not exist and
should be deleted.** An agent that opens this item expecting a broken
CLI flag will burn a session finding nothing.

### 0.2 The thing that *is* broken was misdiagnosed as the flag

The observation that produced the item was downstream, in the
slot-conflict work: `slot_conflict_need_rebuild`
(`rust/portuale/src/pretend.rs` ~7368, backlog #27) requires a shown
parent that is **installed** and whose **atom carries a built
slot-operator binding** (`cat/pkg:S/SS=` — the function returns `None`
immediately unless `atom.slot_operator == Equals && atom.sub_slot.is_some()`).
Nobody could build a fixture that reaches it, and the conclusion drawn
was "`--dynamic-deps=n` must not be switching the walk source".

The actual cause is the opposite of what was assumed. In real portage,
a built `:S/SS=` atom reaches the walk **in the default mode too** —
`--dynamic-deps=n` is not needed and never was. Real's
`FakeVartree._apply_dynamic_deps`
(`lib/_emerge/FakeVartree.py:146-191`) overwrites the installed
package's metadata with the live ebuild's, and then **appends the vdb's
own built slot-operator atoms back on top**:

```python
# preserve built slot/sub-slot := operator deps
built_slot_operator_atoms = None
if (not self._ignore_built_slot_operator_deps
        and _get_eapi_attrs(pkg.eapi).slot_operator):
    built_slot_operator_atoms = find_built_slot_operator_atoms(pkg)   # :171
...
if built_slot_operator_atoms:
    ...
    for k, v in built_slot_operator_atoms.items():
        live_metadata[k] += " " + " ".join(str(atom) for atom in v)   # :180
self.dbapi.aux_update(pkg.cpv, live_metadata)                        # :182
```

Portuale never does this in any mode. Under `dynamic_deps = true` it
reads the ebuild's `:=` (unbound, `sub_slot == None`); under
`dynamic_deps = false` it reads the vdb and gets the bound form but
loses every other dep update. Real gets **both at once**, which is the
documented Gentoo behaviour and is even written down in this repo
already — `docs/Paragone_solver_portage.md:365`: *"Per un pacchetto
installato Portage usa le dipendenze dell'ebuild corrente nel
repository e non quelle del VDB … Dal VDB prende invece le parti `:=`
registrate al momento della build."*

**That missing append is backlog #26's real content, and it is what
unblocks #27.**

### 0.3 The model is wrong, not just the code — real has no "walk source"

Portuale threads `dynamic_deps: bool` as a parameter down to **one**
function, `enqueue_dependencies` (`lib.rs` ~19592, the branch at
~19653). Real has no per-walk switch at all. Real applies the choice
**once, up front, to the installed-package view itself**:

- `create_depgraph_params.py:111-119` turns `--dynamic-deps` +
  `--nodeps` into `myparams["dynamic_deps"]`.
- `depgraph._load_vdb` (`depgraph.py:886-945`) constructs the
  `FakeVartree` with `dynamic_deps=…` and preloads the overlay over
  **every installed package** (in parallel, `_dynamic_deps_preload`
  948-980).
- From then on every consumer — the `--deep` walk, `_complete_graph`,
  blockers, depclean, slot-operator probes, the merge-order closure,
  even the `Scheduler` (`Scheduler.py:436-445`) — reads the same
  `pkg._metadata`.
- The handful of consumers that must see the **true vdb snapshot** use
  `pkg._raw_metadata` instead, with an explicit comment saying why:
  `depgraph._changed_deps` (3180-3245) — *"Use `_raw_metadata`, in
  order to avoid interaction with `--dynamic-deps`."*

So real has a **two-layer installed-metadata model**:

| Layer | Real | Content |
|---|---|---|
| raw | `pkg._raw_metadata` | the vdb record, verbatim |
| effective | `pkg._metadata` | `dynamic_deps` ? (ebuild deps + EAPI + KEYWORDS, **plus** the raw built `:=` atoms re-appended) : raw, with package-move global updates applied on the not-applicable fallback |

Portuale has the raw layer only, plus one call site that sometimes
reads the ebuild instead. **Every other installed-metadata consumer
silently ignores `--dynamic-deps`** — ~13 sites in Rust, ~9 in Python
(inventory in §2). That is the scope of #26.

### 0.4 Verdict

Re-title the item: **"installed-package metadata overlay
(`FakeVartree._apply_dynamic_deps`)"**. Delete the "unit test disagrees
with the CLI" sentence — it is false. Keep the `portage-repo` pointer.
The item is **not** "unexplained"; it is explained above and has a
one-function oracle.

Payoffs, in order of confidence:

1. Built `:=` atoms become visible to the default-mode walk → **#27
   (`need_rebuild` trailer) stops being blocked**, and the `:=`
   re-binding that #24-S4 does through `bind_slot_operator_deps` gets a
   consistent upstream.
2. `--dynamic-deps=n` starts meaning what it means in real across every
   consumer, not just the `--deep` recursion — most visibly in
   `depclean`/`prune_cleanlist` and `required_set_reachable_cps`, where
   a stale vdb dep currently decides an unmerge.
3. One shared helper replaces ~22 open-coded `read_vdb_string(… ,
   dep_key)` sites, which is the kind of consolidation that makes the
   *next* installed-metadata slice cheap (EAPI/KEYWORDS overlay,
   package-move fallback).

Risk it is not worth doing: low. It is a correctness item with a
narrow, fully-specified oracle. Risk it is *bigger than it looks*:
moderate — see §8.

---

## 1. Ground truth (cite these, do not re-derive)

### 1.1 Real's machinery, by role

| Role | Function | File:line | Note |
|---|---|---|---|
| option → param | `create_depgraph_params` | `create_depgraph_params.py:111-119` | `--dynamic-deps` default `y` for a SOURCE install; `--nodeps` forces off |
| construct | `depgraph._load_vdb` | `depgraph.py:886-945` | builds the `FakeVartree` once per `frozen_config`; **shared across every backtracking depgraph** |
| preload | `depgraph._dynamic_deps_preload` | `depgraph.py:948-980` | pulls each installed cpv's ebuild metadata (cache hit, else `EbuildMetadataPhase`) |
| idempotence | `FakeVartree.dynamic_deps_applied` | `FakeVartree.py:193-202` | the overlay must run **once** per instance; a second pass would derive from already-overwritten metadata |
| the overlay | `FakeVartree._apply_dynamic_deps` | `FakeVartree.py:146-191` | §1.2 |
| keys overlaid | `FakeVartree._portdb_keys` | `FakeVartree.py:94` | `Package._dep_keys + ("EAPI", "KEYWORDS")` — **not just the five `*DEPEND`** |
| built `:=` rescue | `find_built_slot_operator_atoms` | `dep/_slot_operator.py:24-38` | `use_reduce` each dep key against `pkg.use.enabled`, keep `Atom.slot_operator_built` |
| lazy path | `FakeVartree._aux_get_wrapper` / `_match_wrapper` | `FakeVartree.py:110-144` | same overlay, on demand, for anything the preload missed |
| raw escape hatch | `depgraph._changed_deps` | `depgraph.py:3180-3245` | uses `pkg._raw_metadata` **deliberately** |
| report suppression | `depgraph._changed_deps_report` | `depgraph.py:1308-1322` | silent when `changed_deps == y` **or** `dynamic_deps` in myparams |
| merge side | `Scheduler` | `Scheduler.py:436-445` | its own `FakeVartree` with the same flag |
| opt-out | `--ignore-built-slot-operator-deps` | `create_depgraph_params.py:105-107`, `FakeVartree.py:171` | suppresses **only** the built-`:=` append |

### 1.2 `_apply_dynamic_deps`, branch by branch

Read `FakeVartree.py:146-191` and implement exactly these five arms:

1. **No live metadata** (ebuild gone from the tree) → `_DynamicDepsNotApplicable`.
2. **Either EAPI unsupported** (installed or live) → `_DynamicDepsNotApplicable`
   (bug #368725's comment explains the direction: both supported ⇒ prefer live).
3. **Built `:=` rescue**: unless `--ignore-built-slot-operator-deps`, and
   only if the *installed* EAPI has slot-operator support, collect
   `find_built_slot_operator_atoms(pkg)` from the **pre-overlay**
   metadata. If any exist and the **live** EAPI has no slot-operator
   support → `_DynamicDepsNotApplicable`. Otherwise append them, per
   key, space-joined, to the live value (`:180`) — an **append**, not a
   replace, so the ebuild's unbound `:=` stays alongside the bound one.
4. **Apply**: `aux_update(pkg.cpv, live_metadata)` — overwrites the five
   dep keys **plus `EAPI` and `KEYWORDS`**.
5. **Not-applicable fallback**: run `perform_global_updates` (package
   moves) over the raw vdb metadata instead. Portuale already has
   `apply_updates_to_dep_string` (`lib.rs` ~615, ~1291); this arm is
   where it belongs on this path.

### 1.3 The upstream oracle to translate

`lib/portage/tests/resolver/test_changed_deps.py::testChangedDeps` is
the exact shape of this item: installed `app-misc/A-0` with **no**
recorded deps, whose ebuild has `DEPEND`/`RDEPEND = app-misc/B`.

| upstream case | options | expected |
|---|---|---|
| 1 | `-uD --usepkg --dynamic-deps=n` `@world` | `mergelist=[]` |
| 2 | `-uD --usepkg --dynamic-deps=y` `@world` | `mergelist=["app-misc/B-0"]` |
| 3 | `-uD --usepkg --changed-deps=y` `@world` | `["app-misc/B-0", "app-misc/A-0"]` |
| 4 | `--usepkgonly --changed-deps=y` `app-misc/A` | `["[binary]app-misc/A-0"]` |
| 5 | `--usepkg` `app-misc/A` | `["app-misc/B-0", "app-misc/A-0"]` |
| 6 | `--binpkg-changed-deps=n --changed-deps=y --usepkg` | `["[binary]app-misc/A-0"]` |

**Trap for the translating agent:** the file rebinds `test_cases = (…)`
a second time just before the loop, so upstream only actually executes
case 6. Cases 1-5 are still valid expectations (they are what the
resolver does), but do **not** report "upstream passes these" — it
never runs them. Pin all six anyway; if one diverges, adjudicate it
against a live `emerge` run, not against the file.

Cases 3/4/6 double as the regression guard that portuale's
`deps_changed` (`lib.rs` ~7367) keeps reading the **raw** layer after
this slice — see §2's verdict column.

---

## 2. Portuale today — the inventory (this table is the deliverable of S1)

Every site that reads an installed package's `*DEPEND` from the vdb.
The **verdict** column is this plan's *proposal*; S1's job is to
confirm or correct each one against real, and it is the artifact the
implementation slices are driven from.

Rust (`rust/portage-repo/src/lib.rs` unless noted; line numbers drift):

| # | ~line | function | role | proposed verdict |
|---|---|---|---|---|
| 1 | 19652 | `enqueue_dependencies` | the `--deep` AlreadyInstalled walk | **effective** (already is — the only one) |
| 2 | 5159 | `installed_reverse_dependents` | who depends on X | **effective** |
| 3 | 5849 | `topological_removal_order` | unmerge ordering | **effective** |
| 4 | 6069 | `unresolved_runtime_deps` | depclean breakage check | **effective** |
| 5 | 6258 | `depclean_cleanlist` | what may be removed | **effective** |
| 6 | 6489 | `prune_cleanlist` | protect from removal | **effective** |
| 7 | 12620 | `required_set_reachable_cps` | `@world`/`@system` closure | **effective** |
| 8 | 12720 | `rebuild_if_entries` | `--rebuild-if-*` | **effective** |
| 9 | 13223 | `grandparent_use_conflict` | circular-dep suggestions | **effective** |
| 10 | 13328 | `installed_has_foreign_dependents` | | **effective** |
| 11 | 7367 | `deps_changed` | `--changed-deps` / `--changed-deps-report` | **RAW** — real `_changed_deps` says so in a comment (`depgraph.py:3201`) |
| 12 | 11982 | `slot_operator_rebuild_scan` | built `:=` detection | **RAW** — mirrors `find_built_slot_operator_atoms`, which real reads pre-overlay |
| 13 | 12411 | `slot_operator_eliminate_rebuilds` | #24-S4 undo, rule 8 dep compare | **decide at Gate 0.2** — real compares re-evaluated tree deps against the installed node's deps; which layer the "installed" side is, is the one genuinely ambiguous call in this table |
| 14 | `merge_order.rs` 935 | `add_installed_dependency_closure` | scheduling-graph edges | **effective**, and note the `is_injected_libc` interaction (§8.3) |
| 15 | 5502 | `find_libc_deps` (`virtual/libc` RDEPEND) | libc provider identity | **RAW** — confirmed: real `_installed_libc_deps` (`depgraph.py:3163-3176`) passes `_frozen_config._trees_orig[eroot]["vartree"].dbapi`, the **original** vartree, never the `FakeVartree` |
| 16 | 4975 | virtual expansion (`lib.rs` ~4951-4990) | installed virtual → provider | **effective** — **verify** |

Python mirror (`python/emerge_pretend_reference.py`): the same roles at
~8129, ~8699, ~8808, ~16497, ~17520, ~17988, ~18172, ~18258, ~18709.
S1 must produce the Rust↔Python pairing explicitly; a site present on
one side only is itself a finding.

Not in scope (they are not the installed-metadata view): `regen.rs`,
`ebuild_merge.rs`, `ebuild_phases.rs`, `binpkg.rs`, `ebuild_package.rs`,
`solver_bridge.rs` (bridge has its own `dynamic_deps: false`, keep it),
`resolver_trace.rs`.

---

## 3. The shape of the fix

**Do not** thread a `dynamic_deps: bool` through sixteen more call
sites. That reproduces the bug one level up: the next reader still has
to know, per site, which layer is right.

Introduce the two-layer view explicitly, matching real's own names:

```rust
/// Real `Package._raw_metadata` vs `Package._metadata` for an installed
/// package. `Raw` is the vdb record verbatim; `Effective` is what
/// `FakeVartree._apply_dynamic_deps` leaves behind.
enum InstalledMetaLayer { Raw, Effective }

/// One installed package's `*DEPEND` for `key`, at `layer`.
/// Real `FakeVartree.py:146-191`.
fn installed_dep_string(
    view: &InstalledMetaView,   // carries root, repos, dynamic_deps,
                                // ignore_built_slot_operator_deps
    pkg: &InstalledPackage,
    key: &str,
    layer: InstalledMetaLayer,
) -> String
```

Requirements on the implementation:

- **`Effective` with `dynamic_deps == false` must be byte-identical to
  `Raw`.** That is the invariant that makes the whole slice safe: with
  the flag off, nothing changes anywhere.
- **`Effective` with `dynamic_deps == true`** = the live ebuild's value
  for `key`, plus `" "` plus the space-joined built `:=` atoms found in
  the *raw* value for that same `key` — reduced against the package's
  own vdb `USE` (real `pkg.use.enabled`, and for an installed package
  that is the recorded USE; portuale already has `read_vdb_flag_set(…,
  "USE")` and relies on it in `enqueue_dependencies`' own comment).
- **Cache it.** Real preloads once per depgraph and is explicit that
  re-application is a bug (`dynamic_deps_applied`). Portuale re-reads
  the vdb on every pass of the `'backtrack` loop; with an ebuild
  md5-cache read added per installed package per site, a `@world -uD`
  would go from ~O(installed) file reads to ~O(installed × sites ×
  passes). A `HashMap<(cat,pkg,ver), [String; 5]>` memo on the view,
  built lazily, is the minimum. Measure against
  `docs/performances-tuning.md`'s own `-puD` baseline (§5 there) and
  report the delta in the slice's commit message — the 17× win from
  2026-09-07 is not to be given back.
- **`--ignore-built-slot-operator-deps`** already parses
  (`pretend.rs` ~8659) and reaches `solver_bridge`. Thread it into the
  view; it suppresses only the append (real `FakeVartree.py:171`).
- **EAPI / KEYWORDS overlay** (`_portdb_keys`, `FakeVartree.py:94`):
  Gate 0.3 decides whether it lands here or is a documented carve-out.
  Recommendation: carve out, and say so in the helper's doc comment —
  portuale's installed-package EAPI/KEYWORDS consumers are a different
  audit, and mixing them in doubles the blast radius for no pinned
  divergence.
- **Dual-language in lockstep** (AGENTS.md step 4). The Python mirror
  gets the same helper with the same name (`_installed_dep_string`) and
  the same layer enum, or the two will drift on the next slice.

---

## 4. Rules and invariants for every slice

Reuse `docs/024-slot-operator-plan.md` §2 verbatim. Restated where this
item adds something:

1. **`--dynamic-deps=n` is a null transformation.** After every slice,
   `portuale emerge --dynamic-deps=n <anything>` must produce
   byte-identical output to the pre-slice binary. Make this a script in
   `/tmp/…/scratchpad`, run it on every slice, and say so in the commit.
2. **Pin before you change.** Every behaviour this slice moves gets a
   failing/xfail pin *first* (S2), then the fix flips it. Same
   xfail-first discipline as #24-S2.
3. **Rust == Python, empirically.** Not "pytest is green" — run both
   binaries over `fixtures/` and diff, per AGENTS.md step 4.
4. **No new `read_vdb_string(… , <dep key>)` call sites.** After S3,
   grep must show the dep-key reads funnelling through the one helper.
   Add that grep to the slice's acceptance.
5. **Surface, don't chase.** A divergence this slice reveals but does
   not own (e.g. a `_complete_graph` membership shift) is recorded as a
   finding with its oracle and left alone.
6. **Stop conditions.** If S1's inventory grows past ~20 sites per
   language, or if the Gate 0.2 answer makes #24-S4's rule 8 unstable,
   stop and re-scope with the owner rather than pushing through.

---

## 5. Slices

### S0 — Correct the record (S, ½ h)

No code. Fix the three places that assert a bug that does not exist:

- `docs/backlog-tasks.md` #26 — retitle per §0.4, drop the "unit test
  disagrees" sentence, point at this plan.
- `docs/scope-backlog.md:134-137` — replace the "unexplained, needs
  owner eyes" bullet with the §0.2 diagnosis.
- `docs/what-this-proves.md` ~15974 — this one is *history* and must
  not be rewritten (AGENTS.md step 7); the correction goes in a **new**
  paragraph at the end, which S6 writes.

Acceptance: the reproduction transcript in §0.1 re-run and pasted into
the commit message.

### S1 — Inventory + decision table (F, 2-3 h)

Produce the real artifact of this item: §2's table, confirmed.

For **each** site, in both languages: name it, state what it feeds,
find real's counterpart, and record `Raw` / `Effective` **with a
file:line citation**. Where real has no counterpart (portuale-only
machinery, e.g. `grandparent_use_conflict`), say so and reason from the
nearest analogue.

Deliverable: `docs/026-oracle.md` (new), §2's table filled in, plus the
Rust↔Python pairing. This is the spec S3 is implemented from.

Why F: every row is a judgement about real's semantics, and a wrong
`Raw`/`Effective` call silently changes depclean or merge-order
behaviour on a real system. This is the slice that earns the item.

### S2 — Oracle pins (M, F brief, 2-3 h)

1. Translate `test_changed_deps.py` cases 1-6 (§1.3) into the
   contract suite's synthetic-root style (the #24-S2 helpers at
   `tests/test_emerge_pretend_contract.py` ~15314 are the pattern).
   Expect 1, 3, 4, 6 to pass today and **2 to be the interesting one**.
2. Add the built-`:=`-append pin: an installed consumer whose **ebuild
   RDEPEND is empty** (or carries only the unbound `:=`) and whose
   **vdb RDEPEND carries `cat/pkg:S/SS=`**. `dev-libs/revdepslotconsumer`
   / `dev-libs/revdepslottarget` is exactly this shape and already
   exists — check whether it can be reused before adding a fixture
   (AGENTS.md step 5). Pin, as **xfail**, that the bound atom is in the
   default-mode walked deps.
3. Add the `--dynamic-deps=n` null-transformation pin from §4.1.
4. Add a `--ignore-built-slot-operator-deps` pin (the append is
   suppressed, the ebuild deps are not).

Acceptance: new pins are xfail-strict where they should fail; the rest
of the suite is untouched.

### S3 — The overlay helper + threading (M writes, F reviews, 4-6 h)

Implement §3, both languages, one commit. Convert the sites S1 marked
`Effective`; leave the `Raw` ones reading the vdb but route them
through the helper with `Raw` so the grep in §4.4 is clean.

Land the memo cache in this slice, not later — a separate "make it
fast" commit means the intermediate commit is the one someone bisects
onto.

Acceptance:
- S2's xfails flip to pass; nothing else moves.
- §4.1 null-transformation script: byte-identical.
- `cargo fmt --check`, `clippy --release --all-targets` zero warnings,
  `cargo test --release` whole workspace, `pytest tests -q` whole suite.
- Perf delta on the `-puD` baseline reported (§3).
- The `grep -n 'read_vdb_string(.*DEPEND' rust/portage-repo/src/lib.rs`
  acceptance from §4.4.

### S4 — `need_rebuild` fixture (#27 payoff) (M, 1-2 h)

With the append landed, build the fixture `#27` was blocked on and let
`slot_conflict_need_rebuild` fire for real: an installed parent whose
built `:S/SS=` atom reaches the slot-conflict renderer, with each of
the three reasons (`--exclude` match, `--useoldpkg-atoms` match,
masked/unavailable ebuild) and the `--usepkgonly` silence.

If it still cannot be reached, **stop and report why** — that is a
finding about the slot-conflict renderer, not a reason to keep
patching here. Close or re-scope #27 explicitly either way.

### S5 — Real-tree validation (L0, + L1 if depclean moved) (F triage, 2-3 h)

`TEST/run/l0-resolver.sh`. Baseline is #24-S6: **120 probes, 96 clean,
parity 0.800**. This slice touches depclean and the merge-order
closure, so an L0 order shift is *possible* — adjudicate each finding
against real, do not re-tune `merge_order.rs` from inside this item
(cluster I is #17).

Run L1 (`TEST/run/l1-merge-from-binpkg.sh`) only if S1 marked a site
that the merge path reads; L1 must run **without**
`L1_SKIP_PORTAGE_UPGRADE=1` (memory note `real-world-testing-plan`).

Acceptance: no regression vs the S0 archives, or every delta
adjudicated in `TEST/findings/l0.md` with its oracle.

### S6 — Docs closure (S, ½ h)

`what-this-proves.md` paragraph (new, with a runnable live-verified
example — the §0.1 transcript plus the built-`:=` one),
`docs/scope-backlog.md`, `docs/backlog-tasks.md` #26 → DONE (and #27's
status, per S4), `docs/026-oracle.md` verdict table.

---

## 6. Owner gates

- **G0.1 — Re-scope accepted?** #26 becomes "installed-package metadata
  overlay". If the owner wants the item kept literal ("just explain the
  flag"), S0 alone closes it and the rest becomes a new item.
- **G0.2 — Which layer does #24-S4's rule-8 dep comparison read?**
  (`slot_operator_eliminate_rebuilds`, `lib.rs` ~12411.) Real compares
  re-evaluated tree deps against the installed node's — and that node's
  metadata *is* overlaid. Getting this wrong makes the undo path
  oscillate. Recommend: `Effective`, with a pin from #24's
  `slotundo-unnecessary` case proving `abi_rebuilds: []` still holds.
  Evidence for that recommendation: real's own `_eliminate_rebuilds`
  (`depgraph.py:3933-3934`) reaches for `_installed_libc_deps` +
  `Package._dep_keys` on the graph's packages and makes **no**
  `_raw_metadata` call — unlike `_changed_deps` twenty lines earlier,
  which does. The absence of the escape hatch is the signal.
- **G0.3 — EAPI/KEYWORDS overlay: in or carved out?** Recommend carved
  out, documented in the helper.
- **G0.4 — Not-applicable fallback (global updates): in or carved
  out?** Recommend in (S3) — portuale already has
  `apply_updates_to_dep_string`, so it is a few lines, and leaving it
  out makes the "ebuild deleted from the tree" case silently wrong.
- **G0.5 — Perf budget.** What regression on the `-puD` baseline is
  acceptable if the memo is not enough? Recommend: none; if the memo
  does not hold it, stop at S3 and re-scope.

---

## 7. Difficulty and routing

Overall: **6/10**. Not algorithmically hard — no search, no
backtracking, no ordering. Hard because it is **wide** (≈22 call sites,
two languages, byte-parity required) and because the value is in ~16
per-site semantic judgements, not in the code.

Compare: easier than #22/#23/#24 (no resolver search), harder than a
Tier-1 slice (it is not one file and it touches depclean).

| Slice | Work | Tier | Why |
|---|---|---|---|
| S0 | doc correction | **S** | mechanical; the transcript is in §0.1 |
| S1 | inventory + decision table | **F** | the whole item. 16 real-semantics calls, each with a silent-wrong failure mode |
| S2 | oracle pins | **M** (F brief) | pattern-following on #24-S2's helpers; the upstream-test trap in §1.3 needs flagging up front |
| S3 | helper + threading + cache | **M**, **F review** | spec-driven once S1 exists; the review is for the `Raw`/`Effective` call at each converted site |
| S4 | `need_rebuild` fixture | **M** | fixture craft; escalate to F only if it still can't be reached |
| S5 | L0 validation + triage | **F** | adjudicating merge-order/depclean deltas against real is judgement |
| S6 | docs | **S** | |

**Can a cheaper model run this end to end? No — and the split matters.**
S1 and S5 are where a mid-tier model reliably fails on this repo: S1
because "which layer" has no local signal (the code compiles and the
tests pass either way), S5 because an L0 delta looks like noise until
you check it against real. S2/S3/S4 are ~60% of the wall-clock and are
genuinely safe for Sonnet **provided S1's table is committed first** —
that is the whole reason to front-load it as its own deliverable.

Sequencing note: S1 before S2. A mid-tier model handed this plan
without S1's table will thread `dynamic_deps` through every site it
finds and produce a large, plausible, wrong diff.

Total: roughly 12-18 h of agent time across 2-3 sittings.

---

## 8. Risks

1. **Perf.** The overlay adds an ebuild-metadata read per installed
   package. Unmemoized, inside the `'backtrack` loop, that is the
   `-puD` regression this repo spent a session buying back
   (`perf-investigation-2026-09-07`). Memo in S3; Gate 0.5.
2. **depclean blast radius.** Sites 4-6 decide what gets **unmerged**.
   `--dynamic-deps` is on by default, so `Effective` there is a
   live-system behaviour change, not a pretend-only one. It is what
   real does — but S5 must exercise it, and the S2 pins must cover a
   depclean case, not only a merge case.
3. **`is_injected_libc` interaction.** `merge_order.rs`'s
   `add_installed_dependency_closure` strips
   `doebuild._inject_libc_dep`'s phantom `>=libc` atom from the vdb
   (L0 cluster I slice 10, `264163f`). Under `Effective` that atom is
   **not there in the first place** (real never sees it, which is why
   the strip was needed). Re-read `is_injected_libc`'s doc comment
   before converting site 14: the right move may be that the strip
   becomes dead on the `Effective` path and stays live on `Raw`.
   Getting this wrong regresses ~40 L0 order findings.
4. **#24-S4 coupling.** Gate 0.2. If rule 8's comparison shifts layer,
   the `slotundo-*` pins move with it; #24's oracle table
   (`docs/024-oracle.md`) is the regression guard.
5. **Contract-suite isolation.** `git clean -fdq fixtures/` deletes
   untracked new fixtures — `git add` first. And the suite has a
   pre-existing order-dependent isolation bug: compare failing test
   *names* against a clean-`main` baseline, never counts (memory note
   `contract-suite-pollutes-fixtures`).

---

## 9. Non-goals (state them in the docs, do not drift into them)

- Real's **parallel** preload (`TaskScheduler` +
  `EbuildMetadataPhase`, `depgraph.py:960-980`) — portuale reads
  `md5-cache` directly and has no metadata-generation phase to
  schedule. The memo is the counterpart; say so.
- The `Scheduler`-side `FakeVartree` (`Scheduler.py:436-445`) — the
  merge path, a separate audit.
- `--ignore-soname-deps` / `PackageDbapiProvidesIndex`
  (`FakeVartree.py:80`) — soname atoms are an established unreachable
  cut (`portage-dep` cannot parse them).
- `--solver=` bridges keep `dynamic_deps: false`
  (`solver_bridge.rs:1234`); that is #34's territory.
- EAPI/KEYWORDS overlay, if Gate 0.3 goes as recommended.
