# Abort-path spec (backlog #19, Slice 1 oracle)

Source of truth for the DFS-partial / abandon-path slice. All behavioural
claims about real portage cite `3rdparty/portage` by line; all outputs
were captured live (see §3) and are stored under
`fixtures/abort-captures/`. Portuale-side line numbers are
`rust/portage-repo/src/lib.rs` and `rust/portuale/src/pretend.rs` at the
Slice 1 commit.

## 1. The parked branch: what it changed and why it was parked

`git branch -a` shows exactly one DFS branch with code intent,
`backlog/019-DFS-partial`, and it contains **no product code** — only the
three plan/analysis docs (`docs/019_DFS-partial.plan.md`,
`.chatgpt.md`, `.fable.md`, commit `9c1e66f` "Add plan"). The actual
parked *work* is the `explore/dfs-graph-backtracker` investigation plus
three main-line doc commits:

- `ffb39ff` "explore: flag-gated DFS resolution walk
  (`PORTUALE_DFS_WALK=1`)": drained the resolve queue LIFO instead of
  FIFO. Result: 109/117 L0 probes byte-identical BFS↔DFS;
  plasma-meta 487→484; **38 contract failures, all
  order-of-representation** (~22 slot-conflict instance/parent order, ~7
  autounmask cascade order, 4 `--debug` narration order, 5 misc). Same
  package sets, same versions throughout.
- `0eb8d96` "explore: DFS walk verdict — lateral move, not a
  foundation": `merge_order.rs` already replays real `_create_graph`'s DFS
  stack traversal over the resolved graph, so the final merge-list order
  is derived from graph *structure*, not walk order. The DFS walk changes
  nothing about merge order (cluster I untouched) and is not the
  foundation for the truncation.
- `7262779` "docs: the truncation post-pass is also blocked (prototype
  reverted)": a post-pass truncating at the first autounmask-flip node
  never fires, because portuale takes the non-circular `go-bootstrap`
  `||` branch where real takes the in-graph `>=dev-lang/go` on pass 1
  and detects the self-cycle. Reproducing plasma-meta needs
  `circular_dependency`-map-driven backtracking, not a post-pass.
- `bb9e147` "docs: circular-dep backtracking design + real-mechanism
  trace": the correction that the plasma-meta/podman truncation is NOT a
  `get_best_run` pick — `_backtrack_depgraph` breaks at `backtracked ==
  0` via `need_config_change()` (autounmask changes present,
  `--autounmask-backtrack != y`), the `get_best_run` re-run is skipped,
  and the pass-1 partial digraph (DFS-discovery prefix up to the
  `want_restart` autounmask node) is what gets displayed. Verdict:
  "faithfully reproducing the prefix still needs the DFS resolution walk
  ... or an approximate prefix (cluster A: worse than status quo).
  resolve-compare.py already collapses both probes to one benign
  `truncated` finding. Branch parked, not merged."

Why parked, in one paragraph: every cheap approximation (DFS walk,
BFS-prefix post-pass) was built and measured, and each either reshuffled
representation without fixing anything (38 order-pinned regressions, zero
merge-order gain) or fired on the wrong trigger (portuale's `||`
branch choice differs from real's pass-1 choice, so the truncation
condition is never true where it should be). The faithful mechanism —
pass-retaining backtrack loop + abort semantics over a real digraph — is
a resolver-architecture change, and the L0 comparator already suppresses
the two real-tree probes to a benign finding. So the branch was parked
*with the mechanism understood*, not with the problem unsolved. This
spec re-grounds that understanding in fresh synthetic fixtures plus a
live oracle, per the plan.

## 2. Fixtures (Slice 1.2)

Ten packages under `fixtures/repo/dev-libs/`, md5-cache entries alongside
(the `abort-nonexistent` cp intentionally has no ebuild dir, following
the `ormiss-nonexistent` precedent):

| package | deps | role |
|---|---|---|
| `abort-leaf-a` / `abort-leaf-b` | none | unrelated leaves before/after the failure |
| `abort-masked-mid` | `leaf-a maskeddep leaf-b` | masked-only dep in the middle |
| `abort-masked-last` | `leaf-a leaf-b maskeddep` | sibling: failing dep last |
| `abort-cycle-a` ↔ `abort-cycle-b` | hard `DEPEND` both ways, empty `RDEPEND`, no `IUSE` | unserializable cycle arms (same shape as `hardcyclea`/`b`) |
| `abort-cycle-mid` | `leaf-a cycle-a leaf-b` | cycle entry in the middle |
| `abort-cycle-last` | `leaf-a leaf-b cycle-a` | sibling: cycle entry last |
| `abort-unsat-mid` | `leaf-a abort-nonexistent leaf-b` | unsatisfiable atom in the middle |
| `abort-unsat-last` | `leaf-a leaf-b abort-nonexistent` | sibling: unsat atom last |

`maskeddep` is reused (masked by user-level
`fixtures/etc/portage/package.mask`, never unmasked anywhere). No name
collisions existed (`ls | grep abort` was empty). Rust==Python verified
split-stream identical on all six tops before capture (exit 0/1/0 for
masked/cycle/unsat today — the "same wrong thing" the xfail tests pin).

## 3. Oracle capture (Slice 1.3)

Method: `TEST/` container `localhost/test-portuale:latest`, throwaway
`podman run --entrypoint /bin/bash`, fixtures copied to `/tmp/abortcap`
(mounted at `/fixtures`, writable), `PORTAGE_CONFIGROOT=/fixtures
ROOT=/fixtures PYTHONHASHSEED=0`, `LC_ALL=C.UTF-8 TZ=UTC`. Two
fixture-tree adaptations were needed *in the copy only* (committed
`fixtures/` is untouched): a container `repos.conf` with absolute
`location = /fixtures/...` (real portage rejects the fixture tree's
relative locations) mounted over the fixture one, and generated
`Manifest` files (`EBUILD <file> <size> BLAKE2B … SHA512 …`, same shape
as `ebuild … manifest` emits — verified byte-shape against one real
`ebuild manifest` run) because real portage masks every Manifest-less
ebuild `masked by: corruption` while portuale reads md5-cache directly.
`/etc/make.local` bind-mounted (fixture `make.conf` sources the absolute
path; real portage resolves it against `/`, portuale chroot-style).

Each of the six tops ran as `emerge -pv`, `emerge -pvt`, `emerge -pv
--columns`, `emerge --pretend --debug`. Raw stdout/stderr/exit/cmdline
per probe-mode: `fixtures/abort-captures/` (98 files) plus
`fingerprint.tsv` and `upgrade.log`.

Version caveat: the container ships portage **3.0.81.3**; the L0 pin
upgrade to **3.0.82.2** failed inside the throwaway container, so every
capture is against 3.0.81.3 (`fingerprint.tsv`). The abort-path code
below is identical in the vendored 3.0.82.2 checkout, which is what the
spec cites — the oracle is therefore cited against the version it was
captured on, and the mechanism against the version portuale mirrors. No
3.0.81.3→3.0.82.2 delta in this path is known; Slice 6 re-runs L0 at the
pin and will confirm. Companion check: `emerge --pretend -pv` output is
identical to bare `emerge -pv` modulo timing/updates/sandbox-scheduling
lines for one probe per shape (the flag is a no-op on the abort path —
both forms reach `actions.py:460` before any pretend split), which is
why the contract tests use the suite-conventional explicit `--pretend`
form against this bare-`-pv` oracle.

Environment noise (NOT part of the oracle, must be filtered before any
comparison): `Performing Global Updates` + `profiles/updates/2Q-2024`
(repo-copy artefact), `masters attribute` / `repo_name` / `EAPI '0'` /
`Invalid atom … package.use.force` warnings (fixture idioms newer than
3.0.81.3), `FEATURES … cgroup, observability`, `Unable to unshare: EPERM`
(container sandbox), `blockusedeptarget` depend-phase errors (an
intentional broken-ebuild fixture elsewhere in the tree, picked up by
repo scans), `Invalid news item` ×3 (intentional `--check-news`
fixtures), `11 news items … 'gentoo'` / `5 news items … 'testrepo'`
(container + fixture news), and the `Dependency resolution took X s
(backtrack: N/20)` timing (non-deterministic by plan invariant — a
deliberate cut, never pinned).

## 4. What real does, per shape (the oracle)

### 4a. Masked-only dependency (`abort-masked-mid`, `-last`) — exit 1, NO merge list

stdout (all four modes): header `These are the packages that would be
merged, in order:`, `Calculating dependencies ... done!`, the timing
line (`backtrack: 1/20`), news items — **zero merge lines, zero `Total:`
line**, even in the `-last` sibling where every other dep resolved.
stderr: the shipped disclosure block (`!!! All ebuilds that could
satisfy "dev-libs/maskeddep" for /fixtures/ have been masked.` + the
`package.mask` file dump + `(dependency required by …)` chain), then
mask docs. `--debug` stdout adds the walk narration (§5).

Portuale today: full 3-line merge list + `Total: 3 packages (3 new)` +
disclosure, exit 0. Deltas: list present (should be absent), counters
present (should be absent), exit 0 (should be 1).

### 4b. Unsatisfiable atom (`abort-unsat-mid`, `-last`) — exit 1, NO merge list

Same display shape as 4a: no merge lines, no `Total:`, in every mode
including `-last`. stderr block is `emerge: there are no ebuilds to
satisfy "dev-libs/abort-nonexistent" for /fixtures/.` + the same dep
chain (real `_show_unsatisfied_dep`'s else-branch,
`depgraph.py:7016-7063`). Portuale today: full list + `Total: 3` + bare
`!!! no visible ebuild for dependency …`, exit 0. Same three deltas.

### 4c. Unserializable cycle (`abort-cycle-mid`, `-last`) — exit 1, REDUCED tree, cumulative counters

stdout (`-pv`, identical under `-pvt`): the header/calc/timing lines,
then the **cycle-reduced remainder as a tree** — real
`_show_circular_deps` (`depgraph.py:10425-10483`) forces
`--verbose --tree` and `display(handler.merge_list)`:

```
[ebuild  N     ] dev-libs/abort-cycle-mid-1.0::testrepo to /fixtures/ 0 KiB
[nomerge       ]  dev-libs/abort-cycle-a-1.0::testrepo to /fixtures/
[nomerge       ]   dev-libs/abort-cycle-b-1.0::testrepo
[ebuild  N     ]    dev-libs/abort-cycle-a-1.0::testrepo  0 KiB
[ebuild  N     ]  dev-libs/abort-cycle-a-1.0::testrepo to /fixtures/ 0 KiB
[ebuild  N     ]   dev-libs/abort-cycle-b-1.0::testrepo  0 KiB

Total: 4 packages (4 new), Size of downloads: 0 KiB
```

Membership: `{mid, cycle-a, cycle-b}` — **both leaves drained out and
are absent**, in `-mid` and `-last` identically (the mid/last position
is unobservable in real's output). Order: leaf-drain order over the
stuck remainder (`_prepare_reduced_merge_list`,
`circular_dependency.py:58-74`), rendered as a tree with real's node
duplication (`cycle-a` under two parents) and `[nomerge]` marking.
Counters: `Total: 4 packages (4 new)` counts **display rows**, not unique
packages (3 unique merge packages, 4 `[ebuild N]` rows — the duplicated
`cycle-a` row counts twice). `--columns` renders the same rows in
columns. stderr: the `circular dependency graph:` digraph dump (only
under `--debug`), the `(…ebuild scheduled for merge) depends on …`
message, the USE-flag advisory (no `IUSE` here, so the generic note).
`--debug` stdout additionally shows the complete DFS walk (§5) and the
stage summaries.

Portuale today: full flat 5-line list + `Total: 5` (over the full list) +
the shipped flat cycle re-display (3 lines) + error, exit 1. Deltas: the
full flat list + its counters must go (replaced by the reduced list +
row-counted counters); the re-display becomes the *only* list; tree
nesting/`[nomerge]`/duplication is portuale's standing dedup-by-design
cut (see §7).

### 4d. Fourth shape (NOT covered by new fixtures): backtrack-exhausted autounmask+cycle

No new fixture: the trigger needs autounmask changes *plus* an
unresolvable cycle *plus* `--autounmask-backtrack != y`, i.e. the
plasma-meta/podman cluster-A shape documented in `bb9e147` and
`TEST/findings/`. Mechanism (re-derived from source, not re-captured):
`_backtrack_depgraph` breaks at `backtracked == 0` via
`need_config_change()` (`depgraph.py:12228-12234`, `11708-11760`), the
`get_best_run` re-run at `:12250-12260` is skipped, and the pass-1
partial digraph reaches `display_problems()` → `_display_autounmask()`
→ `_show_merge_list()` (`:10488-10494`), which displays the partial
`_serialized_tasks_cache`. Whether v1 covers this shape is a Gate-0
scoping question (G0.3 below).

## 5. Mechanism (source citations, vendored 3.0.82.2)

Walk: `_create_graph` (`depgraph.py:3254-3271`) drains `_dep_stack`
LIFO, returning 0 the moment any `_add_pkg_deps`/`_add_dep` fails —
recursive DFS in effect, since `_wrapped_add_pkg_dep_string`
(`:4519-4600`) calls `_add_dep` **inline** per atom (`:4596-4597`), not
via the stack. Selection order inside one dep string:
`_minimize_children` (`:4751-4775`) `_select_package`s every atom first
and **yields unresolvable atoms (`dep_pkg is None`) inline at
`:4768-4770`, before any resolvable atom is yielded** (`:4773-4775`).
Observed consequence (masked/unsat `--debug` captures: zero `Child:`
lines before `backtracking due to unsatisfied dep`): a masked/unsat atom
aborts its whole dep string before any sibling in the same string is
visited, *regardless of declared position* — the `-mid`/`-last`
distinction is unobservable in the admitted set, and a fortiori in the
output (there is no output list at all).

Abort recording: `_add_dep`'s `dep_pkg is None` arm (`:3483-3522`)
stores `_backtrack_infos["missing dependency"]`, sets `_need_restart`,
and (under `--debug`) prints `backtracking due to unsatisfied dep:`
(`:3508-3521`). `_select_files` maps the three failure sites to
falsy success: `_create_graph()` 0 → `return 0` (`:5676-5678`);
`altlist()` raising `_unknown_internal_error` (the `_serialize_tasks`
give-up at `:10262-10294`, which also stashes the stuck remainder in
`_circular_deps_for_display` at `:10264` and feeds the
`circular_dependency` backtrack map at `:10266-10289`) → `return False`
(`:5680-5683`); autounmask changes → `_success_without_autounmask =
True`, `return False` (`:5805-5810`).

Backtrack loop: `_backtrack_depgraph` (`:12175-12282`) breaks on
`success or need_config_change() or not allow_backtracking or
backtracked >= max_retries` (`:12228-12234`). The masked/unsat captures
show exactly one retry (`backtrack: 1/20`): pass 0 records `missing
dependency`, pass 1 applies it as `runtime_pkg_mask` (visible in the
`--debug` capture), then `backtracking aborted after 1 tries`
(`:12241-12248`) and the `get_best_run` re-run (`:12250-12260`) fails
again. `need_display_problems()` (`:11697-11706`) is false throughout
(no config change, no circular display yet), so the loop runs to the
retry cap logic and returns `success = 0/False`.

Exit mapping: `action_build` (`actions.py:410-462`) — `not success` →
`display_problems(); return 1` (`:460-462`). `display()` (which prints
the merge list, `:10612-10623` → `Display.__call__`,
`resolver/output.py:770-884`, always returning `os.EX_OK`) is **never
reached** on the abort path — hence no list and no counters for
masked/unsat. For cycles, the only list shown is
`_show_circular_deps`' `display(handler.merge_list)` (`:10425-10444`),
where `merge_list = _prepare_reduced_merge_list()`
(`circular_dependency.py:41-42,58-74`): leaf-drain order over the stuck
remainder — "only packages involved in the circular deps". Counters
(`output_helpers.py:88-176`) accumulate per displayed row inside
`Display.__call__` (`output.py:717-767`), so tree-duplicated rows count
multiply — `Total: 4` for 3 unique packages (§4c).

Error blocks: `display_problems()` (`:11104-11303`) order is circular
(`:11113`) → slot → blockers → autounmask (`:11140`) → missing args →
pprovided → masked-license/installed → `_unsatisfied_deps_for_display`
(`:11279-11280`) → buildpkgonly/quickpkg-direct merge-list+error
(`:11282-11303`). Masked/unsat surface via `_show_unsatisfied_dep`
(`:6471-7097`): REQUIRED_USE (`:6933`) vs missing-USE (`:6973`) vs
all-masked (`:7000-7014`) vs no-ebuilds (`:7016+`) + dep chain
(`:7065-7075`) + mask docs. The `--autounmask-only` early `return 0`
(`actions.py:456-458`) precedes the success check — abort exit codes do
not apply to it.

## 6. Spec answers (per the plan's §1.4 questionnaire)

- **Membership.** Masked/unsat: the empty set is displayed (no list at
  all — the plan's "partial list" premise does not hold for these two
  shapes; the observable is the *absence* of the list). Cycle: the
  `_serialize_tasks` stuck remainder (already-drained leaves excluded),
  i.e. cycle members plus their unserialized requirers — for the
  fixtures, `{top, cycle-a, cycle-b}`. Fourth shape: the pass-1 partial
  digraph's `altlist()` (not re-derived here; see `bb9e147`).
- **Order.** Masked/unsat: n/a. Cycle: `_prepare_reduced_merge_list`
  leaf-drain order over the remainder, rendered as a real `--tree` with
  node duplication — NOT DFS acceptance order and NOT the normal
  `_serialize_tasks` order (serialization is what gave up).
- **Counters.** Masked/unsat: absent entirely (no `Total:` line).
  Cycle: computed over the displayed rows only — `Total: 4 packages (4
  new), Size of downloads: 0 KiB`; duplicated tree rows count multiply.
  (`Size of downloads` is 0 KiB here — no `SRC_URI`s; the `myfetchlist`
  dedup in `output.py:777-782` is orthogonal.)
- **Exit code.** 1 in all 24 probes, set at `actions.py:460-462` (`not
  success → display_problems → return 1`). Portuale's hook is
  `pretend::run` (`pretend.rs:7293`) consuming `GraphResult`
  (`lib.rs:13137`): circular already exits 1; masked/unsat exit 0 today.
- **Backtrack interaction.** The abort happens *inside* the retry loop:
  masked/unsat show `backtrack: 1/20` (one `missing dependency`
  feedback + `runtime_pkg_mask` pass, then `get_best_run` re-run fails).
  The portuale hook therefore belongs *around* its `'backtrack loop,
  not inside a single pass — the outcome must survive the loop's own
  retry/feedback, with the final (best-run-equivalent) pass deciding.

## 7. Stop-condition verdict (plan §1.4: stop if membership depends on unmodelled state)

**Not hit.** Masked/unsat need no traversal-order state at all (the list
is absent; only the error block + exit code must be built — both already
exist in portuale: `masked_deps` disclosure at `lib.rs:13246-13256` /
`pretend.rs:1177-1200`, the bare unsat line, and the exit-code site in
`pretend::run`). Cycle needs the `_serialize_tasks` stuck remainder,
which portuale's `merge_order` + `cycle_report` machinery already
computes (`cycle_display`, `lib.rs:13234-13245` — the remainder
*isolation* is what shipped 2026-09-10; only its *placement* changes
from "after the full list" to "instead of the full list"). Two
deliberate cuts carry forward to Slice 3: (a) real's tree duplication +
`[nomerge]` marking is unreachable — portuale's tree model dedups by
design (standing cut, cf. backlog §A circular entry); (b) the `Total:`
row-vs-unique counting follows from (a) — portuale counts unique
packages, real counts duplicated rows; byte-parity of the counter is
therefore gated on the tree decision (G0.2). Nothing observed needs
real's `_dynamic_config` reinstall bookkeeping or the 1854-node re-walk.

## 8. Gate-0 questions for the owner (plan Gate 0)

- **G0.1 (exit codes).** Oracle: exit 1 on all three shapes. Adopt exit 1
  (breaking the exit-0 convention for masked/unsat; circular already
  exits 1), or keep exit 0 with the list suppressed? Reconciliation
  needed with the standing "reported residuals stay informational, exit
  0" slot-conflict convention either way (Slice 5.6).
- **G0.2 (truncation-point parity).** For masked/unsat the question is
  moot (no list — any order gives the same output). For cycles the
  truncation point is the `_serialize_tasks` drain state, not a DFS
  prefix: "same membership (remainder), deterministic flat order" is
  achievable; "byte-parity" additionally requires real's tree
  duplication + `[nomerge]` rows, which the dedup-by-design cut forbids.
  Recommended: membership + flat order, tree shape stays cut.
- **G0.3 (v1 shapes).** Proposed `(a)(b)(c)` map 1:1 onto §4a/§4c/§4b
  and are fully oracled here. Open: is the fourth shape (autounmask +
  cycle partial altlist, §4d — the original plasma-meta #19) in v1, or
  does it stay parked with the L0 `truncated` suppression? It needs
  pass-retaining backtrack state portuale does not keep.
- **G0.4 (rollout gate).** `PORTUALE_ABORT_PATH=0` fallback vs
  unconditional? The masked/unsat flip changes exit codes on existing
  contract cases (`maskneedpkg`, `kwneedpkg` CASES entries expect 0
  today), so a gate decides whether Slice 2 can land
  behaviour-neutrally.

## 9. Provenance

- Captures: `fixtures/abort-captures/`, portage 3.0.81.3, 2026-09-11,
  `PYTHONHASHSEED=0`, command lines in `*.cmd` files.
- Real source: vendored `3rdparty/portage` (3.0.82.2), line numbers in
  §5 verified against the checkout.
- Parked work: `backlog/019-DFS-partial` (`9c1e66f`, docs only),
  `origin/explore/dfs-graph-backtracker` (`ffb39ff`, `0eb8d96`),
  `7262779`, `bb9e147`.
- Portuale behavior quotes: `rust/target/release/portuale` at the Slice
  1 commit; Rust==Python split-stream identical on all six tops.
