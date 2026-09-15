# Recap: backlog planning/investigation docs retired 2026-09-15

This file is the extracted, LLM-useful content of 35 planning/design/
investigation docs moved to `docs/history/` on 2026-09-15 (full list at
the end). Those documents were agent briefs, multi-model drafts, and
oracle write-ups written *before or during* the work; their outcomes
are already recorded, tersely, in `docs/backlog-tasks.md` (the status
line for each numbered item) and `docs/what-this-proves.md` (the
permanent, append-only "what got proven" record with runnable
commands). **Read those two first for "is it done and what shipped".**
This file exists for what those two don't carry: the real-Portage
mechanism ground truth, the traps that cost someone a wrong turn, and
the dead ends — the stuff a future session re-touching this code would
otherwise have to re-derive from the vendored `3rdparty/portage/`
source or rediscover by trial and error.

**A recurring process note, not a technical fact:** several of these
items (#22, #24, #26, #37, #38, Tier-1 brush/fetch) were worked by
asking 2-3 independent LLMs (deepseek/claude/chatgpt/musespark/fable/
opus) to draft a plan, then merging the drafts into one canonical
`.plan.md` / the most-corrected draft, with a `§0 Adjudications` table
recording where they disagreed and which checkout evidence settled it.
The individual drafts are gone; the adjudication tables' *conclusions*
are folded into the sections below where they changed the outcome.

Every `file:line` citation below was correct in the source document at
the commit it was written against (mostly `ec13936`/`be76d0d`/`c7ad477`
in September 2026) and **will have drifted**. Re-locate by symbol name
(grep the function/const name) before trusting a line number — this is
the same rule `AGENTS.md` step 1 states for the live docs.

---

## #19 — DFS-partial merge-list truncation → the abort-path model

**Status:** DONE 2026-09-11, S1-S6 (`docs/backlog-tasks.md` #19). Spec
kept live at `docs/abort-path-spec.md`.

**The reframe that unblocked it:** the backlog line read as "make
portuale's merge list match real's DFS truncation point", which looked
like a resolver-architecture rewrite (BFS → DFS). The actual shape,
found by reading `depgraph.py`'s abort paths together with two other
parked items (masked-dependency abort, circular-dep partial list):
real doesn't truncate by DFS position, it **abandons the resolve**
outright when it hits an unfixable state (masked-only dep, an
unserializable cycle, a mid-walk unsatisfiable atom). The visible
"partial list" is just whatever had been accepted before the abandon,
`Total: N` is cumulative over that partial list, and exit is 1. So #19
is **one abort-outcome model with three consumers**, not a traversal
rewrite: `enum ResolveOutcome { Complete, Aborted { reason, partial } }`
threaded through `GraphResult`, gated during rollout by
`PORTUALE_ABORT_PATH` (kept as the permanent flag; `=0` is the legacy
"report, don't enforce" behaviour some call sites still want).

**Trap that mattered:** don't try to byte-match real's exact
truncation *point* on the first pass — that point is a function of
real's DFS visit order, which a BFS resolver doesn't have. Get
membership/counters/exit-code semantics right against fixtures first;
visit-order parity (if ever wanted) is a separate, harder slice with
the real-tree L0 bed as the only real oracle.

**Residue (deliberately not #19):** plasma-meta/podman merge-list
truncation is a `||`-selection gap (real's `get_best_run()` choosing
between partial backtrack attempts), tracked separately — see
[[cluster-i-merge-order]] / `circular-dep-backtracking-parked` in
project memory; that branch is parked unmerged, not abandoned.

---

## #22 — `dep_zapdeps` finer choice bins

**Status:** DONE 2026-09-11 (`docs/backlog-tasks.md` #22); its
downgrade-guard tail shipped later as #35 (2026-09-15).

**Two backlog-line corrections that mattered before any code:** the
function is real `dep_zapdeps` in `lib/portage/dep/dep_check.py`
(**not** `depgraph.py`, which only mentions it in comments), and
`rust/portage-repo/src/solver_bridge.rs` is **out of scope** — it only
walks `DepEntry::AnyOf` to over-approximate a closure for pubgrub/
resolvo; there is nothing zapdeps-shaped to port there (that's #33/#34
territory).

### Ground truth: the algorithm, condensed

Real ranks every `||` alternative into **eight** bins (not nine —
`preferred_installed` and `preferred_any_slot` are the *same list
object* as `preferred_in_graph`, a trap that fooled more than one
draft):

```
0 preferred_in_graph      (== preferred_installed == preferred_any_slot, ONE bin)
1 preferred_non_installed
2 unsat_use_in_graph
3 unsat_use_installed      (keys on all_installed_slots, NOT all_installed — a second trap)
4 unsat_use_non_installed
5 other_installed
6 other_installed_some
7 other_installed_any_slot (bug 522652, cp-level fuzzy match)
8 other
```

Three independent per-alternative facts feed the classification (a
weak model collapses these into one boolean — don't):
`all_available` (every atom matches a visible package, USE ignored),
`all_use_satisfied` (those matches also satisfy USE deps),
`all_use_unmasked` (bug 515584: when USE is unsatisfied, are the flags
that would need changing sitting in `use.mask`/`use.force`?).
Classification shape (`graph_db` present, portuale's case):

```
not all_available:
    all_installed        -> other_installed
    some_installed        -> other_installed_some
    any cp installed      -> other_installed_any_slot
    else                  -> other
conflict_downgrade or installed_downgrade or circular_atom -> other
all_use_satisfied:
    all_in_graph or all_installed -> preferred_in_graph (aliased)
    else                          -> preferred_non_installed
else (USE unsatisfied):
    not all_use_unmasked  -> other
    all_in_graph           -> unsat_use_in_graph
    all_installed_slots    -> unsat_use_installed
    else                   -> unsat_use_non_installed
```

Two-pass return: `for allow_masked in (False, True): for bin in bins:
for choice in bin: if choice.all_available or allow_masked: return`.
Consequence: the whole `other_*` family only ever returns on pass 2 —
its effect is entirely on the failure path (autounmask / #20's masked
disclosure), never on a healthy `@world`.

In-bin reorder (only the part with a *user-visible* effect on a
healthy system — do this slice first, it has no USE-machinery
dependency): promote an upgrade over a non-upgrade, an in-graph choice
over a not-in-graph one (unless that would eliminate an upgrade), and
`all_installed_slots` over any-slot — all inside the winning bin, never
across bins. Portuale's pre-#22 `resolve_disjunctions` had an early
`break` on the first `Installed`-rank alternative that made any in-bin
ordering structurally impossible; removing it was itself a design
decision (traded for accepting a full-ordering pass over every
alternative).

**More traps, each cost someone a wrong port:** the bug-600346
`continue` (an internally-inconsistent `cp_map` entry) skips only the
`cp_map` update, not `slot_map`; a nested all-of alternative inside a
`||` contributes only its *unsatisfied* atoms to that alternative's
own atom set; `atom_currently_satisfiable`-style helpers must not
silently do the without-USE and with-USE candidate listing twice for
every atom (the overwhelming majority of atoms have no `[use]` block,
so both probes are byte-identical work — split "is it available" from
"does it satisfy USE" and reuse the first result when there's no `[use]`
block to strip).

**Where it lives now:** one shared closure, `disjunction_preference`
in `rust/portage-repo/src/lib.rs` (used by both the main New/Upgrade
walk and the `enqueue_dependencies` AlreadyInstalled recursion — a
real bug (F1 in the 022-fix follow-up) was exactly these two closures
drifting out of sync after the first commit only updated one). The
downgrade guards (`conflict_downgrade`/`installed_downgrade`, bug
531656) were deliberately deferred out of #22 (documented cut: "needs
a live mutating `graph_db` neither implementation had") and shipped
five days later as #35 once the resolver's own in-progress `entries`
turned out to already **be** that live graph_db — see the #35 entry
below.

---

## #23 — backtracking resolver search (`Backtracker`)

**Status:** DONE 2026-09-11 (`docs/backlog-tasks.md` #23). Design:
`docs/023-backtracking_resolve.md`; oracle verdict table:
`docs/023-oracle.md`.

Ported real's actual search shape: a node-stack DFS (`Backtracker`:
`get`, param-only dedup), bug-375573's `_check_runtime_pkg_mask`
discard-with-reason, config/slot-conflict/missing-dep `feedback`, and
`get_best_run` — replacing what had been a linear one-bundled-trial
retry loop (`MaskPhase`). `--backtrack=N` bounds `mask_steps`; config
retries stay free; a dead end is abandoned rather than settled into
the abort path. `023-oracle.md`'s method — translate an upstream
`lib/portage/tests/resolver/*.py` case (its own `ResolverPlayground`)
into a portuale fixture and diff the two mergelists — is the pattern
reused by nearly every later resolver item (#24, #35, #36); see
"Oracle methodology" at the end of this file.

**Genuine finding from the second-opinion review (not closable inside
#23):** case `btnr` (upstream `testBacktrackNoWrongRebuilds`) stays
divergent for an architectural reason, not a search-shape bug — an
installed package that resolves via the `AlreadyInstalled` fast path
never becomes a slot-conflict party (`resolved_slots` indexes
merge-bound outcomes only), so **no direct slot conflict can even
form** for the case the search is supposed to discard. This is the
same underlying gap #25 spent its whole life on (installed instances
never being first-class graph participants) — see the #25 entry.
`btnr` is still open under #25's residue as of 2026-09-15.

**`023-refactor-inventory.md`'s only lasting value** is the *method*,
not its tables: before refactoring a ~2900-line function into a struct
split (`ResolveCtx`/`BacktrackParams`/per-pass `PassState`), someone
did an exhaustive grep-and-manually-verify pass over every variable's
write sites and every `continue 'backtrack` guard, and recorded the
counts (15 cross-pass `let mut`s, 24 per-pass, 7 retry sites) to check
the plan's own estimates against reality before trusting it. The
line numbers and even the function's shape are long stale (the
function has since become the named `run_pass`/`collect_feedback`/
`assemble_result` pipeline); don't use its tables, reuse its method
for the next big-function refactor.

---

## #24 — slot-operator rebuild undo path

**Status:** DONE 2026-09-12, S1-S7 (`docs/backlog-tasks.md` #24). Plan:
`docs/024-slot-operator-plan.md`; verdict table: `docs/024-oracle.md`.

Real ships a `_eliminate_rebuilds` pass with **eight ordered rules**
plus a graph-aware `:=` re-binder; a demoted rebuild is *undone* (not
just skipped) via a `slot_operator_undone` latch that the next
backtrack pass consults. The a522084 `B-0` rebuild-miss symptom was
misdiagnosed for a while as "the undo path is missing"; it was
actually that the complete-mode gate silently skipped under `--deep`
(S1) — the undo machinery (S4) was the *second* fix, not the first.
By S3 the rebuild became a **walked graph node** routed through
`Backtracker` itself (seeded like real's own
`@__auto_slot_operator_replace_installed__` `SetArg`), not a
post-resolution synthetic entry — that's what let deps, `:=`
re-binding and merge order all come out real-shaped for free instead
of needing three separate hacks.

**`024-S4-review.md`'s one durable finding:** bug 614390
(`test_oracle_slotop_complete`) stays `xfail(strict=True)` *after* S4,
and the review traced why: it's not an undo-rule bug at all, it's the
same selection-fast-path gap `btnr` (#23) and #25/#36 keep surfacing —
a bare `dev-libs/socc` resolves via the already-installed fast path
before `meta`'s later `=socc-1` pin is seen, and that fast path never
consults `resolved_slots`, so real's `_add_pkg` slot-parent check
(`depgraph.py:2160-2185`) never gets ported equivalent. Filed under
#36's territory, not #24's; the review's recommendation (accept S4 as
shipped, keep 614390 xfail, don't drag a selection-path fix into a
slot-operator slice) is what happened.

**Non-goals, explicitly cut and still cut:** the v2 probe family
(`#24b`: update probe + `check_reverse_dependencies`), `#24c`
(`slot_operator_mask_built`), `#24d` (`prune_rebuilds`), `#24e`
(`_slot_conflict_backtrack_abi`), `#24f` (in-walk unsatisfied probe),
`IUSE_EFFECTIVE` in the built-dep domain, `--rebuild-if-*` through the
same path, the `--debug` "backtracking due to missed slot abi update"
narration.

---

## #26 — installed-package metadata overlay (`--dynamic-deps` two-layer model)

**Status:** DONE-PARTIAL 2026-09-12 (`docs/backlog-tasks.md` #26). This
is the item the two `024-dynamic-deps_n_disagreement.{opus,deepseek}.md`
drafts cover — the filename kept the "024" the user typed, but the
actual backlog number is **#26**.

**The backlog line itself was wrong** ("the CLI and a unit test
disagree on `--dynamic-deps=n`'s walk source") — verified live, they
don't disagree; the CLI switches the walk source exactly like the
tests already asserted. The real bug was one level deeper and had been
misdiagnosed as the flag not working.

### Real's two-layer model (the actual mechanism)

Real does **not** pick "ebuild deps OR vdb deps" per call. It applies
`--dynamic-deps` **once, up front, to the whole installed-package
view**, before any consumer ever reads it:

- `depgraph._load_vdb` (`depgraph.py:886-945`) builds one `FakeVartree`
  shared across every backtracking pass, and `_dynamic_deps_preload`
  overlays every installed package's metadata in parallel.
- `FakeVartree._apply_dynamic_deps` (`FakeVartree.py:146-191`): if
  `dynamic_deps` is on, overwrite the installed package's metadata with
  the **live ebuild's**, then **append the vdb's own recorded built
  `:=` atoms back on top** (`find_built_slot_operator_atoms`,
  `dep/_slot_operator.py:24-38` — only atoms with both slot *and*
  sub-slot count as "built"; `foo:2=` is not, `foo:2/2=` is). This is
  an **append, not a replace** — the ebuild's unbound `:=` and the
  vdb's bound one coexist.
- The handful of consumers that must see the **raw** vdb record
  (never the overlay) explicitly opt out via `pkg._raw_metadata` — with
  a comment saying why. `depgraph._changed_deps` is the canonical
  example ("`--changed-deps` must not interact with `--dynamic-deps`").
- `EAPI`/`KEYWORDS` are overlaid too, not just the five `*DEPEND` keys
  (`FakeVartree._portdb_keys`).

So real has exactly **two layers** per installed package — `Raw` (the
vdb record verbatim) and `Effective` (the overlay above) — and every
consumer must be assigned to one or the other; there is no third
option. Portuale had only the Raw layer, plus one lone call site
(`enqueue_dependencies`) that sometimes read the ebuild instead — every
*other* installed-metadata consumer (~16 Rust sites, ~9 Python) silently
ignored `--dynamic-deps` entirely, in the direction that matters most:
under the **default** (`--dynamic-deps=y`), they were all wrong.

**A genuine cross-check finding, not obvious from either draft alone:**
one draft (opus) proposed the append above as sufficient to unblock #27
(`need_rebuild`'s slot-conflict trailer). A second, independent draft
(deepseek) found that assumption **wrong**: an installed parent's own
dependency atoms were never being recorded into `slot_pullers` at all
(`enqueue_dependencies` — the only walker of an installed package's
deps — had no `slot_pullers` parameter), so `slot_conflict_need_rebuild`
had no parent to render regardless of what the atom looked like. Both
fixes were needed; what shipped (`InstalledMetaLayer` + the built-`:=`
append **and** installed-parent `slot_pullers` recording) matches the
second draft's diagnosis. Lesson for future multi-draft work: a
"payoff" claim in one draft is not verified just because it sounds
plausible — cross-check it against the actual call graph.

**Where it landed, and why it's DONE-**PARTIAL**, not DONE:** shipped
as `InstalledMetaLayer::{Raw,Effective}` + `installed_dep_string` (per-
pass memo — real preloads once per depgraph and portuale re-reads the
vdb every backtrack pass, so an unmemoized version would regress the
2026-09-07 17× perf win), with `--ignore-built-slot-operator-deps`
gating the append and EAPI/KEYWORDS deliberately carved out (documented
cut, not this item's scope). The built-`:=` **append itself** stays
behind `PORTUALE_DYNAMIC_DEPS_APPEND` (default off) because turning it
on exposed a real regression (F-A1, `docs/025-tier2-closeout.deepseek.md`
§11): a solvable slot conflict that real resolves via its slot-op
probe got folded incorrectly, and a cascade probe dropped a consumer.
As of 2026-09-15 (post python-reference-removal), only one Rust test
(`test_oracle_slotop_undo_cascade`) still fails with the gate on — the
whole remaining blocker.

**A load-bearing interaction to know before touching this again:** the
`is_injected_libc` strip in `merge_order.rs`'s
`add_installed_dependency_closure` (cluster-I slice 10 — see the L0
merge-order memory note) exists *because* portuale reads the raw vdb,
which carries `doebuild._inject_libc_dep`'s phantom `>=libc` atom that
real never sees under its own default dynamic-deps walk. If a future
change moves that call site's layer from `Raw` to `Effective`, the
strip becomes dead code on that path (the phantom atom simply isn't
there under `Effective`) and must not also run — re-read
`is_injected_libc`'s doc comment before touching it; getting this
wrong regresses ~40 L0 order findings.

---

## #25 — `_complete_graph` installed nomerge nodes (F-B3/F-B4, R3 series)

**Status:** DONE-PARTIAL 2026-09-15 (`docs/backlog-tasks.md` #25).
Design: `docs/025b-complete-graph-nodes.md` (§9 carries the final
correction). Closeout evidence: `TEST/findings/l0.md` "R3b", "R3c
attempt", "R3c revalidation", "R3e′ run".

This was the last, largest merge-order item, run as slices R3a-R3e′
across several waves. **Read `TEST/findings/l0.md`'s "R3c
revalidation" section directly if you touch merge order again** — it
corrects an earlier wrong theory (see below) with a fresh real
`--debug` oracle, and the docs in `docs/025b-complete-graph-nodes.md`
§8 are the *superseded* version that the §9 correction fixes.

**What it actually found, across three sub-problems:**

- **F-B3 (pre-bias order):** looked like a scheduler-graph DFS bug; was
  actually the resolver's own *seed* order. Real `_resolve` processes
  each `SetArg`'s atoms `sorted(arg.pset.getAtoms(), key=str)`
  (`depgraph.py:5500` — a `>=`-prefixed atom sorts before a plain one);
  portuale expanded `@world`/`@system` in raw profile `packages` order.
  Sorting just the appended `@system` segment (R3b, `9284f8c`) fixed
  the `-pe @system` divergence from index 2 to three small hash-order
  residues. `GraphEntry::insertion` (a whole new field) turned out to
  be unnecessary — `build_digraph`'s existing DFS already reproduces
  real's insertion order once the *seed* does.
- **F-B4 (an installed node's in-edges vanishing on same-slot
  supersede — the `nghttp2 -> systemd` gedit/nautilus symptom):**
  the wrong theory, tried twice (`R3c`, both reverted): "real draws no
  edge for a dep initially satisfied by installed" / "an installed
  node's in-edges vanish when a same-slot merge supersedes it". A
  fresh, **merged-stream** real oracle (stdout+stderr in one file, so
  the walk/solver/digraph-dump ordering is visible — a separate-stream
  capture hides this) showed the real mechanism is different: real's
  `_solve_non_slot_operator_slot_conflicts` removes the superseded
  installed node (dropping all its in-edges) and then **re-walks its
  parents**, which **redirects** their edges to the merge — i.e. the
  edge comes back, it doesn't vanish. In gedit/nautilus that re-walk
  happens to *abort* partway (at gnome-keyring's unsatisfiable
  `gcr[gtk]`, the same rc-1 failure the probe already has), so the
  parents still queued when it aborts (nghttp2 among them) never get
  their edge redirected — and *which* parents are queued first is
  CPython set-iteration order, not portable. When the re-walk
  completes (most probes), every edge redirects exactly like portuale
  already does. **Consequence: "satisfied-at-add ⇒ no edge" is false in
  general** — the blunt version of that rule broke `_system`/`_world`
  when tried, which is exactly why. F-B4 reclassifies as cluster-A
  abort residue (`gedit-nautilus-cluster-a-rewalk-abort` in
  `TEST/compare/known-divergences.yaml`), not a #25 architecture gap.
- **The original item (installed nodes as first-class graph
  participants):** turned out to already be met **at the scheduler-
  graph level** — node sets equal real's on 16/18 L0 order probes once
  F-B3 was fixed. What is *not* met is the resolver-selection level:
  an installed package resolved via the `AlreadyInstalled` fast path
  never enters `resolved_slots`, so it can never become a slot-conflict
  *party* — this is the same root cause `btnr` (#23) and 614390 (#24)
  hit. That's the genuinely open residue, and it needs a resolver
  change (not a scheduler-graph one) to close.

**Residues, named, not chased further:** the `virtual/man` `||`-bundle
pop-timing (12 nodes, 22 positions late) and a `docbook-xml-dtd`
4.5/4.2 swap from real's own hash-seeded `_minimize_children` sets
(`depgraph.py:4751-4781`) — both on `-pe @system`; and `btnr`'s
installed-instance-as-slot-conflict-party gap, shared with #23/#24/#36.

**Lesson for the next session that thinks it found the F-B4
mechanism:** capture real's `--debug` output with stdout and stderr
**merged into one file, unbuffered** (`PYTHONUNBUFFERED=1 ... >log
2>&1`). A prior session's separate-stream capture hid the walk/solver/
dump ordering and produced the wrong theory that cost two implementation
attempts before the merged-stream re-capture corrected it.

---

## #28 — mrg-director: wire the remaining scaffolded slots

**Status:** DONE 2026-09-15 (`docs/backlog-tasks.md` #28). Call-graph
snapshot + the "should `Director` be the production entry" decision:
`docs/028-director-proposal.md`.

`mrg-director` (`rust/mrg-director/src/lib.rs`) defines eight traits
(`Resolver` — a re-export of `portage_repo::Resolver` — plus
`SchedulerPolicy`, `MergeEngine`, `NewsSelector`, `Fetcher`,
`PackagesDb`, `BinpkgIndex`, `RepoCache`) meant to let a different
algorithm/backend swap in behind one constructor argument. H1-H4
(2026-09-14/15) routed the remaining four traits' real production call
sites through them (`Fetcher` → `fetch_src_uri`'s candidate loop,
`PackagesDb` → CONTENTS reads via `VdbReader`, `BinpkgIndex` → the
remote `Packages` lookup via `RemoteBinhostIndex`, `RepoCache` → the
build-entry metadata read, which itself delegates to the #41 decision
point — cache → depcachedir → depend-phase fallback). **`Director`
itself stays test-only, by owner decision (D6, "slots only")**: every
slot now has exactly one production implementation and no runtime
selection need, promoting `Director` would be a `pretend::run`
decomposition (ask/display/resume/`--keep-going` as explicit stages)
rather than a wiring change, and it would touch every L1/L2/L3-gated
merge path for zero behaviour change. `PackagesDb::reverse_dependents`,
`PkgdirBinIndex` (local `$PKGDIR` still resolves by directory scan) and
`VolatileCache` stay consumer-less by design, not bugs.

**Revisit triggers**, if they ever fire: a second implementation in a
non-solver slot needing runtime choice (e.g. a content-addressed
`MergeEngine`), a non-`emerge` front end that shouldn't go through
`pretend::run`, or wanting the `action_build` stage split for its own
sake (resume/`--keep-going` owned by one type).

---

## #35 — `downgrade_probe` + live `graph_db` for `dep_zapdeps`

**Status:** DONE 2026-09-15 (`docs/backlog-tasks.md` #35). Carved out
of #22/#23 on 2026-09-11 specifically *because* it looked like it
needed state neither language modeled — "a live, mutating `graph_db`
that reflects the CURRENT in-progress backtrack attempt's own slot
choices". It shipped trivially once #25's R3 work established that the
resolver's own in-progress `entries` **is** that live view — no new
structure was needed, just reading it from inside `disjunction_preference`.

Real's `conflict_downgrade`/`installed_downgrade` guards (bug 531656)
demote an otherwise-available `||` alternative to the `other` bin when
picking it would pull a *lower* version into a slot the graph already
holds at a higher version, unless `_downgrade_probe` says the downgrade
is desirable (no visible not-installed candidate at the same-or-higher
version exists — i.e. the current one is masked or gone from the tree).
Oracle: upstream `test_or_choices.py::testConflictMissedUpdate` through
portage's own `ResolverPlayground`, with a control run forcing
`_downgrade_probe` to always return `True` (which reproduces exactly
what portuale did *before* this fix — merges nothing, the "missed
update" the bug report is named for). Landed as `alternative_downgrade_
demoted`/`downgrade_probe`/`visible_tree_matches` in `portage-repo`'s
`disjunction_preference`. L0-verified neutral on the real tree (byte-
identical on all 120 probes) — this class of guard just doesn't fire
on the corpus, but the fixture oracle proves it fires correctly when
it should.

---

## #29 — L2: portuale as archive producer, and the gpkg differential-test methodology

**Status:** DONE 2026-09-14 (`docs/backlog-tasks.md` #29). Plan:
`docs/029_portuale-as-builder.deepseek.md`. Findings:
`TEST/findings/l2.md`.

### The gpkg format, as verified on disk (reference table)

```
<cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar        # multi-instance layout
└── <pf>-<BUILD_ID>/
    ├── gpkg-1                              # 0-byte format-version marker
    ├── metadata.tar[.zst]                  # flat members metadata/<KEY>; NO CONTENTS (computed at install time)
    ├── image.tar[.zst]                     # members image/...
    └── Manifest                            # DATA lines: SHA512 + BLAKE2B
    [optional *.sig sidecars]
```
Compression is a filename *suffix*, not a format switch — both `.tar`
and `.tar.zst` member names must be accepted. `Packages` index stanza
fields: `BUILD_ID`, `BUILD_TIME`, `CPV`, `DEFINED_PHASES`, `EAPI`,
`KEYWORDS`, `MD5` (of the archive), `PATH`, `SHA1`, `SIZE`, `USE`,
`MTIME`, `REPO`.

### Test-bed method (the pattern this repo now reuses for L3)

Layer separation matters and was worth stating precisely once so later
layers don't blur it: **L0** = `--pretend` resolution at real-tree
scale (a resolver bug found here goes to L0's own pipeline, never an
L2/L3 allowlist). **L1** = both package managers merge one *identical
prebuilt* binpkg set (no source build at all). **L2** = portuale as
*archive producer* plus cross-install (does real Portage accept and
correctly consume a portuale-built archive, and vice versa). **L3** =
each PM builds its *own* bytes from source (no shared `$PKGDIR` at
all). A finding that's archive-shaped (`Packages` fields, gpkg
metadata members) belongs to L2, not L3; a finding that's resolution-
shaped belongs to L0.

**Compiled-payload bytes are never the pass/fail gate** except for the
hand-written, non-compiled `porttest` fixtures — two independent
builds of the same real package differ in bytes regardless of which
PM built them (debug paths, build order, toolchain state). The L2/L3
gate is structure + metadata + path parity; a compiled-payload diff is
tolerated by construction (`diff.py --tolerate-payload`), with the
"is this actually payload-shaped noise" discriminator built and proven
in S4 (never `compiler-nondeterminism` as a blanket allowlist reason —
that was explicitly forbidden as a rule, because it's indistinguishable
from silently hiding a real bug).

**Traps that cost setup time, worth not re-discovering:**
`TEST/compare/diff.py`'s allowlist-matching used to be hardcoded to
the string `"l1"` — an L2/L3 entry in `known-divergences.yaml` was
silently ignored until the layer filter was parameterized. The layer
design doc's prose was xpak-flavoured ("strip `build-info/BUILD_TIME`")
where gpkg's equivalent is a flat `metadata/<KEY>` member — implement
the *intent* (blank the volatile keys, normalize `environment.bz2`
through the same rules `normalize.py` already has), not the literal
xpak path shape. `SOURCE_DATE_EPOCH` is an L3 concern, not L2 — L2
normalizes `BUILD_TIME`/`BUILD_ID` away instead.

**Blockers that had to close first, in order:** #37 (build-phase env)
→ #38 (packaging transforms) → #43 (binrepo `verify-signature`) → #39
(gpkg metadata completeness) → #40 (a consequence of #39's `Packages`-
index `EAPI` fix). Two core fixes surfaced along the way and are worth
remembering as a class of bug: a `Packages` header `VERSION` mismatch
made real Portage silently ignore every portuale-built archive
(nothing in the archive itself was wrong — real just never looked), and
a missing `FILESDIR` → repo `files/` symlink made every `eapply` die on
a portuale-built source tree.

---

## #30 — L3: source-build parity

**Status:** DONE-PARTIAL 2026-09-14/15 (`docs/backlog-tasks.md` #30).
Plan: `docs/030_L3-source-build-parity.deepseek.md`. Findings:
`TEST/findings/l3.md`. The producer prerequisites (#37, #38) and the
`l3-core`/`@system` unblock (via backlog #44/#45, the L3 P-track in
`docs/backlog_tier_5_and_2_sliced.opus.md`) both landed by 2026-09-15;
`l3-core`/`@system` are unblocked but a full run is user-triggered, not
yet executed as of this recap.

**Traps specific to a real full-source-tree differential bed (not
covered by L1/L2's traps above):**

1. `emerge @system` alone is a **no-op** on the test image — `@system`
   is already installed. A real rebuild needs `--emptytree` (`-e`) plus
   `--usepkg=n` on both PMs, forcing a genuine from-source rebuild.
2. The **process-env vs resolved-config asymmetry was the actual
   blocker**, and it's the same bug #37 fixes: real exports
   `config.environ()` (make.conf/profile/env.d, `SOURCE_DATE_EPOCH`,
   `MAKEOPTS`, `FEATURES`, `USE`, ...); portuale's pre-#37 phase env was
   a curated whitelist plus raw process-env reads. Any determinism block
   (a fixed `-j1`/`MAKEOPTS`) must go in the *resolved config*
   (e.g. appended to the container's `/etc/portage/make.conf`), not the
   calling shell's environment — otherwise the two PMs are silently not
   running under the same settings even though the command line looks
   identical.
3. The image's stage3 `make.conf` carries `userpriv`/`usersandbox`/
   `userfetch`/`usersync` — all **portuale non-goals** (real would run
   the build as an unprivileged `portage` user; portuale runs as root
   regardless). A same-config-different-execution mismatch like this
   shows up as ownership/generated-file noise in every single probe if
   the determinism block doesn't explicitly disable them.
4. A **partial run is not a parity run.** If either PM dies mid-`-e
   @system`, don't snapshot and diff as if it completed — the container
   left mid-rebuild is no longer a valid comparison target at all
   (e.g. it may have a half-rebuilt glibc). Mark the report `partial`
   and keep the merge log for triage instead.
5. `sys-apps/portage` and `dev-lang/python` are themselves inside
   `@system` — an `-e @system` run rebuilds the reference Portage and
   the interpreter *while running under them*. Compare installed state
   only, never the running process.

---

## #37 — build-phase env completeness

**Status:** DONE 2026-09-13 (`docs/backlog-tasks.md` #37). Merged plan
(the scope authority; the three per-model drafts are superseded):
`docs/037_Build-phase-env-completeness.plan.md`.

### The mechanism: real's phase env is a full dump, not a whitelist

The single wrong assumption every draft started from and had to be
corrected on: real's `config.environ()` (`config.py:3263-3350`) is
**not** a curated export of specific known keys. On the `setup` phase
it dumps essentially the **whole resolved config plus the whole
process environment**, filtered only by `environ_filter`
(`special_env_vars.py:257`, an exclude-list); only from the **second**
phase onward does it additionally restrict to `environ_whitelist`
(`:83-250`) — because by then the values are already saved in
`$T/environment` from phase 1 and get re-sourced. Portuale's pre-#37
env was a curated ~45-key whitelist; real's on-disk `environment.bz2`
carried **160** keys for the same package (`l2-bpkgonly-env`
recon). **A whitelist-only export can never pass a byte-parity
acceptance bar** against real's `environment.bz2** — this recon finding
overturned the original plan's "keep the curated whitelist, add the
missing six vars" recommendation mid-slice (owner decision, revised
2026-09-13): export set = `(resolved config scalars ∪ process env) −
environ_filter − PORTUALE_COMPUTED` (the keys portuale itself derives —
`D`/`ED`/`T`/`WORKDIR`/`PATH`/etc. — which must always win over
anything upstream).

**`USE` in the phase env and `USE` in binpkg/vdb metadata are the same
value** — real's `PORTAGE_USE` (`config.py:3329`, "filtered by IUSE and
implicit IUSE"), which is exported to the phase as plain `USE` (real's
`environ_filter` drops the `PORTAGE_USE` name itself, keeping only the
`USE` alias). A package with empty `IUSE` still gets
`abi_x86_64 amd64 elibc_glibc kernel_linux` — the *implicit* USE_EXPAND
family, not "no flags". Getting this wrong (exporting only the
package's own declared-and-enabled flags) was the single largest
symptom class in the recon.

**Concrete bugs found and fixed as part of this, each a specific
gotcha worth remembering if the phase-env code is touched again:**
portuale exported `AA` (real pops it for EAPI ≥ 4, `eapi_exports_AA`);
portuale exported `O` (the ebuild dir — real's own `environ_filter`
drops it); the arch/multilib family (`ARCH`, `ELIBC`, `KERNEL`, `ABI`,
`DEFAULT_ABI`, `MULTILIB_ABIS`, `LIBDIR_*`) is **not** in real's
whitelist by name at all — it reaches the ebuild only via the `setup`
phase's `configdict["defaults"]` and then persists through the saved
`$T/environment`; portuale re-runs a fresh shell per phase, so it must
export this family explicitly on every phase, not rely on "whitelist
covers it". Missing this family is exactly what made `oniguruma` land
in `/usr/lib` instead of `/usr/lib64` and broke `jq`'s `econf` — the
concrete real-set failure that motivated this whole item.
`PORTAGE_COMPRESS=""` is real's documented *disable*, so an export set
must distinguish "set to empty" (export empty) from "unset" (omit) —
`build_config_env`'s pre-#37 skip-empties shape was wrong for this. The
brush backend's `phase_setup_script` exported config text via `export
NAME=value` with **no shell quoting** — a `CFLAGS` value containing
`$(...)` would execute; fixed as part of this item (single-quote
escaping) since config text was about to start reaching it for real.

**Where it lives:** `portage_profile::phase_environ(&Config) ->
Vec<(String,String)>` (pure, unit-tested, config-free — `ebuild_phases`
itself stays config-free so standalone `ebuild <file>` keeps working
without a resolved `Config` at all) + `emerge_build::entry_phase_env`
for the per-entry tail (`SLOT`, `PORTAGE_REPO_NAME`,
`PORTAGE_REPO_REVISIONS`, the per-package USE). `extra_env` stays the
last (winning) layer on top. The `depend` phase's env is deliberately
untouched (byte-identical to before — it never ran through this
builder and must not start to).

**Real-execution-only item, no Python mirror**: this whole class of
fix touches build-phase execution, which has no
`emerge_pretend_reference.py` counterpart (AGENTS.md's own carve-out).

---

## #38 — packaging transforms: dostrip / splitdebug / docompress

**Status:** DONE 2026-09-13 (`docs/backlog-tasks.md` #38). Merged plan:
`docs/038_Packaging-transforms.plan.md`; compact mirror with the same
conclusions: `docs/038_Packaging-transforms.babeltele.md`.

**The one-sentence verdict that saved most of the implementation
effort:** portuale already runs the real vendored script
(`bin/misc-functions.sh::install_qa_check`) that *contains* the
`ecompress`/`estrip` gates, on every source-build path, unconditionally,
after every `install` phase — nobody needed to write a stripper. The
gates were false **only** because of #37: `FEATURES` in the phase env
was the raw process env (missing `binpkg-dostrip`/`binpkg-docompress`,
which real gets from `make.globals` because `FEATURES` is an
incremental variable), and `PORTAGE_COMPRESS` was never exported (so
`ecompress` would have `die`d on its own guard). One draft (musespark)
misidentified the call site as `__dyn_package`; it's `install_qa_check`,
called from `run_commands_async` right after `install` — worth stating
because a model reading only `__dyn_package` will conclude the wrong
thing needs wiring.

**`instprep` is a separate mechanism, not a slice of the same gates.**
Real runs `__dyn_instprep` from `dblink.treewalk()`
(`vartree.py:4440-4450`) at **merge time**, on *every* merge including
binary ones — its whole purpose is to strip/compress an already-built
binpkg that was assembled with `-binpkg-dostrip`/`-binpkg-docompress`
disabled (the complement condition). It's idempotent (`.instprepped`
marker) and runs as `treewalk`'s **first** step, before `INSTALL_MASK`/
collision-protect/`pkg_preinst`. Whether to implement it or file it as
a separate item (#38b) was deliberately left as an explicit user
decision (it adds a new phase to *every* merge, source or binary — real
blast radius) rather than a default; the user chose to implement it.

**Two bugs found while implementing `instprep` are worth remembering
as a class**, both about build-directory reuse: (1) `PORTAGE_TMPDIR`'s
CLI-boundary default was `/var/tmp/portage` where real's
`make.globals:35` says `/var/tmp` — this shifted every build's
`WORKDIR`, and therefore the DWARF `comp_dir` baked into stripped debug
objects and the salted `.build-id` link names, off from real's; fixed
as a #37 follow-up (`portage_repo::PORTAGE_TMPDIR_DEFAULT`), confirmed
by the splitdebug oracle bytes matching exactly afterward. (2) Filed as
new **backlog #42**: portuale's emerge never ran real's pre-build
`clean` phase (`EbuildBuild._start_pre_clean`), so a rebuild into a
dirty `${PORTAGE_BUILDDIR}` silently reused an already-instprepped
image from a previous run — fixed by adding `ebuild_phases::run_clean`
at real's exact positions (before every source build, unconditionally
after a successful `--buildpkgonly`, after a merge unless
`FEATURES=noclean`).

**A structural, unfixable-by-construction residue, worth recognizing
rather than chasing:** the `setuid` test fixture builds three
byte-identical binaries sharing one build-id; which of the three names
becomes the `.build-id` symlink's target is `estrip`'s own
`___parallel` race, and **real disagrees with itself run to run** (the
oracle capture links one binary, a later real build links a different
one). This is not a portuale bug and is narrowed to one allowlist row
by design, not chased to zero.

**A brush-backend gap found and left as-is (not this item's scope):**
under `--shell brush`, `porttest/splitdebug` silently produces an
**empty image with rc 0** — brush no-ops the compiled fixture's own
`src_compile` before `estrip` ever runs. Filed under the existing
brush-pin workflow (backlog #6, later root-caused as a `declare -f`
heredoc bug — see the Tier-1 section below); `--shell` default stays
`bash`.

---

## Tier 1: brush (backlog #5/#6) and fetch (backlog #14)

**Status:** brush fixes DONE 2026-09-14, PR submission still open
(user-gated GitHub auth, backlog #5); fetch mirror negotiation DONE
2026-09-14 (backlog #14). Source: `docs/backlog_tier_1_sliced.opus.md`
(the babeltele mirror carries the same conclusions, compressed).

### brush (Rust bash reimplementation, embedded as an alternate phase-execution backend): four real bugs, one root cause chain

`porttest/splitdebug` silently building an **empty image with rc 0**
under `--shell brush` was traced to a chain of four independent bugs,
each masking the next — a good example of why "it silently produces
wrong output" needs single-bug isolation rather than one combined fix:

1. **`declare -f` heredoc quoting** (in a *staged, unmerged* brush
   patch, `fix/declare-f-heredoc-serialization`): the phase-env save
   step uses `declare -f` to serialize shell functions; brush's
   deferred-body code emitted a heredoc terminator tag **with its
   quotes still attached** (`'EOF'` instead of `EOF`) when the original
   tag was quoted (`<<-'EOF'`), producing an unparseable
   `${T}/environment`. The next phase's `source` of that file silently
   loses the ebuild's own `src_compile`, and the *default* no-op
   `src_compile` runs instead — that's the "empty image, rc 0".
2. **A broken `source` doesn't fail the phase.** Under bash,
   `source bad.sh || die` correctly dies (bash's `source` returns 2 on
   a parse error). Under brush, the *standalone* binary aborts the
   whole script (so the `||` branch would work) — but portuale's
   *embedded* runner (`run_one_phase_brush`) only checked for an `Err`
   from `run_string`, and ignored a non-zero `ExecutionResult`, so the
   phase silently continued instead of dying. Bug 1 turned into wrong
   output specifically because of bug 2.
3. **The embedded shell has no `$BASH`.** Real's
   `__filter_readonly_variables` (`phase-functions.sh`) discovers bash's
   special variables by running `env -i -- "${BASH}" -c '...'`; brush's
   *embedded* `Shell` (unlike its standalone binary) left `$BASH` empty,
   so that probe failed, the special-variable list came back empty, and
   `BASHOPTS`/`EUID`/`PPID`/`SHELLOPTS`/`UID` got saved into
   `${T}/environment` and then hit `declare: cannot mutate readonly
   variable` on every re-source.
4. A brace-expansion/IFS defect (fifth fix in the pin set, not detailed
   in the surviving doc beyond its name).

All five fixes are staged as `fix/*` branches on a thin fork
(`vivo75/brush`), rebased onto upstream and re-pinned as of 2026-09-14;
portuale is pinned to that fork. **Opening the five PRs upstream is
still open** — it needs the user's own GitHub auth, tracked as backlog
#5; dropping the fork happens only after upstream merges.

### fetch: `GENTOO_MIRRORS` mirror negotiation was silently broken against every real mirror

The `portage-fetch` module doc claimed real's `layout.conf` mirror-
directory-structure negotiation could be skipped because "the real,
well-known `GENTOO_MIRRORS` entries actually use flat layout" — **false**,
verified live: `distfiles.gentoo.org/distfiles/layout.conf` publishes
`0=filename-hash BLAKE2B 8`, so a flat-layout URL for any real distfile
404s and a hashed one (`.../distfiles/80/which-2.23.tar.gz`) 200s. This
meant every `mirror://gentoo/…` fallback and every dead-upstream
recovery attempt could **never succeed** against a stock
`GENTOO_MIRRORS` list — the single most consequential Tier-1 gap, fixed
first. Landed: per-mirror `layout.conf` fetch + a `.mirror-cache.json`
in real's own on-disk format (matching what real portage itself writes
to `/var/cache/distfiles/.mirror-cache.json` on this host, confirmed by
inspection), lazy-resolved. Two smaller, independently-found bugs in
the same area: a `mirror://` URI's third-party expansions were tried
**twice** (once inline, once again in the primary-URI tail) because
portuale had no `tried_locations` set the way real does; and a
`/`-rooted `GENTOO_MIRRORS` entry (real's `fsmirrors`, filesystem-copy
semantics) was being handed to `wget` as if it were a URL. The
third-party mirror **shuffle** stays a deliberate, documented
non-determinism cut — real shuffles for load-balancing, portuale stays
deterministic, and every candidate is digest-verified regardless so the
shuffle only affects which mirror is tried first, never correctness.

---

## The Tier-5-and-2 sliced plan: waves 1-8, all tracks closed

**Status:** all seven tracks (P, C, R, H) DONE or deliberately closed
as of 2026-09-15. Full narrative:
`docs/backlog_tier_5_and_2_sliced.opus.md` (887 lines — the scope
authority; the babeltele file is a compressed mirror, same
conclusions).

This was the orchestrating plan for the *last* batch of Tier 5 (L2/L3
producer parity: #41, #44, #45) and Tier 2 (resolver: #17, #20, #25,
#28, #35, #36) items, run as eight parallel-track "waves". Rather than
re-narrate all eight waves here (`docs/backlog-tasks.md`'s per-item
entries already carry the final state for #17/#20/#25/#28/#35/#36/#41/
#44/#45), the two things worth keeping that aren't fully captured
elsewhere:

- **#17 (F-B1 merge-order frontier-drain timing) is CLOSED as a
  deliberate cut**, not fixed — the last item on this whole plan to
  close. Two levers were tried under a 600-second time box (per owner
  decision D5): re-measuring against R3b's `@system` seed-sort fix
  (no effect — that fix only covers the explicit top-level-argument
  path) and porting the identical seed-sort fix to
  `add_installed_dependency_closure`'s complete-mode `@system` seeding
  (zero measured effect on all 120 L0 probes, verified both by a
  14-probe container re-run and a full L0 run). The negative result is
  itself the finding: it rules out seed/discovery order as the cause
  and confirms the residue is **installed-node drain timing** inside
  `_serialize_tasks` — which installed package's own dependency chain
  clears one round earlier or later than real's — which needs
  per-node tracing across the ~400-node graph to close, a
  multi-session budget the time box correctly declined to fund. Full
  lever table: `TEST/findings/l0.md` "R5 — #17 closed".
- **Owner decisions D1-D7**, the binding record of every product
  judgment call made across the whole plan (exit-1-on-abort framing,
  Python-mirror scope, `depcachedir` write-back semantics, how much L0
  merge-order movement a single slice may cause, the #17 time box, the
  mrg-director "slots only" scope, and — the last one, D7, 2026-09-15 —
  "close #25 on the R3c-revalidation evidence rather than fund a
  faithful CPython-set-order port") — see `docs/backlog-tasks.md`'s and
  `docs/scope-backlog.md`'s cross-references for the current pointers;
  the plan doc's own §3 table is the full rationale for each.

---

## Oracle methodology (the pattern reused across #23/#24/#26/#35/#36)

Worth stating once since half the items above used it: when the
question is "what does real Portage actually do in this exact
scenario", the most reliable answer is **not** reading `depgraph.py`
and reasoning about it — it's translating the matching case from
`3rdparty/portage/lib/portage/tests/resolver/*.py` (upstream's own
`ResolverPlayground` unit tests) into a small standalone Python script
that imports `ResolverPlayground` directly and runs it against the
**vendored, pinned** portage version, then reading its actual output.
This sidesteps both "what does the container's installed portage do"
(version drift) and "what do I think the source does" (misreading).
Traps in this method, each one that bit someone:

- A test file can **rebind its own `test_cases` tuple** a second time
  right before the loop that consumes it (seen in
  `test_changed_deps.py`) — so upstream may only actually *execute* one
  of the cases a naive reading would assume all run. Don't report "the
  upstream test proves case N" without checking the file actually runs
  case N.
- `PYTHONHASHSEED` must be pinned (`=0`) for reproducible results — real
  portage's own set-iteration order is hash-seed-dependent in a few
  documented spots (the `llvm-core/llvmgold` ↔ `llvm-core/llvm-
  toolchain-symlinks` adjacent-pair wobble is the known example; a
  handful of `_minimize_children` docbook-slot orderings in #25's R3b
  work is another).
- Needs `PORTAGE_GNUPGHOME` pointed at a writable copy of the vendored
  `.gnupg` fixture directory (`chmod 700`) to run at all.

---

## Files moved to `docs/history/` (source of the sections above)

- `019_DFS-partial.{plan,chatgpt,fable}.md`
- `022-agent-task-22-zapdeps.{chatgpt,claude,fable}.md`, `022-fix.md`,
  `022_dep_zapdeps.{chatgpt,claude,fable}.md`
- `023-backtracking_resolve.md`, `023-oracle.md`,
  `023-refactor-inventory.md`
- `024-S4-review.md`, `024-dynamic-deps_n_disagreement.{deepseek,
  opus}.md`, `024-oracle.md`, `024-slot-operator-plan.md`
- `025-tier2-closeout.deepseek.md`, `025b-complete-graph-nodes.md`
- `028-director-proposal.md`
- `029_portuale-as-builder.deepseek.md`
- `030_L3-source-build-parity.deepseek.md`
- `037_Build-phase-env-completeness.{claude,deepseek,musespark,
  plan}.md`
- `038_Packaging-transforms.{babeltele,claude,deepseek,musespark,
  plan}.md`
- `backlog_tier_1_sliced.{babeltele,opus}.md`
- `backlog_tier_5_and_2_sliced.{babeltele,opus}.md`
