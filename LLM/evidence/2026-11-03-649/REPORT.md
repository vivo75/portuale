# S0 #649 — per-version expansion over-merges where real holds the binding

**Status:** S0 record, no product change. Branch `backlog/649-over-merge-s0`
(portuale; pmtest untouched).
**Bed log:** `../pmtest/differential-test-bed/logs/l0-20261103T163945Z/`
(graded binary `33f9fdcb-dirty`, #647 in; tree pins identical to the
zero-extra pre-fix run, so the extras are product-attributed).
**Bed tree snapshot:** gentoo `e07309ba688fb12eaaebe204cc8d346e57227d1b`
(`l0-report.txt:6`, `fingerprint.tsv:3`; profile
`default/linux/amd64/23.0/systemd`, `l0-report.txt:9`). All ebuild quotes
below are verbatim from that snapshot (host tree HEAD verified == pin,
clean — Task 1 report).
**Oracle:** Portage 3.0.83 (`3rdparty/portage`, `ResolverPlayground`).
**Probes:** `probe649_mutations.py` (playground oracle `B0`/`N0`/`N1`/`N2` +
probe643-style trace) and `probe649_gate.py` (fixed-binary gate check
`N0`/`N1`/`N1pinned`/`N2` on scratch worlds under `/tmp/s649`), same dir.
**Prior S0s:** `LLM/evidence/2026-11-03-645/REPORT.md` (#645 trigger this
inverts — there the mechanisms stayed silent where Portage merged),
`LLM/evidence/2026-10-28-572/REPORT.md` (#572 seed feed),
`LLM/evidence/2026-11-02-643/REPORT.md` (#643 consumer-side expansion).

## 1. The single trigger

**Portuale's one-shot seed guard fires on the transient while Portage
re-resolves.** On the `dev-lang_perl` cell the transient `dev-lang/perl-5.44.0`
is co-scheduled in some pass's `entries` (both outputs' WARNING blocks show
it scheduled-then-skipped). Portuale's `select_expansion_arms`
(`crates/portage-repo/src/resolve/rev_dep_or.rs:510`) with
`keep_demoted: false` (seed feed, call site `:757-766`) runs the bug-531656
downgrade guard (`:546-561`) against those entries: highest in slot 0 is the
transient 5.44.0, so the installed `=dev-lang/perl-5.42*` arm is demoted and
the `~perl-core/*` arm is the sole survivor (`survivors.remove(0)`, `:573`),
which the caller filters pass (`:775` not installed-satisfied, `:778`
satisfiable, `:782` not already merged) and pushes as a seed (`:789-797`).
Portage demotes the same arm **transiently** (`dep_check.py:531-544`, filed at
`:634-635` — N1 first-eval trace: `other (via downgrade guard :634-635)`,
highest 5.44.0) but the post-skip re-evaluation bins `=perl-5.42*` as
`preferred_in_graph` (`:636-655`) and demotes `~perl-core/*` as
`preferred_non_installed`/unavailable (`:599-616`) — Portage holds
(`['dev-lang/perl-5.42.2']` on all of N0/N1/N2), portuale seeds
`~perl-core/Encode-3.210.0` + `~perl-core/Test-Harness-3.500.0` with no
demotion vehicle (`:681-683` merge commitments). Seeds persist across passes
in `grown.virtual_expansion_seeds` (`feedback/mod.rs:318-327`,
`backtrack_params.rs:391`), so the transient need only be present in SOME
pass's entries — the cell's WARNING (`5.44.0 ... ebuild scheduled for
merge`) proves it was.

Cell diff (Task 1, §Step 1): real Total 1 vs portuale Total 3; agreed
merge-row 1 is the `dev-lang/perl-5.42.2` reinstall; portuale-extra rows 2–3
are `perl-core/Encode-3.210.0` + `perl-core/Test-Harness-3.500.0`. Recorded,
not a finding: portuale backtracked 2/20 vs real 1/20 (consistent with the
seed-triggered RevDep re-walk, `feedback/mod.rs:323-327` — not separately
proven).

## 2. Consumer path (verbatim depstrings, dev-lang_perl cell)

Request: `dev-lang/perl` atom (`atomlists/l0-resolve.txt:52`), rc 0/0. It
resolves to the reinstall of installed `dev-lang/perl-5.42.2`
(`vdb-list.txt:116`); perl-5.44.0 is scheduled-then-skipped via the
`dev-lang/perl:0` conflict on both sides (`*-dev-lang_perl.txt:11-16` real /
`:12-…` portuale).

The merged perl-5.42.2 carries a LIVE in-graph consumer PDEPEND,
`dev-lang/perl/perl-5.42.2.ebuild:72-82`:

```
PDEPEND="
	!minimal? (
		>=app-admin/perl-cleaner-2.31
		>=virtual/perl-CPAN-2.290.0
		>=virtual/perl-Encode-3.120.0
		>=virtual/perl-File-Temp-0.230.400-r2
		>=virtual/perl-Data-Dumper-2.154.0
		>=virtual/perl-Math-BigInt-1.999.842
		virtual/perl-Test-Harness
	)
"
```

Merged perl row shows `USE="gdbm -berkdb -doc -minimal"` → `minimal` OFF →
`!minimal?` TRUE on both sides. The two virtual deps each match an installed
virtual (the binding Portage holds) plus tree versions:

- `>=virtual/perl-Encode-3.120.0` → installed
  `virtual/perl-Encode-3.210.0-r2` (`vdb-list.txt:297`; tree also has
  `virtual/perl-Encode-3.240.0`). Installed/tree arm,
  `virtual/perl-Encode/perl-Encode-3.210.0-r2.ebuild:10-15`:
  ```
  RDEPEND="
  	|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40* ~perl-core/${PN#perl-}-${PV} )
  	dev-lang/perl:=
  	!<perl-core/${PN#perl-}-${PV}
  	!>perl-core/${PN#perl-}-${PV}-r999
  "
  ```
  → portuale merges `~perl-core/Encode-3.210.0` (merge-row 2). Real: no merge.
- `virtual/perl-Test-Harness` (unversioned) → installed
  `virtual/perl-Test-Harness-3.500.0-r1` (`vdb-list.txt:310`; tree also has
  `virtual/perl-Test-Harness-3.520.0`). Installed/tree arm,
  `virtual/perl-Test-Harness/perl-Test-Harness-3.500.0-r1.ebuild:10-15`:
  ```
  RDEPEND="
  	|| ( =dev-lang/perl-5.42* ~perl-core/${PN#perl-}-${PV} )
  	dev-lang/perl:=
  	!<perl-core/${PN#perl-}-${PV}
  	!>perl-core/${PN#perl-}-${PV}-r999
  "
  ```
  → portuale merges `~perl-core/Test-Harness-3.500.0` (merge-row 3). Real: no
  merge.

**Attribution lock** (merge rows alone suffice): the merged pins `3.500.0` /
`3.210.0` exist ONLY in the installed versions' (5.42-arm) conjunctions. The
rival newer arms — `perl-Test-Harness-3.520.0.ebuild:11`
(`|| ( =dev-lang/perl-5.44* ~perl-core/Test-Harness-3.520.0 )`) and
`perl-Encode-3.240.0.ebuild:11`
(`|| ( =dev-lang/perl-5.44* ~perl-core/Encode-3.240.0 )`) — pin
`perl-core/Test-Harness-3.520.0` / `perl-core/Encode-3.240.0`, for which NO
tree ebuild exists. Portuale's choice is the installed versions'
conjunctions, where Portage holds the installed binding.

**Transient twin:** `dev-lang/perl/perl-5.44.0.ebuild:72-82` PDEPEND is
textually identical in shape (`!minimal? ( … >=virtual/perl-Encode-3.120.0 …
virtual/perl-Test-Harness )`). Merge rows alone cannot tell whether portuale
fired on the live 5.42.2 PDEPEND or kept residue of the transient 5.44.0
PDEPEND — Task 2 resolved it (next paragraph), and the resolution dissolves
the question: the seed path never reads WHICH consumer string pulled the
virtual (it keys on the installed virtual's own RDEPEND + `required_by`,
present either way) — the transient graph state is the difference.

**Live-vs-transient resolution: transient-required.** `N0` (no 5.44.0 in
tree, live-only) HOLDS on the fixed binary; `N1pinned` (`=dev-lang/perl-5.42.2`
request on the N1 tree — the merged perl's own live PDEPEND evaluated, the
transient never scheduled) HOLDS; `N1` (same tree, bare request, transient
co-scheduled) OVER-FIRES with the exact cell pins. The live 5.42.2 PDEPEND
path alone does not fire.

## 3. Per-mechanism gate that fires, and the Portage demotion with no counterpart

**Portuale gate** — `virtual_expansion_seeds`
(`crates/portage-repo/src/resolve/rev_dep_or.rs:691-799`) →
`select_expansion_arms(SelectExpansionArmsArgs { split, consumer, config,
root, entries, update, gate, keep_demoted: false })` (`:510-574`, call site
`:757-766`):

```rust
if !keep_demoted
    && alternative_downgrade_demoted(AlternativeDowngradeDemotedArgs {
        root, config, entries, self_cp: &self_node,
        constraints: &no_constraints, atoms: &arm,
        queued: &[], update, gate, view: OrView::Normal,
    })   // :546-559 — slot-0-wide like Portage's: highest in
{        //   slot 0 is the transient 5.44.0 in pass.entries
    continue;   // :560 — installed =perl-5.42* arm demoted HERE
}
if !keep_demoted && arm.iter().all(|a| atom_matches_installed(root, a, config)) {
    installed_survivor = true;   // :562-563
} else {
    survivors.push(arm);         // :564-566
}
// ...
if installed_survivor || survivors.is_empty() {
    return Vec::new();           // :569-571
}
survivors.remove(0)              // :573 — sole survivor: ~perl-core/*-3.x
```

For each kept virtual (`AlreadyInstalled` + non-empty `required_by`, `:710`
/`:713`; installed version has tree ebuild, `:728`; RDEPEND expandable,
`:754`): the `=perl-5.42*` arm is available (installed) but guard-demoted;
`=perl-5.40*` is unavailable-or-demoted; `~perl-core/Encode-3.210.0` /
`~perl-core/Test-Harness-3.500.0` is the SOLE survivor
(`installed_survivor == false` → returned at `:573`). Caller filters pass
(`:775` not installed-satisfied, `:778` satisfiable, `:782` not already
merged) → seeds
`((virtual,perl-Encode,3.210.0-r2), ~perl-core/Encode-3.210.0)` (+ TH twin)
→ merge commitments with no demotion vehicle (`:681-683`).

**Structural fact carrying the attribution:** NO merge-bound consumer string
on any N-world contains a top-level `||` with a bare virtual arm (perl PDEPEND
= plain conjuncts; virtual `||` arms name `dev-lang/perl` + `perl-core`, never
category `virtual`, so `expand_arm` returns `None` for every arm) —
`expand_consumer_virtual_arms` CANNOT fire; any extra is the
`virtual_expansion_seeds` feed (`feedback/mod.rs:318`).

**Portage demotion with no portuale counterpart** — two halves
(`3rdparty/portage`, `dep_check.py`):

- (a) TRANSIENT demotion of the installed arm: `installed_downgrade = True`
  (avail `5.42.2 < highest_in_slot 5.44.0`, highest in graph, no
  `downgrade_probe`, `:531-544`) filed at `:634-635`
  (`elif conflict_downgrade or installed_downgrade: other.append(...)`).
  N1/N2 first-eval trace lines are the verbatim record (slot atom is
  slot-0-wide: `dev-lang/perl:0`, highest `5.44.0`).
- (b) FINAL demotion of the fall-through after the slot-conflict skip drops
  5.44.0 from the graph: the re-evaluated group bins `=perl-5.42*` as
  `preferred_in_graph` (`:636-655`, all atoms' matches in graph) and
  `~perl-core/*` as `preferred_non_installed`/unavailable (`:599-616`,
  all-installed criterion fails: nothing installed provides `perl-core/*`).
  N1/N2 late-eval trace lines; NO perl-core `ADD_DEP` with a scheduled child.

Consumer-expansion ground: `dep_check.py:197,220,264` (N1 EXPAND-OUT shows
the per-version conjunctions with `=virtual/...` pins; `:264`
(`a.append(virt_atom)`) is the unconditional `=<cpv>` pin append — verified
at close-out review).

Portuale has no (b): seeds are computed once per pass from transient-inclusive
`entries` and persist in `grown` — there is no post-skip re-resolution of the
seed decision.

## 4. Killed candidates (one line each)

- **Entry "installed virtual version has no tree ebuild" sub-form: DEAD by
  inspection** — host tree at the pin carries BOTH
  `virtual/perl-Encode-3.210.0-r2.ebuild` and
  `virtual/perl-Test-Harness-3.500.0-r1.ebuild`. What survives is the guard
  interaction itself: the transient co-scheduled 5.44.0 demotes the installed
  arm.
- **Consumer-side `expand_new_style_virtual_arm` (`:838`): DEAD** —
  structurally unreachable on this shape (no top-level-`||`-with-virtual-arm
  consumer string exists); N1-gate over-fires with the consumer gate
  impossible ⟹ seeds.
- **`update`-gate suppression (`:869-876`, #647): DEAD as cause** — `update`
  is false on this shape (bare request, no `--update`); it neither fires nor
  suppresses here (tripwire, stays).
- **A fourth the trace names: DEAD by exhaustion** — the trace enumerates
  every arm's bin per evaluation (guard / availability / installed /
  in-graph); no other mechanism appears.
- **#648 re-kill:** corners (a)/(c) describe SKIPPED shapes and CANNOT
  over-fire — (a) exits at `:728` (`else { continue; }`, no tree ebuild for
  the installed version) and `:866-868` (`return None`); (c) exits at `:754`
  (`if !virtual_rdepend_is_expandable ... continue`) and `:923-925`
  (`continue`). The over-fire path REACHES `:789-797` (`out.push(seed)`); all
  four corners return before any push — a skip cannot emit a merge
  commitment. Direction proved by the early-exit lines.
- **N1 vs N2 perl-row divergence (U vs cell R+skip): recorded, NOT the
  lock** — the seed computation is identical in both postures (transient in
  entries → guard demotes → seeds persist); the final perl row is a second,
  selection-side divergence (background reverse-edge pressure absent from the
  scratch worlds). Follow-up pointer, not a blocker.
- **N2's pin-holder: inert, recorded** — unreferenced installed leaves never
  enter portuale's walk, so the cell's R+skip posture (background
  reverse-edge pressure) is NOT replicated in scratch. The lock does not
  depend on it, but the follow-up fix MUST be validated against a world
  where the transient ends skipped, not upgraded (Task 2 concern 2 — §6).

## 5. Spread: same pins confirmed, two recorded stops

Recounted from `l0-report.txt`: Test-Harness extras on **13** cells,
perl-core/Encode on **10**, Sub-Name on **5** (matches the plan entry; the
13th TH cell is the allowlisted gnome-shell cluster abort).

- **dev-perl/XML-Parser — Encode + Sub-Name SAME SHAPE** (live in-graph
  consumers: `HTTP-Message-7.10.0.ebuild:21`,
  `IO-HTML-1.4.0.ebuild:17`, `Try-Tiny-0.320.0.ebuild:19-21` with
  `minimal` OFF; installed `virtual/perl-SLU-1.680.100_rc` satisfies
  `>=1.400.0` → real merges no Sub-Name, portuale merges it). TH: NO live
  in-graph consumer (whole-file grep over all 31 merged ebuilds) — only the
  transient perl-5.44.0 PDEPEND reaches it → STOP on attributing TH in this
  cell; rides the transient-path verdict. CONFIRMED for Encode + Sub-Name.
- **dev-db/mariadb — same pins, transient-only consumers** (no merged ebuild
  mentions either virtual outside inactive test gates; only the transient
  5.44.0 PDEPEND reaches them). CONFIRMED same over-fire shape on the pins.
- **virtual/perl-ExtUtils-MakeMaker — cleanest twin** (totals 1 vs 3, same two
  extras verbatim; installed 7.760.0 ↔ 5.42 arm; consumers transient-only).
- **media-gfx/gimp — recorded-and-stopped, NOT this S0's cell** (real merges
  `perl-core/Encode-3.210.0` itself AND `dev-lang/perl-5.44.0` U AND
  `dev-perl/Sub-Name` itself — Portage-side posture differs:
  upgrade-through + takes the arm itself; the "holds the binding" premise
  does not hold). No re-scope (ledger Ruling 1).

## 6. Follow-up slice — #650

**#650 (shaped and numbered by this S0; verified free: `grep -nE '(^|\b)(#?650)\b'`
over `LLM/backlog-tasks.md`, `LLM/scope-backlog.md` and
`LLM/backlog-tasks-2026-10.md` returns nothing, and the highest filed number
is #649 — re-grep at filing time per the plan, 648/649 taken): the seed feed
must not commit fall-through arms chosen under a transient guard-demotion —
re-check seeds against the post-skip graph (Portage's late-eval bins) or
drop seeds whose guard-demotion input was transient.**

### Exact product sites / signatures

All in `crates/portage-repo/src/resolve/rev_dep_or.rs` unless noted:

1. `fn select_expansion_arms(args: SelectExpansionArmsArgs<'_>) -> Vec<String>`
   (`:510`) — the `keep_demoted: false` + guard path (`:546-561`) is where
   the transient demotes the installed arm. The fix lives here or at its
   inputs: the guard cannot distinguish a transient highest-in-slot from a
   final one.
2. `pub(crate) fn virtual_expansion_seeds(ctx: &ResolveCtx<'_>, pass:
   &PassResult, grown: &BacktrackParams) -> Vec<((String, String, String),
   String)>` (`:691`) — call site `:757-766` (`entries: &pass.entries`,
   `keep_demoted: false`), push `:789-797`.
3. Seed persistence: `feedback/mod.rs:318-327`
   (`grown.virtual_expansion_seeds.insert(seed)` → `PassDecision::Feedback(RevDep)`
   re-walk with `grown` carried) + `BacktrackParams::virtual_expansion_seeds:
   BTreeSet<((String, String, String), String)>` (`backtrack_params.rs:391`)
   — the vehicle by which a transient-seen seed outlives the skip.
4. Portage oracle lines the fix must mirror: `dep_check.py:531-544` +
   `:634-635` (transient demotion — portuale already has this) and
   `:636-655` + `:599-616` (post-skip re-resolution — the missing half).

### Exact fixture / probe

Hermetic fixture family mirroring the N1 scratch world (installed
perl-5.42.2 + installed virtuals at the 5.42 arms with the verbatim real
RDEPENDs, tree perl-5.44.0 with the textually-identical PDEPEND, bare
`dev-lang/perl` request): the fixed binary MUST reproduce the over-fire
today (RED pin: `perl-core/Encode-3.210.0` + `perl-core/Test-Harness-3.500.0`
merged), and hold (`[ebuild R] dev-lang/perl-5.42.2` only) after the fix —
oracle the playground N1/N2 mergelists `['dev-lang/perl-5.42.2']` exactly.

Plus a MANDATORY second leg (Task 2 concern 2 — the N2 pin-holder gap): the
fix MUST be proven against a world where the transient ends SKIPPED, not
upgraded, else the gate cannot distinguish "seeds respect the skip" from
"seeds never saw pressure". Candidate shape: N2 + a REFERENCED subslot
pin-holder (in the request closure, so the walk reaches it — the
unreferenced N2 leaf never walks) reproducing the cell's R+skip posture;
unproven — finding that shape is the follow-up's first job, S0-descoped as
to the exact holder, not invented here. Fallback oracle that already exists:
the container-cell probe `emerge -pv dev-lang/perl` on the bed image
(R+skip posture WARNING-proven) with a follow-up binary — extras gone, skip
preserved.

### Oracle

Playground N1/N2 mergelists `['dev-lang/perl-5.42.2']` (hold — recorded in
`probe649_mutations.py`), fixed binary holds on both fixture legs, bed proof
with a follow-up binary on the `dev-lang_perl` cell (extras gone, R+skip
preserved, totals back to 1).

### Tripwires the follow-up must not move

The M6b/M5 legs, every #647 pin, p02/p03, the #646 tie, Q19 skip rows,
libc-silence, nodeps/onlydeps (seeds gated on full walks), deep/emptytree —
stay green; backtrack count deliberately unasserted (#572 precedent:
portuale 1/20 feed vs Portage 0/20 inline). Do NOT touch #648 (skipped-shape
corners — different family) or #644 (nested-`||`/mixed-`||` cuts). No yaml
moves: the fresh `[extra]` rows stay unallowlisted until the fix proves them
gone, and the two removed `perl-544-stabilization-*` entries stay removed —
the follow-up must not re-add them.

## 7. Bed verdict + known unknowns

- Fresh `[extra]` perl rows stay unallowlisted by design (the regression under
  study, filed as #649 precisely to avoid laundering them); the two removed
  yaml entries stay removed.
- Known unknowns for the follow-up (recorded, not blocking this S0): the
  `update` value reaching `select_expansion_arms` from the seed path was not
  instrumented per-pass (code-read: `entries: &pass.entries` confirmed at
  the call; `update:` field value assumed false from the bare-request shape —
  if the fix touches guard inputs, instrument per-pass entries membership
  first; the seed decision itself has no debug log — worth adding with the
  fix). The N1-gate backtrack 1/20 vs N0/N1pinned 0/20 fits the
  seed-triggered RevDep re-walk but is not separately proven.
- Residual from Task 1 (low): vdb RDEPEND contents are container-local, so
  tree-arm substitution at equal revisions stands in; the probe headers
  state it.
- Probe docstring notes: `probe649_gate.py` usage line (`[N0|N1] (default:
  both)`) is stale against its own default (`["N0", "N1", "N2"]`, `:234`,
  plus `N1pinned`) — recorded for the follow-up's fixture pass, not fixed
  here (this commit pass covers only the two review-flagged cosmetics:
  mutations usage now `[B0|N0|N1|N2]` / "all four", gate scratch dir now
  `/tmp/s649` matching `SCRATCH_BASE`).
- No product code, no fixture commits, no pmtest changes, no yaml moves, no
  bed re-runs, no pushes. Evidence only.
