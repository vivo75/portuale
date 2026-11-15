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
