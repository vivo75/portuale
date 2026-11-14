# crates/portuale/src/plan_world.rs
`mrg --plan-world[=update|emptytree] [--sort alpha|depend|resolver]`: what
`emerge -uDNp @world` would install, worked out by `portage-world`.

## fn plan_json (#745, #746)

- `Row { slot, pass }` carries the charter-2a second build through to the
  `--json` document: one `entries` object per row in row order (`merge_order`
  is the row index), and only a `Pass::Rebuild` entry carries
  `"built_twice": 2` — with no `from_version`, since it rebuilds the same
  version to break a cycle rather than replacing an installed one. The first
  entry keeps today's fields. No-breaker output is byte-identical (the
  fixture pins prove it).
- The stderr summary counts unique slots (`rows` would overcount the pair)
  and appends `, N slot(s) built twice: cat/pkg:slot` (sorted) when a
  breaker fired. `--sort alpha` and `--sort resolver` print each slot once
  with no `built_twice`: the rebuild is an ordering artefact of the
  `depend` walk, not a second package.
- The checker side lives in pmtest: `check_final_state` collapses an
  identical cpv in one slot before the slot check (a second build is not a
  second package; two different versions still fail), and `check_order`
  turns a `build-order` finding into `cycle-noted` when the plan merges the
  identical cpv in the identical slot again later with the need matched
  there (`_second_build`, next to `_order_finding`).
