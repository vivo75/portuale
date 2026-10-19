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
    /// Leaders of the sessions started for `PhasePlacement::NewSession`
    /// children (pgid == pid); signalled as a group.
    pgids: std::collections::HashSet<i32>,
    /// `PhasePlacement::ParentGroup` children: signalled by pid only, as
    /// their group is portuale's own.
    pids: std::collections::HashSet<i32>,
}

/// Where a phase's process tree lives relative to the terminal.
/// Portage: pid-ns-init, `os.setsid()`; doebuild.py, `_global_pid_phases`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhasePlacement {
    /// Own session and process group: `^C` and terminal job-control signals
    /// reach portuale only, which kills the group through the registry.
    NewSession,
    /// portuale's own (foreground) process group: the phase may use the
    /// terminal, and is killed by pid only.
    ParentGroup,
}

impl PhasePlacement {
    /// `config`, `preinst`, `prerm`, `postinst` and `postrm` stay in the
    /// parent's group, every other script gets its own session.
    pub(crate) fn for_phase(phase: &str) -> Self {
        match phase {
            "config" | "preinst" | "prerm" | "postinst" | "postrm" => Self::ParentGroup,
            _ => Self::NewSession,
        }
    }
}

/// A fresh, empty registry for one `run_build_scheduler` call.
pub(crate) fn new_scheduler_registry() -> std::sync::Arc<std::sync::Mutex<SchedulerRegistry>> {
    std::sync::Arc::new(std::sync::Mutex::new(SchedulerRegistry {
        terminated: false,
        pgids: std::collections::HashSet::new(),
        pids: std::collections::HashSet::new(),
    }))
}

/// RAII guard: while alive, `spawn_trackable` on this thread registers into
/// `registry`. Restores the previous value on drop. Scoped per-thread, never
/// process-wide (a global registry could kill an unrelated test's subprocess).
pub(crate) struct SchedulerRegistryGuard(
    Option<std::sync::Arc<std::sync::Mutex<SchedulerRegistry>>>,
);

impl Drop for SchedulerRegistryGuard {
    fn drop(&mut self) {
        SCHEDULER_REGISTRY.with(|cell| *cell.borrow_mut() = self.0.take());
    }
}

pub(crate) fn scope_scheduler_registry(
    registry: std::sync::Arc<std::sync::Mutex<SchedulerRegistry>>,
) -> SchedulerRegistryGuard {
    let previous = SCHEDULER_REGISTRY.with(|cell| cell.borrow_mut().replace(registry));
    SchedulerRegistryGuard(previous)
}

/// Marks the registry terminated and `SIGTERM`s every registered process
/// group. Best-effort: an already-exited group yields `ESRCH`, ignored —
/// Portage's non-blocking "return without waiting" semantics
/// (PollScheduler.py, _terminate_tasks; AsynchronousTask.py, cancel).
pub(crate) fn kill_registered_children(registry: &std::sync::Mutex<SchedulerRegistry>) {
    let (pgids, pids): (Vec<i32>, Vec<i32>) = {
        let mut registry = registry.lock().unwrap();
        registry.terminated = true;
        (
            registry.pgids.iter().copied().collect(),
            registry.pids.iter().copied().collect(),
        )
    };
    for pid in pids {
        // SAFETY: plain ints; `pid` is a child this process spawned.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    for pgid in pgids {
        // SAFETY: `pgid` is recorded by this process, so negating it cannot
        // overflow and names a group we own (`kill` takes plain ints).
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
    }
}

/// `cmd.status()` registered in this thread's registry for its duration. Every
/// top-level subprocess a scheduled build spawns goes through this. A spawn
/// that lands after the run was terminated (the worker was between phases
/// when the failure hit) is killed at once, so it can never outlive the run.
pub(crate) fn spawn_trackable(
    cmd: &mut std::process::Command,
    placement: PhasePlacement,
) -> std::io::Result<std::process::ExitStatus> {
    use std::os::unix::process::CommandExt as _;
    if placement == PhasePlacement::NewSession {
        // SAFETY: only `setsid`, which is async-signal-safe, runs between
        // fork and exec. It also makes the child its own group leader.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn()?;
    let pgid = child.id() as i32;
    super::signal_exit::note_spawned(pgid, placement);
    let registry = SCHEDULER_REGISTRY.with(|cell| cell.borrow().clone());
    if let Some(reg) = &registry {
        let terminated = {
            let mut registry = reg.lock().unwrap();
            match placement {
                PhasePlacement::NewSession => registry.pgids.insert(pgid),
                PhasePlacement::ParentGroup => registry.pids.insert(pgid),
            };
            registry.terminated
        };
        if terminated {
            // Best-effort like the bulk kill above: an already-exited child
            // reports `ESRCH` and there is nothing to kill.
            // SAFETY: as above.
            unsafe {
                match placement {
                    PhasePlacement::NewSession => libc::kill(-pgid, libc::SIGTERM),
                    PhasePlacement::ParentGroup => libc::kill(pgid, libc::SIGTERM),
                };
            }
        }
    }
    let status = child.wait();
    super::signal_exit::note_exited(pgid);
    if let Some(reg) = &registry {
        let mut registry = reg.lock().unwrap();
        registry.pgids.remove(&pgid);
        registry.pids.remove(&pgid);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spawn that lands after termination is killed at once instead of
    /// running to completion: without the sticky flag this sleeps out the
    /// full minute and only then fails the bound.
    #[test]
    fn a_spawn_after_termination_is_killed_at_once() {
        let registry = new_scheduler_registry();
        // The failure lands while this worker has no child yet (between
        // phases): the one-shot kill finds nothing to signal.
        kill_registered_children(&registry);
        let _guard = scope_scheduler_registry(registry);
        let started = std::time::Instant::now();
        let status = spawn_trackable(
            std::process::Command::new("sleep").arg("60"),
            PhasePlacement::NewSession,
        )
        .expect("spawn sleep");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "a post-termination spawn must be killed, not waited out (took {:?})",
            started.elapsed()
        );
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGTERM),
            "the late spawn must die by the termination signal, not exit: {status:?}"
        );
    }
}
