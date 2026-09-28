# G210 report — backlog #210: slot/sub-slot change without revbump as version change

Status: NEEDS_CONTEXT (no commit; both worktrees left clean — see §6).
No judgment call was taken; the three questions in §5 need owner rulings
before S1/S2 can land. Do not flip backlog entry #210 (still OPEN).

## S0 — the rule (3rdparty/portage 3.0.82.2, `lib/_emerge/depgraph.py`)

- `_complete_graph` (`:8562`). Under `complete_if_new_ver`, for each
  `operation == "merge"` node, `inst_pkg = vardb.match_pkgs(node.slot_atom)`
  (`:8604`); same cp, same version (`inst_pkg < node or node < inst_pkg`
  false, `:8608`), but `inst_pkg.slot != node.slot or
  inst_pkg.sub_slot != node.sub_slot` (`:8611-8614`) sets
  `version_change = True` (`:8617`) — with the comment `:8615-8616`
  "slot/sub-slot change without revbump gets similar treatment to a
  version change".
- Second arm (`:8634-8645`, under `complete_if_new_slot` =
  `--rebuild-if-new-slot`, default y): a merge node whose
  (slot, sub-slot) matches no installed instance of the same cp also
  sets `version_change`.
- `version_change` (or `use_change`) sets `myparams["complete"] = True`
  (`:8647-8648`); then `_load_vdb`, complete-mode selection
  (`_select_pkg_from_graph`, `:8662`), deep walk, and the required-set
  re-seed: `args = _initial_arg_list[:]` (`:8677`), required sets
  (`:8686-8709`), set args appended (`:8723-8731`), `_set_args` /
  `_expand_set_args(add_to_digraph=True)` / `_add_dep` per atom
  (`:8734-8743`), `_create_graph(allow_unsatisfied=True)` (`:8754`) —
  so the consumers' atoms become parent atoms constraining
  `_select_pkg_highest_available` (`:7233`).

## S0 — portuale's gate (this repo, `rust/portage-repo/src/lib.rs`)

- The auto-enable half is already ported: `complete_graph_auto_enable`
  (`:20496-20512`) returns true for `Reinstall { slot_changed: true,
  .. }` under `if_new_ver` (`:20505-20509`), with unit test
  `complete_graph_auto_enable_reinstall_slot_change_needs_if_new_ver`
  (`:51346`). The CLI two-pass (`pretend.rs:11830-11839`) therefore
  already re-resolves with seeds for exactly this shape.
- The gap named by the triage is real: `reverse_dependency_constraints`
  (`:14811`) fills its `upgrading` map (`:14849-14864`) only from
  `Upgrade`/`Downgrade` outcomes (`:14849-14851`); `Reinstall` falls
  into `_ => continue`, and `:14865-14867` early-returns when the map is
  empty. A same-version slot-change reinstall never constrains.
- The `Reinstall` payload already carries the exact signal real's
  `:8611-8618` computes: `slot_changed: bool` (`:6707`, computed by
  `slot_changed()` `:9225-9257` = vdb SLOT vs current ebuild SLOT for
  the same version). `being_replaced` already includes Reinstalls via
  `merge_bound_cpv` (`:16178`), and `build_residual_slot_conflicts`
  already renders Reinstall merge versions (`:18243`).

## S0 — fixture (built, collision-checked, then reverted — recipe in §6)

EAPI 8 only (owner B12 rule). No `reinstslot*`/`slotch*` collisions
(`ls fixtures/repo/dev-libs | grep` empty; only the pre-existing
`subslot*` family, which covers other shapes):

- `fixtures/repo/dev-libs/reinstslottarget/reinstslottarget-1.0.ebuild`
  (`SLOT="0/2"`, no RDEPEND) + md5-cache entry (`_md5_` = md5 of the
  ebuild bytes, `ebbf4b1af8d183dd7c8f7bee1b3c8486`) + vdb
  `fixtures/var/db/pkg/dev-libs/reinstslottarget-1.0/` (`CATEGORY`,
  `SLOT="0/1"`, `repository="testrepo"`, newline-terminated like the
  existing entries).
- `fixtures/repo/dev-libs/reinstslotconsumer/reinstslotconsumer-1.0.ebuild`
  (`SLOT="0"`, `RDEPEND="dev-libs/reinstslottarget:0/1"`) + md5-cache
  (`_md5_=14b36f47105ca90c00e79dd8e27cd307`) + vdb (`SLOT="0"`,
  `RDEPEND="dev-libs/reinstslottarget:0/1"`, live == recorded so the
  Effective (dynamic-deps, default on) and Raw layers agree).

Deviation from the brief's shape, deliberate and reported: the brief
says consumer with `L:=` bound to `/1`. A built `:=` records as
`=L-1:0/1=` and `reverse_dep_constraint_atom` (`:14544-14551`) strips
it to `=L-1` — satisfied by the reinstall candidate, so it can never
fire in this scan by #24 S5 design (it is the update-probe/`check_
reverse_dependencies` rebuild domain = #211/G13, not a constraint).
The only pin flavour that can exercise the upgrading-map rule is a
plain slot-bound pin (`L:0/1`, kept verbatim, fails vs the `/2`
candidate) — see Q1.

## S0 — the one real probe (rule 13, verbatim)

Command (one `podman run`; staging replicates
`l0-fixture-oracle/stage.sh` minus its profile-cosmetic steps 9–11,
which only affect unrelated profile files; Manifests + categories +
repo absolutization + vdb replacement included):

```
podman run --rm \
  -v /home/vivo/repo/PORTUALE/wt-210-slot-change-reinstall/pmtest/fixtures:/fixtures:ro \
  -v /tmp/opencode/g210/probe.sh:/probe.sh:ro \
  --entrypoint /bin/bash localhost/test-portuale:latest /probe.sh
```

(portage in image is 3.0.82.2; `PYTHONHASHSEED=0`;
`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=$FX` staged copy.)

Real's output on all three invocations — byte-identical merge list:

```
### emerge -p --changed-slot dev-libs/reinstslottarget
[ebuild   R    ] dev-libs/reinstslottarget-1.0 [1.0] to /tmp/g210-probe/fixtures/
rc=0
### emerge -p --changed-slot dev-libs/reinstslottarget dev-libs/reinstslotconsumer
[ebuild   R    ] dev-libs/reinstslottarget-1.0 [1.0] to /tmp/g210-probe/fixtures/
rc=0
### emerge -p --changed-slot --update dev-libs/reinstslottarget dev-libs/reinstslotconsumer
[ebuild   R    ] dev-libs/reinstslottarget-1.0 [1.0] to /tmp/g210-probe/fixtures/
rc=0
```

i.e. real reinstalls T and silently ignores the walked consumer's
`:0/1` pin (no constraint, no rebuild, no notice), rc 0. Full log was
at `/tmp/opencode/g210/probe.log` (ephemeral; the three result blocks
above are verbatim).

Pre-fix portuale (release binary from this worktree, `fixture_env`
equivalent) prints exactly the same on all three shapes:

```
[ebuild   R    ] dev-libs/reinstslottarget-1.0 [1.0]
rc=0
```

(`--json` confirms T is `reinstall` with `"changed_slot": true` and C
is an `already_installed` entry, `"requested": true` — so C IS in
`graph_cps`.) Pre-fix parity with real holds on the probed shapes.

## S1 experiment (scratch, reverted): naive port DIVERGES from the probe

Scratch diff (9 lines, `reverse_dependency_constraints` `:14849-14852`):

```rust
PretendOutcome::Reinstall { version, slot_changed: true, .. } => version,
```

added to the `upgrading` match. Rebuilt release, re-ran shape 2
(`--pretend --changed-slot T C`):

```
[ebuild   R    ] dev-libs/reinstslottarget-1.0 [1.0]

!!! Multiple package instances within a single package slot have been pulled
!!! into the dependency graph, resulting in a slot conflict:

dev-libs/reinstslottarget:0

  (dev-libs/reinstslottarget-1.0:0/2::testrepo, ebuild scheduled for merge) USE="" pulled in by

  (dev-libs/reinstslottarget-1.0:0/1::testrepo, installed in '.../fixtures') USE="" pulled in by
...
rc=1
```

Mechanically certain: C is an entry (`graph_cps` ∋ C; C is also in
`slot_op_reachable` since `ResolveCtx::new` seeds it with `req.atoms`,
`:21087-21091`), the `T:0/1` pin fails vs the `/2` candidate, the
single candidate cannot hold it → dropped → residual renders
(`inst.slot == slot`, `:18292`) → rc 1 via the #62 rule. Real,
probed: silent rc 0. So the brief's "port the rule" as a bare
upgrading-map fill introduces a divergence on the only
contract-testable shape. The scratch edit was reverted
(`git checkout -- rust/portage-repo/src/lib.rs`).

Why real is silent (best available characterization, from the probe +
real's structure, NOT separately grounded — see Q3): a no-op explicit
arg is never a digraph node, so its recorded atoms never become parent
atoms. Portuale over-approximates real's complete-mode graph
membership twice: `slot_op_reachable` seeds `req.atoms` unconditionally
(`:21087-21091`), and `graph_cps` includes `AlreadyInstalled` arg
entries. Harmless for Upgrade/Downgrade (the walk enforces the same
pins through live dep edges) but observable for Reinstall (both walks
skip an AlreadyInstalled root's deps, so the scan is the only
enforcer and real has none).

## §5 — questions for the owner (no default taken)

- Q1 — pin flavour: the brief's `L:=` consumer cannot exercise this
  rule (stripped to `=L-1`, always satisfied; rebuild domain is
  #211/G13). (a) Re-scope #210's fixture to a plain slot-bound pin
  (`L:0/1`, recipe in §6)? (b) Close #210 as covered-by-design on the
  `:=` shape? (c) Something else?
- Q2 — gate: reproducing the probed silence needs a consumer-gate
  refinement the brief does not scope (e.g. skip top-level args
  settling `AlreadyInstalled` — no-op args aren't digraph nodes;
  `being_replaced` already covers merge-bound args; `top_level_cps`
  exists on `ResolveCtx` `:21041` but is not threaded into the scan).
  The boundary (AI-args only? all non-merge-bound args? NVC too?)
  touches #54/#91 semantics. (a) Approve that gate + a silence pin on
  the args-shape + a scan-level positive Rust unit test (direct
  `reverse_dependency_constraints` call: Reinstall{slot_changed} +
  reachable consumer → pin found; pre-fix → none)? (b) Reject —
  withdraw #210? (c) A different boundary?
- Q3 — world-shape grounding: the only shape where the rule plausibly
  constrains per real (consumer in @world → nomerge node → parent
  atom → conflict) is unprobed (rule 13: one run, used on the
  arg-shapes) and untestable in-contract (the fixture world file is
  shared). (a) Coordinator runs an `FX_WORLD_EXTRA=dev-libs/
  reinstslotconsumer` probe (bed or manual) to ground it before S1?
  (b) Leave it as BED-PENDING after the commit?

## §6 — tree state, fixture recipe, beds

- Commits: none (NEEDS_CONTEXT). Backlog #210 left OPEN (untouched).
- Trees: portuale worktree clean (`git status` empty after revert);
  pmtest worktree clean (the §6 fixture files were removed after the
  runs to avoid perturbing concurrent suites — depclean/prune/-C
  tests read the whole vdb).
- Fixture recipe (to recreate): the two ebuilds + two md5-cache
  entries + two vdb dirs exactly as in "S0 — fixture" above
  (vdb files newline-terminated; `_md5_` values quoted there were
  computed over the written ebuild bytes).
- Gates not run (nothing to gate; reverted). BED-PENDING: none
  requested beyond Q3(a) — L0 / fixture-oracle run against any future
  commit as usual per the Track G guard.

## Commits (pmtest first; no push per rule 2)

- None. Branch `backlog/210-slot-change-reinstall` in both worktrees,
  no commits.

## Round 2 (2026-09-28) — DONE, two argument cells; @world diagnosed, stopped per brief

Status: DONE (both worktrees committed, no push per rule 2; backlog
#210 left OPEN -- entry flip is the coordinator's/owner's call, not in
the brief). The three coordinator questions are implemented as decided
in `g210b-brief.md` (Q1 fixture as-is, Q2a gate approved, target =
world-shape cells + @world diagnosis).

### What changed (portuale `rust/portage-repo/src/lib.rs`, one commit)

1. `reverse_dependency_constraints` fills its `upgrading` map from
   `Reinstall { slot_changed: true, .. }` as well as
   `Upgrade`/`Downgrade` (real `_complete_graph` 8611-8618).
2. Q2a gate: a top-level argument settling `AlreadyInstalled` is never
   a constraint source (no-op arg, not a digraph node in real), unless
   the required sets reach it too. New `ResolveCtx::world_reachable`
   closure (world/selected/system seeds alone, no arg seeding -- the
   existing `slot_op_reachable` deliberately includes args and cannot
   serve); threaded to both scan call sites. #54/#91 untouched (their
   consumers are never no-op args).
3. Reinstall-sourced pins match the recorded atom verbatim (real's
   re-seed constrains selection as-is, 8677-8754): the built `:0/1=`
   withholds here, while Upgrade pins keep the #24 S5 strip-into-rebuild
   rule (update probe, 2494-2502).
4. `rev_dep_pin_holdable` and `constraint_withheld_updates` count the
   installed instance (withholding keeps it; no repo candidate sits at
   the old slot -- without this every slot-change pin dropped for the
   residual report, rc 1).
5. Required-set passes keep a fully-pinned argument installed instead
   of aborting the run (real `_select_pkg_from_graph`, 8662; new
   `complete_mode_withheld_version`, gated on seeds-fed + positive pins
   + installed-satisfies, so genuine absences still abort).

### Verified locally (world-extra copies of the committed fixture)

- `--changed-slot T` and `--changed-slot --update --deep T`: no merges,
  skipped-update warning for `T:0` listing BOTH `:0/1` (consumer) and
  `:0/1=` (bound), rc 0 -- real's shape (probe run
  `l0-fx-20260927T225534Z`). Portuale rendering gaps vs real remain
  (one block, bare `USE=""`, no `^` markers/root suffixes -- the known
  #227 explanation class, shared with blk0).
- No-world `T consumer` args: silent `[ebuild R] T`, rc 0 -- real's
  round-1 probe shape, unchanged.
- @world cell: STILL FAILS (rc 1, bogus `no ebuilds to satisfy
  ":0/1="` with doubled installed+argument chain). Root cause is
  different from the missing constraint, so stopped per brief: the
  forward walk evaluates the installed world-member argument's
  Effective recorded deps as hard edges -- stale `:0/1=` resolves to
  installed T while `:=` raw-matches the /2 ebuild, dual-instance
  conflict, unsolvable, NVC before the reverse scan can engage. Real
  never attempts the reinstall there (merges unrelated updates, rc 0).
  Fixing it means changing installed-argument dep-walk semantics, a
  separate slice (needs its own real probe; my rule-13 run is unused).

### Gates (all on final content)

- `cargo fmt --check` clean, `cargo clippy --release --all-targets`
  zero warnings.
- `cargo test --release` whole workspace: all green (portage-repo 853
  incl. 2 new scan tests -- slot-change pin both flavours + no-op-arg
  gate; portuale binary 664; rest ok).
- Full pmtest suite: 2073 passed, 37 skipped, 8 xfailed, rc 0, no
  corpus drift, no bless. (Baseline 2071/37/7: +3 is this slice's new
  tests; the 8th xfail is tree vintage -- 6 marked defs incl. 1x3
  params -- all strict, none flipped.)
- Focused neighbours green: #54/#91/#90/#79/#24-slot-op/#25/#57-triangle/
  keeper/revdep/slotconflict/r25/whpin/rdcpin rows (31 passed + 1 xfailed).

### Beds (coordinator; not run per rule 13)

- Expect `l0-fixture-oracle-g210.txt` cells 1-2 to drop their exit+extra
  findings and show only #227 explanation diffs (rendering); the @world
  cell still fails (exit + unsatisfied message) until its own slice.
- L0 must stay clean (no L0 probe covers this shape).

### Commits (pmtest first; no push per rule 2)

- pmtest `834e66f` (fixture + g210 list + all.sh registration +
  contract pins) on `backlog/210-slot-change-reinstall`.
- portuale `d43cf7b0` (the rule above; quotes pmtest 834e66f) on
  `backlog/210-slot-change-reinstall`.
