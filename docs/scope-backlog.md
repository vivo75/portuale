# Scope backlog

What real portage does that portuale doesn't (either side), the standing
non-goals, and the honest distance to a drop-in replacement. **Not** a
Python-vs-Rust parity backlog — every slice ships on both sides in one
commit, verified byte-identical via the shared contract suite. An
inventory scan (CLI flag tables, `--json` fields, git history) finds zero
Rust-vs-Python behavioural gaps.

`what-this-proves.md` is the authoritative record of what has shipped;
`git log` is the slice history. Re-verify any entry here against both
before trusting it.

> **Compaction passes:** 2026-08-31, 2026-09-03, 2026-09-05, **2026-09-10**
> (this one: collapsed the B/C/D/E "complete" narrative, reframed Part 4,
> added §I test-bed and the `--solver=` real-tree findings). Pre-pass
> snapshots in `history/scope-backlog-<date>.md`, which carry the fuller
> per-cut investigation trail; `git log` has the same per-commit.

---

## Part 1 — shipped (capability index)

The `emerge` / `ebuild` loop is real and live — it resolves, builds,
merges, and unmerges real Gentoo packages (verified end to end against a
real tree in `TEST/`). At a capability level:

- **`--pretend` resolution** — the full atom / slot / sub-slot / USE-dep
  grammar; `||` groups; every `*DEPEND` key; the `--update` / `--deep` /
  `--newuse` / `--changed-*` / `--with-*` / `--exclude` / `--newrepo`
  selection family; every `package.*` file, repo-scoped across main + overlays;
  the whole `USE_ORDER` chain; `USE_EXPAND`; `REQUIRED_USE`; keyword /
  license / PROPERTIES / RESTRICT masking; slot-operator rebuild edges;
  blocker + slot-conflict detection; a `'backtrack` retry loop
  (reconciles solvable slot conflicts, masks unsolvable ones via
  `runtime_pkg_mask`, drives the full `--autounmask*` read-only family and
  both `||`-preference feedback paths); the full `resolver/output.py`
  bracket layout + colour + counters + `--tree` / `--columns`; bare
  command-line names; `--json` provenance trace; `--pretend --debug`
  resolver trace; alternate `--solver=pubgrub|resolvo` backends (see §J).
- **Real ebuild phase execution** — the full `pkg_pretend → … → install`
  chain via an embedded `brush` (default `bash`) over unmodified
  `bin/*.sh`; real eclass `inherit()`; real `SRC_URI` fetch (Manifest
  digests, `mirror://`, `RESTRICT`, resume).
- **Real filesystem mutation** — `ebuild <file>` merge / unmerge / qmerge
  / package / config / info / prerm / postrm; real `CONFIG_PROTECT`,
  `collision-protect` / `protect-owned`, preserve-libs (full `LinkageMap`,
  merge **and** unmerge), `env_update()` / `ldconfig`, fifo/device
  `CONTENTS` nodes, `os.lchown` / `os.chown` preservation.
- **Non-`--pretend` `emerge`** — `emerge <atom>` source build+merge
  (New / Upgrade / Downgrade / Reinstall, in-place same-slot replace);
  `--getbinpkg[only]` remote download+merge; `--buildpkg[only]`;
  `FEATURES=buildpkg`; `--keep-going`; `emerge -jN` parallel scheduler +
  `--load-average` + build-log capture + `--quiet-build`; `--resume` /
  `--skipfirst` (mtimedb); `--ask` / `CLEAN_DELAY`; world / world_sets
  recording; `@world` / `@selected` / `@installed` / `@set`; `-C` /
  `--depclean` / `--prune` / `--config` / `--deselect` real removal;
  `FEATURES=unmerge-backup` quickpkg; gpkg `.sig` signing/verification.
- **Whole-`emerge` actions** — `--info` (byte-exact vs live, incl.
  config-layer stack), `--regen`, `--check-news`, `--search` /
  ambiguous-name, `package.provided`.
- **Infra** — musl static build; edition 2024; the `err-*` per-crate
  error model; `mrg` applet (clap over the emerge codepath) + the
  `mrg-director` eight-slot contract layer; the `TEST/` L0 + L1
  differential test beds.

---

## Part 2 — genuinely still open


### A. Resolver

Every *forward-pass* resolver feature is shipped, and the `'backtrack`
retry loop (real `_emerge/resolver/backtracking.py` shape) reconciles a
**solvable** slot conflict, masks a version to resolve an **unsolvable**
one via real `runtime_pkg_mask`, drives the full `--autounmask*` in-loop
family, the slot-operator-rebuild sub-slot cascade, and both real
`||`-preference feedback paths — see Part 1 and `what-this-proves.md`
for the cited detail. What remains is architectural — a single-pass BFS
can't grow into these incrementally:

- **"Backtracking exhausted" diagnostics** — *narrower than it looks:*
  the `--backtrack=30` advisory-hint gating already ships; real's
  remaining signal is the `Dependency resolution took X s (backtrack:
  N/M).` report line, whose timing is non-deterministic (a deliberate
  cut — portuale is a deterministic tool).
- **Masked-dependency abort parity** — the *disclosure* half shipped
  2026-09-10 (a dependency atom matching masked-only ebuilds reports
  real's "All ebuilds … have been masked" block plus its
  `(dependency required by …)` chain instead of the bare
  `!!! no visible ebuild` line; see `what-this-proves.md`). The *abort*
  half **shipped 2026-09-11** with backlog #19 (Slices 1–5): real's
  abandon (no merge list, exit 1) is ported dual-language behind
  `PORTUALE_ABORT_PATH` (`=0` keeps the legacy report-don't-enforce
  list + exit 0); `dev-libs/abort-masked-{mid,last}` + `maskneedpkg` /
  `kwneedpkg` pin it.
- **Slot-collision notice's remaining cuts** — `pkg_use_display` for a
  package with non-default USE **shipped 2026-09-05**: every instance
  header and every shown parent line now carries that package's own
  `pkg_use_display(pkg, modified_use=…)` (`USE="…"` + `USE_EXPAND`
  groups, every IUSE flag, enabled-first, `( )`-wrapped for force/mask),
  via a new per-instance/per-parent `use_display` on `SlotConflict`
  (`what-this-proves.md`'s "slot-collision notice `pkg_use_display`"
  entry).   The `use` reason keys have since **shipped observably**
  (`dev-libs/slotusegroup` fixture: unconditional `[y]` before violated
  `[x]`, `^` spans, no color; see `what-this-proves.md`): classification,
  display selection, unconditional-first ordering, and USE-token markers
  all work dual-language, byte-identical. The `need_rebuild` trailer code
  is also landed but still dormant (no fixture can trigger it yet). Still
  cut or blocked:
  - operator/USE-token colorization **shipped (Tier 1, decided)** --
    with one deliberate divergence: real `highlight_violations` wraps
    the violated spans in red and then marks the *pre-color* indices,
    so its `^` line drifts (genuine upstream bug, and real's own red
    varies by terminal colormap, so byte-parity with the drift is
    unachievable anyway); portuale wraps the same spans but marks the
    displayed string, so color and carets agree (see
    `history/scope-backlog-2026-09-05.md` for the original analysis and
    `what-this-proves.md` for the decision);
  - the `soname` reason key -- **unreachable, not merely unimplemented**:
    `portage-dep` cannot parse soname atoms and dep flattening drops
    unparseable tokens, so no soname parent atom can ever reach the
    collision renderer (a deliberate non-gap, not a cut);
  - the `need_rebuild` fixture (blocked on the vdb walk below);
  - **resolver substrate, shipped or scoped** (all verified live against
    real portage): slot-reuse USE re-verification **shipped** (a
    `>=T-1.0[x]` parent no longer silently reuses a resolved x-off
    instance -- it pulls a second instance and reports the conflict;
    the backtrack solvability pre-check is USE-aware too); conflict
    records merge per (slot, existing, current) triple (real keeps one
    handler per slot); puller filing is USE-aware and subslot-carrying
    (built-`:=` parents no longer vanish);
    `--dynamic-deps=n` not switching the AlreadyInstalled walk source
    via the CLI (both languages walk ebuild deps; the
    `dynamic_deps_picks_ebuild_vs_vdb_deps` unit test disagrees --
    unexplained, needs owner eyes);
    literal bound `:=` atoms in ebuilds -- **investigated (Tier 1),
    premise contradicted, not a gap**: no "improper context for
    slot-operator built atom syntax" masking exists in the vendored
    checkout, site-packages 3.0.82.2, PyPI 3.0.82.2, or upstream main
    -- all parse `:slot/sub=` and match it structurally, exactly like
    `portage-dep` already does (see `what-this-proves.md`).
  - instance display order (resolved-first vs real's arbitrary
    set-iteration order -- pre-existing, no semantic content).
- **Circular-dep's remaining cuts** — full elementary-cycle enumeration /
  `large_cycle_count` **shipped 2026-09-10** (`merge_order.rs` ports of
  `digraph.get_cycles` + `_prepare_reduced_merge_list` over the
  scheduling graph, dual-language: `> 3` records fire the "lot of
  cycles" trailer with suggestions, and the cycle members re-display as
  their own flat list between the merge list and the error block;
  `dev-libs/cyc4a`–`cyc4d` four-ring fixture, verified live against real
  3.0.82.2; see `what-this-proves.md`). The partial flat list +
  cumulative counters **shipped 2026-09-11** with backlog #19 (Slices
  1–5): an unserializable cycle renders the stuck remainder as the only
  list (flat lines, unique-package counters) with the circular block +
  suggestions + `large_cycle_count` trailer after it; a masked/unsat
  abort in the same graph suppresses the circular block (walk died
  before serialization — oracle `abort-masked-cycle`); an autounmask
  coincidence prints circular-then-USE (real `display_problems` order).
  Still cut: the tree *nesting* (`[nomerge]` marking, node duplication
  -- portuale's tree model dedups by design; row-counted `Total:` goes
  with it), and backtrack-masking members out of the cycle.
  The *conditional* `followup_change` grandparent
  variant has a fixture now (`dev-libs/fucyclea`/`fucycleb`/`fucyclec`,
  see `what-this-proves.md`); the *hard*-clash case has had one since
  2026-09-05.
- **`emerge --pretend --debug`: real's resolver trace — shipped
  2026-09-07.** All six stages plus the header/cycle dumps, dual-language
  (`portage-repo/src/resolver_trace.rs` + the `emerge_pretend_reference.py`
  mirror), on real's own stdout/stderr split: `Arg:`/`Atom:`, the
  per-atom `ebuild:`/`installed:` candidate list, the per-package
  `Parent:`/`Depstring:`/`Priority:`/`Candidates:` / `Child:`/`Parent
  Dep:` / `Virtual Parent:` / `Exiting...` narration, the `forced
  reinstall atoms:` / `slot operator dependencies:` / `forced rebuilds:`
  summaries, and the `\ndigraph:\n\n` + `debug_print()` merge-digraph
  dump + `runtime cycle digraph` dumps. Deliberate divergences (goal is
  duplicated info, not a byte-copy of real; contract pins Rust==Python):
  plain-text node labels, portuale's post-prune closure + pseudo-arg
  nodes as the node set, BFS-ordered narration (successful pass only),
  ebuild+installed candidates only, `abi_rebuilds`-only slot-op dump.
  See [`history/emerge-pretend-debug.md`](history/emerge-pretend-debug.md) and
  `what-this-proves.md`.
- **Merge-list order, remaining cuts.** The `_serialize_tasks` port
  itself shipped 2026-09-06 (`portage-repo/src/merge_order.rs`): a typed
  `DepPriority` digraph, the `DepPriorityNormalRange`/
  `DepPrioritySatisfiedRange` `ignore_priority` ladder, `_merge_order_bias`
  + `_find_deep_system_runtime_deps`, `find_smallest_cycle`/`gather_deps`,
  `asap_nodes` (`PDEPEND` promotion), and real's own
  `_dep_disjunctive_stack` deferral of `||`/`virtual` deps. Verified
  live against real portage: `net-libs/rest` 15/15, `sys-devel/gcc`
  14/14, `app-crypt/gnupg` 14/14 exact-position. The `asap_nodes`
  *libc-first* seeding (real merges the `virtual/libc` / `virtual/os-headers`
  provider asap, bug #303567 / #328317) **shipped 2026-09-07**
  (`merge_order::seed_toolchain_asap`: the graphed `virtual/libc` /
  `virtual/os-headers` entry's `RDEPEND` providers seed `asap_nodes`
  before the selection loop, os-headers first). `--implicit-system-deps=n`
  **shipped** (the `_merge_order_bias` early return, threaded
  CLI → `ResolveRequest` → `serialize_merge_order` on both sides; see
  `what-this-proves.md`). The `_FrontierDigraph` perf layer **shipped
  2026-09-10** (`merge_order.rs::SerializeFrontier`: per-node/per-filter
  surviving-child counts + per-filter ready heaps, wired into the hot
  leaf queries, `PORTAGE_SERIALIZE_FRONTIER_DISABLE` falling back to
  plain scans; pure perf, no behaviour change -- see
  `what-this-proves.md` for the numbers). Still open: blocker/uninstall
  interleaving (a `--pretend` merge graph has no uninstall nodes to
  interleave).
- **`_complete_graph` as graph *nodes*.** Its reverse-dependency
  **atoms** shipped 2026-09-07 (`reverse_dependency_constraints` — a vdb
  reverse scan fed into the `'backtrack` loop's `slot_constraints`,
  closing the `media-libs/libdisplay-info` membership divergence; see
  `what-this-proves.md`). **Shipped 2026-09-10** the rest of what the
  nodes observably do: pins jointly unsatisfiable with the hard pullers
  are *dropped* instead of enforced (explicit versioned requests and
  hard dependency requirements merge anyway), and the dropped pins
  build residual conflict records pairing the merge instance against
  the installed instance (highest installed version matching the pin)
  with installed consumers as `installed in '<root>'` parents --
  verified live against real 3.0.82.2 on three probe shapes (explicit
  pin, hard-dep pin, needer/othermod triangle) and pinned
  dual-language (`paired`/`keeper`/`needer`/`othermod` fixtures; see
  `what-this-proves.md`). Reported residuals stay informational, exit
  0, by the standing conflict convention (real exits 1). Still open
  inside this shape: `--backtrack=0` skips the feed loop entirely
  (real still completes its graph -- satisfiable-pin enforcement at
  max=0 needs within-pass enforcement, its own slice);
  reinstall-with-slot-change stays outside the scan gate; the 1854-node
  re-walk itself stays deliberately unported (the `_serialize_tasks`
  validation showed the nodes are not needed for ordering; nothing
  observed needs them for membership beyond what the pins +
  residuals now cover).
- **`--root-deps` / multi-root, remaining edges.** *Mostly a non-gap for
  this fork* — the ebuilds are all EAPI 7+, where `--root-deps=rdeps` is
  a complete no-op and `BDEPEND`/`IDEPEND` always resolve against the
  running root (which portuale does, `--root-deps` or not). The full
  multi-root graph (a `root` per dependency edge) stays a deliberate
  edge-by-edge approximation; a running-root entry's `PDEPEND` stays a
  target-`ROOT` concern (a permanent non-gap).
- **Slot-operator rebuild v1 cuts — closed 2026-09-12 (#24 S1–S5), v2
  carved out.** Investigated 2026-09-05: real's slot-operator machinery
  is a *reconciliation* with an undo path
  (`_slot_operator_update_probe`/`_backtrack`/etc.,
  `depgraph.py:2400-3200`); portuale's `slot_operator_rebuild_entries`
  fixpoint had no undo path at all, so "single-pass" and "no
  `--changed-slot` interaction" were the same missing piece, not two —
  see `history/scope-backlog-2026-09-05.md` for the full citations.
  **Shipped:** S1 the complete-mode gate under `--deep` (the a522084
  `B-0` miss was the missing auto-enabled pass); S2 22 oracle pins over
  the `test_slot_operator_*` family (`docs/024-oracle.md`); S3 the
  rebuild is a **walked graph node** routed through the `Backtracker`
  (`slot_operator_rebuild_scan` → `BacktrackParams::
  slot_operator_replace_installed` → in-walk seed + flip, real's
  `@__auto_slot_operator_replace_installed__`), so the consumer's deps
  are re-walked, its `:=` re-bound and its merge order real — the
  synthesiser is gone from the default solver (the `--solver=` bridges
  keep it as a documented legacy wrapper); S4 `_eliminate_rebuilds` —
  the nine ordered rules + the graph-aware `:=` binder
  (`bind_slot_operator_deps`, real `_eval_deps` over `_graph_trees`),
  demoting via the `slot_operator_undone` latch (`slotundo-unnecessary`
  MATCHes; a522084 `B-0` is kept by rule 8); S5
  `_slot_change_probe` — the slot-move-without-revbump detector (bug
  456208) schedules the installed child from the merge-bound parent's
  unbuilt `:=`/`:S=` dep (`slotchange-1`/`regslotchange` MATCH), real's
  `--changed-slot` rule 3 landed behaviour-neutrally, and two S2
  expectations were corrected live (the flag half has no `r`/edge in
  real; conflict-mass is the update probe, not this one). **Still
  cut:** bug 614390's `complete` case is a *selection* gap, not the
  undo (named bare `socc` resolves before meta's `=socc-1` through the
  already-installed fast path, which skips `resolved_slots`; real's
  `_add_pkg` slot-parent check catches it — #36 overlap); the v2 probe
  family `#24b`–`#24e` (update probe + `check_reverse_dependencies`,
  `slot_operator_mask_built`, `prune_rebuilds`,
  `_slot_conflict_backtrack_abi`) and `IUSE_EFFECTIVE` in the
  built-dep domain. `--changed-slot` itself ships standalone
  (`slot_changed`), and the unbuilt probe keeps the pre-existing
  `--ignore-built-slot-operator-deps` / `--rebuild-if-new-slot=n` scan
  gates (a documented narrowing vs real, no v1 oracle).

- **DFS-partial abort path (#19) — Gate-0 decisions recorded 2026-09-11
  (Slice 1 oracle: `docs/abort-path-spec.md`, fixtures
  `dev-libs/abort-*-mid/-last`, captures `fixtures/abort-captures/`,
  24 strict-xfail contract tests).** Owner answers: (G0.1) adopt real's
  **exit 1 on abort** for all three oracled shapes (masked-only,
  unserializable cycle, unsat atom) — the `maskneedpkg`/`kwneedpkg`
  CASES exits flip in Slice 5, and the slot-conflict "informational,
  exit 0" convention is reconciled explicitly there; (G0.2)
  **membership + deterministic flat order**, not byte-parity — real's
  tree duplication + `[nomerge]` rows + row-counted `Total:` stay a
  deliberate cut under the dedup-by-design rule (moot for masked/unsat,
  which print no list at all); (G0.3) **all four shapes in v1** —
  including the autounmask+cycle partial-altlist shape (the original
  plasma-meta cluster-A truncation), which still needs a synthetic
  fixture (Slice 3 prerequisite) plus pass-retaining backtrack state;
  (G0.4) the path lands **flag-gated** (`PORTUALE_ABORT_PATH=0`
  fallback), so Slice 2 stays behaviour-neutral.

- **DFS-partial abort path (#19) — Slice 3 (membership/order) shipped
  2026-09-11.** `abort_outcome` / `_abort_outcome` classify the settled
  graph into `ResolveOutcome::Aborted { reason, partial }` on both sides:
  a `NoVisibleCandidate` dependency with a merge-bound requirer →
  `MaskedDep` (has a `MaskedDepReport`) or `UnsatisfiedAtom` (anything
  else, `[use]`-dep mismatches included), empty partial; else a hard
  cycle → `UnserializableCycle { members = cycle_display }`, partial =
  the remainder entries in leaf-drain order. Walk-time beats
  serialize-time (oracle `abort-masked-cycle`). The fourth Gate-0 shape
  was oracled (`abort-au-cycle`, `abort-au-restart-cycle`, spec §4d) and
  is the cycle shape — no `AutounmaskPartial` variant, no
  pass-retaining state. Observable change with the gate on: exit 1 for
  every such abort (the Slice 2 arm, now placed after the circular
  block); 14 CASES + 25 pinned assertions re-pinned 0→1 (`maskneedpkg`,
  `kwneedpkg`, `missingdep`, the `--autounmask-use=n` shapes, `--root-deps`
  build-dep misses, `--usepkgonly` binpkgs with no binary for a dep — all
  "unsatisfiable dep of a to-be-merged parent", real `_add_dep` returns
  0). The list itself is still the full one (Slice 4), the error blocks
  unchanged (Slice 5). Deliberate cuts recorded: first-failure choice in
  BFS admission order when two walk-time failures coexist; installed
  parents' unsatisfied deps never abort (real's
  `_initially_unsatisfied_deps` rescue — the `--deep` sub-cases are
  uncaptured). **Real-tree residue (not the abort path):** plasma-meta/
  podman need the `||` choice to follow real (`>=dev-lang/go` in-graph
  on pass 0, cycle, abort with the remainder when autounmask changes are
  present; `circular_dependency`-map re-resolve otherwise) — portuale's
  `circular_self` pick of `go-bootstrap` on pass 0 hides the cycle. Stays
  under the L0 `truncated` suppression until that `||` slice.

- **Autounmask side findings from the Slice 3 oracle (open, not #19;
  strict xfails `test_autounmask_only_resolve_prints_no_terminated_early_
  notice`, `test_autounmask_cascade_flip_before_dep_walk_pulls_the_gated_
  leaf`, captures `fixtures/abort-captures/dev-libs_{abort-au-plain,
  aucasctop}.*`, spec §4e).** (a) Real prints the "backtracking has
  terminated early" notice only when the autounmask change coincides
  with another failure (`need_config_change` returns on
  `_success_without_autounmask` before setting
  `_autounmask_backtrack_disabled`, `depgraph.py:11713-11717`);
  portuale prints it for every change with backtrack off. (b) Real's
  DFS applies an in-graph `[use]` flip before walking the flipped node's
  own deps when that node was pushed but not yet popped (`aucasctop`:
  `aucascleaf` IS listed, `Total: 4`); portuale's already-resolved-slot
  re-check leaves the `flag?`-gated dep out. The Slice 1 captures were
  also re-verified with a clean `/etc/make.local` (the host file had
  leaked `--binpkg-respect-use=y` → `--autounmask-use=n`): unchanged.
   Also noticed (pre-existing, not introduced here): `--json` on
   `abort-masked-mid`/`abort-unsat-mid` orders the `no_visible_candidate`
   entry differently in Rust (BFS admission position) and Python (after
   the sibling leaves) — invisible in the text modes, which are
   byte-identical; the contract suite never diffs `--json` on those.
   (Slice 5 correction: that order gap never existed — it was a stale
   binary + a `"None"`-vs-`""` installed-node label artifact, both
   resolved; `--json` merge_order is byte-identical on those atoms, and
   the label renders `""` on both sides like Rust's `unwrap_or("")`.)

- **DFS-partial abort path (#19) — Slices 4+5 (rendering + error-block
  wiring) shipped 2026-09-11, closing the item.** Slice 4 renders the
  outcome's partial instead of `entries` on every mode (`-p`/`-pv`/
  `-pvt`/`--columns`/`--debug`/`--json`): masked/unsat show no list and
  no `Total:`, cycles show the flat remainder with counters over its
  rows; the duplicate re-display is skipped on a gated cycle abort;
  `--json` gains the `aborted` reason field with `entries` as the
  partial list, exit 1 in every format. Slice 5 wires the consumers:
  the circular block prints before the autounmask section on a gated
  cycle abort (real `display_problems` order) and is suppressed on a
  masked/unsat abort; disclosures re-emit in full-entries order (stderr
  sequence unchanged — their move into the `AbortReason` block is
  deferred, content identical). The 24 Slice-1 xfails are plain passing
  tests; remaining strict xfails: the two spec-§4e autounmask findings
  (not #19) and one `--debug` narration (Python trace shows the
  pre-flip USE on a backward-cascade Child: line, real+Rust post-flip).
  Deliberate cuts carried forward: tree nesting + `[nomerge]` +
  row-counted `Total:` (dedup-by-design, Gate G0.2); first-failure
  choice in BFS admission order; installed parents' unsatisfied deps
  never abort (real's `_initially_unsatisfied_deps` rescue).
  **Gate G0.1 reconciliation (Slice 5.6):** exit 1 covers the three
  unfixable abandon shapes (masked/unsat/cycle — the resolver cannot
  proceed, no list exists). Slot conflicts keep the standing
  informational exit 0: the backtrack loop reconciles the solvable ones
  (resolution *succeeds* with notices) and unsolvable residuals print
  with actionable suggestions — a completed resolve with diagnostics,
  not an abandonment; 6 CASES entries pin exit 0, and flipping them is
  out of #19's scope (real exits 1 there via a different
  `select_files` failure — a documented divergence, as before).

### B / C / D / E — complete; residual documented cuts only

**B. Scheduler / build orchestration** (2026-09-04): merge-hook log
capture, tokio-runtime kill-in-flight, `mtimedb["resume"]` rotation,
`--ask` TTY/re-prompt, `elog` syslog/custom, `elog` `mail`/`mail_summary`
(Tier 1: MIME + sendmail-binary/plain-SMTP delivery, per-package and
run-wide summary; STARTTLS stays a documented cut),
`FEATURES=compress-build-logs` (Tier 1: `.gz` log paths + gzip pump +
gunzip tails), resumed binary entries resolving from local `$PKGDIR`
only (Tier 1: the binhost fallback is gated on `remote_binary`).
Complete; no residual cuts.

**C. Config resolution depth** (2026-09-03; later refined by the
per-level `USE_EXPAND` fold, slices J-neovim / P). The whole `USE_ORDER`
chain incl. per-profile-level `defaults` interleaving, `env.d` from
`eroot` (Tier 1, not `config_root`), and the per-entry expand map for
`package.env` files (Tier 1: global-map + within-file/cross-file
chaining; `env.d` values verified already-correct literal/order). No
residual cuts (the broot half of `_get_env_d`'s two-file merge stays a
documented cut: no distinct `BROOT`, and host `/etc/profile.env` must
never leak into deterministic resolution).

**D. Sandbox / build isolation** (2026-09-04): the `FEATURES` isolation
set (`unshare` + `sandbox`) wraps the six `src_*` phases;
`network-sandbox` / `live` / `test_network` exemptions; real `FEATURES`
passthrough to the phase env; `Packages`-index `USE` back-fill;
per-package `package.env` on standalone `ebuild <file> <phase>` runs
(Tier 1: atom-matched on the ebuild's md5-cache identity) and
`PORTAGE_RESTRICT` / `PROPERTIES` reduction on the config-`USE`
`depend` phase (Tier 1). SELinux sandbox,
`userpriv`/`fakeroot` — non-goals (Part 3).

**E. Binary packages / fetch** (substantially complete 2026-09-04..09):
remote-binhost MD5+SHA1 indexing, gpkg mtime revalidation,
binpkg-multi-instance for both formats, `--binpkg-changed-deps` /
`--rebuilt-binaries` / `--use-ebuild-visibility` overrides, `.sig`
signing/verification, `BUILD_TIME`-vs-installed reinstall, quickpkg
multi-instance. `identical_binary` and `--useoldpkg-atoms` +
multi-instance were investigated and found already-correct. Fetch
candidate ordering / `RESTRICT=primaryuri` **shipped (Tier 1)**:
local flat-layout mirrors, public `GENTOO_MIRRORS`, inline
`mirror://` expansions, literals (appended, or prepended with the
third-party group under `primaryuri`), plus `FEATURES=force-mirror`;
still cut inside that shape: third-party shuffle (determinism), live
`layout.conf` negotiation, on-filesystem `fsmirrors` copies, and the
multi-URI-per-file interleave.


### F. Whole `emerge` actions

The action and modifier-flag surface is broadly complete — `--regen`
stale-entry pruning + eclass masters-chain lookup, `--check-news` real
`.unread`/`.skip` write-back, `--info <atom>`'s `( )` force/mask wrap +
ANSI USE colour all shipped 2026-09-05, see `what-this-proves.md`'s
"Whole emerge actions backlog" entry for the cited detail. Remaining:

- `--info` **config-layer completeness shipped 2026-09-07**: portuale now
  reads `cnf/make.globals` (the base db, via a new multi-line-quote /
  apostrophe-comment-safe `logical_lines`), `/etc/profile.env` (the
  `env.d` db — `CONFIG_PROTECT*` fragments + scalars like `LANG`/`LEX`),
  and `<PORTDIR>/profiles/info_vars` (the extra `myvars` names). The
  `const.INCREMENTALS` displays (`FEATURES`, `CONFIG_PROTECT`,
  `CONFIG_PROTECT_MASK`, `ENV_UNSET`) are `-*`/`-tok`-resolved then
  sorted, exactly as real `config.regenerate()` stores them
  (`Config::resolved_incremental`); `USE_EXPAND` variable display values
  are USE-consistent-resolved (`-* intel …` → `intel …`,
  `GRUB_PLATFORMS`); `CBUILD` defaults to `CHOST`, `PORTAGE_CONFIGROOT`
  is stamped; env-only `info_vars` (`SHELL`) fall through to the process
  env. `Binary Repositories:` now shows `location` + `verify-signature`
  in real `BinRepoConfig.info_string()` field order. Verified
  byte-exact against a live `emerge --info` for **every** `VAR="…"` line
  + both repo blocks. Dual-language, contract-pinned
  (`test_info_stacks_make_globals_profile_env_and_info_vars`).
  **Follow-on shipped same day**: (b) and (c) below are now closed —
  `UseManager.extract_global_USE_changes` (the `*/*` user-`package.use`
  fold onto global USE, USE_EXPAND shorthand included) + global
  `use.force`/`use.mask` applied to the `--info` USE line and USE_EXPAND
  values (`resolved_global_use`, real `regenerate()`'s trailing
  `myflags.update(useforce); difference_update(usemask)`); and
  `RepoConfig` gained `sync_type`/`sync_uri`/`volatile`/
  `module_specific_options` with the global `/usr/share/portage/config/
  repos.conf` merged under the user's, so the `Repositories:` block
  prints real `info_string()`'s fields. The `USE=` line and both repo
  blocks now `diff`-clean against a live run.
  **Host-state header shipped 2026-09-07** (the last `--info` piece):
  the `Portage <ver> (python…, <profile>, <gcc>, <libc>, <kernel>)` line
  (portage/glibc from vdb, gcc from `gcc -dumpversion`, profile a
  faithful `get_profile_version` port, kernel from `uname`), the
  65-char rule (+ centred `System Settings` title under `--info <atom>`),
  `System uname:` (real `platform.platform(aliased=1)` rebuilt from
  `uname` + `/proc/cpuinfo` + glibc), `KiB Mem:`/`KiB Swap:`
  (`/proc/meminfo`), per-repo `Timestamp of repository`
  (`metadata/timestamp.chk`) + `Head commit of repository` (`git
  rev-parse HEAD` for a git repo), the `sh:`/`coreutils:`/`ld:` probes,
  and the `info_pkgs` version table (six hardcoded atoms +
  `profiles/info_pkgs`, one-level `expand_new_virt` for
  `virtual/os-headers`, `<ver>::<repo>` rows). `diff <(emerge --info)
  <(portuale emerge --info)` is now a **single line** — the `KiB Mem`
  free value, which changes between the two process spawns. The
  contract's `--info` `rust==python` checks run through a
  `_normalize_info` regex filter that blanks the host-state values to
  `XXX` first. The 1-byte trailing-newline mismatch is fixed. `--info`
  is now byte-for-byte parity with real modulo genuinely-live memory.
- `--info`: the
  `(non-installed binary)` candidate path and the `pkg_info()` phase run
  itself both shipped 2026-09-05: `--usepkg --info` now selects the
  highest local `$PKGDIR` binary that defines `pkg_info()` and renders
  its `(non-installed binary) was built with the following:` block, and
  for every selected ebuild/binary/installed package that defines
  `pkg_info()` portuale prints `>>> Attempting to run pkg_info() for
  '<cpv>'` and actually runs the phase. The deterministic message is
  dual-language contract-tested; the phase's own output is Rust-only
  (`test_portuale.py`), the same test-architecture split
  `--config`/`--regen` use. The empty-`DEFINED_PHASES` falsy-check quirk
  (real `actions.py:2350`: an installed match with no `DEFINED_PHASES`
  file at all still gets `pkg_info()` attempted, while `"-"` does not) is
  matched now too (2026-09-06). The installed block's `CHOST`/
  `CFLAGS`/… **shipped 2026-09-08**: read from the vdb
  `environment.bz2` via real `_aux_env_search` (a pure-Rust `bzip2`
  backend in `portage-repo` -- default `bzip2` 0.6 over
  trifectatechfoundation's libbz2-rs-sys, zero C linkage, so the
  musl-static story and the subprocess-free boundary both hold; the
  C `bzip2-sys` backend stays off), including the `var_assign_re` /
  multi-line-continuation parser, the missing-file-means-all-`Unset:`
  rule, and the present-but-empty-matches-empty-prints-nowhere rule
  (see `what-this-proves.md`);
- `--regen` — **complete (2026-09-09)**: `--jobs`/`--load-average`
  threading (`thread::scope` dispatch with real `AsyncScheduler` /
  `PollScheduler._can_add_job` semantics + the per-builddir key
  serialization matching real `doebuild()`'s `EbuildBuildDir` lock;
  byte-identical cache *and* stdout vs serial; `--jobs=0` = CPU count
  per real `main.py:1023-1041`); the `_pull_valid_cache` shortcut (skip
  the `depend` phase when the on-disk entry is already valid --
  performance only, content-identical; the file is left untouched,
  mtime preserved); and `metadata_regen_retry`'s `cp_retry` (re-run a
  whole cp whose phase failed with an *unexpected* returncode --
  exit-code semantics: the phase's own exit verbatim, setup failures
  code 1, up to 3 passes, first-seen cp order for determinism; a
  finally-failed cpv is dropped from the valid set so no stale entry
  survives). One deliberate divergence, documented in `regen.rs`:
  real `emerge --regen`'s own `action_regen` never retries -- the loop
  is `egencache`'s path, folded into portuale's single regen tool.
  See `what-this-proves.md` for the cited detail.
 - `--check-news`: versioned/slotted `Display-If-Installed` atoms
   (2026-09-05), a `[use]`-dep in the atom (2026-09-07, checked against
   the matched version's vdb `IUSE`/`USE` via `use_deps_satisfied` —
   `portage_repo::installed_pkg_iuse_and_use`), a malformed atom
   making the whole item invalid (2026-09-07, moved into
   `news_item_valid`), and the `News-Item-Format` 1.x/2.x EAPI
   atom-validity gate (2026-09-08, real `isValid`'s `eapi="0"`/`"5"`
   split) are all handled now — the gate is implemented as a narrow
   field check (1.x rejects `:slot`/`:sub`/`:=`/`:*`/`[use]`, 2.x keeps
   parse-at-all), *not* a `portage_dep` EAPI parametrization, so no
   Part 3 non-goal is crossed (the backlog's earlier "needs EAPI
   parametrization" premise turned out stale);
- `--metadata` is an architectural no-op (portuale reads
  `metadata/md5-cache` directly, models no `depcachedir`);
- `--sync` is a permanent non-goal (points at `emaint sync`); GLSA /
  `@security` is not in scope.

### G. Shell backend

- minimize + report the brush `declare -f` heredoc bug upstream (it
  corrupts a function with a redirected here-doc, which is why the
  default backend is `bash`, not the embedded `brush`);
- periodic re-pin to keep up with upstream `reubeno/brush` `main` (see
  `brush-pin.md`'s checklist).

### H. The `mrg` applet + `mrg-director`

`mrg` is a clap front end over portuale's own emerge codepath
(`to_emerge_argv` → `pretend::run`) — resolution output and exit codes
are literally `emerge`'s. The parser covers the full `lib/_emerge/main.py`
surface. Rust-only, no Python reference. **Open: nothing** — everything
open for the emerge codepath (the other Part 2 sections) is open for
`mrg` by definition.

**`mrg-director`** (eight-slot contract layer, "Section H complete"):
`Resolver`, `PackagesDb`, `RepoCache`, `Fetcher`, `MergeEngine`,
`BinpkgIndex`, `NewsSet`, `SchedulerPolicy` — traits + ≥1 impl each.
**Wired into real paths (2026-09-10, Tier 2.28):** `SchedulerPolicy`
(via `run_build_scheduler`, as before), `MergeEngine` (the binary
crate's `SourceEngine`/`BinaryEngine` adapters execute every
`run_merge_plan` unit and the serial source-merge loop through the
seam), `NewsSet` (binary-crate `FilesystemNews` evaluates every
`--check-news` repo through the seam), `PackagesDb` (`VdbReader` reads
the vdb for real -- versions highest-first, CONTENTS paths, recomputed
reverse dependents), `Fetcher` (`WgetFetcher` runs the shared
`portage_fetch::download_via_wget` transport; Manifest verification
stays at the `fetch_src_uri` call site, the seam's documented
narrowing). `RepoCache`/`BinpkgIndex` were already real delegations to
`portage_repo` reads and gain `Director`-level methods; their
resolve-path call sites stay direct (the trait lives above the crate
that resolves). `Fetcher` / `NewsSet` are permanent singles by design.

**Hard invariant: `mrg` is portuale-only — no portage counterpart, no
Python reference.**

### I. Container test bed — L2–L5 (not started)

L0 (resolver parity) + L1 (merge parity) are shipped and run live
(`TEST/README.md`). The forward layers, planned in
`history/real-world-testing.md` §5/§14 and distilled for execution
into `real-world-testing.md` (§2–§8: controls, triage, L2–L5 designs,
risks, metrics), are not built:

- **L2** — portuale as builder: `emerge -b` the L1 set from source,
  structural `.gpkg.tar` checks (`gpkg-structure.sh`), cross-install
  (portuale-built archive merges under portage and vice versa).
- **L3** — full source-build parity: both PMs build `@system` / a desktop
  `@world` from source with `SOURCE_DATE_EPOCH` + `-j1`; diff VDB
  metadata + CONTENTS structure (tolerate compiled-artefact sha diffs).
- **L4** — `mrg` remote merge over SSH (`remote-merge.md` §6).
- **L5** — lifecycle & failure injection: `-C` / `--depclean` diffs,
  soname bump → preserved-libs, `CONFIG_PROTECT`, `--resume` after
  SIGKILL, disk-full / corrupt-archive / binhost-500 fault injection.

### J. Alternate `--solver=` backends (pubgrub / resolvo)

Selected at runtime; drive lu-zero's `portage-atom-pubgrub` /
`portage-atom-resolvo` bridges over the same repo facts
(`portage-repo/src/solver_bridge.rs`). Plumbing is well-tested against
tiny synthetic fixtures (`test_portuale.py`), but **both break at
real-tree scale** (smoke-tested 2026-09-10, not fixed):

- **`--solver=resolvo` is effectively non-functional on real targets** —
  the bridge's `install_order` cannot linearise any closure containing a
  toolchain cycle (glibc↔gcc↔perl — nearly every real target), and
  `solver_bridge.rs` surfaces the raw `Debug` of the leftover ids
  (`dependency cycle left unorderable: [280, 53, …]`). Works only on
  trivial cycle-free closures.
- **`--solver=pubgrub` massively over-merges** once the closure is
  non-trivial (`net-libs/nodejs`: 48 packages vs portage's 8). It appears
  to feed pubgrub the over-approximated reachability closure (both
  branches of every `flag?()` followed) as the actual dependency graph.
- Bridge output carries the forced-flag `( )` USE markers (Tier 1 --
  same `forced_or_masked_flags` call as the walk).
- `GraphResult`'s circular deps are wired from the bridge result (Tier
  1 -- `find_hard_cycles` over the entry `deps` edges); `slot_conflicts`
  and `autounmask_*` stay empty by construction (a solved engine plan
  admits no same-slot divergence, and no relaxation loop ran) -- so no
  `[slot conflict]` / "USE changes are necessary" block under
  `--solver=`, and an invisible candidate still fails instead of
  suggesting a flip. The module doc's "v1 cuts" list is current again
  (Tier 1 doc refresh: blockers, ABI rebuilds, merge-order edges,
  markers, circular deps all named as wired).

These predate the current merge — the `--solver=` bridge has been this
way since it was added. `docs/solver-backends-analysis.md` has the
backend comparison.


---

## Part 3 — explicit non-goals / architecture boundaries

Standing decisions, not oversights.

- **`--autounmask-write`** and any config-*writing* autounmask mode
  (conflicts with "never writes config" — the read-only suggest/resolve
  half is shipped). Note `emerge --deselect` / `-C` / `--depclean` *do*
  write `world` / `world_sets` / the vdb now — those are user state the
  package manager owns, not `/etc/portage` config.
- **A real backtracking resolver** (see Part 2.A — listed there because it
  is the substantive gap, here because `agent-context.md` scoped it out for v1).
- **PyO3 / in-process FFI embedding** — would foreclose the
  two-sibling-implementations end state.
- **EAPI 0/1/2/3/4/6** — dead in this repo; the `portage-*` crates have no
  EAPI parametrization at all within the 5+ floor.
- **`bsd_chflags`** — `None` on non-BSD; portuale is Linux-only/musl-static.
- **RPM binary packages, repo syncing (`emerge --sync`), news items,
  GLSA/`@security`, GPG for sync/webrsync (`sync-*-verify-signature`)
  and for xpak (which has no signature mechanism at all),
  Prefix/cross-`ROOT` beyond
  the `ESYSROOT` distinction** — not in scope. (gpkg binpkg
  `.sig` signing/verification itself is shipped — see Part 2.E.)
- **xpak (`.tbz2`) binary packages for `mrg`** — out of scope. `mrg`
  targets the modern gpkg format, not full binary-format compatibility;
  xpak is the old format. The shared server-side binpkg reader happens to
  open `.tbz2`, so it may work incidentally on `mrg`, but it is never a
  tested or guaranteed `mrg` path (`emerge`/`ebuild` keep their own local
  xpak support, which is fully in scope and tested). See
  `docs/remote-merge.md` §1 / §14.
- **`equery` / `portageq` / `etc-update` / `dispatch-conf`** — separate
  tools, separate binaries.
- **Directory merge traversal order** — sorted by filename for test
  determinism, not real `os.listdir()`'s arbitrary/OS-dependent order.
  `CONTENTS` line order carries no semantics portage itself relies on
  (unmerge re-sorts, `qmerge`/`qlist` sort on read), and determinism is
  worth more here than bug-compatible arbitrariness — see
  `ebuild_merge.rs`'s module doc comment.
- **Switching CLI option parsing to `clap`** — evaluated 2026-09-02,
  rejected. The parser (`pretend.rs`'s parse loop + `emerge_options.rs`
  tables) faithfully reproduces `emerge`'s `argparse` quirks that `clap`
  has no idiom for: optional values consumed only when they look like an
  integer (`--deep[=N]`, `--jobs[=N]`, `--backtrack[=N]`), `true_y_or_n`
  (bare / `=y` / `=n` / space `y`/`n`) vs `y_or_n` (required),
  `action:"append"` atom lists where each occurrence is itself
  space-split, `-pX requires an argument and can't be bundled`, and the
  exact real error strings. It also carries the
  recognized-but-unimplemented machinery (a real emerge option reports
  "not yet implemented in portuale", not "unknown") and is kept
  structurally parallel to the Python reference so the two parsers can't
  drift. `clap` would fight every one of these; ~1500 lines across two
  languages under ~1100 contract tests, near-zero payoff. This applies
  to the **`emerge`/`ebuild` parsers only** — the new `mrg` applet
  (Part 2.H) is the deliberate counter-example and *does* use clap.

---


## Part 4 — distance to a drop-in replacement

portuale is a working package manager for the **happy path of operating
on one package (or a small dependency closure) at a time** — build,
merge, unmerge, world management, all real. The gap to a full drop-in is:

1. **Resolver depth (§A).** The `'backtrack` loop's architecture is in
   place (reconciles solvable slot conflicts, masks unsolvable ones,
   renders the notices, autounmask levels inside the loop, `||`-preference
   + slot-op-rebuild feedback). `dep_zapdeps`'s finer choice bins are now
   **done** (`all_available`/`all_use_satisfied` split, `unsat_use_*` with
   the bug-515584 unmask gate, `all_installed_slots`, in-bin
   upgrade-preference ordering, and the `other_*` bins + `allow_masked`
   two-pass return, shipped 2026-09-11, one shared probe + tie-break pair
   both languages —    `minimize_slots` and the `conflict_downgrade`/
   `installed_downgrade`/`circular_atom` guards stay deliberate,
   documented cuts, the latter two carved to #35). The backtracking
   search itself is **done** (2026-09-11, backlog #23: real's node-stack
   search with ranked one-node-per-choice masks, similar grouping,
   mask-step budget, `get_best_run`, and dead-end abandonment -- see
   `docs/023-oracle.md` and `what-this-proves.md`). What's
    left is depth on other pieces already built: the follow-ups that own
    the remaining resolver divergences (`#25` installed-nomerge
    instances for btnr, `#24` `:=` rebuild for the a522084 `B-0`,
    `#36` mask-aware selection fallback for mg3 at tight budgets;
    slot conflicts keep the standing informational exit 0 --
    reconciled explicitly under the #19 Slice-5 entry above, not
    flipped), and the `_serialize_tasks`
    frontier-timing at real-tree scale (L0 merge-order: ~19 probes,
    correct set / slightly-off sequence).
2. **The Part 2 tails** — F's `--info` host-state
   half, G's brush re-pin, §J's `--solver=` real-tree bugs (E's fetch
   ordering and §J's notices/markers/doc cuts shipped as Tier-1
   slices). Each is one focused slice.

B / C / D are complete; F is substantially complete.
