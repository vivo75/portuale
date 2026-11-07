# Health-checker survey, 2026-11-07 (read-only; no code changed)

Paths: P = /home/vivo/repo/PORTUALE/portuale, T = /home/vivo/repo/PORTUALE/pmtest.
PCS = T/pytests-contract-suite.

## 0. What exists today

- `PCS/output_invariants.py` (482 lines) is the only plan checker. Input is the
  `--pretend --json` document (`check_json` :198) or the plain/--tree/--quiet text
  (`check_plain_use` :349, `check_summary` :361, `check_cross_mode` :391).
  It uses no Portage output; its docstring says "no expected value".
  Driven by `PCS/test_output_invariants.py` (:91 over CASES, :105 over every fixture
  atom, :138-:227 determinism) and, on the bed, by
  `T/differential-test-bed/compare/check-invariants.py` (reads the L0 run dir
  `portuale/<slug>.txt` and `portuale-modes/<slug>.{json,tree,quiet}.txt`; also runs
  the plain-text checks on `real/` as a control, so it touches Portage output only for that).
- `check_json` checks, and nothing more:
  duplicate JSON keys (:213); `merge_order == index` (:222); `required_by` non-empty for
  non-seeds with every owner an entry or installed (:226-:232); reachability from seeds
  (:236-:253); no duplicate cat/pkg:slot without a reported slot conflict (:255-:264);
  merge order respects `required_by` edges, cycle members excluded (`_cyclic_edges` :151),
  edges relaxed when an installed instance exists or only RDEPEND/PDEPEND carries the
  edge (:266-:330). That last exemption copies Portage's `ignore_priority` ladder
  (HARD_DEP_VARS :24).
- Meta-tests of the checker itself: test_output_invariants.py :247-:390.
- Not in the checker: any VDB, REQUIRED_USE, blocker, visibility or message check.

## 1. `--json` document (P/crates/portuale/src/pretend/json.rs)

Top level (:420): `entries, backtrack{restarts,max}, slot_conflicts, changed_deps_report,
autounmask_{keyword,use,license,mask}_changes, abi_rebuilds, aborted`.
Entry (`entry_to_json` :33): category, package, merge_order, outcome (new/upgrade/downgrade/
reinstall/already_installed/no_visible_candidate/uninstall, :49-:56), version, from_version,
reinstall flags (:80-:85), slot, source (ebuild/binary), build_id, provenance and
mask/keyword/use suggestions (:134-:185), requested (:195), required_by (:207, cat/package
only, aggregated per cp, no atom, no dep class), use_flags (:226), blockers (:233-:243:
atom, strong, matched_*, unsolvable).
`aborted` (:310-:334): reason in masked-dep, unsatisfied-atom (atom, parent),
unserializable-cycle, virtual-cycle (members).
Missing for a standalone checker: per-entry dependency atoms by class (the
`DepEdge`/`DepPriority` data in P/crates/portage-repo/src/merge_order/priority.rs:11-40
exists in memory, not in JSON); installed set (the checker takes it from `root`/`vdb-list`
snapshot, :103-:126); REQUIRED_USE; candidate set / visibility; sub-slot; the "stays
installed" set.

## 2. Per property

### P1 post-plan state: RDEPEND/PDEPEND, blockers, REQUIRED_USE, one package per slot
(a) Partly. Slots: yes, :255-:264 (duplicate slot allowed only if a slot conflict is
    reported). Blockers: `blockers` field emitted (json.rs:243) but never checked. RDEPEND
    closure and REQUIRED_USE: not checked by the checker. Resolver-side, REQUIRED_USE is
    enforced inside the resolver (portage-required-use crate, resolve/autounmask.rs, queue.rs)
    and by contract tests; blockers in P/crates/portuale/src/pretend/conflicts.rs. All
    independent of Portage output at run time (but they were built to match it).
(b) Needs: final-state set = (installed minus uninstalled/replaced) plus entries, each with
    its RDEPEND/PDEPEND/DEPEND/BDEPEND text and effective USE, REQUIRED_USE, SLOT/sub-slot,
    blockers. JSON gives USE (`use_flags`), slot, version and blockers; dep strings are not
    given. A checker could read them itself from `metadata/md5-cache` of the fixture repo
    (as `_dep_vars_for_atom` :128 already does) and from the VDB (`portage-vdb`, `vdb_cmd`).
(c) Gap: medium (about 400-600 lines Python): a PMS dependency-string evaluator
    (use-conditionals, `||`, `^^`, slot/sub-slot, `:=`, use-deps with defaults) plus
    atom matching. Do NOT reuse portage-dep/atom code from the Rust tree, so the checker
    stays independent; the Python helper `lib/portage` is off the table by decision.

### P2 install order (DEPEND/BDEPEND before dependent, RDEPEND present at the end)
(a) Approximated at :266-:330 using `required_by` as the edge list, with a class lookup
    only to relax soft edges. Relies on `required_by` produced by the resolver (circular:
    it trusts the plan's own edge claim) and on the cp-level aggregation, which forced
    several exemptions (multi-instance, installed child, :299-:316). No Portage reliance
    except the ignore-ladder logic copied from `_serialize_tasks`.
(b) Needs per-entry DEPEND/BDEPEND edges evaluated against the entry's own USE, resolved
    to the entries or installed packages. Not in JSON; derivable from md5-cache (see P1).
(c) Gap: small once P1's evaluator exists (about 100 lines); the cp-aggregation
    exemptions disappear if edges are recomputed from dep strings.

### P2a cycles resolved by a legal break, package built twice allowed
(a) Detection in Rust: P/crates/portage-repo/src/merge_order/cycles.rs
    (`find_smallest_cycle` :21, `elementary_cycles` :143, `reduced_merge_order` :218,
    `cycle_report` :271) plus the ignore rungs (priority.rs, select_nodes.rs :78 `used_ig`,
    :413-:442). Break = relax runtime/runtime_post/satisfied edges only, never buildtime.
    Unbreakable => `AbortReason::UnserializableCycle` (portage-repo/src/resolve/result.rs:47-70),
    exposed as `aborted.reason = unserializable-cycle` (json.rs:327). The checker only
    tolerates cycles (`_cyclic_edges`, test :335); it never validates that a break was legal.
    portuale does not build a package twice to break a cycle: the plan has no split/
    two-pass entry (no field or code path found); a hard cycle aborts. So the
    "accept building twice" behaviour is a new feature, not just a check.
(b) Needs the dep graph with classes (not in JSON) and, for double builds, a way to tell
    the first from the second instance of the same cpv in `entries` (entries are
    unique per cpv today; `slots` Counter at :255 would flag a repeat).
(c) Gap: checker side medium (about 150 lines: SCC of the hard-edge subgraph must be
    empty; every relaxed edge must be soft; end state must pass P1). Resolver side
    (producing the double build) is a separate feature, large.

### P3 best visible version satisfying all constraints
(a) Not checked anywhere in the invariants. The resolver chooses in
    portage-repo/src/resolve/*; tests pin choices against fixtures and Portage, not a
    property. Backtracking records exist (`backtrack` field, :420).
(b) Needs the set of visible candidates per cp (keywords, package.mask, license,
    ACCEPT_KEYWORDS, profile masks, EAPI floor) and the union of atoms constraining the
    cp (all entries' deps plus the user's atoms plus installed packages' RDEPEND).
    JSON offers neither; a checker needs its own visibility reader for the whole config
    tree (make.conf, profiles, package.* dirs - note "package.* dir fragments not
    recursive" quirk, memory #89).
(c) Gap: largest. About 500+ lines for an independent visibility model, and it is exactly
    where an independent reimplementation can drift. Cheaper partial: use the JSON's
    `autounmask_*` / `provenance` fields to flag any entry that needed an unmask, then
    check only version ordering among candidates visible by keyword (PMS version compare
    is already independent in `versions-harness`).

### P4 after unmerge/depclean: no missing dependency, no missing linked library
(a) Dependencies: `P/crates/portage-repo/src/depclean.rs:20` `DepcleanResult` (cleanlist,
    kept_parents, unresolved hard runtime deps :32-:35), halt at
    P/crates/portuale/src/pretend/removal/depclean.rs:216 (`depclean_unresolved_halt`).
    Libraries: `lib_consumer_scan` (:38) with `needed_elf` (P/crates/portuale/src/needed_elf.rs)
    and `apply_depclean_lib_check` (:148, opt-in via --depclean-lib-check); preserved libs
    via ebuild_merge/{preserve_gate,plib_merge,plib_unmerge}.rs. All are pre-removal
    protections that mimic `_calc_depclean`; nothing verifies the finished system.
    No Portage dependence at run time; there is none in the checker either.
(b) A post-state scan needs the VDB (`vdb_cmd verify` P/crates/portuale/src/vdb_cmd.rs:325
    checks DB backends, not dependencies) with RDEPEND/PDEPEND/USE/SLOT per installed
    package, and NEEDED.ELF.2 per package for sonames. All on disk under var/db/pkg;
    no JSON involved.
(c) Gap: small to medium (about 200 lines): walk var/db/pkg, evaluate RDEPEND with the
    stored USE (shared evaluator with P1), match NEEDED.ELF.2 DT_NEEDED sonames to
    PROVIDES/other packages' sonames or system paths (reuse the algorithm from
    needed_elf.rs only as a spec; write it fresh).

### P5 error messages name cause and offer a fix
(a) No checker. Messages are text only: P/crates/portuale/src/pretend/problem_reports.rs
    (16 format!/eprintln sites, e.g. show_slot_conflicts :474, show_skipped_missing_deps
    :98, show_deferred_unsat :398, show_cycle_redisplay :1021) and depclean.rs:216-:262.
    Structured causes do exist but are narrow: `AbortReason` (4 variants, result.rs:47),
    `SlotConflict` (json.rs:247, includes atom and parents), `autounmask_*` changes
    (the actionable "fix" in Portage style), `MaskedDepReport`. Most messages are
    Portage's wording by design, rendered straight from those or from ad hoc strings;
    no common "Diagnostic{cause, subject, fixes[]}" type was found.
(b) A checker would grep stderr for category/package, atom, USE flag tokens and for a fix
    marker; with `aborted.reason` and `slot_conflicts` in JSON it can know the expected
    subject and verify the text names it. Fix presence is judgeable only for autounmask
    blocks or "emerge --..."/"USE=" lines.
(c) Gap: small for a heuristic text check (about 100 lines, per-error-class table), but
    any strict guarantee needs a structured diagnostic type in Rust first (medium-large:
    touch every message site).

## 3. Running on the bed, portuale only

- The L0 cells already write per-slug captures to the run dir and run
  `check-invariants.py <run>` afterwards; `real/` capture is optional (control only). Drop
  the Portage leg by selecting `PMTEST_PM=portuale` (the registry, run/lib.sh:23-34) and
  skip the diff step; check-invariants reads only `portuale/` and `portuale-modes/`.
- The container's vdb and repos are not visible from the host (check-invariants docstring
  :13-:16), which is why it needs `vdb-list.txt` and `dep-classes.tsv` snapshots (made by
  compare/snapshot.sh). A fuller checker should snapshot more: the full var/db/pkg tree
  (small) and the md5-cache of the repos (large, but readable in place via the repo mount),
  or run inside the container (podman exec) where everything is local.
- For P1/P2/P4 on finished merges: after an L1/L2/L3 cell, run the post-state checker on
  the container's `/var/db/pkg` (P4 needs the real `/usr/lib` ELF files and NEEDED.ELF.2).
  L3 (`run/l3-source-parity.sh`) is the real source-build case; L1
  (`run/l1-merge-from-binpkg.sh`) the binpkg merge. Both already keep a portuale-only
  control arm (`L3_CONTROL=1`, per project notes); the checker can run on that arm alone.
- Plan checks (P1-P3, P5) should run on the existing L0 `-pv --json` captures:
  roughly hundreds of atoms, no extra container time.

## 4. Suggested order and sizes

1. Evaluator + md5-cache/VDB reader (shared): medium, enables P1, P2, P2a, P4.
2. P1 + P2 + P2a checker over `--json` + md5-cache: medium; wire into check-invariants.py.
3. P4 post-state scanner for the bed's L1-L3: small-medium.
4. P5: heuristic text check first; structured-diagnostic refactor later.
5. P3 last (own visibility model), or a partial check via provenance fields.
Smallest JSON additions that would shortcut work: per-entry `deps` {class: [atoms]} and
effective `required_use`, a `kept` list (installed packages surviving the plan), and a
`visible_candidates` list per cp. All three are cheap to emit from data already in
`GraphEntry`/`DepEdge`.
