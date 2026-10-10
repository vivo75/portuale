# Evidence — backlog #271 (`--autounmask-only` re-shows the merge list), D3 S0

Fresh probe pairs, 2026-10-01 (batch date 2026-09-30): host `opencode`
run, real Portage **3.0.82.2** (`/usr/sbin/emerge`, staged fixture via
`/tmp/opencode/d2/stage-host.sh` adapted as `/tmp/opencode/d3/stage-host.sh`
— fixture-only deltas identical to the oracle's `stage.sh`, only the
fixtures path re-pointed at this worktree pair) vs this worktree's
`portuale` release binary against the same staging.

Identical effective options on both sides (`--ignore-default-opts`,
`PORTAGE_CONFIGROOT=ROOT=PORTAGE_RUNNING_ROOT=<staged>`,
`DISTDIR=<staged>/distfiles`, `CLEAN_DELAY=0`, empty
`EMERGE_DEFAULT_OPTS`, `LC_ALL=C.UTF-8 TZ=UTC PYTHONHASHSEED=0`).

Each file below is the run's **stdout followed by stderr** (same
convention as `../2026-09-30-265/`); return codes are in the table.

## Cells

- `autounmaskkeywordpkg`: `emerge --ignore-default-opts --pretend --autounmask --autounmask-only dev-libs/autounmaskkeywordpkg`
- `useflagpkg-missingflag`: `emerge --ignore-default-opts --pretend --autounmask --autounmask-only "dev-libs/useflagpkg[missingflag]"`
- `*.control.txt`: the same argv without `--autounmask-only`.

## Result (the item, confirmed)

| cell | real rc | real stdout | real stderr | portuale rc (pre-fix) | portuale stdout (pre-fix) |
|---|---|---|---|---|---|
| keyword only | 0 | merge list (1 row) | keyword block | 0 | `""` (suppressed) |
| keyword control | 1 | same list | same block | 1 | same list |
| useflag only | 0 | merge list (3 rows) | USE block | 0 | `""` (suppressed) |
| useflag control | 1 | same list | same block | 1 | same list |

Real's `--autounmask-only` stdout is byte-identical to its control
stdout (modulo the `Dependency resolution took N.NN s` timing and the
first-run-only `Performing Global Updates` noise, both staging/host
artifacts); stderr is byte-identical full stop. The only observable
difference between the only-run and the control on the real side is the
exit code (0 vs 1).

## Real-source gate (vendored `3rdparty/portage`, 3.0.82.2)

- `lib/_emerge/actions.py:456-458`: `if "--autounmask-only" in myopts:
  mydepgraph.display_problems(); return 0` — returns **before** the
  normal `display()` (`:464+`, the `mergelist_shown` branch), so the
  suppression portuale ports (`show_merge_list = !autounmask_only`)
  matches the normal path but misses the re-show below.
- `lib/_emerge/depgraph.py:11140` (`display_problems` tail):
  `self._display_autounmask()` runs on the `--autounmask-only` path too.
- `lib/_emerge/depgraph.py:10625` (`_display_autounmask`): each of the
  four change loops calls `self._show_merge_list()` first (keyword
  `:10686`, p_mask `:10733`, USE `:10767`, license `:10803`).
- `lib/_emerge/depgraph.py:10488-10495` (`_show_merge_list`): `self.display(
  self._dynamic_config._serialized_tasks_cache)` unless already
  displayed — the merge list is re-shown exactly once, ahead of the
  change blocks.

So the port is: under `--pretend --autounmask-only`, show the same merge
list the control shows (real's `_show_merge_list` has no
pretend/ask/verbose gate of its own), keep the changes block on stderr,
keep rc 0 via the existing early return.

## Deliberate, precedent-backed cuts vs real's bytes (unchanged by this slice)

- Portuale omits real's `to <root>` row suffix (`Package.__str__` when
  `ROOT != "/"`) and real's `for <root>.` / `to <root>/` decorations.
- Portuale prints no `These are the packages …` / `Calculating
  dependencies …` / `Dependency resolution took …` preamble under
  `--pretend` (pre-existing pretend shape), and no eselect-news notice.
- The keyword cell's first real capture carries the one-time
  `Performing Global Updates` pass (kept verbatim).
- The useflag cell's third row already carries `USE="foo missingflag"`
  on the portuale side too (control-probed pre-fix), so no D4 overlap.
