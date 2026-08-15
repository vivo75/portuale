//! Child processes registered for the build scheduler to kill.
//!
//! Every scheduled build's top-level subprocess runs in a fresh process group
//! (via `spawn_trackable`), registered here while it runs, so a failing package
//! (without `--keep-going`) kills whole trees with one group-directed signal.
//! Portage: PollScheduler.py, _terminate_tasks; Scheduler.py, _terminate_tasks
//!
//! One registry per `run_build_scheduler` call, shared with its worker threads via
//! a thread-local pointer (not a threaded-through parameter). Safe because no phase
//! body ever `tokio::spawn`s an inner task: `block_on` drives the future on the
//! calling thread, the one the guard was set on. Threads without a guard (tests,
//! standalone runs, single-job builds) never register, so they can't be killed.
//!
//! Termination is sticky: `kill_registered_children` marks the registry, so a
//! worker that had no child at that instant (between phases, still resolving)
//! has its *next* spawn killed at once instead of escaping into a 120 s sleep
//! nobody will ever signal. Portage's equivalent is the `_terminated_tasks`
//! gate (Scheduler.py): after termination no new work is scheduled and every
//! live task is cancelled, so nothing outlives the failure.
thread_local! {
    /// The current thread's registry, if any; see above.
    pub(crate) static SCHEDULER_REGISTRY: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<SchedulerRegistry>>>> =
        const { std::cell::RefCell::new(None) };
}

/// The kill set of one `run_build_scheduler` call: the live process groups
/// plus whether termination was already requested.
pub(crate) struct SchedulerRegistry {
    terminated: bool,
    pgids: std::collections::HashSet<i32>,
}

/// `cmd.status()` registered in this thread's registry for its duration. Every
/// top-level subprocess a scheduled build spawns goes through this. A spawn
/// that lands after the run was terminated (the worker was between phases
/// when the failure hit) is killed at once, so it can never outlive the run.
pub(crate) fn spawn_trackable(
    cmd: &mut std::process::Command,
) -> std::io::Result<std::process::ExitStatus> {
    use std::os::unix::process::CommandExt as _;
    cmd.process_group(0);
    let mut child = cmd.spawn()?;
    let pgid = child.id() as i32;
    let registry = SCHEDULER_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = &registry {
        let terminated = {
            let mut registry = reg.lock().unwrap();
            registry.pgids.insert(pgid);
            registry.terminated
        };
        if terminated {
            // Best-effort like the bulk kill above: the group exists (this
            // process just spawned its leader), an already-exited one reports
            // `ESRCH` and there is nothing to kill.
            // SAFETY: as above.
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
        }
    }
    let status = child.wait();
    if let Some(reg) = &registry {
        reg.lock().unwrap().pgids.remove(&pgid);
    }
    status
}
