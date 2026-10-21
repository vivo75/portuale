# crates/portuale/src/pretend/merge_run.rs
Build and merge the plan from source, after saving it as the resume list.

## fn run_source_merge_plan
- The up-front resume save is Portage `Scheduler.merge() -> _save_resume_list()` (backlog #168), same as the `--getbinpkg` arm above; the failure-path re-save is `_save_resume_list` on failure. `write_resume_list` itself is unconditional now (Portage always assigns -- an all-noop plan records the empty list, backlog #179).
- Backlog #444: the failure-path save is `unmerged_for_resume_save` (still-unmerged entries plus any postinst-failed one, whose merge landed but which Portage never removes from the saved list except through a resume pass), and `merged_entries` excludes postinst-failed entries (Portage's `EbuildMerge._merge_exit` skips `world_atom` on the postinst-failure path), so a dying `pkg_postinst` records no world entry.
- Backlog #196 fix round 1: the failure-path GLEP 42 notice is Portage `post_emerge.py:155` regardless of retval, gated on the vdb having changed (`:112-117`).
- Portage `Scheduler._save_resume_list`: on a merge failure, record every still-unmerged package so `emerge --resume` can pick up where this left off.

## fn run_binary_merge_plan
- Merge a plan with binary packages: on a remote host, or here after saving the resume list.
- Remote execution (`mrg --remote-hostname` + `--getbinpkgonly`, published via the `REMOTE_EXEC` handoff): resume/world bookkeeping is skipped (the client owns that state -- see remote.rs). The download/ledger `$PKGDIR` is the placed config's own `PKGDIR` (`config.pkgdir`, honouring the resolve's `ConfigRootOverride`), not the process `$PKGDIR` env the local plan uses -- the server stores against the config it resolved against (plan §7/§8).
- Slice 6: `CONFIG_PROTECT` comes from `Config::resolved_incremental` (profile + make.conf stack) unless explicitly flagged.
- Backlog #231 (b): Portage `post_emerge`'s tail (info regen, preserved-libs notice, config-file check, the GLEP 42 notice at `post_emerge.py:155`) runs only once the vdb changed (`post_emerge.py:112-117` early-returns otherwise -- printing just the notice, and only under `--pretend`). `run_remote_plan` ships every unit through the transport; the local `${ROOT}/var/db/pkg` is never touched. Every `--pretend` exit returned long before this dispatch, so the tail would print nothing. Pinned by the local-transport contract test. The pre-resolution notice already printed before the resolve, like Portage.
- Each successful merge below shrinks the saved list again (`run_merge_loop` / the `-j` scheduler); a later failure write warns. `--buildpkgonly` and remote execution are deliberately untouched by the save here.
- Backlog #174: a merge-time binpkg digest failure already printed Portage's full tail (digest block + `>>> Failed to emerge ...`, `Scheduler._failed_pkg_msg`); Portage prints neither a resume-list notice nor an `emerge:` line for a merge-time package failure. Backlog #187 extends the same silence to the missing-file sentinel (Portage `BinpkgVerifier`'s ENOENT arm).
- The `--getbinpkg` failure save was previously missing entirely -- a failed mixed-source-and-binary run left nothing for `--resume` to find. `entries_not_merged` tags source vs binary via its own `ResumeEntryKind`.
- Silent on write failure, like the `rotate_resume_to_backup` above (a later failure write warns).
- `emerge --getbinpkg`/`-g` (and `-G`, binary-only): merge every resolved entry, per-entry `Binary` vs `Source` -- see `emerge_getbinpkg::run_merge_plan`.
- Backlog #196 fix round 1: Portage `post_emerge.py:155` prints the GLEP 42 notice regardless of retval -- gated on the vdb having changed (`:112-117`), which here is `unmerged.len() < entries.len()` (at least one entry landed; the same structural approximation as the success path's `!entries.is_empty()` below).
