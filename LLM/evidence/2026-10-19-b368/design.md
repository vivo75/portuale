# #368 design: `--solver=pubgrub` residue (binaries, notices, `--deep=N`, greedy lookahead)

Source: backlog #368 (`LLM/backlog-tasks-2026-10.md:255`); cuts listed in
`solver_bridge.rs:14-27`; S-9 validator in `solver_bridge.rs:3455,3885,4093`.
Portage oracle below is 3.0.82.2 at `/usr/lib/python3.14/site-packages/_emerge/depgraph.py`.
No host `emerge` was run for this design (fixture cells below say what must be captured).

## (a) Binaries under --usepkg/--usepkgonly/--getbinpkg
1. Today: `LazyRepo::load_versions` (`solver_bridge.rs:241-295`) offers md5-cache
   ebuilds only; `BridgeRepo::versions_for` has no binary arm; every entry maps
   with `CandidateSource::Ebuild`, `built=false` (`:956-986`). Any binary flag is
   silently ignored (plan = ebuild plan).
2. Portage: binary candidates join the pool from the bintree; presence of the
   `--usepkg` key (even `=n`) turns `bdeps=auto` off
   (`create_depgraph_params.py:97-103`); a built package's DEPEND/BDEPEND are
   dropped (`depgraph.py:4194-4216`); binary wins the same-version tie
   (`matched_packages[-1]`); `--usepkgonly` drops the ebuild db (`depgraph.py:809`).
3. Oracle cell (existing fixtures, `fixtures/binhost/Packages` has 14 entries):
   `emerge -pv --getbinpkg=y --usepkg=y dev-libs/binebuildtie`
   (ebuild + USE-matching binary at 1.0 → Portage prints `[binary N]` with the
   `g` column); control `dev-libs/binbdepconsumer` (recorded BDEPEND
   `dev-libs/binbdeponly` must not expand); remote-only `dev-libs/remotebinpkg`.
   Expected class: action-letter flip (`N` ebuild → `N` binary) + `g` column.
   PKGDIR helper precedent: synthetic `Packages` text in
   `test_emerge_pretend_contract.py:6513-6545`; hermetic binhost via
   `fixtures/etc/portage/binrepos.conf` (`file://$CONFIGROOT/binhost`).
4. Gate: new bed mode — `L0_PORTUALE_OPTS='--solver=pubgrub'` with
   `L0_EMERGE_OPTS='-pv --usepkg'` (today G-PB runs `--usepkg=n --getbinpkg=n`,
   which pins binaries off; `in-container.sh:29-30` already forwards both knobs).
5. Size: 3–4 slices (binary version rows + built-USE vs recomputed USE (#420
   warns) → tie-break + BDEPEND drop → `--usepkgonly` ebuild-db drop).

## (b) Slot-conflict notice + remaining autounmask notices
1. Today: `slot_conflicts: Vec::new()` (`:635`), `autounmask_*` empty
   (`:647-650`), masked/use_unsat/plain_miss empty (`:661-665`). S-9 fills abort
   notices only (masked / REQUIRED_USE / autounmask-USE, `:3455,3885`); a
   solvable-but-conflicted plan prints no notice and a flip-needing candidate
   fails instead of suggesting (module doc `:15-21`).
2. Portage: `_show_slot_collision_notice` (`:1712-1730`, informational, plan
   unchanged) via `slot_conflict_handler`; `_process_slot_conflicts`
   (`:2108-2134`); autounmask levels retried per level (`:2459-2462`);
   final text in `display_problems` (`:11104`).
3. Oracle cells: slot notice — `dev-libs/subslotparenta dev-libs/subslotparentb`
   (two built slot-op parents that must NOT conflict) as the no-notice control,
   plus a `dualslotpkg`/`avoidslotpkg` pair that does collide → class:
   notice-only (same plan + stderr block). Autounmask remainder — keyword/
   license/mask flips, e.g. existing `~arch`/license fixtures → class: exit/text
   (`autounmask` suggestion block vs today's engine `Error::Detail` line).
4. Gate: existing G-PB modes (`l0-pubgrub` allowlist layer,
   `compare/known-divergences.yaml:198-230` already holds the G4a/b/c exit rows).
5. Size: 2–3 slices (slot-collision notice 1; flip suggestions 1–2 — needs the
   visibility filter opened per level, the riskiest half).

## (c) `--deep=N`
1. Today: every site uses `!matches!(req.deep, Deep::NotRequested)` (`:168`,
   `:1483`, `:1757`, `:3291`) — `Bounded(n)` counts as unlimited.
2. Portage: `_too_deep` (`:7359-7375`) returns `depth > deep` for int deep;
   installed-node deps at depth d expand only if `d+1 <= n`.
3. Oracle cell: existing `dev-libs/deeppkg` (+ `resolve/queue.rs:8-13`,
   `tests/queue.rs:177` `Deep::Bounded(1)`): `emerge -pv --solver=pubgrub
   --deep=1 dev-libs/deeppkg` vs Portage same argv → class: set difference
   (shallower plan). Must capture: Portage text for depths 0/1/2 on that fixture.
4. Gate: G-PB modes (no new mode; `--deep` is orthogonal to `--usepkg`).
5. Size: 1 slice (thread depth through `LazyRepo::build`/`vdb_expand` and the
   kept-leaf rule; default solver's `Deep::recurses_at` is the pattern).

## (d) `_greedy_slots` blocker lookahead
1. Today: `target_slots` (`:1154-1195`) ports the slot filter via shared
   `greedy_slots_below_best` but never the lookahead (code-note line 153).
2. Portage: `_greedy_slots(..., blocker_lookahead)` (`:5896-5930` + `:5385-5400`
   two-pass re-select): greedy older-slot atoms that would blocker-conflict are
   discarded. Default solver ports both passes (`resolve/greedy_slots.rs`, #359 S3).
3. Oracle cell: existing `pg34-gs` (slot 1 installed/upgradable, slot 2 new) +
   a blocker between the greedy slot-1 pick and slot 2 (`pg359-gsblk` pattern):
   `emerge -pvu dev-libs/pg359-gsblk` → class: one `U` row present/absent.
   Must capture: Portage `-pvu` on that fixture pair (lookahead on vs off).
4. Gate: G-PB modes.
5. Size: 1 slice (re-run target selection per backtrack attempt — the bridge has
   no attempt loop, so scope to a pre-solve filter like #359 S3).

## Order, sizing, and what is NOT worth doing
- Order: (b-slot-notice) → (c) → (a) → (d). Total 7–9 slices.
- NOT worth doing now: **(d) blocker lookahead**. Reason: it fires only when a
  greedy older-slot pick is blocked by the newer slot's blocker — #359 S3's
  default-solver port showed near-zero corpus signal, and the bridge would need
  a per-attempt re-select loop it does not have. File and defer.
- Implement first in this batch: **(b) slot-conflict notice** (1 slice, read-only
  display over the solved plan reusing the S-9 validator shape; no pool change).
- New numbers (#430+ free; max cited is #429): **#430** (a) bridge binary pool,
  **#431** (b) remaining pubgrub notices, **#432** (c) `--deep=N` in the bridge,
  **#433** (d) greedy blocker lookahead — file #433 as deferred with this design
  attached.
