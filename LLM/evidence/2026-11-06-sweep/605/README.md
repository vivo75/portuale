# #605 + #671 captures (2026-11-07)

Host Portage 3.0.83 vs portuale after the change; same setup as `../603/`
(`$FX` = fixture copy plus `w605-repo.json`; `$R` from `r605a..d.json`; scripts
in `../603/`). `w605x-1` is installed built with `w605flag`; `w605x-2` merges
with the flag off (no USE setting enables it).

- `A-*` (`w605cona`: `|| ( w605x[w605flag] w605y )`, `w605y-1` installed): the
  post-merge `||` judges the `[use]` against the `w605x-2` node, takes `w605y`;
  plain `U`, rc 0. Without the #605 half, the #671 half alone printed a false
  slot conflict here (checked by building without the `or_view.rs` change).
- `B-pv` (`w605conb`: `|| ( w605x[w605flag] w605w )`, `w605w` not installed) and
  `C-*` (`w605conc`: plain `w605x[w605flag]`): the atom still matches the
  installed `w605x-1`, so Portage pulls it back in: slot conflict, "Change USE:
  +w605flag", rc 1 (#671). portuale printed a plain `U`, rc 0, before.
- `C-unsat` (`r605d`: `w605x-1` installed without the flag): the atom matches
  nothing installed, initially unsatisfied (#603), no conflict.
- `C-switches-off`: no complete mode, no conflict.
- Remaining differences, not from this change: the conflict block's ROOT suffix
  (#672; the comparison in the commit strips it), and `C-walked`,
  `C-puD-world` (the consumer is walked: #673).
