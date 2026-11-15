# crates/portuale/src/plan_world.rs
`mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver]`: what
`emerge -uDNp @world` would install, worked out by `portage-world`.

## fn plan_json (#745, #746) — second-build machinery deleted 2026-11-14

- Plan `02.749-resolver-2a`: one `entries` object per row in row order
  (`merge_order` is the row index), one row per slot, no second-build key —
  the `built_twice` field, the `pass` parameter, and the stderr `built
  twice` clause went with the `Row`/`Pass` deletion (the owner ruled the
  positional duplicate illegal). The removed shape (history): the #745
  tree ordered `[prov, virt, user, user]` — the consumer's `Pass::Rebuild`
  second row right after the cycle closed — and the checker excused the
  first build's need with a `cycle-noted` naming that row. No-breaker
  output is byte-identical (the fixture pins prove it). The stderr summary
  counts unique slots;
  `--sort alpha`, `depend` and `resolver` all print each slot once — the
  duplicate was an ordering artefact of the `depend` walk, not a second
  package.
- The checker side went the same day (pmtest): `check_order` keeps the
  charter-literal boundary (a build atom satisfied at start; runtime
  needs by the end), a build need met only by a later merge is always a
  `build-order` failure — merge-it-first, or, when the later merge's
  build closure reaches back, the cycle message with the #753 pointer
  (`reaches` on the build-only `_build_graph`); `check_final_state` fails
  any repeat in one slot (the identical-cpv collapse is gone). A genuine
  build-time cycle is `build_time_cycles` non-empty → one stderr report
  line per cycle; rc stays 0 and the checker's independent `build-order`
  is the gate's other half — no pointer rides `--json` by design.

## Charter 2a bootstrap (backlog #753)
- `select_bootstrap`/`apply_bootstrap` follow the resolver: the shared leaf
  rule over the same cycles + guard map, the same STOP-1/2/3 refusals, plus
  a refusal when the boot slot has no recorded USE (no invented override —
  `planned_use` always covers a merging slot, so this is unreachable, not a
  fallback). `PlanRow` carries the optional override; `entry_json` renders
  `"bootstrap":true` + `"reduced_use":["-<flag>"]` on boot rows only and
  the declared-minus-flag USE, `text_row` the
  `(bootstrap: USE="…" temporary …)` suffix. Acyclic output is byte-identical
  (pinned by `an_acyclic_plan_carries_no_bootstrap_key`).
- The report line keeps the #749 wording and names the reduction inside
  balanced parens for bootstrapped cycles only; the count line is now
  `{slots} to install ({rows} builds including {boot} bootstrap)`.
- `resolver_and_fast_plan_agree_on_the_bootstrap_rows` proves agreement on
  the same tree (same order, USE sets, markers) with no duplicated rule,
  argv order included: the leaf name-sorts each reported cycle before
  scoring (review I2), so reversed argv picks the same reduction — the
  old note claiming a mirrored pick is superseded.

## fn run (bootstrap, #760)
- `select_bootstrap`/`apply_bootstrap` moved to `portage-world` (`World::build_plan`, see `portage-world/bootstrap.md`). It returns `(boot slot, reduced flag)` as `Bootstrap { node, flag }`; the caller keeps today's single-rows-plus-report shape when there is none (STOP rules 1-3: no single-flag reduction, a boot that still needs a member, more than 3 affected rows, or no recorded USE to reduce from).

## fn the_fast_plan_takes_the_newest_arm_of_a_disjunctive_dependency

- Charter property 3 (plan 02.740): the fast plan takes the `||` arm
  whose atoms the best visible candidates match, so the compat package
  is new and the provider upgrades instead of staying pinned.

## fn a_kept_virtual_with_a_stale_provider_keeps_todays_cut_and_order

- Charter property 2 (PMS 8.1; backlog #751, plan `02.751` §6-A): the
  S0 probe tree — kept `virtual/w751virt`, installed `w751prov-1`
  with a stale `w751base:0/1=` binding rebuilt as `w751prov-2`,
  `w751base-2` with `BDEPEND w751prov` (so the virtual's provider
  edge is the cycle's only cut), consumer `w751user-1` with
  `BDEPEND virtual/w751virt`. The rows keep today's cut and order —
  one row per slot (backlog #749: the `prov`/`user` duplicates are
  gone), the virtual never a merge — and the checker reports no
  `build-order`. No report line either: the `base <-> prov` pair is
  runtime-closed, not a build-time cycle. Limitation: a kept
  virtual's stale provider rebuilds after the consumer that builds
  with it, yet nothing exhibits a consumer building against a
  broken copy (here the installed copy is still usable at the
  consumer's build) — theoretical, pinned, not proven safe.

## fn a_runtime_cycle_is_one_row_per_slot

- Charter property 2 (backlog #749): the mixed `R/B/R` #745 shape is a
  runtime-closed cycle, not a genuine build-time one — every build
  atom is satisfied at start, so one row per slot and no finding at
  all. No report line either: the `prov -> base` half is `RDEPEND`,
  so the report reads nothing here.

## fn resolver_and_fast_plan_agree_on_the_bootstrap_rows

- Resolver/fast-plan agreement (backlog #753, S1.4): both emitters call
  the shared leaf with the same cycles + guard map, so the same tree
  yields the same affected-slot rows — same order, same USE sets, same
  markers. No new sort, no new tie-break anywhere.
- Reversed argv must agree row-for-row: member order is name order
  inside `bootstrap_reduction`, so the insertion-order walk cannot
  flip the pick (backlog #753 I2).

## fn a_genuine_build_cycle_bootstraps_with_a_marked_boot_row

- Charter property 2a (backlog #753): the owner's USE-conditional pair
  bootstraps to three rows — the reduced-USE boot build, the middle,
  the definitive rebuild with the declared USE — with the `bootstrap`
  marker and suffix on the boot row only, rc 0, and the report line
  naming the reduction. The failure the checker reports on the same
  plan without the bootstrap is still pinned in pmtest
  (`test_use_conditional_build_cycle_fails`).

## fn a_cycle_through_a_kept_package_orders_the_repaired_provider_first

- Charter property 2 (PMS 8.1: a build tool must be usable, backlog
  #746): the repaired provider is ordered before the package that builds
  with the tool needing it, and the checker reports no `build-order`.

## fn kept_middle_tree

- The #746 shape: `w746auto` builds with the kept `w746help`, whose
  `RDEPEND` needs the rebuilt `w746loc`; `w746get` and the new `w746icu`
  close the cycle back to `w746auto`. The installed `w746loc` records a
  stale `:=` on `w746prov` (installed `0/2`, recorded `0/1=`), so the
  plan rebuilds it without moving the provider (backlog #741).

## module

- `mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver]`: what
  `emerge -uDNp @world` (or, with `emptytree`, `emerge -pe @world`) would
  install, worked out by `portage-world` instead of the resolver. One
  row per slot whose best visible version is not the installed one, and an `R`
  row for a kept package whose recorded `:slot/sub-slot=` dependency the plan
  breaks or that is already stale (with `plan-world-newuse`, also one whose
  USE changed). A stale binding the plan cannot repair is named on stderr
  instead of gaining a row. It sees no blockers
  and no `REQUIRED_USE`, so it can list less or more than the resolver; `--sort resolver` shows the
  resolver's own order for the same rows. `--sort depend` puts a package
  after what it needs before it is merged or usable: a runtime edge out of
  a package the plan builds with still orders (PMS 8.1, backlog #746), and
  a cycle with no cuttable edge is reported, never papered over
  (charter 2a, backlog #749) — except the charter 2a bootstrap, which
  serializes the one shape a single USE-flag reduction breaks (backlog
  #753): the boot row (reduced USE, temporary, marked) plus the definitive
  rebuild with the declared USE. A `PDEPEND` orders nothing (PMS). One row
  per slot, always, plus the boot row when a cycle bootstraps.
