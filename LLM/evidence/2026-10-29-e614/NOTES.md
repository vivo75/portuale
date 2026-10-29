# #614 oracle — Portage's `--pretend --debug` resolution display

Captured 2026-10-29 on this host with Portage 3.0.83 (`emerge --version`:
`Portage 3.0.83 (python 3.14.8-final-0, ...)`), against a copy of the pmtest
fixture tree at `/tmp/opencode/ph-e614` (the `PORTAGE_CONFIGROOT` / `ROOT` /
`PORTAGE_RUNNING_ROOT` shape the contract suite uses). `capture.sh` reproduces
both sides.

## What Portage prints

| atom | rc | banner | `Calculating dependencies ... done!` | `Dependency resolution took …` |
|---|---|---|---|---|
| `dev-libs/diamond` | 0 | yes (`portage-diamond.txt:58`) | **no** | **no** |
| `dev-libs/abort-masked-mid` | 1 | yes (`portage-abort-masked-mid.txt:33`) | **no** | **no** |

`abort-masked-mid` additionally prints the debug trace; the two spinner lines are
absent from both files.

## What portuale prints today

Both lines in both files (`portuale-diamond.txt:75-76`,
`portuale-abort-masked-mid.txt:62-63`), e.g.

```
Calculating dependencies ... done!
Dependency resolution took (backtrack: 0/20).
```

This is the defect #614 names: the `backtrack: N/M` text is part of the
resolution report, so a `--debug` row carries a count line Portage never prints.

## Cause (Portage source, pinned 3.0.83)

- `--debug` sets `PORTAGE_DEBUG=1`: `_emerge/main.py:1267-1269`.
- `_emerge/actions.py:4200` reads `PORTAGE_DEBUG` into `debug`, and
  `:4220-4221` then sets `spinner.mode = spinner.QUIET`.
- In QUIET mode `begin_notice` writes nothing and `displays_notice()`
  (`_emerge/stdout_spinner.py:92-93`) is false, so `end_notice()`
  (`_emerge/stdout_spinner.py:126-139`) returns `False` without completing the
  notice.
- `backtrack_depgraph` calls `_show_resolution_report` only when
  `spinner.end_notice()` is true (`_emerge/depgraph.py:12350-12351`), so the
  report (and its `backtrack: N/M` suffix, `depgraph.py:12265-12275`) is never
  printed.
- The banner survives because `_start_resolution_display`
  (`_emerge/depgraph.py:12216-12260`) prints it with `writemsg_stdout`, not
  through the spinner.

The QUIET condition is therefore `--quiet` **or** `--nodeps` **or** `--debug`
(`depgraph.py:12259-12260` for the first two, `actions.py:4220` for `--debug`).
