# crates/portuale/src/emerge_build/failure.rs
The failed-build endgame (backlog #426). Ground truth: `LLM/evidence/2026-10-19-b423/portage.txt`, design `LLM/evidence/2026-10-19-b426-design.md`.

## struct Ledger
One per merge run. `failed` prints Portage's `>>> Failed to emerge <cpv> for <ROOT>, Log file:` block (`Scheduler._failed_pkg_msg`) for a source phase failure and keeps the summary row plus the `${T}` for the elog; `resuming` prints `*** Resuming merge...` and the re-plan notice (`_calc_resume_list`); `dropped` prints the 75-column `emerge --keep-going: <cpv> ... dropped because it requires <atoms>` notice and keeps it as that package's elog; `finish` flushes the `Messages for package` blocks in failure order and prints the stderr list (`The following [N ]package[s] ha[s|ve] failed...`, wrapped at 72). A failure already shown makes `finish` return `REPORTED_FAILURE`, so the CLI adds no `emerge:` line.

## fn run_merge_loop
Under `--keep-going` a failure triggers one re-plan like Portage's resume loop: the failed package's transitive dependents (`required_by`) are dropped, the progress counter restarts over what is left (`_pkg_count.curval = 0`). The `-jN` scheduler does the same once, when its pass ends, for the dependents that never started.

## Known cuts
`>>> Verifying ebuild manifests`; the trailing `/` on `ROOT`; the dependency-resolution time; `ROOT`-qualified drop notice wraps at the same 75 columns but on a path without the `/`. Background single-failure full log dump and the `Error messages for package` block (`Scheduler.py:1337-1413`) and `postinst failed` rows are not ported (#437).
