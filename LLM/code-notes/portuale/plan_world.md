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
