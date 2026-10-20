# crates/portuale/src/title.rs
The terminal title around an `emerge` run, ported from Portage's
`lib/portage/output.py` (`xtermTitle`, `xtermTitleReset`) as called from
`lib/_emerge/actions.py:3694` (start) and `lib/_emerge/main.py:1377-1382`
(reset in the `finally:`).

## fn title_enabled
- The gate is stderr-is-a-tty **and** `$TERM` prefix-matches Portage's
  `_legal_terms_re` (`^(xterm|xterm-color|Eterm|aterm|rxvt|screen|kterm|rxvt-unicode|gnome|interix|tmux|st-256color|alacritty|konsole|foot)`,
  `output.py:254-256,263-268`, cached per process there) **and** `notitles`
  is absent from the resolved `FEATURES` (`actions.py:3694`,
  `main.py:1377`). `output.py`'s `dotitles` global is always true
  (`notitles()` is a no-op missing `global`), so there is no fourth gate.
- Written to stderr (`sys.__stderr__`), never stdout. The start title is
  `socket.gethostname()`-prefixed and truncated to 253 characters
  (`output.py:271-277`).
- The reset (`output.py:291-332`): with `$PROMPT_COMMAND` unset, the
  `LOGNAME@HOST:PWD` title (`HOSTNAME` split at the first dot, `$PWD`
  `~`-abbreviated against `$HOME`); with it set, `$SHELL -c` runs on
  stderr (`fd_pipes={0: stdin, 1: stderr, 2: stderr}`) and nothing else
  is written; with it set-but-empty, nothing is written. The reset path
  re-checks the isatty/`TERM` gates for the command branch
  (`output.py:303-307`); the guard evaluates once since neither changes
  mid-run.

## struct TitleGuard
- Constructed in `pretend::run` right after argv parsing succeeds (so
  parse errors print no title, like Portage dying before `run_action`),
  covering the `emerge` and `mrg` applets (both end in `pretend::run`).
  `Drop` runs the reset, mirroring `main.py`'s `finally:` on every
  return path. `--help` returns before, so it stays title-free.
- Backlog #453 C: Portage's spinner/timing bytes on a tty are an
  intentional cut (oracle verdict 1a); the title is the only tty-only
  output ported. Pinned by pty tests in the contract suite (title
  present with controlled env; absent piped / `TERM=dumb` /
  `FEATURES=notitles` / stderr-redirected).
