//! Fan a list of independent jobs out over the cores.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Jobs a thread takes at a time.
const CHUNK: usize = 16;

/// Runs `one` on every job on up to one thread per core. Each thread owns a
/// local value from `make_local`, passed to every job it runs; the locals are
/// returned in no particular order, so the caller must merge them with a
/// commutative step (union, then sort).
pub(crate) fn for_each_job<T, L>(
    jobs: &[T],
    make_local: impl Fn() -> L + Sync,
    one: impl Fn(&T, &mut L) + Sync,
) -> Vec<L>
where
    T: Sync,
    L: Send,
{
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(jobs.len().div_ceil(CHUNK))
        .max(1);
    let next = AtomicUsize::new(0);
    let work = || {
        let mut local = make_local();
        loop {
            let start = next.fetch_add(CHUNK, Ordering::Relaxed);
            if start >= jobs.len() {
                return local;
            }
            for job in &jobs[start..jobs.len().min(start + CHUNK)] {
                one(job, &mut local);
            }
        }
    };
    if threads == 1 {
        return vec![work()];
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads).map(|_| s.spawn(work)).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_job_runs_exactly_once_whatever_the_thread_count() {
        let jobs: Vec<usize> = (0..1000).collect();
        let locals = for_each_job(&jobs, Vec::new, |j, seen: &mut Vec<usize>| seen.push(*j));
        let mut all: Vec<usize> = locals.into_iter().flatten().collect();
        all.sort_unstable();
        assert_eq!(all, jobs);
    }

    #[test]
    fn no_jobs_still_gives_one_empty_local() {
        let locals = for_each_job(&[] as &[u8], || 0u8, |_, _| {});
        assert_eq!(locals, vec![0]);
    }
}
