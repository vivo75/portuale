# crates/portuale/src/ebuild_phases/scheduler_registry.rs
Child processes registered for the build scheduler to kill.

## module
- Portage's `_terminate_tasks` sends kill signals and returns without waiting for exit status, once `_keep_scheduling` sees a package fail without `--keep-going`.
- A whole process group, not one pid: the descendant tree (`unshare` -> `sandbox` -> bash -> e.g. a compiler) inherits the group unless one starts its own session (none do), so `kill(-pgid, …)` reaches the whole tree. The child enters via `process_group(0)` (`setpgid` before `exec`), so its pid becomes the group id.
- `run_build_scheduler` creates one fresh registry and shares it with its `std::thread::scope` workers (`scope_scheduler_registry` guard at the top of each closure). Not a threaded-through parameter, to avoid a signature change across `run_commands_logged`/`run_one_phase`/….
- A single global registry would be unsafe: an unrelated `cargo test` thread's subprocess could be killed by a different test's scheduler-failure path.
