# n230 report — Track N17, backlog #230: skipped-update block rendered like real

**Status: DONE_WITH_CONCERNS** (all directives implemented; concern: the
fixture-oracle blk0 cells are predicted to still flag explanation findings
after the allowlist removal — see BED-PENDING. No judgment calls taken;
every directive below follows the brief verbatim).

## What changed

Portuale commit `61ffbc88` (pmtest counterpart `9648488`, committed first).

**Real-source grounding** (`3rdparty/portage`, 3.0.82.2, plus a live
3.0.82.2 container probe — see Probes):
- Block shape: `_show_missed_update_slot_conflicts`
  (`lib/_emerge/depgraph.py:1652`) — one `<slot_atom>` header per missed
  upgrade, `(cpv, state) <USE displays> conflicts with` plus one
  `<atom> required by <parent> <USE displays>` + `^`-marker pair per
  parent.
- Highest-missed-per-slot: `_get_missed_updates` (`:1529`, keep-max per
  `(root, slot_atom)`, `:1553-1562`) — already shipped (#129 review);
  verified still holding (single `-3` block for `b c a`).
- USE content: `pkg_use_display` (`lib/_emerge/UseFlagDisplay.py:55`) —
  whole effective USE (`PORTAGE_USE`) masked to the valid-IUSE domain
  (`config.py` setcpv `:2166-2220`: EAPI 5+ `IUSE_EFFECTIVE`
  (`_calc_iuse_effective`), pre-EAPI-5 `_get_implicit_iuse` — ARCH +
  arch.list + `USE_EXPAND_HIDDEN`-derived `xxx_.*` + per-package
  use.mask/use.force + `build`/`bootstrap`), grouped per non-hidden
  `USE_EXPAND` var, `( )` force/mask wraps, `ARCH` discarded.
- Markers: `format_unmatched_atom`
  (`lib/_emerge/resolver/output.py:892`, slot span `:967-975`).
- Arg-parent arm (`depgraph.py:1681-1686`): bare indented `str(parent)`
  for `PackageArg`/`AtomArg`, no atom/marker.
- Root suffixes: `Package.__str__` (`lib/_emerge/Package.py:568-608`,
  `to '<ROOT>'` / `in '<ROOT>'` iff `ROOT != "/"`) and the group-header
  `for <root>` gate (`depgraph.py:1662-1664`).

**Portuale** (`rust/portage-repo/src/lib.rs`, `md5_dict.rs`,
`rust/portuale/src/pretend.rs`):
- New `skipped_update_use_display_for` (tree candidates) and
  `skipped_update_installed_use_display_for` (vdb consumers): faithful
  `pkg_use_display` — `mask_use_to_valid_domain` +
  `assemble_pkg_use_display` + `render_use_group` (enabled-first,
  byte-sorted, `USE`-first groups). New `md5_dict::eapi_has_iuse_effective`
  (`Eapi >= 5`, same gate shape as `eapi_has_slot_operator`).
- Wired into all three `SkippedUpdate` producers (direct-solve removals,
  reverse-pin withholds, backtrack-mask rows). The slot-collision notice
  keeps its IUSE-only `pkg_use_display_for` untouched (zero re-pins
  there, verified by the green suite).
- Renderer (`pretend.rs`): reuses `render_pkg_use_display`; new arg-parent
  arm (empty `consumer_cpv` → bare `    <atom>`, no marker);
  installed consumers now render `(cpv, installed in '<root>')` with the
  real path; `skip_conflict_caret_line` gains mismatched
  `:slot[/sub-slot][op]` spans (span-collecting rewrite; op+version
  behavior byte-identical on all grounded shapes).
- Root-suffix directive followed verbatim: merge-scheduled nodes and
  headers stay bare (slot-collision-notice convention; the #206
  rationale — tmp `ROOT`s must not leak into output), installed nodes
  carry the real path. Stated in the render-site doc comment.

**Pmtest** (`9648488`): re-pinned all 5 WARNING sites —
`USE="(globalforceflag) (stableforceflag)"` for flagless pre-EAPI-5
packages (blk0), `USE="" ELIBC="glibc"` for flagless EAPI-8 packages
(orbtblocked, btparent, mg2top, slotconflictoldconsumer) — plus per-site
docstring grounding notes. Removed the `skipped-updates-use-display`
allowlist entry, as directed. No pins touched for the slot-collision
notice, the `!!!` tails, or the negative (`have been skipped` absent)
tests.

## Key finding (bed-grounded, debug-verified)

Real's bed blk0 rendering mixes bare missed lines with `to`-suffixed
parents because **pre-EAPI-7 `DEPEND` resolves against the host-config
running-root `/` tree** (`depgraph.py:4219-4226`: `depend_root =
_running_root.root` when the ebuild has no BDEPEND support; `create_trees`
always builds that second tree from the host config when `ROOT != "/"`,
`portage/__init__.py:497-529`). Debug proof: `Conflict: (/, dev-libs/blk0x:0)`.
Consequences: the missed line's `USE="(test-rust)" ABI_X86="(64)"
LLVM_TARGETS="(X86)"` is container-profile content no fixture-tree
rendering can reproduce, and the `to` suffixes mark target-rooted
parents. Portuale deliberately stays single-rooted (suite determinism),
so these two deltas are architectural, not rendering bugs. A third delta
is owned by portuale's documented "no EAPI parametrization" precedent:
portuale reads `use.stable.force` unconditionally while real ignores it
at profile-EAPI 0 (bed logs: `--- EAPI '0' does not support
'use.stable.force'`), hence the extra `(stableforceflag)` — consistent
with portuale's own `-pv` pins (`test_use_stable_force_...`, which show
it too) and with its resolution (the flag is genuinely enabled for
`stableforceflag?` deps).

## Gates (all in the worktree pair)

- `cargo fmt --check`: clean.
- `cargo clippy --release --all-targets`: 0 warnings (one collapsed-`if`
  fixed during the run).
- `cargo test --release` (whole workspace): all green — portage-repo 855
  (incl. 3 new #230 display tests), portuale bin 671, all other crates
  green, 0 failed.
- Full pmtest suite (`--basetemp=/var/tmp/pmtest-n230 -p no:cacheprovider`,
  502 s): **2072 passed, 37 skipped, 6 xfailed, 1 failed** — the single
  failure is `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`
  on its `_assert_harvested` corpus-agreement line only (all explicit
  re-pins pass). The xfail delta vs n227's baseline (7→6) comes from the
  merged #199/#217 test edits on this branch, not this change (all
  sampled xfails are strict + unrelated: au-cascade, slotop probes,
  cyc0, soc; a strict XPASS would fail loudly).
- `test-skipped-updates.py` (comparator, untouched): ALL OK.

## Corpus drift (reported, NOT blessed)

Drifted calls are exactly the skipped-block shapes, nothing else:
named `#0` calls of `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`,
`test_oracle_slot_conflict_masks_highest_version_first`,
`test_oracle_selective_update_is_a_noop`,
`test_oracle_no_aggressive_downgrade`,
`test_oracle_two_simultaneous_conflicts_defer_second_to_later_pass`,
`test_oracle_missed_update_siblings_masked_together`,
`test_oracle_slotop_revdeps_libgit2_stays_empty`,
`test_oracle_slotop_conflict_rebuild`, plus the metamorphic `expanded:`
matrix for `btgp, btparent, libgit2-glib, mg2top, mgfa, mgxa,
orbtblocked, gitg` (132 drift lines, all `stdout changed` on these
shapes). Coordinator blesses.

## BED-PENDING (coordinator)

- `differential-test-bed/run/l0-fixture-oracle-all.sh` (fixture oracle).
  Prediction from an offline simulation (bed-real captures from
  `l0-fx-20260924T211909Z` vs this binary on a fixture copy, compared
  with the new `resolve-compare.py`; staged-vs-committed tree deltas do
  not touch blk0 USE): package lists now MATCH on all four blk0 cells
  (no `package list differs` anywhere — the #129 collapse holds), but
  each cell still reports one `skipped-update explanation differs`
  finding with three residual causes: (1) missed-line USE
  (container `(test-rust)`+expands vs fixture display — unmodelled `/`
  tree, see Key finding); (2) parent `to '<root>'` (cut per directive);
  (3) parent `(stableforceflag)` excess (stable-force precedent, see Key
  finding). **The brief's "must be clean" expectation is therefore NOT
  met** — after the bed run, either re-allowlist the residual narrowly
  or file follow-up items; I did not flip entry #230.
- `differential-test-bed/run/l0-resolver.sh` (L0): expect green — no L0
  probe prints this block on either side (n227's offline re-run showed 0
  block occurrences in 120 probes), and this change only touches the
  block's USE/marker/parent rendering.

## Probes (all read-only; no bed executed, no host state changed)

Several `podman run --rm localhost/test-portuale:latest` replications of
the staged blk0 cell (`--backtrack=0 dev-libs/blk0b dev-libs/blk0c
dev-libs/blk0a`, image portage 3.0.81.3 — the relevant functions are
identical in 3.0.82.2, verified by diff of `Package.__str__`,
`_show_missed_update_slot_conflicts`, `pkg_use_display`): reproduced the
bed output byte-for-byte, then dumped both trees' `PORTAGE_USE`
(target: `[amd64, cpu_flags_x86_sse2, globalforceflag]`; `/`:
`[abi_x86_64, amd64, elibc_glibc, kernel_linux, llvm_targets_X86,
test-rust]`), the `--debug` conflict roots (`Conflict: (/, ...)`), and
`emerge --info dev-libs/pkginfopkg` (`USE="alpha -beta" ELIBC="glibc"` —
the masking proof). Staging replicated `stage.sh` steps 1/2/5/6/CATEGORIES
only, in throwaway containers.

## Commit

- pmtest `9648488` (allowlist removal + 5 re-pins) on
  `backlog/230-skipped-block-display`.
- portuale `61ffbc88` (display + renderer + 3 unit tests) on
  `backlog/230-skipped-block-display`, quotes the pmtest sha.
- Entry #230 NOT flipped (bed confirmation pending). No other docs
touched (a `what-this-proves.md` paragraph needs a live-verified bed
claim this slice cannot yet make).

---

# Fix round 1 (2026-09-28) — Status: NEEDS_CONTEXT

**Interruption note.** The previous run of the fix brief merged `main`
into both worktrees (portuale `051ada69`, pmtest `5e48d74`) and was cut
off by a provider quota outage during probe analysis, with no progress
file written and no fix commits made. This run resumed from that state:
both merges kept as-is (verified current — portuale `main` is
`7f1fc9f2`, pmtest `main` is `2c9730c`, both merges are their direct
children), both trees clean, then executed brief steps 2–5 plus the
gates. No committed work was redone; nothing uncommitted was discarded
(trees were clean).

## Step 2 — re-derivation against the post-#220 tree

One rule-13 real-Portage probe (verbatim output at the end of this
section; image portage 3.0.81.3 — the relevant functions are identical
in 3.0.82.2 per the round-0 diff check; fixture tree post-#220:
`profiles/*/eapi` = 5, all blk0 ebuilds EAPI 8):

- **(1) missed-line USE from the running-root tree: STILL EXISTS,
  narrowed.** Real's missed line is now `(dev-libs/blk0x-3:0/0::testrepo,
  ebuild scheduled for merge) USE="" ABI_X86="(64)"` — the old
  `(test-rust)` / `LLVM_TARGETS` container-profile content is gone
  (EAPI-8 `IUSE_EFFECTIVE` masking), but the line is still rooted at
  `/`: `Package.__str__` appends `to '<ROOT>'` to a merge-operation
  package iff `ROOT != "/"` (`Package.py:599-602`), and the missed line
  is a merge-operation package with NO suffix while every parent in the
  same cell carries `to '<target>'`. Corroboration: `ABI_X86` appears
  nowhere in `fixtures/` (grep-verified), so the group is host-only
  content. Portuale renders the fixture-tree `USE="" ELIBC="glibc"`.
  The brief's expectation that (1) is gone is therefore NOT met; per
  the brief, (1) was not normalised anywhere.
- **(2) parent `to '<root>'` suffix: STILL EXISTS** (verbatim in every
  probed parent line). Portuale stays bare per the #206 cut.
- **(3) `(stableforceflag)` excess: GONE.** No `stableforceflag`
  anywhere in real's blk0 output (profile EAPI 5 → honoured, then
  masked from the display as outside `IUSE_EFFECTIVE`); portuale no
  longer renders it either (same gate). Both sides now agree on the
  parents: `USE="" ELIBC="glibc"`.
- Extra: real's `blk0c blk0a blk0b` cell prints no block, matching
  portuale's `[]` pin for that order.

Probe (verbatim, single `podman run`):

```
$ podman run --rm -v /home/vivo/repo/PORTUALE/wt-230-skipped-block-display/pmtest/fixtures:/fixtures:ro -v /tmp/opencode/n230fix:/probe:ro --entrypoint /bin/bash localhost/test-portuale:latest /probe/probe.sh
=== portage version ===
Portage 3.0.81.3 (python 3.14.6-final-0, default, gcc-15, unavailable, 6.18.39-serv x86_64)
=== fixture profile eapi ===
5
5
=== blk0 ebuild EAPIs ===
==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0a/blk0a-1.ebuild <==
EAPI=8

==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0b/blk0b-1.ebuild <==
EAPI=8

==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0c/blk0c-1.ebuild <==
EAPI=8

==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0x/blk0x-1.ebuild <==
EAPI=8

==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0x/blk0x-2.ebuild <==
EAPI=8

==> /tmp/n230fixprobe/fixtures/repo/dev-libs/blk0x/blk0x-3.ebuild <==
EAPI=8
############ CELL: --backtrack=0 dev-libs/blk0b dev-libs/blk0a dev-libs/blk0c
WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/blk0x:0

  (dev-libs/blk0x-2:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with
    <dev-libs/blk0x-2 required by (dev-libs/blk0b-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/n230fixprobe/fixtures/') USE="" ELIBC="glibc"
    ^               ^

############ CELL: --backtrack=0 dev-libs/blk0b dev-libs/blk0c dev-libs/blk0a
WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/blk0x:0

  (dev-libs/blk0x-3:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with
    <dev-libs/blk0x-2 required by (dev-libs/blk0b-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/n230fixprobe/fixtures/') USE="" ELIBC="glibc"
    ^               ^
    <dev-libs/blk0x-3 required by (dev-libs/blk0c-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/n230fixprobe/fixtures/') USE="" ELIBC="glibc"
    ^               ^

############ CELL: --backtrack=0 dev-libs/blk0c dev-libs/blk0a dev-libs/blk0b
############ CELL: --backtrack=0 dev-libs/blk0c dev-libs/blk0b dev-libs/blk0a
WARNING: One or more updates/rebuilds have been skipped due to a dependency conflict:

dev-libs/blk0x:0

  (dev-libs/blk0x-3:0/0::testrepo, ebuild scheduled for merge) USE="" ABI_X86="(64)" conflicts with
    <dev-libs/blk0x-2 required by (dev-libs/blk0b-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/n230fixprobe/fixtures/') USE="" ELIBC="glibc"
    ^               ^
    <dev-libs/blk0x-3 required by (dev-libs/blk0c-1:0/0::testrepo, ebuild scheduled for merge to '/tmp/n230fixprobe/fixtures/') USE="" ELIBC="glibc"
    ^               ^
```

(`probe.sh` stages a writable fixture copy exactly like the round-0
probe — `repos.conf` location, `binrepos.conf`, `make.local`,
`categories`, updates-comment strip, overlay `masters`, vdb copy,
`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=$FX` — and prints the
four blk0 cells' skipped blocks. Staging script kept at
`/tmp/opencode/n230fix/probe.sh`.)

## Step 3 — comparator `to`-suffix normalisation (authorised, done)

`_norm_skipped_detail` strips ` to '<root>'` on both sides (portuale
never emits it); installed `in '<root>'` lines untouched; USE content
deliberately not normalised (pmtest `2963703`). New `test-skipped-updates.py`
case 12: a suffix-only pair compares clean, while a missed-line USE
delta on the same pair still fires. All 18 self-test checks pass
(pre-existing case 5 still fires on USE deltas). Offline simulation
(probe real text vs native portuale text, `bca` cell): exactly ONE
`skipped-update explanation differs` finding, confined to the missed
line (`ABI_X86="(64)"` vs `ELIBC="glibc"`); package lists match;
parents clean. Consequence: the four blk0 cells will NOT come out
clean — the allowlist entry stays a coordinator decision from the bed
(not re-added, per step 6).

## Step 4 — arg-parent arm (done, producer-pinned)

Traced all `SlotConflictParent` constructions: production
empty-`parent_cpv` rows come solely from top-level argument pullers
(`slot_conflict_puller_cpv` returns `""` iff `pc` is empty; those
puller tuples carry `current_atom`, i.e. the CLI text), and only the
direct-solve producer forwards them (reverse-pin always builds a
non-empty consumer; backtrack-mask `continue`s on empty `pc`). So the
renderer printing `s.atom` bare reproduces real's `str(parent)` arm
(`DependencyArg.__str__` returns `self.arg`, the raw CLI text)
exactly — no struct change needed. New unit test
`direct_solve_reports_an_argument_parent_with_the_cli_text` pins the
producer row (empty `consumer_cpv`, `atom` == CLI text, solve verdict
unchanged). The renderer arm comment now states the invariant and
names the arm unpinned end to end (no contract/bed cell grounds an
Argument parent — blk0 parents are all Packages).

## Step 5 — installed-consumer wrap trigger (done)

`skipped_update_installed_use_display_for` doc comment now states the
exact trigger: an installed skipped-block consumer carrying a
profile/repo force/mask flag — real wraps it (`forced_flags =
chain(pkg.use.force, pkg.use.mask)`, `UseFlagDisplay.py:60`, applied
to installed parents via `depgraph.py:1696-1700`) while portuale
renders it bare. No grounded case (all blk0 parents are merge nodes).

## Merge-fallout maintenance (my call, flagged below)

The directed #220 merge invalidated the round-0 unit test's
pre-EAPI-5 premise (`skipped_update_use_display_for_renders_the_fixture_blk0_shape`
failed: portuale renders `[("USE",""),("ELIBC","glibc")]`). I updated
its expectation to the probe-grounded post-#220 shape — byte-identical
to real's own parent lines — rather than leave the gate red; the
implementation is untouched (comments/tests only, zero rendering
change). The parallel pmtest pins were NOT touched (rule 7): they now
fail and need coordinator authorisation (see Questions).

## Gates

- `cargo fmt --check`: clean. `cargo clippy --release --all-targets`:
  0 warnings. `cargo test --release` whole workspace: all green
  (portage-repo 866 incl. the 2 #230 tests, portuale bin 708, rest
  green, 0 failed).
- `compare/test-skipped-updates.py`: ALL OK (18/18).
- Full pmtest suite (`--basetemp=/var/tmp/pmtest-n230fix`, 556 s):
  **2086 passed, 37 skipped, 5 xfailed, 4 failed**:
  - `test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`
    (the known round-0 corpus-agreement failure, unblessed — unchanged);
  - `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`
    (round-0 blk0 pins expect `(globalforceflag) (stableforceflag)`,
    portuale now prints `USE="" ELIBC="glibc"` — stale premise, see
    Questions);
  - `test_oracle_210_slot_change_reinstall_withheld_with_a_skip_notice[changed-slot]`
    and `[changed-slot-update-deep]` (#210 pins expect `USE=""` for
    `reinstslottarget-1.0`, portuale prints `USE="" ELIBC="glibc"` —
    same #220-EAPI cause; that ebuild is EAPI 8 on `main` too).
- Corpus drift reported, NOT blessed — same scope as round 0, no new
  drift from this round's changes (comments/tests only): the 8 named
  `#0` calls (`test_or_group_alternative...`,
  `test_oracle_missed_update_siblings_masked_together`,
  `test_oracle_no_aggressive_downgrade`,
  `test_oracle_selective_update_is_a_noop`,
  `test_oracle_slot_conflict_masks_highest_version_first`,
  `test_oracle_slotop_conflict_rebuild`,
  `test_oracle_slotop_revdeps_libgit2_stays_empty`,
  `test_oracle_two_simultaneous_conflicts_defer_second_to_later_pass`)
  plus the `expanded:` matrix for `btgp, btparent, libgit2-glib,
  mg2top, mgfa, mgxa, orbtblocked, gitg`.
- BED-PENDING (coordinator): `differential-test-bed/run/l0-fixture-oracle-all.sh`
  and `differential-test-bed/run/l0-resolver.sh` from the pmtest
  worktree. Prediction from the offline simulation: package lists
  MATCH on all four blk0 cells with one narrowed explanation finding
  each (missed-line `ABI_X86` group only).

## Questions for the coordinator (why NEEDS_CONTEXT)

1. Residual vehicle for narrowed cause (1): USE normalisation is
   forbidden by this brief, so the blk0 cells stay red without a
   narrowed allowlist entry (missed-line USE-from-`/`-tree, now exactly
   the `ABI_X86="(64)"` group). Restore it narrowly, or rule otherwise?
2. Authorise re-pinning the 3 stale-pin tests (blk0 + #210 ×2) to the
   probe-grounded post-#220 shapes, and bless the listed corpus drift?
3. Confirm the round-0 unit-test expectation update (same grounding)
   stands.

## Commits (paired, on `backlog/230-skipped-block-display`)

- pmtest `2963703` — comparator `to`-suffix normalisation + test 12.
- portuale `a6825b5d` — arg-parent producer pin + doc triggers + blk0
  unit-test post-#220 update (quotes the pmtest sha).
- Entry #230 NOT flipped. `READY-FOR-BEDS a6825b5d` written to
  `n230-progress.md` (beds compare binaries; the failing contract pins
  do not affect bed execution).

---

# Fix round 2 (2026-09-28) — Status: DONE

Executed the fix2 brief verbatim with one flagged scope note (below).
No portuale code changed (tree still `a6825b5d`, verified clean); no
allowlist/oracle/corpus file touched; corpus NOT blessed.

## What changed

Pmtest commit `75a766b` (standalone — no portuale counterpart) on
`backlog/230-skipped-block-display`, re-pinning the three stale-pin
tests authorised by the brief:

- `test_upstream_blocker_pg0_all_orders_pin_x1_and_uninstall_y1`
  (blk0): missed + parent lines
  `USE="(globalforceflag) (stableforceflag)"` →
  `USE="" ELIBC="glibc"`. Parents are real's own text from the round-1
  probe (`podman run localhost/test-portuale:latest /probe/probe.sh`:
  every parent renders `USE="" ELIBC="glibc"`); the missed line keeps
  portuale's fixture-tree display with a docstring note that real
  renders `USE="" ABI_X86="(64)"` (running-root `/` tree, backlog #242,
  bed-side narrowed allowlist `skipped-updates-cross-root-missed-line`
  left untouched per the brief).
- `test_oracle_210_slot_change_reinstall_withheld_with_a_skip_notice`
  `[changed-slot]` + `[changed-slot-update-deep]`: missed line `USE=""`
  → `USE="" ELIBC="glibc"`, grounded by the fixture-oracle g210 list
  (coordinator bed capture
  `differential-test-bed/logs/l0-fx-20260928T100830Z` in this worktree:
  real 3.0.82.2 prints the same explanation, both argument cells clean
  on this branch at `a6825b5d`).

## Scope note (flagged, not a judgment call)

The brief's "change only the USE text" holds literally for the blk0
pin (merge-scheduled parents carry no root suffix), but the #210 pin
— written on `main` against main's portuale, which lacks #230's
rendering — was additionally stale in two non-USE bytes, so pinning
only USE text would have left both cells red. The re-pin therefore
also takes portuale's actual installed-consumer lines
(`(cpv, installed in '<root>')`, root interpolated from
`tmp_path / "fixtures"`, the `_assert_residual_slot_conflict_block`
precedent) and the slot-span `^` marker lines (`29sp+^^^^` /
`29sp+^^^^^`). No behaviour was chosen: every byte is bed-grounded —
real's capture in `l0-fx-20260928T100830Z` shows the identical
`installed in '<ROOT>'` + bare-`USE=""` + caret shape (real lists the
bound parent first, portuale the consumer first; the comparator sorts
explanation lines, `resolve-compare.py:_parse_skipped_block`, so the
cells compare clean). The docstring states all of this. If the
coordinator wants the #210 pin reduced to USE text only, say so and
the cells go back to red pending a renderer change.

## Gates

- Focused: the 3 re-pinned tests pass
  (`--basetemp=/var/tmp/pmtest-n230fix2`).
- Full pmtest suite (`--basetemp=/var/tmp/pmtest-n230fix2full -p
  no:cacheprovider`, 509 s): **2089 passed, 37 skipped, 5 xfailed, 1
  failed** — the single failure is the known round-0
  corpus-agreement failure
  (`test_or_group_alternative_yields_to_the_next_when_backtracking_masks_it`,
  `_assert_harvested` line only; all its explicit re-pins pass).
- `compare/test-skipped-updates.py` not re-run (untouched this round;
  green in round 1). Cargo gates not re-run (no portuale change).
- Corpus drift reported, NOT blessed — same scope as rounds 0/1, no
  new drift from this round (pins only): the 8 named `#0` calls
  (`test_or_group_alternative...`,
  `test_oracle_missed_update_siblings_masked_together`,
  `test_oracle_no_aggressive_downgrade`,
  `test_oracle_selective_update_is_a_noop`,
  `test_oracle_slot_conflict_masks_highest_version_first`,
  `test_oracle_slotop_conflict_rebuild`,
  `test_oracle_slotop_revdeps_libgit2_stays_empty`,
  `test_oracle_two_simultaneous_conflicts_defer_second_to_later_pass`)
  plus the `expanded:` matrix for `btgp, btparent, libgit2-glib,
  mg2top, mgfa, mgxa, orbtblocked, gitg`.
- BED-PENDING (coordinator): `differential-test-bed/run/l0-fixture-oracle-all.sh`
  and `differential-test-bed/run/l0-resolver.sh` from the pmtest
  worktree. Prediction: blk0 cells clean modulo the narrowed
  `skipped-updates-cross-root-missed-line` entry (missed-line
  `ABI_X86` group only; parents and package lists match), g210
  argument cells fully clean (already captured green in
  `l0-fx-20260928T100830Z`).

## Commits

- pmtest `75a766b` (3 re-pins + docstring groundings) on
  `backlog/230-skipped-block-display`. Standalone: portuale untouched.
- Entry #230 NOT flipped. `READY-FOR-BEDS 75a766b` written to
  `n230-progress.md`.
- Full-suite log: `/tmp/opencode/n230fix2/full.log` (fresh basetemp
  dirs `/var/tmp/pmtest-n230fix2`, `/var/tmp/pmtest-n230fix2full`).
