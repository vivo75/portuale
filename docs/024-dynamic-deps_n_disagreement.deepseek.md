# Backlog #26 — `--dynamic-deps=n` "walk-source disagreement" — agent plan (deepseek draft)

Status: proposed. Written 2026-09-12 against `main` @ `be76d0d` (post
#24 S7). Filename keeps the `024-` the user asked for; the backlog item
is **#26** (`docs/backlog-tasks.md:47`), not #24. A sibling draft
(`docs/024-dynamic-deps_n_disagreement.opus.md`, same tree, reviewed by
this author) proposed re-scoping the item to the full installed-metadata
overlay; §0.5 records where this draft agrees, differs, and adds.

**Read first:** `AGENTS.md` (step 8 is the verification pass), `docs/agent-context.md`,
`docs/024-slot-operator-plan.md` §2 (rules and invariants — reuse them),
`docs/024-oracle.md` (oracle-pinning method), `docs/what-this-proves.md`
`### emerge --dynamic-deps / --dynamic-deps=n (2026-09-02)` (~9619) and
the `need_rebuild` note at ~15973, and `docs/history/scope-backlog-2026-09-10.md:138-153`.

Real portage = the `3.0.82.2` tree that ships here
(`/usr/lib/python3.14/site-packages/{_emerge,portage}`) unless stated.
Every line number drifts; **re-locate by function name before trusting
one** (AGENTS.md step 1).

Model tiers, same convention as the #22/#23/#24 plans:

| Tier | Meaning | Examples |
|---|---|---|
| **F** | frontier | Claude Opus 5 / Fable 5.1 |
| **M** | mid | Claude Sonnet 5 |
| **S** | small | Claude Haiku 4.5 |

"F review" = a frontier model reads the full diff before the user is
asked to commit, whoever wrote it.

---

## 0. Opinion — the backlog line is false as written, and it hides three real divergences

The backlog line (`docs/backlog-tasks.md:47`):

> **`--dynamic-deps=n` walk source** — the
> `dynamic_deps_picks_ebuild_vs_vdb_deps` unit test disagrees with the
> CLI path; both languages currently walk ebuild deps. Unexplained —
> needs owner investigation.

and `docs/scope-backlog.md:134-137` / `what-this-proves.md:15973-15975`
("… until `--dynamic-deps=n` switches the walk source via the CLI —
open mystery, both languages agree").

### 0.1 The claimed bug does not reproduce (verified live, 2026-09-12)

```
$ FX="$(realpath fixtures)"
$ run() { PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
      rust/target/release/portuale emerge "$@"; }
$ runpy() { PORTAGE_CONFIGROOT="$FX" ROOT="$FX" PORTAGE_RUNNING_ROOT="$FX" \
      python3 python/emerge_pretend_reference.py "$@"; }

$ run --pretend -D --noreplace dev-libs/changeddepspkg
[ebuild  N     ] dev-libs/newpkg-1.0        # current ebuild RDEPEND
$ run --pretend -D --noreplace --dynamic-deps=n dev-libs/changeddepspkg
                                            # empty: vdb RDEPEND is samepkg,
                                            # already installed
```

Python prints byte-identical stdout in both cases
(`test_dynamic_deps_chooses_ebuild_vs_vdb_deps_for_an_installed_deep_dep`,
`tests/test_emerge_pretend_contract.py:13249-13267`), the unit test is
green (`rust/portage-repo/src/lib.rs:24715-24781`), and the trace proves
the queued dependency changed. `--debug` under `=n` (stdout, byte-equal
Rust/Python):

```
Parent:    (dev-libs/changeddepspkg-1.0:0/0::testrepo, installed)
Depstring: dev-libs/newpkg (RDEPEND)          <-- entry deps: EBUILD
...
Child:         (dev-libs/samepkg-1.0:0/0::__unknown__, installed)
Parent Dep:    dev-libs/samepkg required by (dev-libs/changeddepspkg-…)
                                          ^-- what was actually walked: VDB
```

**The flag works. Delete the "does not switch / unit test disagrees"
sentence outright.** An agent sent to find a broken CLI flag will burn a
session proving a negative (this author did, before finding §0.3.1).
`enqueue_dependencies` branches on `dynamic_deps` at
`rust/portage-repo/src/lib.rs:19649-19664`; `ctx.dynamic_deps` is wired
at 17167 from `ResolveRequest.dynamic_deps` (14966 / 15520), which
`pretend.rs:10446` fills from the parse at 8088-8101 after the real
`dynamic_deps && !nodeps` gate at 10362. The Python mirror is
`_enqueue_dependencies` at 14394-14399, parse at 19892, gate at 21668.

### 0.2 The "disagreement" is real — but it is inside portuale, not CLI-vs-unit

The same trace shows it: under `=n` the **walk** reads the vdb snapshot
while the **display/edge model** (`GraphEntry::deps`) still reads the
ebuild. `already_installed_deps` is deliberately built from the repo
md5-cache with a comment saying so
(`lib.rs:17125-17130`, assigned at 17302). That is the only sense in
which "both languages walk ebuild deps" is true: every *other* consumer
of an installed package's metadata in both languages does. It is a
display/edge inconsistency, not a walk bug — but it is exactly the kind
of self-contradicting output that produced the backlog line, and it must
be fixed or the next investigator re-derives the same wrong conclusion.

### 0.3 Three real divergences, in dependency order

**0.3.1 The default (`--dynamic-deps=y`) drops the vdb's built `:=`
atoms.** Real does not choose one source: `FakeVartree._apply_dynamic_deps`
(`_emerge/FakeVartree.py:146-191`) overwrites the installed package's
metadata with the **live ebuild's**, then appends the vdb's own
`slot_operator_built` atoms back on top — `find_built_slot_operator_atoms`
(`portage/dep/_slot_operator.py:24-38`), `live_metadata[k] += " " + …`
(:180), `aux_update` (:182). `slot_operator_built` is `:=` **with a
sub-slot** (`portage/dep/__init__.py:2156-2162`: `foo/bar:2=` is *not*
built, `foo/bar:2/2=` is). Portuale's `dynamic_deps=true` path reads the
ebuild only, so a vdb-only built atom is invisible in the default mode.
Real's own documentation reproduced in-repo says the same:
`docs/Paragone_solver_portage.md:365` — current ebuild deps, *plus* the
`:=` parts recorded at build time from the VDB.

**0.3.2 An installed parent's dependency atoms are never recorded as
slot pullers.** `state.slot_pullers` is fed only by the top-level
Argument path (`lib.rs:16531-16537`) and by the merged-candidate
flat-deps path (18432-18442). `enqueue_dependencies` — the only walker of
an installed package's deps — queues tokens but has no `slot_pullers`
parameter (signature 19589-19611), so none of its atoms can ever appear
as a `pulled in by` line or as a `need_rebuild` candidate. The Python
mirror has the same hole (`slot_pullers` written at 12067 and 13518
only). **This, not the missing append, is what actually keeps #27's
`slot_conflict_need_rebuild` dormant** (`pretend.rs:7368`: the function
returns `None` unless the atom is built (`:=` + sub-slot) *and* the
parent is installed; it is only ever called on `c.parents`, and
`c.parents` comes from `slot_pullers`). The sibling draft's S4 assumes
the append alone unblocks #27; it does not (§0.5).

**0.3.3 The raw/effective distinction is not modelled.** Real applies
`--dynamic-deps` **once, up front, to the whole installed view**
(`depgraph._load_vdb`, `depgraph.py:886-945`, preloading every installed
package in parallel at 948-980; shared across backtracking passes), and
the exceptions deliberately escape to `pkg._raw_metadata`
(`_changed_deps`, 3180-3245: *"Use `_raw_metadata`, in order to avoid
interaction with `--dynamic-deps`."*). Portuale reads raw vdb strings
open-coded at ~16 Rust sites and ~9 Python sites. Only one of them
(`enqueue_dependencies`) knows about `dynamic_deps`. Under the default,
all the others are wrong in the same direction: they should see
ebuild+append. This is the sibling draft's re-scope proposal and it is
correct as a map (§1.3), but see Gate 0.3 for how much of it to land
here.

### 0.4 Reconstructed misdiagnosis

While hunting a `need_rebuild` fixture (#27), the author used
`--dynamic-deps=n` to make the vdb's built `:=` atom walk, saw no
conflict parent, and concluded "the flag isn't switching the walk". The
walk *was* switching (§0.1); the parent was absent for §0.3.2; and the
default they may have compared against drops the built atom for
§0.3.1. The `--debug` line then "confirmed" it because
`GraphEntry::deps` still prints the ebuild (§0.2).

### 0.5 Relationship to the sibling draft (`…opus.md`)

| Point | Sibling draft | This draft |
|---|---|---|
| CLI/unit disagreement | false; verified | same, independent reproduction + `--debug` trace evidence |
| `GraphEntry::deps` under `=n` | not mentioned | §0.2, S4 — its own behaviour-neutral pin |
| Two-layer raw/effective model | the core proposal; 16-site conversion | accepted as the target *model*, but first slice is the single behaviour-changing consumer (`enqueue_dependencies`) + append; remaining sites are a staged conversion (S4) with L0 between |
| #27 enabler | "the append unblocks it" | **wrong**: `slot_pullers` recording (S5) is also required; without it the fixture still cannot fire |
| EAPI/KEYWORDS overlay | carve out (G0.3) | same recommendation, same gate |
| `slot_operator_rebuild_scan` stays raw | site 12 raw | agreed (it *is* the `find_built_slot_operator_atoms` equivalent, read pre-overlay) |
| `--dynamic-deps=n` null transformation | invariant | same invariant, made a scripted acceptance test (S2) |
| Scope | one 6-slice re-scope | staged: S0 closes the literal item, S1 decides the re-scope size, Gates bound the blast radius |

Neither draft should be merged verbatim; §0.6 is the recommendation.

### 0.6 Recommended scope (Gate 0.1)

1. **S0 closes #26 as stated** ("we investigated; the claim is false;
   here is the reproduction and the corrected wording").
2. **Re-scope the real work as a new item** (call it #26b or a fresh
   number) = §0.3: *installed-package effective/raw metadata view
   (`FakeVartree._apply_dynamic_deps`)*, with S1-S7 below.
3. **#27's status stays "blocked", but the blocker is corrected** to
   "installed-parent puller recording" (S5). If the owner prefers, S5
   moves to #27 directly; the unit of work is the same.

This keeps the backlog honest without letting a false one-liner stay in
the tree, and it avoids a 16-site refactor being smuggled in under a
"walk source" title.

---

## 1. Ground truth (cite these, do not re-derive)

### 1.1 Real's machinery, by role

| Role | Function | File:line | Note |
|---|---|---|---|
| option → param | `create_depgraph_params` | `create_depgraph_params.py:105-119` | default `y` for a source install; `--nodeps` forces off |
| construct | `depgraph._load_vdb` | `depgraph.py:886-945` | FakeVartree lives in `frozen_config`, shared by every backtracking pass |
| preload | `depgraph._dynamic_deps_preload` | `depgraph.py:948-980` | per installed cpv, md5-cache or `EbuildMetadataPhase` |
| idempotence | `FakeVartree.dynamic_deps_applied` / `dynamic_deps_preload` | `FakeVartree.py:193-207` | overlay must run once; a second pass would derive from overwritten metadata |
| the overlay | `FakeVartree._apply_dynamic_deps` | `FakeVartree.py:146-191` | §1.2 |
| overlaid keys | `FakeVartree._portdb_keys` | `FakeVartree.py:94` | `Package._dep_keys + ("EAPI","KEYWORDS")` |
| built-`:=` rescue | `find_built_slot_operator_atoms` | `portage/dep/_slot_operator.py:24-38` | `use_reduce` each dep key vs `pkg.use.enabled`, keep `Atom.slot_operator_built` |
| raw escape | `depgraph._changed_deps` | `depgraph.py:3180-3245` | uses `_raw_metadata` on purpose |
| opt-out | `--ignore-built-slot-operator-deps` | `create_depgraph_params.py:105-107`, `FakeVartree.py:171` | suppresses only the append |
| conflict-side scan | `slot_collision.py` `need_rebuild` | `slot_collision.py:407-458` | installed parent + `atom.soname or atom.slot_operator_built` |
| merge side | `Scheduler` FakeVartree | `Scheduler.py:436-445` | separate audit; non-goal here |

### 1.2 `_apply_dynamic_deps`, arm by arm (`FakeVartree.py:146-191`)

1. **No live metadata** (ebuild gone) → `_DynamicDepsNotApplicable`.
2. **Either EAPI unsupported** (installed or live) → not applicable.
3. **Built-`:=` rescue**: unless `--ignore-built-slot-operator-deps` and
   only if the *installed* EAPI supports slot operators, collect the
   built atoms from the **pre-overlay** metadata; if the live EAPI does
   not support them → not applicable; else **append** per key (do not
   replace) to the live value.
4. **Apply**: `aux_update` the five dep keys + `EAPI` + `KEYWORDS`.
5. **Fallback**: global package-move updates over the raw metadata.
   Portuale has `apply_updates_to_dep_string` (`lib.rs:615`, `:1291`).

USE is **not** overlaid: `_portdb_keys` excludes `USE`/`IUSE`, so
conditionals keep evaluating against the installed `vdb/USE` — exactly
what `enqueue_dependencies` already does (comment at `lib.rs:19635-19648`).

### 1.3 Portuale today — the consumer inventory (this table is S1's deliverable)

Every site that reads an installed package's `*DEPEND`; the verdict
column is this plan's *proposal* for the target model, to be confirmed
against real per site in S1.

Rust (`rust/portage-repo/src/lib.rs` unless noted; Python mirror
`python/emerge_pretend_reference.py`; line numbers drift):

| # | ~line | function | role | proposed layer |
|---|---|---|---|---|
| 1 | 19652 | `enqueue_dependencies` | `--deep` AlreadyInstalled walk | **Effective** (already is, without the append) |
| 2 | 5159 | `installed_reverse_dependents` | who depends on X | **Effective** |
| 3 | 5849 | `topological_removal_order` | unmerge ordering | **Effective** |
| 4 | 6069 | `unresolved_runtime_deps` | depclean breakage | **Effective** |
| 5 | 6258 | `depclean_cleanlist` | what may be removed | **Effective** |
| 6 | 6489 | `prune_cleanlist` | removal protection | **Effective** |
| 7 | 12620 | `required_set_reachable_cps` | `@world`/`@system` closure | **Effective** |
| 8 | 12720 | `rebuild_if_entries` | `--rebuild-if-*` | **Effective** |
| 9 | 13223 | `grandparent_use_conflict` | circular-dep suggestions | **verify** (portuale-only; nearest analogue) |
| 10 | 13328 | `installed_has_foreign_dependents` | | **verify** |
| 11 | 7367 | `deps_changed` | `--changed-deps[-report]` | **Raw** (real's `_raw_metadata` comment) |
| 12 | 11982 | `slot_operator_rebuild_scan` | built-`:=` detection | **Raw** (it *is* `find_built_slot_operator_atoms`) |
| 13 | 12411 | `slot_operator_eliminate_rebuilds` | #24-S4 rule 8 | **Gate 0.4** |
| 14 | `merge_order.rs:935` | `add_installed_dependency_closure` | scheduling edges | **Effective**, mind `is_injected_libc` (§9.3) |
| 15 | 5502 | `find_libc_deps` | libc provider identity | **Raw** (real `_installed_libc_deps` uses the original vartree) |
| 16 | 4975 | virtual expansion | installed virtual → provider | **verify** |
| + | 17120-17162 | `already_installed_deps` | `GraphEntry::deps` display | **Effective** (§0.2, S4) |

Python pairing is not 1:1 (helper names differ); S1 must publish the
explicit Rust↔Python map and flag any site present on one side only.
Not in scope (not the installed-metadata view): `regen.rs`,
`ebuild_merge.rs`, `ebuild_phases.rs`, `binpkg.rs`, `ebuild_package.rs`,
`solver_bridge.rs` (keeps its own `dynamic_deps: false`, #34),
`resolver_trace.rs`.

### 1.4 Upstream oracle to translate: `test_changed_deps.py::testChangedDeps`

Installed `app-misc/A-0` with **no** recorded deps; its ebuild has
`DEPEND`/`RDEPEND = app-misc/B`.

| case | options | expected merge list |
|---|---|---|
| 1 | `-uD --usepkg --dynamic-deps=n @world` | `[]` |
| 2 | `-uD --usepkg --dynamic-deps=y @world` | `["app-misc/B-0"]` |
| 3 | `-uD --usepkg --changed-deps=y @world` | `["app-misc/B-0", "app-misc/A-0"]` |
| 4 | `--usepkgonly --changed-deps=y app-misc/A` | `["[binary]app-misc/A-0"]` |
| 5 | `--usepkg app-misc/A` | `["app-misc/B-0", "app-misc/A-0"]` |
| 6 | `--binpkg-changed-deps=n --changed-deps=y --usepkg` | `["[binary]app-misc/A-0"]` |

**Trap:** upstream rebinds `test_cases` just before the loop, so only
case 6 actually executes. Pin all six, adjudicate any divergence against
a live `emerge`, never report "upstream passes these". Note this oracle
does **not** cover the built-`:=` append (plain atoms only): S2 must add
a purpose-built fixture for that.

---

## 2. Rules and invariants for every slice

Reuse `docs/024-slot-operator-plan.md` §2 verbatim. Added here:

1. **`--dynamic-deps=n` is a null transformation.** After every slice,
   `portuale emerge --dynamic-deps=n <anything>` must be byte-identical
   to the pre-slice binary. Script it once (S2), run it every slice,
   report it in the commit. Under the two-layer model this falls out
   structurally (`=n` ⇒ Effective == Raw), so a violation is a design
   smell, not a test failure to waive.
2. **Pin before you change.** Every moved behaviour gets an
   xfail/strict pin in S2 before the code slice. Same discipline as
   #24-S2.
3. **Rust == Python, empirically.** Run both binaries over `fixtures/`
   and diff; pytest-green alone is not acceptance (AGENTS.md step 4).
4. **One funnel for installed dep strings.** After S3 no new open-coded
   `read_vdb_string(…, <dep key>)` may appear; S1's inventory notes which
   sites keep `Raw` and why. Add a `rg` acceptance to the slice.
5. **Surface, don't chase.** A divergence revealed but not owned (e.g. a
   `_complete_graph` membership shift) becomes a finding with its oracle.
6. **Default-on means live-system behaviour.** Sites 4-6 decide what
   gets *unmerged*; this is not pretend-only. Any change there needs an
   L0 run, not just fixtures.
7. **Stop conditions.** S1 inventory > ~20 sites per language, or an L0
   delta touching merge order (cluster I / #17), or the perf memo not
   holding (Gate 0.7) → stop and re-scope with the owner.

---

## 3. Slices

### S0 — Correct the record (S, ½ h)

No code. Fix the places asserting a bug that does not exist:

- `docs/backlog-tasks.md:47` — retitle per §0.6, drop the "unit test
  disagrees" sentence, point at this plan.
- `docs/scope-backlog.md:134-137` — replace the "unexplained, needs
  owner eyes" clause with the §0.1-§0.3 summary and the corrected #27
  blocker.
- `docs/what-this-proves.md:15973-15975` is history: do **not** rewrite
  it (AGENTS.md step 7); the correction is a new paragraph in S7.

Acceptance: the §0.1 reproduction transcript re-run and pasted into the
commit body; `git grep` finds no remaining "both languages currently
walk ebuild deps" claim.

### S1 — Falsification package + inventory + decision table (M, F sanity-read, 2-3 h)

The artifact of the item. Produce a new `docs/026-oracle.md`:

1. Re-run §0.1's four commands and the `--debug` trace; paste raw output
   (including the `Depstring`/`Child` contradiction).
2. Fill §1.3's table for **both languages** with a real counterpart or an
   explicit "no counterpart, nearest analogue" and a `Raw`/`Effective`
   verdict per site.
3. Confirm §0.3.2 experimentally: attempt the #27 fixture with what
   exists today (installed parent + vdb built `:=` + a same-slot
   conflict) and record whether `c.parents` ever contains the installed
   parent. This settles whether S5 is required.
4. Confirm/deny that `--dynamic-deps` reaches real `depclean` (read
   `_emerge/depclean.py` + `_load_vdb` reachability) before claiming
   sites 4-6 are live-behaviour changes.
5. Check whether `revdepslotconsumer` / `revdepslottarget`
   (`fixtures/var/db/pkg/...`, md5-cache already has `:0/1=`) can be
   reused for the append pin before adding any fixture.

Why F-sanity: a wrong `Raw`/`Effective` call silently changes depclean or
merge order; the mid-tier must not own this alone.

Stop condition: inventory > 20 sites/language → re-scope (rule 7).

### S2 — Oracle pins, xfail-first (M, F brief, 2-3 h)

1. Translate §1.4's six cases into the contract suite (the
   `tests/test_emerge_pretend_contract.py` synthetic-root helpers near
   `_b1_root` ~15314 are the pattern). Cases 1/3/4/6 should pass today;
   case 2 is the default-mode check.
2. **Walk-source-observable fixture** (new; `changeddepspkg` cannot show
   it because its vdb dep is installed): an installed package whose
   ebuild RDEPEND names a *not-installed* `newpkg` and whose vdb RDEPEND
   names a *different not-installed* package. Default prints one, `=n`
   prints the other — a stdout pin for the actual bug this item was
   accused of.
3. **Append xfail**: installed pkg, ebuild RDEPEND empty (or no `:=`),
   vdb RDEPEND `cat/pkg:S/SS=` with the target not installed (or at a
   different sub-slot). xfail-strict that the default-mode walk sees the
   built atom.
4. `--ignore-built-slot-operator-deps` pin: append suppressed, ebuild
   deps unchanged.
5. Script the §2.1 null-transformation diff and commit the script under
   `TEST/` (no binary artifacts).

Acceptance: new pins fail only where expected; no other pin moves; the
xfail markers carry the replacing slice's name.

### S3 — Effective installed-metadata helper + built-`:=` append (M writes, F reviews, 4-6 h)

Implement the two-layer view from §0.3.3/§1.2, both languages, one commit:

```rust
/// Real `Package._raw_metadata` vs `Package._metadata` for an installed
/// package. `Raw` = the vdb record verbatim; `Effective` = what
/// `FakeVartree._apply_dynamic_deps` leaves behind.
enum InstalledMetaLayer { Raw, Effective }
fn installed_dep_string(view: &InstalledMetaView, pkg: &InstalledPackage,
                         key: &str, layer: InstalledMetaLayer) -> String
```

- `Effective` with `dynamic_deps == false` **byte-identical to `Raw`**
  (the whole-slice safety invariant).
- `Effective` with `dynamic_deps == true` = live md5-cache value for the
  key + `" "` + space-joined `slot_operator_built` atoms found in the raw
  value (reduce the raw key against the package's vdb `USE`; only atoms
  with a sub-slot qualify — `:2=` does **not**).
- `--ignore-built-slot-operator-deps` suppresses only the append.
- Not-applicable arms: missing live metadata / unsupported EAPI →
  fall back to raw + `apply_updates_to_dep_string` (Gate 0.6).
- **Memo** per `(cat,pkg,ver)` per pass; real preloads once per depgraph
  and is explicit that re-application is a bug
  (`dynamic_deps_applied`). Measure `-puD` against
  `docs/performances-tuning.md` §5 and report the delta (Gate 0.7).
- Convert only `enqueue_dependencies` in this slice (the single
  behaviour-changing consumer). The Python mirror gets the same helper
  name (`_installed_dep_string`) and layer enum.
- EAPI/KEYWORDS overlay is a Gate 0.5 carve-out (documented in the
  helper).

Acceptance: S2's append xfail flips to pass; case 2 of §1.4 passes; the
`=n` null-transformation script is byte-identical; fmt/clippy
zero-warning/`cargo test --release`/`pytest tests -q` green; perf delta
reported.

### S4 — Remaining `Effective` consumers + `GraphEntry::deps` (M writes, F reviews, 3-5 h)

Two related changes, separate commits so L0 can bisect:

1. `already_installed_deps` (`lib.rs:17120-17162`, assigned 17302) is
   built from the same helper, so `--debug`/`--tree`/`--json`/ancestor
   edges can no longer contradict the walk under `=n`. This is
   behaviour-neutral for resolution, visible in display pins; add a
   pinned `--debug` check for `changeddepspkg --dynamic-deps=n` that the
   `Depstring` and the `Child` line agree.
2. Convert sites 2-10/14/16 per §1.3, one commit per group (depclean
   group 4-6 first, with its own L0 run — rule 2.6). Sites 11/12/15 stay
   `Raw` and are routed through the helper with `Raw` so acceptance 2.4
   can be a grep. Site 13 waits for Gate 0.4.

Acceptance: per-group pins; no open-coded dep-key vdb reads left except
through the funnel; L0 after the depclean group.

### S5 — Installed-parent pullers + the #27 fixture (M, F reviews, 2-3 h)

1. Record `slot_pullers` from `enqueue_dependencies`' flattened atoms
   (mirror 18432-18442; owner key/version are already parameters). This
   is the actual #27 unblocker (§0.3.2). If S1.3 shows the trailer can
   already fire through a Reinstall parent, record that and shrink this
   slice to the fixture + a regression guard.
2. Build the `need_rebuild` fixture: installed parent with a built
   `:S/SS=` atom reaching a same-slot conflict, exercising all three
   reasons (`--exclude`, `--useoldpkg-atoms`, masked/unavailable ebuild)
   and the `--usepkgonly` silence (real `slot_collision.py:407-458`).
3. If it still cannot fire, **stop and report why** — that is a finding
   about the conflict renderer (`pretend.rs:10901-11201`), not a reason
   to keep patching. Update #27's status either way.

Watch: extra puller lines can move currently pinned slot-conflict
outputs (the `slotuse*`/`scuse*` pins). Every moved pin names its oracle.

### S6 — Real-tree validation (F triage, 2-3 h)

`TEST/run/l0-resolver.sh`; baseline #24-S6: 120 probes, 96 clean,
parity 0.800. The default-mode change is broad (16 sites, depclean, the
merge-order closure), so a delta is *expected* somewhere; adjudicate each
against real, do not re-tune `merge_order.rs` inside this item (that is
#17). Run `TEST/run/l1-merge-from-binpkg.sh` only if S1 marked a
merge-path consumer, and without `L1_SKIP_PORTAGE_UPGRADE=1` (memory note
`real-world-testing-plan`). Findings go to `TEST/findings/l0.md`.

### S7 — Docs closure (S, ½ h)

`what-this-proves.md`: new paragraph at the end (history rule) with the
§0.1 transcript and one built-`:=` example, runnable. Update
`docs/scope-backlog.md`, `docs/backlog-tasks.md` (#26 → DONE or
re-scoped with the new number; #27 per S5), `docs/026-oracle.md` verdict
table, `docs/024-dynamic-deps_n_disagreement.opus.md`/`.deepseek.md`
status line.

---

## 4. Difficulty and routing

Overall **5/10**: no search, no backtracking, no ordering algorithm. The
cost is **width** — two languages, ~25 sites, byte-parity, plus the risk
that a wrong layer silently changes depclean/unmerge on a real system.

Compare: much easier than #22/#23/#24; harder than a Tier-1 one-file
slice (depclean + merge-order blast radius).

| Slice | Work | Tier | Why |
|---|---|---|---|
| S0 | record correction | **S** | mechanical; transcript in §0.1 |
| S1 | falsification + inventory + decision table | **F** (M may draft) | ~16 real-semantics calls with silent-wrong failure modes; earns the item |
| S2 | oracle pins, xfail-first | **M** (F brief) | pattern-following; the §1.4 trap needs flagging |
| S3 | helper + append + memo | **M** write + **F** review | spec-driven once S1 exists; invariant `=n` must hold exactly |
| S4 | consumer conversion + entry deps | **M** write + **F** review | each site is a judgement; L0 bisecting |
| S5 | pullers + #27 fixture | **M** + **F** review | conflict pins can move; fixture craft |
| S6 | L0 triage | **F** | adjudicating deltas against real |
| S7 | docs | **S** | |

**Can a cheaper model do it?** Not end to end. S1 and S6 are where a
mid model reliably fails here: S1 because "which layer" has no local
signal (the code compiles and all tests pass either way), S6 because an
L0 delta looks like noise until checked against real. S0/S2/S7 are safe
for a small model; S3/S4/S5 are safe for a mid model **only after S1's
table is committed**, otherwise it will thread a boolean through every
site it finds and produce a large, plausible, wrong diff.

Effort, frontier-hours of agent time: S0 ½, S1 2-3, S2 2-3, S3 4-6
(+review), S4 3-5 (+review), S5 2-3, S6 2-3, S7 ½. Total ≈ 12-18 h across
2-3 sittings.

---

## 5. Sequencing and gates

```
S0 ── S1 ──┬─ go? ─ S2 ─ S3 ─ S4a(display) ─ S4b(depclean) ─ S4c(rest) ─ S5 ─ S6 ─ S7
           └─ no-go: S0 closes the item, S1 table frozen as reference
```

- S0 and S1 are docs-only and can land without a code gate; S1's "go?"
  is Gate 0.1/0.2.
- S2 lands on a fixtures/tests/docs branch; it must not include code.
- S4a (display) precedes S4b/S4c because it makes the walk/edge
  contradiction visible in the very output later slices are validated
  against.
- S5 may move to #27 if Gate 0.6 says so; S6 runs after every
  behaviour-changing group (S3, S4b, S5), not only at the end.
- Do not start S3 before S1's table is committed: that is the whole
  reason to front-load it.

---

## 6. Gate 0 — owner decisions (answer before S2 lands)

| # | Question | Recommendation |
|---|---|---|
| G0.1 | Is #26 closed by S0 with the real work re-scoped as a new item, or is the whole plan one item? | S0 closes #26, re-scope new item — keeps the backlog honest and the blast radius bounded |
| G0.2 | Does `GraphEntry::deps` follow the walk source under `=n` (and the effective layer under `y`)? | yes (S4.1); it is display-only and kills the recurring misreading |
| G0.3 | Do sites 2-10/14/16 convert to `Effective` in this item, or only the walk + depclean group? | convert by group with L0 between (S4); do not land one 25-site diff |
| G0.4 | `slot_operator_eliminate_rebuilds` rule 8 (site 13): `Raw` or `Effective`? | `Effective`; real's `_eliminate_rebuilds` makes no `_raw_metadata` call, unlike `_changed_deps` 25 lines earlier. Pin `slotundo-unnecessary` (`abi_rebuilds: []`) either way |
| G0.5 | EAPI/KEYWORDS overlay in scope? | carve out, document in the helper |
| G0.6 | Not-applicable fallback (global updates) in scope? | in; `apply_updates_to_dep_string` already exists, and leaving it out makes "ebuild deleted" silently wrong |
| G0.7 | Perf budget on the `-puD` baseline if the memo is not enough? | none; stop at S3 and re-scope |
| G0.8 | S5 in this item or attached to #27? | either; the unit is the same, the puller recording is not optional |

---

## 7. Definition of done

- `docs/026-oracle.md` verdict table: every inventory row has a layer
  and a citation; §1.4's cases 1-6 pinned; Rust == Python.
- Default mode walks vdb built `:=` atoms (fixture proves it); `=n` is
  byte-identical to pre-slice on the full fixture corpus (scripted).
- `--debug` under `=n` no longer contradicts itself (`Depstring` ==
  walked child).
- #27's fixture fires `need_rebuild` (three reasons + `usepkgonly`
  silence) or the inability is filed with evidence; #27 status updated.
- Full pass green: `cargo fmt --check`, `cargo clippy --release
  --all-targets` zero warnings, `cargo test --release`, `pytest tests
  -q`; failing-test *names* compared to the clean-`main` baseline, not
  counts (memory note `contract-suite-pollutes-fixtures`).
- L0 no regression vs the S0 archive, or every delta adjudicated in
  `TEST/findings/l0.md`; `-puD` perf delta reported.
- Docs per S7; no prior `what-this-proves.md` paragraph rewritten.

---

## 8. Review checklist (attach to every slice PR)

- [ ] Gate: fmt / clippy zero-warn / `cargo test --release` / `pytest tests -q` green; failing-test names vs clean-`main`
- [ ] Rust and Python diffed empirically over `fixtures/` and every `_b1_root` case
- [ ] `--dynamic-deps=n` null-transformation script identical
- [ ] Every moved pin names its upstream test, fixture, or L0 probe
- [ ] S3: `Effective` with `dynamic_deps=false` byte-identical to `Raw` (unit test both languages); append only for sub-slot atoms; `--ignore-built-slot-operator-deps` gate tested; memo + perf delta
- [ ] S4: no open-coded dep-key vdb read left outside the helper; `--debug`/`--tree`/`--json` agreement pin
- [ ] S5: puller lines carry owner cpv+atom; conflict-renderer pins diffed against the clean-`main` baseline
- [ ] S6: L0 raw diff archived; findings filed with oracle
- [ ] Comments carrying real citations moved, not dropped (`git diff` on `//` lines nets ≈ 0 except new code)
- [ ] New fixtures `git add`ed before any `git clean` (AGENTS step 5)
- [ ] Judgment calls surfaced, not defaulted
- [ ] No `git commit`/`git push` unless the user asked

---

## 9. Risks

1. **Perf.** The append adds an md5-cache read per installed package per
   site per backtracking pass. Unmemoized this is the `-puD` regression
   the repo bought back on 2026-09-07 (`perf-investigation-2026-09-07`).
   Memo in S3; Gate 0.7.
2. **Depclean blast radius.** Sites 4-6 decide unmerges, and
   `--dynamic-deps` is on by default — a live-system change. S2 needs a
   depclean pin, S4b needs L0, not just fixtures.
3. **`is_injected_libc` interaction.** `merge_order.rs:935`'s
   `add_installed_dependency_closure` strips `doebuild._inject_libc_dep`'s
   phantom `>=glibc` from the raw vdb (cluster I slice 10, `264163f`).
   On the `Effective` path the phantom is not there in the first place,
   so the strip may become dead there and must stay live on `Raw`.
   Getting this wrong regresses ~40 L0 order findings. Read
   `is_injected_libc`'s doc before converting site 14.
4. **#24-S4 coupling.** Gate 0.4 can move `slotundo-*` pins;
   `docs/024-oracle.md` is the regression guard.
5. **Suite isolation.** `git clean -fdq fixtures/` deletes untracked
   fixtures; the suite has a pre-existing order-dependent isolation bug —
   compare failing test names, never counts.
6. **Scope creep.** The inventory can grow into "port the whole
   FakeVartree"; the EAPI/KEYWORDS carve-out and the G0.3 grouping are
   the fence.

---

## 10. Non-goals (state in the docs, do not drift)

- Real's parallel preload (`TaskScheduler` + `EbuildMetadataPhase`,
  `depgraph.py:960-980`): portuale reads md5-cache directly; the memo is
  the counterpart.
- `Scheduler`-side FakeVartree (`Scheduler.py:436-445`) — merge path,
  separate audit.
- `--ignore-soname-deps` / `PackageDbapiProvidesIndex` — soname atoms
  are an established unreachable cut.
- `--solver=` bridges keep `dynamic_deps: false` (#34).
- EAPI/KEYWORDS overlay (pending Gate 0.5).
- `--dynamic-deps` timing/`--debug` narration parity (no such real line).
