//! `^C` / `SIGTERM` while a build runs: say so, stop the phases, exit 128+N.
//!
//! Phases live in their own session, so a terminal `^C` reaches portuale only.
//! While an [`ExitOnSignalGuard`] is alive a watcher thread waits for the
//! signal (the handler itself only stores the number), prints Portage's
//! message, `SIGTERM`s every phase `spawn_trackable` has running and, once
//! they are gone, exits with `128 + signal`.
//! Portage: Scheduler.py, merge (`sighandler`); actions.py, `emergeexitsig`

use super::scheduler_registry::PhasePlacement;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

static RECEIVED: AtomicI32 = AtomicI32::new(0);

type LiveMap = Mutex<std::collections::HashMap<i32, PhasePlacement>>;

/// Phase children alive right now, by pid, with where they live.
fn live() -> &'static LiveMap {
    static LIVE: OnceLock<LiveMap> = OnceLock::new();
    LIVE.get_or_init(Default::default)
}

pub(super) fn note_spawned(pid: i32, placement: PhasePlacement) {
    live().lock().unwrap().insert(pid, placement);
}

pub(super) fn note_exited(pid: i32) {
    live().lock().unwrap().remove(&pid);
}

extern "C" fn on_signal(sig: libc::c_int) {
    RECEIVED.store(sig, Ordering::SeqCst);
}

/// How long a signalled phase may take to go away before portuale exits anyway.
const GRACE: std::time::Duration = std::time::Duration::from_secs(60);

/// While alive, `SIGINT` and `SIGTERM` end the run as described above.
pub(crate) struct ExitOnSignalGuard {
    old_int: libc::sigaction,
    old_term: libc::sigaction,
    stop: std::sync::Arc<AtomicBool>,
    watcher: Option<std::thread::JoinHandle<()>>,
}

impl ExitOnSignalGuard {
    /// Installs the handlers; `exit` receives the exit code (the real one is
    /// `std::process::exit`).
    pub(crate) fn install(exit: fn(i32)) -> Self {
        RECEIVED.store(0, Ordering::SeqCst);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        // SAFETY: zeroed `sigaction` structs are valid; the handler only
        // stores into an atomic.
        let (old_int, old_term) = unsafe {
            let mut new: libc::sigaction = std::mem::zeroed();
            new.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            libc::sigemptyset(&mut new.sa_mask);
            let mut old_int: libc::sigaction = std::mem::zeroed();
            let mut old_term: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &new, &mut old_int);
            libc::sigaction(libc::SIGTERM, &new, &mut old_term);
            (old_int, old_term)
        };
        let watcher_stop = stop.clone();
        let watcher = std::thread::spawn(move || {
            while !watcher_stop.load(Ordering::SeqCst) {
                let sig = RECEIVED.load(Ordering::SeqCst);
                if sig != 0 {
                    // SAFETY: plain ints; a second signal must not interrupt the shutdown.
                    unsafe {
                        libc::signal(libc::SIGINT, libc::SIG_IGN);
                        libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    }
                    terminate_and_exit(sig, live(), exit);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        Self {
            old_int,
            old_term,
            stop,
            watcher: Some(watcher),
        }
    }
}

impl Drop for ExitOnSignalGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        // SAFETY: restores the actions saved by `install`.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.old_int, std::ptr::null_mut());
            libc::sigaction(libc::SIGTERM, &self.old_term, std::ptr::null_mut());
        }
    }
}

fn terminate_and_exit(sig: i32, live: &LiveMap, exit: fn(i32)) {
    eprint!("\n\nExiting on signal {sig}\n");
    let started = std::time::Instant::now();
    loop {
        let alive: Vec<(i32, PhasePlacement)> =
            live.lock().unwrap().iter().map(|(p, k)| (*p, *k)).collect();
        if alive.is_empty() || started.elapsed() > GRACE {
            break;
        }
        for (pid, placement) in alive {
            // SAFETY: plain ints; `pid` is a child this process spawned and a
            // new-session child is its own group leader.
            unsafe {
                match placement {
                    PhasePlacement::NewSession => libc::kill(-pid, libc::SIGTERM),
                    PhasePlacement::ParentGroup => libc::kill(pid, libc::SIGTERM),
                };
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    exit(128 + sig);
}

#[cfg(test)]
mod tests {
    use super::*;

    static EXIT_CODE: AtomicI32 = AtomicI32::new(-1);

    fn record_exit(code: i32) {
        EXIT_CODE.store(code, Ordering::SeqCst);
    }

    /// Shutting down stops the registered phase's whole group and asks for
    /// exit code 128+N. Uses a private map and a private child, so no other
    /// test's subprocess is signalled.
    #[test]
    fn terminating_stops_the_registered_phase_and_exits_128_plus_n() {
        use std::os::unix::process::CommandExt as _;
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("60");
        // SAFETY: only `setsid` runs between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn sleep");
        let map = LiveMap::default();
        map.lock()
            .unwrap()
            .insert(child.id() as i32, PhasePlacement::NewSession);
        let waiter = std::thread::spawn(move || child.wait().unwrap());
        // Mirrors `spawn_trackable`: the entry goes away once the child is reaped.
        let started = std::time::Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| terminate_and_exit(libc::SIGTERM, &map, record_exit));
            while !waiter.is_finished() {
                assert!(started.elapsed().as_secs() < 20, "the phase survived");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            map.lock().unwrap().clear();
        });
        let status = waiter.join().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(libc::SIGTERM)
        );
        assert_eq!(EXIT_CODE.load(Ordering::SeqCst), 128 + libc::SIGTERM);
    }
}
