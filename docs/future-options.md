# Future options

Some behaviour is not a plain option yet. `mrg` offers it behind one option,
`--future-options=name,other=value`, so each piece can later be promoted to a
plain option or removed without breaking anyone silently.

## Stages

| Stage | Default | Meaning |
|---|---|---|
| `alpha` | off | An experiment: it can change or disappear. |
| `beta` | on | Likely to stay; `name=false` switches it off. |
| `ga` | on | Always on, kept only so old command lines still parse. |

`mrg --help` lists every option with its stage and the release that introduced
it. A switch is `name` (on) or `name=false`; a value option is `name=value`.
An unknown name is refused (exit 2) and the closest registered names are shown.

## Where they are set

Lowest to highest, per option: the stage default, `make.conf`, the
environment, the command line. The variable is `PORTUALE_FUTURE_OPTIONS` in the
environment and in `make.conf`, with the same syntax as the option. When the
command line carries `--future-options`, `make.conf` is not read at all.

## The options today (all `alpha`, since 300.83)

| Name | Action | Effect |
|---|---|---|
| `check-world-unique` | `--check-world` | A slot with several satisfying versions is unsatisfiable. |
| `check-world-threads=N` | `--check-world` | Number of threads (default: every core). The answer does not depend on it. |
| `check-world-unneeded` | `--check-world` | Also list (`UNNEEDED cat/pkg:slot`) the slots another slot serves. The exit code does not change. |

Only `mrg` has the option: no option here is used by `emerge`. The code is in
`crates/portuale/src/future_options.rs`; adding, promoting or removing an
option is a change to its `GATES` table.
