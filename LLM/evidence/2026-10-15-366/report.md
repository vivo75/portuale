# #366 report: the pubgrub `||` fixpoint follows Portage's walk order (2026-10-15)

Worker brief: backlog #366 (`LLM/backlog-tasks-2026-10.md`) + evidence
`LLM/evidence/2026-10-15-34-366-check.md`, branch `backlog/366-or-order`
in both repos. No commit, no push (nothing below is committed).

## What changed

**portuale, `crates/portage-repo/src/solver_bridge.rs` only (bridge,
pubgrub path; no crate change):**

- New `WalkOrder` (`added: cp -> _add_pkg step`, `resolved: (member cpv,
  class, index) -> disjunction-pop step`), `walk_plain_children`,
  `WALK_CLASS_ORDER`, `walk_add`, `WalkFrame`, `simulate_walk_order`:
  a faithful simulation of Portage `_create_graph` over the solved plan
  — roots in request-atom order, then a LIFO package stack for plain
  deps plus a LIFO disjunction stack drained only when the package stack
  empties (one frame per member per class, groups in written order; a
  frame records every group's step *before* adding taken children, since
  `_select_atoms` finishes before `_minimize_children` adds anything).
  `^^`/`??` are transparent (as in `_queue_disjunctive_deps` /
  `dep_zapdeps`); blockers add no package. Details and Portage file:line
  citations in `LLM/evidence/2026-10-15-366/walk-order.md` (step 1).
- `or_branch_standing` takes `(member_cpv, class, index, walk)`: the old
  order-blind "independently selected" test (in plan + edged from
  another parent/root) now also requires the alternative to have been
  added strictly before this group resolved. A group whose member the
  walk never reaches (complete-mode latecomers) keeps the old answer —
  deliberate fallback, no walk information to refine it.
- `compute_or_overrides` builds one engine-view per member (shared by
  simulation and analysis) plus the request-atom root order.
- Tests: rewrote `s6_compute_or_overrides_moves_the_desired_branch_first`
  onto synthetic `pg366o-t` (shape (a)), added
  `s6_compute_or_overrides_ignores_an_alternative_pulled_later` (pager
  analog: `r -> a -> ||(x y)`, `x -> b -> y`) and
  `s6_compute_or_overrides_moves_the_desired_branch_first_reversed`
  (`pg366o-tb`, shape (b)); synthetic packages added to `s6_temp_repo`;
  updated the direct `or_branch_standing` test call for the new params.

**pmtest:** 6 fixtures `dev-libs/pg366-{a,b,x,y,ra,rb}-1` + 6 real
md5-cache entries (`_md5_` over exact ebuild bytes, verified by hand);
4 `CASES` exit-code entries (ra/rb × -pv/-pve, default solver, rc 0);
5 pinned tests (`test_pg366_ra/rb_pubgrub_pv/pve_…` + `test_pg366_ra_default_pv_…`).
Deleted the fixed allowlist entry
`pubgrub-or-order-blind-virtual-pager` from
`differential-test-bed/compare/known-divergences.yaml` (yaml re-validated,
15 entries left). Temp atomlists removed (verified: no `tmp-366*` left).

## Captures (all in `LLM/evidence/2026-10-15-366/`)

- Portage 3.0.82.2, bed container (image
  `localhost/test-portuale:latest` id `5616d41d00cd`, run script
  `/tmp/pg366-out/run.sh`, provenance in `cmd.txt`):
  `cap-pg366-ra/rb-pv/pve-portage.txt` — all four: `y` present, `x`
  absent, rc 0 (ra displays `y,a,b,ra`; rb `y,b,a,rb`); plus
  `cap-pg366-ra-pv-debug-portage.txt` (`Candidates: ['dev-libs/pg366-y']`,
  `b` walked before `a`'s disjunction resolves). Whatever Portage does
  is the expected output — the pins assert exactly these rows/orders.
- Pre-fix portuale (`-prefix` files, HEAD `0804a022`): all four fixture
  runs already agree with Portage (the fixtures are non-regression pins,
  like the `pg34-tt -pv` pin); the pre-fix divergence is the bed probe
  `l0-20261015T175938Z` (`-pve virtual/pager --solver=pubgrub`: 2
  allowlisted missing rows `sys-apps/less`, `app-misc/editor-wrapper`).

## Failing-first proof

1. Unit: with the walk gate temporarily disabled (old order-blind rule),
   `cargo test --release -p portage-repo --lib s6_compute_or_overrides`
   gives `ignores_an_alternative_pulled_later FAILED`, the other four
   `ok` — i.e. the old rule moves the later-pulled `y` first. With the
   fix, all pass.
2. Bed: pre-fix `l0-20261015T175938Z` shows the 2 missing pager rows
   (allowlisted); post-fix `l0-20261015T182356Z` shows none
   (324/324 rows, 0 missing/0 extra) and the allowlist entry matched
   nothing.

## Verification (step 5 + gates)

- `-pve virtual/pager --solver=pubgrub` (`l0-20261015T182356Z`): Portage
  324 rows vs portuale 324 rows, **0 missing / 0 extra** (less +
  editor-wrapper back).
- `-pve app-editors/emacs --solver=pubgrub` (same run): 329/329, **0/0**.
- `-pv kde-apps/kdecore-meta --solver=pubgrub` (`l0-20261015T183128Z`):
  257/257, **0/0**.
- `cargo fmt --check --all`: clean. `cargo clippy --release --all-targets`:
  0 warnings (fixed 1 `type_complexity` via `WalkFrame` + 2
  `needless_borrow` from the view-hoisting). `cargo test --release -p
  portage-repo`: **1073 passed, 0 failed**. pmtest gate command verbatim:
  **150 passed, 3778 deselected, 1 xfailed**.
- `python3 LLM/tools/code_notes_index.py` run; code-notes entry added
  (`struct WalkOrder`, `walk_plain_children`, `WALK_CLASS_ORDER`,
  `walk_add`, `simulate_walk_order` sections; `#366` bullets under
  `or_branch_standing`, `compute_or_overrides`, and the S6 list).
- Full contract suite and full L0 bed NOT run (controller's, per brief).

## Concerns (read before commit/review)

1. **Fixtures need `git add -f`.** pmtest `.git/info/exclude` (local,
   edited 2026-10-14 21:26) excludes `/fixtures`, so the 12 new files
   (6 ebuilds + 6 md5-cache entries, listed in `cmd.txt`'s fixture
   paragraph) are invisible to plain `git add -A`. Force-add them in the
   pmtest commit (precedent: `f647e86` committed `pg34-*` the same way).
2. **NEW default-solver divergence, out of scope:** `-pv dev-libs/pg366-rb`
   on the *default* solver merges both branches (`N x` beside `N y`, 5
   rows vs Portage's 4); `-pv dev-libs/pg366-ra` is fine. The task
   restricts the fix to the bridge/pubgrub path, so this is unfixed and
   unpinned (only a `CASES` rc-0 entry covers rb). Recommend a backlog
   item for the default resolver's reversed-order `||` handling.
3. **Informational bed rows remain** (non-gate categories, also present
   pre-fix): `MULTI_emptytree-system` USE rows for `sys-apps/portage`
   (identical pre/post), and `[order]` interleaves (`virtual_pager`
   #188 `virtual/pager` vs `libuv`; `kdecore-meta` #11 `libltdl` vs
   `libutempter` — adjacent independent packages). The fix does not
   touch merge order; sets are identical, so these are benign.
4. **No scoped `cargo mutants --re` re-run** on the new/changed functions
   (S6 established the per-function practice). The code-notes S6 line
   claims only failing-first verification for the new tests — true
   (see above) — but a mutant pass would strengthen it.
5. Approximation log: unreached members fall back to the old order-blind
   answer (§What changed); `^^`/`??` atoms count as plain for add-order
   (Portage-transparent); same-frame groups resolve before any of the
   frame's children are added (matches `_select_atoms`-then-`_minimize`
   staging). If a future probe shows a late member's group mis-resolving,
   start here.
