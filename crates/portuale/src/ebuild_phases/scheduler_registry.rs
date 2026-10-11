//! Child processes registered for the build scheduler to kill.
//!
//! Every scheduled build's top-level subprocess runs in a fresh process group
//! (via `spawn_trackable`), registered here while it runs, so a failing package
//! (without `--keep-going`) kills whole trees with one group-directed signal.
//! Portage: PollScheduler.py, _terminate_tasks
//!
//! One registry per `run_build_scheduler` call, shared with its worker threads via
//! a thread-local pointer (not a threaded-through parameter). Safe because no phase
//! body ever `tokio::spawn`s an inner task: `block_on` drives the future on the
//! calling thread, the one the guard was set on. Threads without a guard (tests,
//! standalone runs, single-job builds) never register, so they can't be killed.
thread_local! {
    /// The current thread's registry, if any; see above.
    pub(crate) static SCHEDULER_REGISTRY: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>>>> =
        const { std::cell::RefCell::new(None) };
}

/// A fresh, empty registry for one `run_build_scheduler` call.
pub(crate) fn new_scheduler_registry()
-> std::sync::Arc<std::sync::Mutex<std::collections::HashSet<i32>>> {
    std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// RAII guard: while alive, `spawn_trackable` on this thread registers into
/// `registry`. Restores the previous value on drop. Scoped per-thread, never
/// process-wide (a global registry could kill an unrelated test's subprocess).
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

/// `SIGTERM`s every registered process group. Best-effort: an already-exited group
/// yields `ESRCH`, ignored — Portage's non-blocking "return without waiting" semantics.
pub(crate) fn kill_registered_children(
    registry: &std::sync::Mutex<std::collections::HashSet<i32>>,
) {
    let pgids: Vec<i32> = registry.lock().unwrap().iter().copied().collect();
    for pgid in pgids {
        // SAFETY: `pgid` is recorded by this process, so negating it cannot
        // overflow and names a group we own (`kill` takes plain ints).
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
}

/// `cmd.status()` registered in this thread's registry for its duration. Every
/// top-level subprocess a scheduled build spawns goes through this.
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
