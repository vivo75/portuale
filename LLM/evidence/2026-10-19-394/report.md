# #394 scoping report: in-graph installed parents never rebuilt (2026-10-19)

Task: backlog #394 oracle-first, scoping-first. Branch `batch/2026-10-18`
(both repos, pre-existing). No commits, no pushes, no bed scripts, no
fixtures added, no backlog/what-this-proves edits. Evidence (untracked):
`LLM/evidence/2026-10-19-394/` (README + 14 captures + drivers + scratch
ebuilds). Originals under `/var/tmp/pt-394/` (stages removed on next run;
captures kept in evidence).

## Oracle result: DIVERGE for `--rebuild-if-*`, AGREE for `--changed-deps`

Minimal cell: staged-only `dev-libs/r394top-1` (`RDEPEND` on installed
`rebuildconsumer-1.0`, whose `DEPEND` is `rebuildtrigger`, installed 1.0,
tree 2.0) and staged-only `dev-libs/r394cdtop-1` (`RDEPEND` on installed
`changeddepspkg-1.0`, vdb RDEPEND `samepkg` vs ebuild RDEPEND `newpkg`).
Host Portage 3.0.82.2 vs portuale `5905012f` (fresh `cargo build --release`,
clean tree `50de74f2`), fresh stage per cell per PM, `SYSROOT=$FX` on the
Portage side (without it EAPI-8 `DEPEND` resolves against host `/` —
evidence README §"Topology trap"). Full table in the evidence README;
rows:

- a-noflag `-u --deep r394top` (control): both `U trigger-2.0 [1.0]` + `N top`. AGREE.
- a-unbuilt-nodeep `-u --rebuild-if-unbuilt r394top` (control): both `N top` only. AGREE.
- a-unbuilt-deep / a-newrev / a-newver (`-u --deep --rebuild-if-{unbuilt,new-rev,new-ver} r394top`):
  Portage `U trigger-2.0 [1.0]` + **`rR` consumer-1.0** + `N top`
  (backtrack **1/20**, the restart); portuale `N top` only. **DIVERGE ×3.**
- b-cddeep `--changed-deps --deep r394cdtop`: both `N newpkg` + `R changeddeps` + `N top`. AGREE.
- b-cdnodeep `--changed-deps r394cdtop`: both `N top`. AGREE.

`--changed-deps` needs no work (verify-first agrees): its reinstall fires at
selection time (`depgraph.py:8290-8307`, no restart), and the no-deep drop
matches the s2 hide/drop split. The gap is purely the `_rebuild_config`
restart, uniform across all three flags.

## Decision: DESIGN-ONLY (no product change)

The fix is a real restart loop, explicitly out of scope per the brief. Two
compounding halves, both code-grounded:

1. **No restart.** `slot_operator/eliminate.rs::rebuild_if_entries`
   (`:435-444`) double-continues: unreached parents correctly skipped (S2
   pins), reached parents kept with the walked outcome. Portage instead
   re-walks the whole graph with the recorded parents force-reinstalled
   (`__auto_rebuild__` set, `force_reinstall`, `depgraph.py:5460-5480`,
   `:7883-7885`, `:6649`; outer backtrack loop `:12235`). A post-walk
   synthesis of the `rR` entry (the pre-S3 slot-op shortcut) would print
   the right rows for this cell but skip re-resolving the rebuilt parent's
   own deps — wrong in general, and the S3 precedent rejected exactly that
   for the analogous slot-op path. Not contained.
2. **Phase 2 drops the `U`.** Any `--rebuild-if-*` sets request
   `complete_graph` (`resolve_plan.rs:270`); `resolve_phases.rs`
   `want_complete` then lets the phase-2 complete walk **replace** (not
   extend) the phase-1 result, and the complete walk re-resolves deps from
   installed (`initial_outcome.rs` `complete_locked`), losing the
   non-top-level `U`. Portage's `_complete_graph` runs after the restart
   walk and never demotes (`_complete_mode` False until `:8661`). Fixing
   (1) without (2) lands the `rR` without its `U`; fixing (2) means
   changing what phase 2 is (extend-not-replace) — walk restructuring,
   not one function.

## Design (for the implementing slice)

- **Scan** (new, replaces the neutered tail of `rebuild_if_entries`):
  over the phase-1 walk's reached set (merges *and* kept-installed nodes —
  `in_graph` today holds merges only, so the reached set must be threaded
  out of the pass), record dep→parent edges where the dep merged this run
  and the parent is installed with a buildtime edge (DEPEND/BDEPEND;
  RDEPEND/PDEPEND never qualify, cf. `add`'s `priority.buildtime` gate),
  honouring `--rebuild-exclude`/`--rebuild-ignore` and the
  unbuilt/new-rev/new-ver split (`strip_revision` already exists).
- **Restart** in `resolve_phases.rs` after phase 1: if the scan names a
  parent not already merging, re-run the phase-1 walk with those cps as
  forced-reinstall seeds (slot-op `slot_operator_replace_installed`
  precedent, S3: internal seeds, `(argument)` chains and world writes must
  not leak the seed text, `--backtrack=0` disables). Budget: the restart
  consumes a backtrack try; cap and interaction with `backtrack_max` TBD.
- **Phase 2 after the final walk only**, locked to its merges, and it must
  not demote phase-1 merges (extend-not-replace, or re-resolve locked cps
  from the graph node rather than from installed).
- **Risks**: merge-order/`--tree` re-derivation post-restart (S5 lesson:
  `[order]` rows move — bed-gate on `l0-resolver`); world-file leakage of
  seed atoms; `__auto_rebuild__`-style seeds vs explicit user args
  (`reset_depth=False` but argument-like otherwise); pubgrub bridge
  untouched (default solver only, as S2–S5); complete-phase consumers
  (`slot_op_reachable`, `changed_deps_report`, json widening) must see the
  post-restart graph.
- **Test plan**: Rust unit tests for the scan (reached vs unreached,
  new-rev vs new-ver, exclude/ignore gates, non-buildtime edge ignored);
  walk-level tests for a-deep shapes; pmtest pins for the three flags
  (oracle rows above) + the nodeep/noflag controls; corpus bless review
  (expect a handful of `--rebuild-if-*` rows gaining `rR`); then
  `l0-fixture-oracle.sh` + `l0-resolver.sh` (no new UNEXPLAINED/`[order]`
  rows) per 02.359 §2.G. Fixtures: promote `r394top`/`r394cdtop` shapes to
  real fixtures (egencache-on-a-copy md5) only in the implementing slice.

## Files changed / residue

- Repos: **none** (no product, test, fixture, or doc edits; `git status`
  shows only the untracked evidence dir). No CASES rows appended, no
  corpus bless (nothing implemented).
- New: `LLM/evidence/2026-10-19-394/` (untracked; commit with the
  implementing slice or drop).
- Residue: none filed (brief forbids backlog edits; the design above is
  the handoff). Pre-existing notes: host `/etc/make.local` emptied only
  around Portage cells via sudo, restored byte-exact (`3f29e0…`
  verified); `/var/tmp/pt-394/` scratch retained for the implementer.

DECISION: design-only. DIVERGE (rebuild-if ×3) / AGREE (controls,
changed-deps ×2). No product change made.
