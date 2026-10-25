# S0 #567 — Encode/Sub-Name version-selection (oracle only, no product code)

Date: 2026-10-25. Worker: muse-spark-1.3 (opencode subagent, Task 5).
No product code touched; no pmtest files touched; no commits (manager commits).

## Verdict

A tree-side cause **names itself**, so the 6 `[missing]` rows fold into
Task 4's re-grade: the `dev-lang/perl-5.44.0` stabilization (snapshot
`11c58b7a..350c0882`, the same root cause as the 11 probes) co-schedules
a `perl-5.42.2 → 5.44.0` upgrade in the gnome-shell / libreoffice / gimp
graphs, and that upgrade kills the `=dev-lang/perl-5.42*` branches of the
installed 5.42-era virtuals' `||` dependencies. Portage re-resolves those
`||`s against the post-upgrade graph and falls through to
`~perl-core/Encode-3.210.0` and `dev-perl/Sub-Name`; portuale keeps the
dead 5.42-bound branches and merges neither.

A live resolver semantic gap rides along (portuale never re-verifies a
bound `||` alternative after a co-scheduled upgrade replaces its match),
so this report also drafts the follow-up resolver slice as **#572**
(next free number verified across both backlog files; #571 is Task 12's;
text drafted only — the manager edits and files it). Grading these rows
as pure tree drift would launder that gap (plan Review Focus). #567
itself stays OPEN with this S0 behind it.

## Old snapshot reachability (checked FIRST, per brief)

`11c58b7a` is **not reachable**: `git -C /var/db/repos/gentoo cat-file
-t 11c58b7a` → `fatal: Not a valid object name` (capture
`p13-provenance.out`; the host clone is shallow, 248 commits). The old
pool is therefore grounded in the old bed log alone:

- `l0-20261023T164600Z` (pin `11c58b7a…`, vdb/world identical to the new
  log per the #558 triage): gnome-shell real AND portuale both
  `Total: 453 packages (449 new, 1 in new slot, 3 reinstalls)`, byte-
  identical totals, zero Encode/Sub-Name lines on either side.
- `l0-20261025T084209Z` (pin `350c0882…`): real Total 455 with
  `perl-core/Encode-3.210.0` + `dev-perl/Sub-Name-0.280.0-r1` (`[ebuild
  N]` both), portuale Total 453 without either. Same 2-row delta on
  libreoffice (230 vs 228) and gimp (183 vs 181). No `[exit]` rows on any
  of the three probes (rc 0 = 0); the divergence is merge-list only.
- Both snapshots' `perl-core/Encode` + `dev-perl/Sub-Name` dirs identical
  and vdb identical: per the #558 triage report
  (`.superpowers/sdd/2026-10-25-557-558-560-561/task-2-report.md`
  §Step 3, snapshot diff `11c58b7a..350c0882`), independently consistent
  with the old log (both managers agreed at 453, so no selection gap
  existed on the old pool).

## Hermetic probe (new tree + bed vdb, throwaway container)

`probe-s567.sh` drives container `localhost/test-portuale:latest`
(image `53a68e8e5939…`, Portage 3.0.83, vdb `dev-lang/perl-5.42.2` +
`virtual/perl-Encode-3.210.0-r2` +
`virtual/perl-Scalar-List-Utils-1.680.100_rc` installed, tree at the
`350c0882` shape with `perl-5.44.0` visible) with bed-faithful env
(`PORTAGE_CONFIGROOT=/ ROOT=/ PYTHONHASHSEED=0`) on both managers; the
portuale side is the batch-branch release binary
(`target/release/portuale`, md5 `855c4e1c…`, see `p13-provenance.out`).
Full outputs in `captures/p01–p13`.

Decisive shapes (per-atom cause follows):

| probe | Portage | portuale |
|---|---|---|
| `dev-perl/Encode-Locale` alone (p01/p11) | 1 pkg, no Encode | 1 pkg, no Encode — agree |
| `=perl-5.44.0 Encode-Locale Try-Tiny --tree` (p02/p07) | 5 pkgs: **Encode-3.210.0 N** (child of `[nomerge] virtual/perl-Encode-3.210.0-r2`) + **Sub-Name-0.280.0-r1 N** (child of Try-Tiny) | 3 pkgs: neither |
| `Try-Tiny =perl-5.44.0` (p03/p10) | U perl + Sub-Name N + Try-Tiny N | U perl + Try-Tiny N, no Sub-Name |
| `>=virtual/perl-SLU-1.400.0 =perl-5.44.0` (p04/p08) | U perl + **U virtual-1.700.0** | identical (U 1.700.0) — agree |
| `>=virtual/perl-Encode-2.0.0 =perl-5.44.0` (p05/p09) | U perl + **U virtual-3.240.0** | identical (U 3.240.0) — agree |
| `perl-core/Encode dev-perl/Sub-Name` (p06/p12) | both N (same versions) | both N (same versions) — agree |

(rc 1 on the upgrade shapes on both sides is the known perl
slot-conflict family, orthogonal; the bed's full-graph probes are rc 0
on both sides. p06 additionally carries a skipped-updates warning block
(directly requiring Encode drags perl-5.44.0 into the graph) with rc 0 —
likewise orthogonal. Selection deltas are as tabulated.)

### Per-atom cause

- **perl-core/Encode-3.210.0.** Pool: both managers see versions
  {3.190.0, 3.210.0}, unmasked, amd64-stable (p06/p12 agree on 3.210.0;
  no mask/USE/slot/visibility dimension — nothing masked, no USE
  conditional on the path, slot 0 both). Selection: Portage binds
  Encode-Locale's `>=virtual/perl-Encode-2.0.0` to the *installed*
  virtual-3.210.0-r2 (kept `[nomerge]`, p02 tree), then re-resolves that
  virtual's `|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40*
  ~perl-core/Encode-3.210.0 )` against the post-upgrade graph: the
  installed perl-5.42.2 is being replaced by the scheduled 5.44.0, so
  both perl-bound branches die and the revision-pinned
  `~perl-core/Encode-3.210.0` branch fires → N. (Contrast p05: the same
  atom required *explicitly* upgrades the virtual to 3.240.0 instead —
  the fall-through happens only for the nested-`||`/installed-virtual
  path.) Portage machinery: `dep_zapdeps` `||`-choice bins
  (`3rdparty/portage/lib/portage/dep/dep_check.py`, `dep_zapdeps`:
  `preferred_in_graph` / `preferred_installed` preference for the
  installed satisfier, final `for allow_masked … return choice.atoms`
  selection) applied to the graph state in `_add_dep`
  (`3rdparty/portage/lib/_emerge/depgraph.py:3364`), where the replaced
  perl no longer satisfies its branch. Portuale keeps the
  `=dev-lang/perl-5.42*` branch and selects nothing.
- **dev-perl/Sub-Name-0.280.0-r1.** Pool: sole tree version, unmasked,
  amd64-stable (p06/p12 agree; Try-Tiny merged with `-minimal` on both
  sides, so the `!minimal? ( || … )` guard is active for both — no USE
  cause). Selection: Portage binds Try-Tiny-0.320.0's
  `|| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name )`
  branch 1 to the installed virtual-1.680.100_rc (installed-preference,
  same bins as above), whose own
  `|| ( =dev-lang/perl-5.42* ~perl-core/Scalar-List-Utils-1.680.100_rc )`
  is doubly dead post-upgrade — the perl branch is replaced away, and
  the `~perl-core/…-1.680.100_rc` branch matches **no tree ebuild**
  (`perl-core/Scalar-List-Utils/` holds only 1.630.0/1.680.0,
  p13-provenance) — so branch 1 collapses and Sub-Name fires → N.
  (Contrast p04: the virtual atom required explicitly upgrades to
  1.700.0, whose `||` first branch matches scheduled perl-5.44.0.)
  Portuale keeps branch 1 and selects nothing.

Portuale-side mechanism pointer (read-only; no code touched):
`crates/portage-repo/src/resolve/queue.rs:68,85`
(`or_group_universe`; "chosen out of an `||` group") with the installed
alternative preferred by construction — pinned by
`recursion_prefers_the_installed_any_of_alternative_over_an_earlier_uninstalled_one`
(`crates/portage-repo/src/tests/graph.rs`, "Portage's
`preferred_installed` bin"). What portuale lacks is the second half:
re-verifying a bound `||` alternative once a co-scheduled upgrade
replaces its match (the fall-through to the next branch). That is the
#572 slice, not this S0.

## For Task 4 (manager owns the re-grade)

The 6 `[missing]` rows (gnome-shell/libreoffice/gimp × Encode/Sub-Name)
can move to EXPLAINED under the already-named perl-5.44.0-stabilization
cause with this S0 cited: trigger = co-scheduled perl upgrade (new tree
only; old tree reinstalled 5.42.2, both 453); transition-specific — a
future bed vdb with perl-5.44.0 installed binds the same `||`s to 5.44
branches on both managers and the rows vanish with no code change. Do
not grade them as fixed-by-#568/#569 (different mechanism: provider
fall-through, not pin rendering).

## Drafted follow-up entry (do NOT file from this task; manager edits)

Next free backlog number re-checked 2026-10-25 across both files
(`grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md`: main file max 571,
`LLM/backlog-tasks-2026-10.md` max 420) → **#572**.

> 572. **OPEN (filed 2026-10-25, from #567 S0
> `LLM/evidence/2026-10-25-567/`) — portuale never re-resolves a bound
> `||` alternative after a co-scheduled upgrade replaces its match.**
> Portage binds a nested `||` branch to the installed satisfier
> (`dep_zapdeps` `preferred_installed` bin, `dep_check.py`) and then
> re-evaluates it against the post-upgrade graph in `_add_dep`
> (`depgraph.py:3364`): e.g. under a `perl-5.42.2 → 5.44.0` upgrade the
> installed `virtual/perl-Encode-3.210.0-r2`'s
> `|| ( =dev-lang/perl-5.42* =dev-lang/perl-5.40*
> ~perl-core/Encode-3.210.0 )` falls through to `perl-core/Encode`
> (probe p02), and Try-Tiny's
> `|| ( >=virtual/perl-Scalar-List-Utils-1.400.0 dev-perl/Sub-Name )`
> falls through to Sub-Name because the installed
> `virtual/perl-Scalar-List-Utils-1.680.100_rc`'s own `||` is doubly
> dead (`=perl-5.42*` replaced away,
> `~perl-core/Scalar-List-Utils-1.680.100_rc` matches no tree ebuild;
> probe p03). Portuale (`resolve/queue.rs` `or_group_universe`) keeps
> the dead 5.42-bound branches and merges neither (probes p07/p10),
> leaving the kept installed virtuals' deps unsatisfiable post-merge.
> Oracle: this S0's capture matrix (`p01`–`p12`) on the bed image
> (vdb perl-5.42.2, tree `350c0882` shape). Slice: re-verify bound
> `||` choices against the post-upgrade graph (or the equivalent in
> portuale's walk order) and fall through; pin with hermetic
> upgrade-context `||` probes. Fires only during slot transitions, so
> low priority; same family as, but distinct from, #276. Tier 2. [A]

## Disposition

- #567 stays OPEN with this S0 behind it (no product code in this task).
- Evidence: `probe-s567.sh` + `captures/p01–p13` (this directory).
- No shared pmtest fixtures touched; container was throwaway (`--rm`
  per probe via trap; no Portage probe sides written anywhere).
