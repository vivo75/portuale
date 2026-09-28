# g201 report — Track G3, backlog #201: backtracking-conflict WARNING where real "resolves silently"

Status: NEEDS_CONTEXT (S0 refutes the entry premise — no divergence to port; owner decides close vs re-scope).

Branch: `backlog/201-backtrack-conflict-warning` (both worktrees; no commits made, both clean).

## S0 — diagnosis (grounded)

Reproduced on the entry's fixtures (all `scm0*` ebuilds already EAPI 8 — #220 landed):

```
$ PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX $BIN emerge --pretend dev-libs/scm0a
[ebuild  N     ] dev-libs/scm0c-1
[ebuild  N     ] dev-libs/scm0b-1
[ebuild  N     ] dev-libs/scm0a-1
WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:
dev-libs/scm0c:0
  (dev-libs/scm0c-2:0/0::testrepo, ebuild scheduled for merge) USE="" conflicts with
    =dev-libs/scm0c-1 required by (dev-libs/scm0a-1:0/0::testrepo, ebuild scheduled for merge) USE=""
    ^               ^
!!! The following update(s) have been skipped due to unsatisfied dependencies
!!! triggered by backtracking:
dev-libs/scm0b:0
RC=0
```

Real's oracle answer (3rdparty/portage 3.0.82.2): upstream
`test_slot_conflict_mask_update.py::testBacktrackingGoodVersionFirst` passes
(mergelist `[C-1, B-1, A-1]`, success) — same merge set as portuale.

Live probe (rule 13, one container run; staged fixtures exactly as
`l0-fixture-oracle/in-container.sh`: `stage.sh`, fixture vdb, `ROOT=$FX`,
`PYTHONHASHSEED=0`, `PORTAGE_REPOSITORIES` from the staged tree):

```
podman run --rm ... -v <pmtest>/fixtures:/fixtures:ro
  -v <pmtest>/differential-test-bed/layers/l0-fixture-oracle:/oracle:ro
  --entrypoint /bin/bash localhost/test-portuale:latest
  -c '/oracle/stage.sh /tmp/stage && FX=/tmp/stage/fixtures; ... /usr/sbin/emerge -p --color=n dev-libs/scm0a'
```

Verbatim real output (exit `REAL_RC=0`):

```
These are the packages that would be merged, in order:
Calculating dependencies  ... done!
Dependency resolution took 0.33 s (backtrack: 4/20).
[ebuild  N     ] dev-libs/scm0c-1
[ebuild  N     ] dev-libs/scm0b-1
[ebuild  N     ] dev-libs/scm0a-1 to /tmp/stage/fixtures/
WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:
dev-libs/scm0c:0
  (dev-libs/scm0c-2:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with
    =dev-libs/scm0c-1 required by (dev-libs/scm0a-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/stage/fixtures/') USE="" ELIBC="glibc"
    ^               ^
!!! The following update(s) have been skipped due to unsatisfied dependencies
!!! triggered by backtracking:
dev-libs/scm0b:0
```

Real PRINTS the WARNING. The entry's titular claim ("real prints none") is
contradicted — almost certainly inferred from the `ResolverPlayground`
oracle, which asserts mergelist+success only and never captures output text.
**There is no #201 divergence to port; a "fix" suppressing portuale's block
would DIVERGE from real.**

Real mechanism (3rdparty/portage 3.0.82.2, cited file+symbol):
- `lib/_emerge/depgraph.py::_slot_confict_backtrack` (:2205) masks
  highest-first (the point of the upstream test); `Backtracker`
  (`lib/_emerge/resolver/backtracking.py::_feedback_slot_conflict`) records
  `runtime_pkg_mask[C-2]["slot conflict"] = {(A-1, =C-1)}` (non-empty atoms).
- The final run is conflict-free, so `display_problems` shows missed updates
  (`depgraph.py:11130` → `_show_missed_update`, :1566 → `_get_missed_updates`,
  :1529): C-2 is retained because the selected C-1 is lower, and its mask
  reasons are non-empty.
- `_show_missed_update_slot_conflicts` (:1650) prints the block — the same
  rule portuale's #90-S2 rendering implements. Mechanism match, not a gap.

Not the #236 mechanism: these fixtures contain no slot operators and no
installed packages, so `_slot_operator_update_probe` / `_minimize_children` /
probe-side upgrade forcing never run here — a different code path entirely.
The brief's NEEDS_CONTEXT trigger on that ground does not fire.

Version caveat (same shape as g209 CAVEAT 1): the image carries Portage
3.0.81.3, not the 3.0.82.2 pin (in-container upgrade not attempted — one
probe only). Transfer checked: `git diff portage-3.0.81.3..portage-3.0.82.2`
over `lib/_emerge/depgraph.py` + `lib/_emerge/resolver/backtracking.py`
contains zero lines touching the missed-update / slot-conflict-backtrack
path, and the 3.0.82.2-source reading above predicts exactly the observed
output.

Remaining text deltas in this cell are all filed elsewhere, none for #201:
- `to '<root>'` / `to $ROOT` root suffixes (merge rows + warning block) and
  `ABI_X86="(64)"` / `ELIBC="glibc"` USE-expand flags and block grouping =
  OPEN #230 (entry: "the skipped-update warning block renders bare `USE=""`
  and no root suffix"). Under `ROOT=/` these suffixes vanish on both sides.
- Staged-ROOT resolution behavior per §2 B17 belongs to #242 (this cell has
  no BDEPEND/IDEPEND, so no cross-root resolution is exercised anyway).

## S1 — not started (nothing to port)

No product commit, no pmtest commit, no `READY-FOR-BEDS` (guard needs a
product sha; there is none), no entry flip (brief forbids it).

## BED-PENDING

None required: S0's only bed-adjacent step was the live probe, already run
above (real `emerge -p --color=n dev-libs/scm0a` on the staged fixture tree;
expect: rc 0, `[C-1, B-1, A-1]` + WARNING block, verbatim as pasted).

## Owner question

Close #201 as NOT A GAP (S0 evidence: live real warns identically; the
#50-batch-5 observation read text out of a text-less oracle), or re-scope
it onto the #230 deltas visible in this cell? Either way #230 stays the
owner of the remaining text differences. No resolver code should land under
#201: suppressing the block would break parity with real.
