# Review: is `6c75e78` coherent with S4 of `docs/024-slot-operator-plan.md`?

## Context

You asked me to check the last commit on `backlog/024_slot-operator`
(`6c75e78`, "slot-op rebuild undo path (S4): _eliminate_rebuilds")
against the S4 slice as specified in the plan (§1.2 rules, §S4 slice
spec at plan lines 484-522, §8 checklist line 680).

**Verdict: coherent.** All six S4 sub-tasks are implemented, in the
right place, dual-language, with the skip conditions and the latch.
Four deviations exist; three are disclosed in the commit body, one is
not. Only one of them is a decision for you (the acceptance bar).

## What I verified (read-only)

| S4 spec item | Status | Evidence |
|---|---|---|
| 1. `:=` graph binder in `portage-repo` | ✅ | `rust/portage-repo/src/lib.rs:12022` `bind_slot_operator_deps`, entry-first-then-vdb; py mirror `python/emerge_pretend_reference.py:8825`; unit test `…:29824` |
| 2. Rules 1-8 in real's order | ✅ (one hoist, see D-2) | `lib.rs:12166-12285`; matches `depgraph.py:3859-3970` arm for arm; rule 3 left as an in-place comment for S5 |
| 3. Demote → drop + latch + `Config` feedback | ✅ | `lib.rs:18853-18872`; `slot_operator_undone` field `:15977`, in `params_equal` `:15442`, honoured by the scan `:11967` |
| 4. Skip: `ctx.empty`, live slot conflict | ✅ | `empty` checked inside (`:12180`), `pass.slot_conflicts.is_empty()` at the call site (`:18852`) |
| 5. `abi_rebuilds` drops demoted consumers | ✅ | next-pass scan runs with the shrunk set + latch; test asserts `abi_rebuilds == []` |
| 6. Rust unit tests | ⚠️ shape differs (D-3) | two tests, not one-per-rule; existing fixture pair, not a new one |
| Placement (`collect_feedback`, after S3 scan, before settle) | ✅ | `lib.rs:18832-18874`, gated on the scan not having grown the set — real's `_resolve` 5771-5779 |
| `slotundo-unnecessary` flips | ✅ | strict-xfail removed, replaced with positive assertions on the merge list, "causing rebuilds" absence and `--json` |
| Docs (§S4 oracle, what-this-proves, backlog) | ✅ | `docs/024-oracle.md:374-412`, `what-this-proves.md` appended, `backlog-tasks.md`/`scope-backlog.md` |
| Rule 5 (`!selective && top_level_cps`) safety | ✅ | `top_level_cps` comes from `req.atoms` only; the S3 auto-seed enters at `depth: 1` and never lands there (`lib.rs:16265-16280`) |
| Rust unit test passes | ✅ | ran `cargo test --release -p portage-repo eliminate_rebuilds` → 1 passed |
| Gate counts in the commit body | ✅ | `cargo test --workspace -- --list` → 997, matching the body |

## Deviations

**D-1 — the acceptance bar is not met (disclosed; your call).**
Plan §S4 acceptance and Gate G0.4 both name bug 614390
(`test_oracle_slotop_complete`) as the S4 bar. It stays strict-xfail.
The commit re-attributes it, with a traced reason, to a *selection*
gap (bare `dev-libs/socc` resolves `socc-2`, meta's later `=socc-1`
lands via the already-installed fast path, which never consults
`resolved_slots`; real's `_add_pkg` 2160-2185 makes that a slot
conflict) and argues the undo rules are correct on that shape
(rule 5 keeps `socc-1`'s AtomArg rebuild, rule 8 keeps the graph-bound
`socfoo:=`). This is a surfaced judgment call, per AGENTS.md step 3,
not a silent default — and plan §5's go/no-go rule lists
`slotundo-unnecessary` as an S4-owned case in its own right. Still,
G0.4 as written is unsatisfied.

**D-2 — rule 7 is hoisted, not in place (disclosed, unproven).**
Real checks `pkg.built` → `provides`/`requires` between rules 6 and 8.
Portuale pre-filters `entry.source != CandidateSource::Ebuild`
*before* rule 1 (`lib.rs:12206`), i.e. a binary entry keeps its rebuild
unconditionally. That can only ever keep a rebuild, never over-demote,
so it is safe — but §8's checklist says any reordering needs "a
counter-example fixture or a proof of none", and the doc comment gives
the v2-deferral rationale without stating that proof.

**D-3 — unit-test shape (disclosed).** Plan asked for "one per rule,
on a fixture pair `dev-libs/slotundo{target,consumer}`". Delivered: one
combined test (`slot_operator_eliminate_rebuilds_applies_the_eight_rules_in_order`,
covering rules 0,1,2,4,5,6,7,8, the non-slot-op-entry guard and the
latch) plus the binder test, on the existing `souprov`/`sounneed`
fixtures. Functionally equivalent coverage; no new fixture pair.

**D-4 — stale gate numbers in `what-this-proves.md` (not disclosed).**
The §S4 paragraph ends "`cargo test --release` 995 passed / 0 failed,
`python3 -m pytest tests -q` 1553 passed". The commit body says 997 and
1554. I confirmed the workspace has **997** tests, so the doc paragraph
is the stale one (written before the two new unit tests landed). It is
wrong in an append-only document.

**Non-issue, noted for the record:** rule 4 uses `pass.slot_want` +
`reverse_pins` rather than the plan's wording ("`required_by` atoms of
the entry"). `slot_want` is "every atom text that targeted this cp this
pass", which is *closer* to real's `_parent_atoms[pkg]` than
`required_by` is. Better than spec, not worse.

## Proposed follow-ups (all small; nothing is pushed, so an amend is open)

1. **D-4, do this one regardless:** correct the two counts in the
   `what-this-proves.md` S4 paragraph to 997 / 1554. The append-only
   rule protects narrative, not a wrong number; amend `6c75e78` rather
   than appending a correction paragraph.
2. **D-2:** add one sentence to the rule-7 doc comment on both sides —
   "hoisted above rule 1: the pre-filter can only *keep* a rebuild, and
   rules 1-6/8 are all keep-on-mismatch, so no ordering counter-example
   exists" — which is what §8 asks for. Pure comment change.
3. **D-1:** decide between
   (a) accept S4 as shipped, move G0.4's S4 bar to
   `slotundo-unnecessary` in the plan/oracle, and file the `socc`
   selection gap under backlog #36; or
   (b) hold S4 open until the `_add_pkg` slot-parent check
   (`depgraph.py:2160-2185`) lands so 614390 can MATCH — that is a
   selection-path change, arguably #36's slice, not #24's.
   My recommendation is (a): the undo itself is demonstrably correct on
   that shape, and (b) puts a resolver-selection fix inside a
   slot-operator slice.
4. **D-3:** optional. Either leave as-is and record the accepted
   deviation in `docs/024-oracle.md` §S4, or add the
   `dev-libs/slotundo{target,consumer}` pair when S5 touches these
   fixtures anyway.

## Verification

- `cd rust && cargo test --release -p portage-repo eliminate_rebuilds` — already run, passes.
- After any amend: `cargo fmt --check`, `cargo clippy --release --all-targets`, `cargo test --release` at the **workspace** root (per the `verify-whole-workspace-build` note).
- Contract subset: `python3 -m pytest tests/test_emerge_pretend_contract.py -q -k "slotop or slot_operator"` — note the known fixture-pollution bug: `git add` any new fixture before a `git clean -fdq fixtures/`, and compare failing test *names* against a clean baseline, not counts.
- L0 is S6's per the plan; nothing here needs it.
