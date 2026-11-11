# future_options.rs

## const GATES

The one registry (#706). Promoting an option is changing its `stage`; removing
it is deleting its line. `Stage::Beta` and `Stage::Ga` exist ahead of use, hence
the `allow(dead_code)` with its reason.

## fn resolve

Owner decision 2026-11-11: the precedence is default, `make.conf`, environment,
command line, and `make.conf` is not read at all when the flag is given. This
differs from `resolve_vdb_selection` (`mrg.rs`), whose options are single-valued
and read `make.conf` only when flag and environment leave the choice open;
future options are a list merged per option, so `make.conf` is the lowest layer
instead. The lookup stays lazy: `conf` is a closure over a config load that runs
on first use (`make_conf_lookup_at`, with the explicit `--config-root`
`--check-world` receives).

## emerge

`emerge` does not parse `--future-options`: owner rule 2026-11-11, `emerge`
gets only Portage's options unless it costs under 20 lines, and no option here
is read by `emerge` (all three act on `mrg --check-world`). When one is, the
parse is `resolve(None, env, conf)`, the same call the VDB selection makes from
`emerge` with its flags absent.
