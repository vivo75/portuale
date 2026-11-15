# crates/portage-repo/src/tests/tests_p491.rs
The installed-parent update probe (#491 pass-0 seed), hermetically.

## module
- Oracle: Portage 3.0.83 `--debug` try-1 `@__auto_slot_operator_replace_installed__` set (24 atoms, plasma consumers + libplasma; `LLM/evidence/2026-10-25-491/` `portage-try1-replace-arg.txt`, `analysis.md`).

## fn p491_pass0_seed_stays_quiet_when_no_replacement_is_newer

- Backlog #748 (Branch B): the host `@world` shape — installed
  qtmultimedia-6.11.3 records `ffmpeg:0/60.62.62=`, installed and final
  ffmpeg both sit at `0/60.62.62`, and the only tree replacements are
  the installed version itself; the visible-but-unmerged ffmpeg-9.0.2
  (`0/61.63.63`) the probe finds never merges, so the forced
  same-version reinstall repairs no binding. Reduced: installed
  p491con-2, the newest tree version, recording
  `>=dev-libs/p491lib-1:6/7=`, with installed p491lib-1 at `6/7` and
  tree p491lib-2 at `6/8` available but unmerged. Expected: no seed — a
  same-version "replacement" re-binds nothing.
  Charter property 1 (the final state holds with or without the
  reinstall; a resolver/fast difference is judged against the final
  state, never filed as a defect by itself).
