# crates/portuale/src/emerge_build/failure.rs
The failed-build endgame (backlog #426). Ground truth: `LLM/evidence/2026-10-19-b423/portage.txt`, design `LLM/evidence/2026-10-19-b426-design.md`.

## struct Ledger
One per merge run. `failed` prints Portage's `>>> Failed to emerge <cpv> for <ROOT>, Log file:` block (`Scheduler._failed_pkg_msg`) for a source phase failure and keeps the summary row plus the `${T}` for the elog; `resuming` prints `*** Resuming merge...` and the re-plan notice (`_calc_resume_list`); `dropped` prints the 75-column `emerge --keep-going: <cpv> ... dropped because it requires <atoms>` notice and keeps it as that package's elog; `finish` flushes the `Messages for package` blocks in failure order and prints the stderr list (`The following [N ]package[s] ha[s|ve] failed...`, wrapped at 72). A failure already shown makes `finish` return `REPORTED_FAILURE`, so the CLI adds no `emerge:` line.

## fn is_postinst_failure, fn postinst_failed_error (#444)
A `pkg_postinst` that dies after an otherwise successful merge surfaces as `"{cp}-{ver}: postinst failed ({status})"` (single line, so `is_phase_failure` would also match it -- every consumer checks the postinst arm first). Ground truth: `LLM/evidence/2026-10-21-b444/` (host `emerge` 3.0.82.2): the merge lands, so the endgame is `>>> Failed to install <cpv> to <root>, Log file:` (`_failed_pkg_msg` with `install`/`to`), never `Failed to emerge`, and the summary row carries no `(postinst failed)` suffix -- that suffix is the returncode-OK binary-merge arm (`_do_merge_exit`'s `postinst_failure` branch), unreachable for a source merge whose `EbuildMerge` returns `RETURNCODE_POSTINST_FAILURE`.

## struct MergePassReport, struct MergePassError (#444)
One merge pass's out-channel: the display string for the `emerge:` line (or `REPORTED_FAILURE`) plus the postinst-failed `(category, package)` pairs and whether a `--keep-going` resume pass re-saved the resume list. The world recorder (`pretend::merged_entries`) and the failure-path resume re-save (`pretend::unmerged_for_resume_save`) both read the report, so a postinst-failed entry (vdb entry live) is neither world-recorded nor resurrected in the resume list.

## fn run_merge_loop
Under `--keep-going` a failure triggers one re-plan like Portage's resume loop: the failed package's transitive dependents (`required_by`) are dropped, the progress counter restarts over what is left (`_pkg_count.curval = 0`). The `-jN` scheduler does the same once, when its pass ends, for the dependents that never started.

## Known cuts
`>>> Verifying ebuild manifests`; the trailing `/` on `ROOT`; the dependency-resolution time; `ROOT`-qualified drop notice wraps at the same 75 columns but on a path without the `/`. Background single-failure full log dump and the `Error messages for package` block (`Scheduler.py:1337-1413`) are not ported (#437). The `(postinst failed)` summary-row suffix is not ported either: it belongs to the returncode-OK binary-merge arm, and no source merge reaches it. The setup-phase `Package:`/`USE:`/`FEATURES:` elog header (log-only in Portage, replayed by the lone-failure dump) is not emitted: the shell never prints it and no Rust caller stages it.

## (#459)
- Binary-merge `pkg_postinst` failures surface exactly like source ones in 3.0.82.2 (oracle `LLM/evidence/2026-10-21-f459/`, `--usepkgonly` cells): `EbuildMerge` returns the nonzero rc without the `postinst_failure` flag, so `Completed` prints, `_do_merge_exit` takes the `install`/`to` arm, no world record, and the summary row carries no `(postinst failed)` suffix. Backlog #459's filed claim (world recorded + suffix shown) describes the dead-in-practice returncode-OK arm and was overruled by the probe.
- `source_paths_for` covers `Binary` entries too (both merge under `portage/<cat>/<pf>`; a binary log is the captured hook output). `failed_pkg_label` renders `binary scheduled for merge` for them, and `label_cpv_slot` appends `-<build_id>` for a binary whose index carried `BUILD_ID` (Portage `Package.__str__`, like the merge-list rows).
- The build-phase arm stays source-only: a binary entry's non-postinst merge error keeps its own tail (never the `>>> Failed to emerge` block). Only the postinst arm fires for `Binary` entries.
