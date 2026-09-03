# Plan: autounmask in-loop (convert the post-resolution autounmask pass to real backtracking's config-change feedback)

*Working plan — 2026-09-03. Move to `docs/history/` once complete.*

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
3. **`_autounmask_breakage`.** An autounmask change that breaks a
   *previously satisfied* dep is reverted and the next level tried.
4. **Parent-flip re-resolve of the whole graph.** The `'parent_flip`
   block re-resolves only the freed dep (documented cut); real re-drives
   the whole pass via `needed_use_config_changes`.
5. **`get_best_run`.** After backtracking, real returns the run with the
   most config changes and *no* masks.

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
   Rust==Python byte-identical. *(Not yet: a true per-level version scan
   — portuale still takes the best version at whichever single level
   first matches, which coincides with real for the cross-version
   cases the fallbacks distinguish, but not for a "level-1 unmasks v1
   AND v2, level-2 would unmask v3" chain where real re-scans. No
   fixture exercises that.)*
3. **`_autounmask_breakage`.** Detect an autounmask change that makes a
   previously-satisfied dep unsatisfiable; revert it, escalate. Fixture:
   flipping `x` on `A` to satisfy `Z` also kills `x? ( needed-by-Y )`.
4. **Parent-flip whole-graph re-resolve (#4).** `suggested_parent_use_
   candidate` folds the parent flip into `autounmask_use_config` and
   re-resolves the whole pass instead of just the freed dep. Removes the
   documented `'parent_flip` cut. Fixtures largely exist
   (`test_autounmask_use_parent_flip_*`).
5. **`get_best_run` (maskless preference)** + wire `--autounmask-backtrack
   [=y|n]` (currently inert under `--pretend`) to actually gate the
   re-resolve.
6. **`--autounmask-keep-keywords` / keyword+mask accumulators in the loop**
   — the keyword / unmask analogues of slice 1, with level ordering from
   slice 2.

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
