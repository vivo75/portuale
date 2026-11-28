# crates/portuale/src/pretend/tests/autoclean.rs
AUTOCLEAN tail pins.

## fn autoclean_tail_prints_the_using_system_header_exactly_once (#560)
- Backlog #560 (fix round): with a user-set `AUTOCLEAN=yes`, the post-merge tail prints `>>> Using system located in ROOT tree <root>/` exactly once. Portage prints it inside `_unmerge_display` (`unmerge.py:187-193`, no action gate); portuale's `run_autoclean_after_merge` wrapper printed it too, doubling the line (live `AUTOCLEAN=yes` merge proof in the task report; the shared `run_prune_nodeps_or_clean` header is the single Portage source). Re-exec pattern per `vdb_fuse::tests::a_detached_daemon_writes_its_stderr_to_the_log_file`: the child runs the printing scenario, the parent counts the lines.

## fn autoclean_leftover_root
- Backlog numbers removed from the comment (original text): Backlog #404 (02.346 S11): auto-clean runs the `--clean` selection system-wide after a successful merge with a user-set `AUTOCLEAN=yes`. Scratch vdb with two same-slot instances of one cp: `old-1` (to be selected) and `old-2` (protected). Field files only; no `CONTENTS` means no owned paths, so the unmerge just drops the entry. Portage: `_emerge/actions.py`, `action_build` (`>>> Auto-cleaning packages...` then `unmerge(..., "clean", [], autoclean=1)`), gated on `settings.get("AUTOCLEAN") == "yes"` (make.globals default `no`).
