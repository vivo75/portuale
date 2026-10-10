//! Child processes registered for the build scheduler to kill.

// Real `Scheduler._terminate_tasks`'s own "send kill signals... send
// kill signals and return without waiting for exit status"
// (`PollScheduler.py:106-118`, called once `_keep_scheduling` sees any
// package fail without `--keep-going`): every scheduled build's own
// top-level real subprocess (a phase's `unshare [sandbox] bash
// bin/ebuild.sh <phase>`, or `bin/misc-functions.sh`'s own equivalent)
// is spawned into a fresh process group (`process_group(0)`, `setpgid`
// before `exec`, so the child's own pid becomes the group id) via
// `spawn_trackable`, registered in `registry` for as long as the
// process runs. A whole process GROUP, not just the one pid: real
// portage's own descendant tree here -- `unshare` -> `sandbox` -> real
// `bash` -> whatever the ebuild itself spawns, e.g. a real compiler --
// all inherit the same group unless one of them starts a session of
// its own (none of these do), so a single group-directed signal
// (`kill(-pgid, …)`) reaches the whole tree, not just its outermost
// process.
//
// `registry` is per-`run_build_scheduler`-*call*, not a single
// process-wide singleton: `emerge_build::run_build_scheduler` creates
// one fresh (`new_scheduler_registry`) and shares it with its own
// worker threads only (`scope_scheduler_registry`'s own RAII guard,
// set at the very top of each `std::thread::scope`-spawned closure).
// A thread-local pointer to it -- not a parameter threaded through
// every intervening call (`run_commands_logged`/`run_one_phase`/…) --
// keeps this from being a signature change touching that whole chain;
// safe specifically because none of this module's own async phase
// bodies ever `tokio::spawn` an inner task (confirmed by grep), so
// `runtime.block_on(fut)` always drives the whole future to completion
// on the *calling* OS thread -- the same one the scheduler's own
// `scope.spawn` closure set the guard on, with no tokio-internal
// worker-thread migration to break the thread-local's visibility.
// Any other thread (a concurrent, unrelated test in the same process,
// a standalone `ebuild <file> <phase>` run, a single-job non-scheduler
// build) simply never sets the guard, so its own spawns are never
// registered anywhere and `kill_registered_children` can never reach
// them -- this is what makes a single global registry unsafe (an
// unrelated `cargo test` thread's subprocess could get killed by a
// completely different test's own scheduler-failure path) and why
// this is scoped per-call instead.
thread_local! {
    /// The current thread's scheduler registry, if any -- see the
    /// module comment right above for the full grounding.
    pub(crate) static SCHEDULER_REGISTRY: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>>> =
        const { std::cell::RefCell::new(None) };
}

/// A fresh, empty registry for one `run_build_scheduler` call.
pub(crate) fn new_scheduler_registry()
-> std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>> {
    std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// RAII guard: while alive, every `spawn_trackable` call on *this*
/// thread registers into `registry`. Restores whatever was set before
/// (normally `None`) on drop, including on an early return or panic --
/// see `SCHEDULER_REGISTRY`'s own doc comment for why this is scoped
/// per-thread rather than reaching for one process-wide singleton.
pub(crate) struct SchedulerRegistryGuard(
    Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>>,
);

impl Drop for SchedulerRegistryGuard {
    fn drop(&mut self) {
        SCHEDULER_REGISTRY.with(|cell| *cell.borrow_mut() = self.0.take());
    }
}

pub(crate) fn scope_scheduler_registry(
    registry: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>,
) -> SchedulerRegistryGuard {
    let previous = SCHEDULER_REGISTRY.with(|cell| cell.borrow_mut().replace(registry));
    SchedulerRegistryGuard(previous)
}

/// `SIGTERM`s every process group currently registered in `registry`.
/// Best-effort: a group that already exited between the snapshot and
/// the signal simply yields `ESRCH`, silently ignored, matching real's
/// own "typically... return without waiting for exit status" non-
/// blocking semantics.
pub(crate) fn kill_registered_children(
    registry: &std::sync::Mutex<std::collections::HashSet<i32>>,
) {
    let pgids: Vec<i32> = registry.lock().unwrap().iter().copied().collect();
    for pgid in pgids {
        // SAFETY: `kill` takes only plain ints (no pointers); `pgid` is a
        // process-group id recorded by this process itself, so negating it
        // cannot overflow and names a group we legitimately own.
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
}

/// `cmd.status()`, but registered in the calling thread's own
/// `SCHEDULER_REGISTRY` (if any -- see that thread-local's own doc
/// comment) for the duration. Every real top-level subprocess a
/// scheduled build's own phase/misc-functions execution spawns goes
/// through this instead of a bare `.status()`.
pub(crate) fn spawn_trackable(
    cmd: &mut std::process::Command,
) -> std::io::Result<std::process::ExitStatus> {
    use std::os::unix::process::CommandExt as _;
    cmd.process_group(0);
    let mut child = cmd.spawn()?;
    let pgid = child.id() as i32;
    let registry = SCHEDULER_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = &registry {
        reg.lock().unwrap().insert(pgid);
    }
    let status = child.wait();
    if let Some(reg) = &registry {
        reg.lock().unwrap().remove(&pgid);
    }
    status
}
