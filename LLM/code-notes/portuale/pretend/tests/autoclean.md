# crates/portuale/src/pretend/tests/autoclean.rs
AUTOCLEAN tail pins.

## fn autoclean_tail_prints_the_using_system_header_exactly_once (#560)
- Backlog #560 (fix round): with a user-set `AUTOCLEAN=yes`, the post-merge tail prints `>>> Using system located in ROOT tree <root>/` exactly once. Portage prints it inside `_unmerge_display` (`unmerge.py:187-193`, no action gate); portuale's `run_autoclean_after_merge` wrapper printed it too, doubling the line (live `AUTOCLEAN=yes` merge proof in the task report; the shared `run_prune_nodeps_or_clean` header is the single Portage source). Re-exec pattern per `vdb_fuse::tests::a_detached_daemon_writes_its_stderr_to_the_log_file`: the child runs the printing scenario, the parent counts the lines.
