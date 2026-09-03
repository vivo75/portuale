# Plan: autounmask in-loop (convert the post-resolution autounmask pass to real backtracking's config-change feedback)

> **COMPLETE — 2026-09-03.** All six slices plus the per-level version
> re-scan follow-up shipped (commits `1d6b6db`..`bc58d41` on `main`).
> Kept here as the record of what each slice did and why; the shipped-
> behaviour narrative lives in [`../what-this-proves.md`](../what-this-proves.md).

## The gap

portuale's autounmask already does a lot in a **single BFS pass**: a
candidate masked by `KEYWORDS`/`LICENSE`/`package.mask` alone, or one
whose atom use-dep only mismatches by a settable flag, is accepted, the
flip/unmask is *applied* to that package's own USE, its (post-flip) deps
are walked, and the change is recorded for the `The following … changes
are necessary to proceed:` block. Forward cascades already work
(`test_autounmask_use_resolves_a_dependency_use_dep_mismatch` drops a
`foo?` dep when autounmask flips `foo` off).

What a single pass **cannot** do, and real `_backtrack_depgraph` +
`_feedback_config` + `_autounmask_levels` can:

1. **Backward cascade.** An atom `X[flag]` from a package processed *late*
   needs `flag` on package `A` processed *early*. `A` was already walked
   with its original USE, so `flag?`-gated deps of `A` never appear.
   *(Today portuale silently ignores the mismatch entirely — the
   already-resolved-slot path does a plain `match_from_list` that can't
   see use-deps. Fixture: `dev-libs/aucasctop`, added, currently drops
   `aucascleaf`.)*
2. **`_autounmask_levels` ordering.** Real tries, per candidate in version
   order: USE → +license → +~arch → +missing-kw → +masks, stopping at the
   first level that yields a package — so a lower version fixable by USE
   alone beats a higher version needing `~arch`. portuale's masked-only
   fallbacks fire keyword→license→mask and don't escalate per version.
3. **`_autounmask_breakage`.** An autounmask change that leaves another
   use-dep unsatisfiable and can't be reconciled makes portage abandon
   autounmask *wholesale* (`myparams["autounmask"] = False`,
   depgraph.py:12262) and re-resolve one clean pass. There is no
   per-level "revert this one change and escalate" mechanism — that was a
   misreading in an earlier draft of this plan.
4. **Parent-flip re-resolve of the whole graph.** ✅ **Shipped
   2026-09-03.** The `'parent_flip` block used to re-resolve only the
   freed dep (documented cut); it now folds the parent-USE flip into
   `autounmask_use_config` (keyed `(parent_cat, parent_pkg)` — a
   `package.use` entry) and `continue 'queue`s, so the driver's
   `autounmask_grew` restart re-walks the whole graph and the parent's
   other `flag?`-gated deps re-evaluate. A probe re-resolution of just
   the freed atom still gates it (only a flip that actually helps is
   folded); a parent flip contradicting an accumulated change routes into
   Slice 3's `_autounmask_breakage`. Fixture `dev-libs/pfgraphparent`:
   `pf? ( pfgraphextra )` correctly drops once `pf` is flipped off — the
   old single-dep re-resolve left `pfgraphextra` in the list. Rust==Python
   byte-identical.
5. **`--autounmask-backtrack` gate.** ✅ **Shipped 2026-09-03.** Turned out
   to be a *correction*: real `--autounmask-backtrack` is **off by
   default** (`man emerge`, `depgraph.py:11736` — `need_config_change` ->
   break in `_backtrack_depgraph` once autounmask changes exist), so
   Slices 1 & 4 were shipping the `=y` behaviour unconditionally. Now
   `Config::autounmask_backtrack` (set from `--autounmask-backtrack` /
   implied by `--autounmask-continue`) gates the `autounmask_grew`
   re-drive. **Off (default):** the change is collected + shown; the
   flipped package's own USE line is re-rendered (`refresh_entry_use_
   display`, mirroring real `_pkg_use_enabled` consulting
   `_needed_use_config_changes`) but its `flag?`-gated deps do NOT appear,
   and the parent-flip path falls back to the single-dep local
   re-resolve. **On:** Slices 1/4's full cascade. `get_best_run`'s
   "maskless run" preference is moot for portuale's single-accumulator +
   `MaskPhase` revert model (it already prefers the maskless settled
   state) — not separately implemented.
6. **Keyword / mask backward cascade.** ✅ **Shipped 2026-09-03.** Turned
   out *not* to need a new accumulator: the existing `slot_constraints`
   re-drive already folds every atom that pulled a slot, but
   `resolve_pretend`'s `*_masked_only` fallback was gated on
   `visible.is_empty()` — so `dev-libs/foo` + `>=dev-libs/foo-2` where
   `foo-1` is stable and only `foo-2` is `~arch` left `visible = [foo-1]`
   non-empty and never tried the keyword level. The gate is now "no
   *is_visible* candidate satisfies `atom_str` **and** every
   `extra_constraints` entry" (`need_fallback` / `usable`), so the
   slot-conflict retry re-resolves the slot to the keyword/license/mask-
   masked version and records the change. Same code path serves all three
   levels (Slice 2's ordering). Fixture `dev-libs/kwbacktop`. Not gated by
   `--autounmask-backtrack` (slot-conflict reconciliation never was); the
   real control is `--autounmask` (keyword suggestions off by default).

## Approach

Reuse the shipped `'backtrack: loop` (already carries `slot_constraints`
/ `extra_constraints` / `MaskPhase` across iterations, bounded by
`backtrack_max`). Add a second cross-iteration accumulator:

```rust
// (cat, pkg) -> { flag -> desired state }   (real needed_use_config_changes)
autounmask_use_config: HashMap<(String, String), HashMap<String, bool>>
```

- **Apply**: a 3-line helper called right after each of the 6
  non-test `effective_use_flags` sites in the walk —
  `apply_autounmask_use(&mut use_flags, (cat,pkg), &autounmask_use_config)`
  — exactly mirroring the existing post-`effective_use_flags`
  `use_flags.insert/remove` at lib.rs:10680.
- **Feed back**: when a pass records an autounmask USE change whose
  `(cp, flag, state)` is *not already* in the accumulator, fold it in and
  `continue 'backtrack` (bounded by `backtrack_max`). Converge when a full
  pass adds nothing new.
- Keyword / license / mask analogues get their own accumulators the same
  way (a `HashMap<(cat,pkg), Vec<String>>` of forced `~arch` / unmask
  entries), added in the level-ordering slice.

## Slices (each: both sides + fixture + contract test + Rust unit test, verified byte-identical, committed on request)

1. **Backward-cascade re-resolve (USE).** ✅ **Shipped 2026-09-03.**
   `Config::autounmask_use` applied by `effective_use_flags` (Slice 0) +
   `autounmask_use_config` accumulator + `autounmask_use_change_records`
   (survive across passes like `slot_constraints`) + the already-resolved-
   slot use-dep re-check in the queue walk + a driver-level restart
   (`autounmask_grew` / accumulator-size grew → rebuild `backtrack_config`,
   `continue 'backtrack`). Fixture `dev-libs/aucasctop`: `aucascleaf` now
   appears, `aucascmid` shows `USE="cascade"`, the block prints
   `>=dev-libs/aucascmid-1.0 cascade`. Rust==Python byte-identical; full
   suite 1178 passed. **Slice-1 simplifications** (later slices):
   `autounmask_use` atom is `cat/pkg` not `=cat/pkg-ver`; the re-check
   only fires when the atom's independently-resolved version equals the
   already-resolved one (a `>=`/`<`-bounded atom pulling a *different*
   version of the same slot is not re-checked); no `*` autounmask marker
   on the `-pv` `USE=` line (a pre-existing fresh-path gap, unchanged).
2. **`_autounmask_levels` ordering.** ✅ **Shipped 2026-09-03.** The
   `*_masked_only` visibility fallbacks in `resolve_pretend` now run in
   real's least-to-most-invasive order (`+license` → `+~arch` → `+masks`,
   was `~arch` → license → masks), stopping at the first level that
   yields a candidate — each still picking the highest version it can
   unmask. So a lower license-masked version beats a higher
   keyword-masked one. Fixture: `dev-libs/levelconsumer` →
   `levelpkg-1.0` (@EULA license) chosen over `levelpkg-2.0` (~amd64).
   Rust==Python byte-identical. **Per-level version re-scan follow-up
   (2026-09-03):** the three flat `*_masked_only` fallbacks are now a
   `_autounmask_levels` loop over `visible_with_relax(license, keywords,
   masks)` predicates — cumulative (`allow_license_changes` sticks once
   set), re-scanning every version at each level (real
   `_select_pkg_highest_available_imp`). A candidate blocked by `~arch`
   **and** `LICENSE` resolves once level 2 (`+~arch +license`) is
   reached, and the recording side (now per-category, not
   `*_masked_only`) records *both* changes. Fixture
   `dev-libs/multimaskconsumer` → `multimaskdep-2.0`.
3. **`_autounmask_breakage`.** ✅ **Shipped 2026-09-03.** Faithful to real
   (depgraph.py:12262): when the accumulator ends up wanting the same
   `(cp, flag)` both on *and* off — a contradiction no re-resolve can
   settle — every autounmask change is dropped, all four
   `autounmask_suggest_*` are switched off, and the loop runs one final
   clean pass (`autounmask_disabled` latch keeps it to one). Fixture
   `dev-libs/aubreaktop` → `aubreakwant` needs `aubreaksub[brk]`,
   `aubreakunwant` needs `aubreaksub[-brk]`: before, the two pushed a
   contradictory `>=aubreaksub-1.0 brk` + `>=aubreaksub-1.0 -brk` block
   (and Rust/Python even disagreed on the final `USE=` parity); after,
   both abandon autounmask and print the same clean list with
   `aubreakwant`'s unsatisfiable `[brk]` as the ordinary non-fatal
   dependency `!!! no visible ebuild` note — identical to what
   `--autounmask-use=n` produces. Rust==Python byte-identical.
   **Simplification**: only the *contradiction* signal triggers the
   abandon; real also abandons when an autounmask flip makes a plain
   (non-use) dep vanish that a package matches, but portuale's re-check
   only ever compares use-deps. Also, a fresh-path flip that a later atom
   contradicts on an *already-resolved* slot still slips through
   silently (the re-check recomputes effective USE from `config`, which
   doesn't carry the pass-local fresh flip) — that's the pre-existing
   already-resolved-slot `match_from_list` use-dep blindness, tracked
   separately, untouched here.
4. **Parent-flip whole-graph re-resolve (#4).** ✅ **Shipped 2026-09-03.**
   The `'parent_flip` block folds the parent flip into
   `autounmask_use_config` (keyed `(parent_cat, parent_pkg)`) and
   `continue 'queue`s; the driver's `autounmask_grew` restart re-walks
   everything. The single-dep `resolve_pretend` probe survives only as a
   gate ("does this flip actually help"), and a contradicting flip is
   handed to Slice 3. Removes the documented `'parent_flip` cut. New
   fixture `dev-libs/pfgraphparent` (`pf? ( pfgraphextra )` must drop when
   `pf` flips off) + the existing `parentflipeqpkg` cases, all
   Rust==Python byte-identical.
5. **`--autounmask-backtrack` gate.** ✅ **Shipped 2026-09-03.**
   `Config::autounmask_backtrack` (from `--autounmask-backtrack`, implied
   by `--autounmask-continue`) gates the `autounmask_grew` re-drive --
   **off by default** to match real (`man emerge` /
   `depgraph.py:11736`). Off: collect + display the change, re-render just
   the flipped package's USE line (`refresh_entry_use_display`), no
   graph re-walk; parent-flip falls back to the single-dep local
   re-resolve. On: Slices 1/4's full cascade. The Slice 1/4/3 contract
   fixtures grew a `--autounmask-backtrack=y` variant each.
   `get_best_run`'s maskless-run preference is subsumed by portuale's
   single-accumulator + `MaskPhase` revert model.
6. **Keyword / mask backward cascade.** ✅ **Shipped 2026-09-03.** No new
   accumulator: the `slot_constraints` re-drive already folds every atom
   pulling a slot; `resolve_pretend`'s `*_masked_only` fallback gate is
   now "no *is_visible* candidate satisfies `atom_str` + `extra_
   constraints`" instead of `visible.is_empty()`, so the retry
   re-resolves the slot to the keyword/license/mask-masked version and
   records the change (Slice 2's level ordering, all three at once).
   Fixture `dev-libs/kwbacktop`. Gated by `--autounmask` (keyword
   suggestions off by default), not `--autounmask-backtrack`.

**All six slices shipped.** Move this file to `docs/history/`.

Slices 1 + 4 deliver the two items named in `scope-backlog.md` Part 2.A
("USE/keyword levels tried in sequence inside the loop", "autounmask
parent-flip re-resolve feeding `extra_constraints`"). 2, 3, 5, 6 are
fidelity refinements.

## Risk / guardrails

- The `'backtrack` loop is the most heavily-tested code in the project
  (~1170 contract cases, byte-exact Rust==Python). Every slice runs the
  full suite and must stay byte-identical except for the new fixture.
- Convergence: every feedback path increments `backtrack_iteration` and
  is guarded by `< backtrack_max`; a pass that adds no new accumulator
  entry does **not** loop.
- Determinism: the accumulator is a `HashMap` but is only ever *read* by
  `(cp)` lookup and folded in a fixed (atom-encounter) order, same as
  `slot_constraints`.
