# #573 S0 — what is left of the built-pin residual, and why

**Date:** 2026-10-28. **Tree:** portuale `b4cc97b2` (branch `slice/572-or-reverify`)
plus the fixture/probe files listed at the end (uncommitted). **Bed baseline:**
`../pmtest/differential-test-bed/logs/l0-20261028T083834Z` (gentoo `350c0882`,
`repo_pins_match_file: true`, 120 probes, 44 UNEXPLAINED).

## 1. The item's first half is already gone

#573 was filed from the Task-4 re-grade of `l0-20261025T145359Z`, when **27**
`:0/5.42=` rows with real-package requirers were real-only on the nine healed
probes. On the current tree that half no longer reproduces: on the newer bed log
the skip block is **set-identical** for all nine probes that carry the yaml entry
and for the tenth:

| slug | real rows | portuale rows | real-only | portuale-only |
|---|---|---|---|---|
| dev-lang_perl | 69 | **2** | **67** | 0 |
| dev-perl_XML-Parser | 69 | 69 | 0 | 0 |
| virtual_perl-ExtUtils-MakeMaker | 67 | 67 | 0 | 0 |
| dev-lang_ruby | 69 | 69 | 0 | 0 |
| dev-ruby_rake | 69 | 69 | 0 | 0 |
| kde-apps_kdecore-meta | 69 | 69 | 0 | 0 |
| app-text_texlive | 69 | 69 | 0 | 0 |
| app-text_texlive-core | 69 | 69 | 0 | 0 |
| dev-texlive_texlive-latex | 69 | 69 | 0 | 0 |
| dev-db_mariadb | 69 | 69 | 0 | 0 |

Rows compared as `(atom, consumer)` sets, parsed from the
`… required by (…)` lines of each manager's skip block
(`l0-report.txt` still files the one `dev-lang_perl` finding under
`perl-544-stabilization-built-pin-residual`).

So #573 now has **one** shape left: `emerge -pv dev-lang/perl`, where portuale
prints only `virtual/perl-CPAN`'s two rows against Portage's 69. Whatever the
item text says about "the 27 lines", that part is closed by later resolver work;
the plan below scopes only the remaining 67.

## 2. Why `dev-lang_perl` prints two rows

Two independent causes, one verified hermetically, one bounded by a hermetic
counter-example.

### (a) The mask parent list is deduped per `(cp, atom)`, not per consumer — verified

`resolve/feedback/reverse_dep_pins.rs` registers a skipped version's parents once
per `(cp, atom)`:

```rust
if grown.reverse_dep_masked.insert((pin.cp.clone(), pin.atom.clone())) { … }
```

Every installed `virtual/perl-*` records the *same* stripped atom
(`=dev-lang/perl-5.42*`), so the first pin in sort order creates the mask with
its parent rows and every later consumer's identical-atom pin is dropped from the
masks — and with it from the skip block, which is derived from
`MaskReason::SlotConflict { parents }` (`withhold.rs`, `backtrack_missed_updates`).

Hermetic reproduction, `captures/` (scratch ROOT, fixture `testrepo`):

* two installed in-graph consumers, `dev-libs/p573virt-1` and
  `dev-libs/p573world-1`, recording byte-identical `=dev-libs/p569prov-5.42*
  dev-libs/p569prov:0/5.42=`;
* `emerge -pv dev-libs/p569prov` (5.44 candidate withheld).

| | rows |
|---|---|
| Portage | 4: world built, world menu, virt built, virt menu |
| portuale | 2: **virt built, virt menu** — `p573world` absent |

Portage's four-row block (`captures/portage-p569prov.stderr`):

```
  (dev-libs/p569prov-5.44:0/5.44::testrepo, ebuild scheduled for merge …) conflicts with
    dev-libs/p569prov:0/5.42= required by (dev-libs/p573world-1:0/0::testrepo, installed …)
    =dev-libs/p569prov-5.42* required by (dev-libs/p573world-1:0/0::testrepo, installed …)
    dev-libs/p569prov:0/5.42= required by (dev-libs/p573virt-1:0/0::testrepo, installed …)
    =dev-libs/p569prov-5.42* required by (dev-libs/p573virt-1:0/0::testrepo, installed …)
```

(An earlier revision of this cell kept a tree `p573virt-2` that also required
`=p569prov-5.44*`. That makes Portage's search rebuild `p573virt-1`, and a parent
the graph rebuilds loses its built slot-operator component
(`depgraph.py:2509-2517`), so the block printed three rows. The fixture is gone;
the quirk is recorded as Q23 in `LLM/portage-quirks.md`.)

### (b) Out-of-graph consumers: the complete-mode walk reaches them — decision record

In the bed's `dev-lang_perl` probe Portage lists 63 consumers that are not in the
graph at all (`dev-perl/*`, `app-text/po4a`, `perl-core/File-Temp`, …), while
portuale lists none of them. Their only relationship to perl is the built atom:

```
/var/db/pkg/dev-perl/PerlIO-utf8_strict-0.10.0-2/RDEPEND: … dev-lang/perl:= dev-lang/perl:0/5.42=
/var/db/pkg/app-text/po4a-0.74-r1/RDEPEND: dev-lang/perl:0/5.42=
/var/db/pkg/sys-apps/util-linux-2.42.2/BDEPEND: … po4a …
```

They are reached by Portage only in complete mode: `emerge -pv --debug
dev-lang/perl` in the bed image walks the whole installed graph
(`… <- dev-perl/Syntax-Keyword-Try <- app-text/po4a <- sys-apps/util-linux <-
… <- virtual/libcrypt`), so each consumer becomes an already-installed graph node
and a parent of the skipped update.

**The condition.** Portage lists an out-of-graph consumer of the skipped update
iff both hold:

1. the pass is in complete mode — explicitly, self-enabled by an installed
   package changing version/slot/USE in the graph (`_complete_graph`,
   `depgraph.py:8628-8675`), or forced by `_resolve_conflicts` when any slot
   conflict exists and backtracking is allowed (`:9472-9480`); and
2. the consumer is in the installed dependency closure of the required sets
   (args ∪ `@world` ∪ `@selected` ∪ `@system`) that the complete walk traverses
   (`:8605-8675`). A consumer nothing reaches — an installed leaf no required
   set depends on — is *not* listed even in complete mode.

That second half is what the S0 capture already showed: with an out-of-graph
consumer (`dev-libs/p573real-1`, recording only `dev-libs/p569prov:0/5.42=`) and
no version change in the graph, **neither** manager lists it, because nothing in
that probe makes Portage walk the vdb. So "scan every installed consumer" is
*not* Portage's rule and would add a row Portage does not print.

**The complete-mode cell** (`probe-p573.sh`, cell `p573up`). The bare-provider
argument cannot turn the walk on by itself: the withheld `p569prov-5.44` merge
node is gone by the time the graph settles, and portuale's `portage_complete`
reads the *settled* entries (`ctx.rs:226-238`, `complete_graph.rs:29-52`). So the
cell adds a world member that supplies a real version change:

* installed world member `p573up-1`, tree `p573up-2` — the request carries
  `-u`, so `p569prov-5.44` is still withheld while `p573up` settles as
  `Upgrade 1 -> 2`; that surviving change is Portage's `complete_if_new_ver`
  arm and portuale's `portage_complete` gate;
* installed world member `p573host-1`, recording `RDEPEND=dev-libs/p573real` —
  the installed deep dependency the complete walk reaches, which is how the
  out-of-graph consumer `p573real-1` becomes a graph node and a parent;
* `p573virt-1`/`p573world-1` keep pinning 5.42, so the provider update stays
  withheld (a skip block, not a slot-conflict abort).

Result (`captures/portage-p573up.stderr`): Portage lists five rows —
`p573world` built + menu, `p573virt` built + menu, and **`p573real-1`'s built
atom** — with the merge list unchanged (`[ebuild U] dev-libs/p573up-2`, one
upgrade). Before this slice portuale printed only `p573virt`'s two rows (the
same `(cp, atom)` dedup as §2a, plus no out-of-graph reach at all); after it,
both cells are set-equal.

**The gate, measured** (Task 3 Step 0; one temporary `eprintln!` in
`scan_reverse_dependencies`, then removed). Fields: `complete` = the run's
explicit complete walk, `portage_complete` = the per-pass auto-enable,
`seeds` = `config.complete_seed_atoms` length, `report_reach` =
`ctx.report_reachable` (the required sets' installed closure),
`reachable` = `live_reachable_consumers` (the pass's own walk),
`enforced`/`report` = the scan's buckets, `entries` = the pass's graph.

```
# bed `emerge -pv dev-lang/perl`, phase 1 (the pass that withholds 5.44)
complete=false portage_complete=true seeds=0 report_reach=313 reachable=0 enforced=5 report=64 entries=13
```

`portage_complete` is true on the pass that withholds the update, from the
version-change arm alone (`complete_if_new_ver` sees the transient `5.44` merge
node / the settled `p573up` upgrade before it is masked). What the plain pass
lacks is the *closure*, not the gate: `complete_seed_atoms` is empty until the
complete retry, so `live_reachable_consumers` is empty. The report scan
therefore closes `blocker_retry_seed_atoms` — the CLI fills those on every
phase — in `ctx.report_reachable`, and only that report scan reads it. No
second resolve runs, so the merge list cannot move; an earlier attempt that
forced Portage's `_resolve_conflicts` arm as a phase-2 retry did move it
(`emerge -pv dev-libs/p569pull` merged `p569pull-2` where Portage keeps `-1`)
and was dropped.

Why the report bucket and not a pin: the 27 real consumers' atoms *are*
satisfied by the instance the merge keeps (5.42), so `reverse_dependency_constraints`
correctly never makes them constraints — Portage still records them as missed
update parents because the atom does not match the *skipped* version
(`_process_slot_conflicts`, "Record missed updates", `depgraph.py:2107-2121`).
The 21 virtuals' menu atoms needed the same treatment in the report pass only:
their `||` demotion there uses `or_branch_vdb_deselected_set` (the installed
view), because the post-merge view demoted every arm of `virtual/perl-Carp`'s
group and lost its `=5.42*` row.

After the report pass the bed prints **69 rows / 48 consumers**, set-equal to
Portage (`l0-20261028T205343Z`: 92 clean probes, `dev-lang_perl` gone from the
findings, no new slug), and `perl-544-stabilization-built-pin-residual` no
longer matches anything.

The rejected shapes stay rejected: `dev-libs/p573pull-3/4` aborted instead of
healing, and a tree `p573up-2` that itself requires `=p569prov-5.44*` never
settles (the withheld provider leaves it unsatisfiable, so it is reported as a
skipped update and no version change survives) — that was attempt 1, recorded
here so it is not retried.

## 3. Rejected shapes (do not re-try)

`dev-libs/p573pull-3/4` (`=p569prov-5.44* dev-libs/p569virt …` plus an installed
package upgrade to force complete mode) both **abort** with the slot-conflict
header on both managers (rc 1) instead of healing into a skip block. The
complete-mode trigger for the fixture still needs a shape whose provider update
is *withheld* rather than fatal — that is Task 1's job.

## 4. Provenance

* Baseline bed log: `../pmtest/differential-test-bed/logs/l0-20261028T083834Z`
  (`real/dev-lang_perl.txt`, `portuale/dev-lang_perl.txt`, `l0-report.txt:129`).
* Portage debug walk: `emerge -pv --debug dev-lang/perl` on
  `localhost/test-portuale:latest` (Portage 3.0.83, tree `350c0882`), and the
  matching portuale `--debug` run.
* Hermetic captures + probe: `LLM/evidence/2026-10-28-573/`
  (`probe-p573.sh`, `captures/{portage,portuale}-{p569prov,p573up,p569pull}.*`;
  `p569prov` is the four-row in-graph cell, `p573up` the complete-mode cell,
  `p569pull` the unchanged control).
* Fixtures added for the probe (repo side only; the installed vdb is built into
  the probe's scratch ROOT so the shared fixture vdb is untouched):
  `fixtures/repo/dev-libs/{p573real,p573virt,p573world,p573up,p573host}/` and
  their `fixtures/repo/metadata/md5-cache/` entries. `p573virt-2` was deleted
  after it turned the four-row oracle into a three-row quirk (Q23).
