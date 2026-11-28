//! Ranking of settled passes for `PORTUALE_BACKTRACK_EXPLORE`.

use crate::resolve::backtracker::Backtracker;
use crate::resolve::outcome::PretendOutcome;
use crate::resolve::result::GraphResult;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How good a settled pass is; lower is better, fields compared in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Score {
    pub(crate) unresolved: usize,
    pub(crate) skipped: usize,
    pub(crate) removals: usize,
    pub(crate) downgrades: usize,
    pub(crate) rebuilds: usize,
    pub(crate) packages: usize,
}

/// One settled pass seen while exploring.
struct Settled {
    node: usize,
    depth: u32,
    score: Score,
    at: Duration,
}

/// `PORTUALE_BACKTRACK_EXPLORE=<n>`: measure what exploring up to `n`
/// settled passes would give, without changing the returned result. The
/// first settle is kept and returned; the rest only feed the report.
pub(crate) struct Explore {
    limit: usize,
    started: Instant,
    /// Per search node: its parent and how long its pass took.
    passes: HashMap<usize, (Option<usize>, Duration)>,
    settled: Vec<Settled>,
    /// When the pass loop ended; what follows is the final re-pass.
    loop_end: Option<Duration>,
    last_mark: Duration,
    pub(crate) first: Option<GraphResult>,
}

impl Explore {
    /// `None` unless the variable holds a number of at least 2.
    pub(crate) fn from_env() -> Option<Self> {
        let limit: usize = std::env::var("PORTUALE_BACKTRACK_EXPLORE")
            .ok()?
            .parse()
            .ok()
            .filter(|n| *n >= 2)?;
        Some(Explore {
            limit,
            started: Instant::now(),
            passes: HashMap::new(),
            settled: Vec::new(),
            loop_end: None,
            last_mark: Duration::ZERO,
            first: None,
        })
    }

    /// The pass of search node `node` took `took`.
    pub(crate) fn pass_done(&mut self, bt: &Backtracker, took: Duration) {
        if let Some(n) = bt.current {
            self.passes.insert(n, (bt.nodes[n].parent, took));
        }
    }

    /// The pass loop is over.
    pub(crate) fn loop_done(&mut self) {
        let now = self.started.elapsed();
        self.loop_end = Some(now);
        self.last_mark = now;
    }

    /// A step of the tail finished; prints how long it took.
    pub(crate) fn mark(&mut self, step: &str) {
        let now = self.started.elapsed();
        eprintln!(
            "explore tail {step}: {:.3}s",
            (now - self.last_mark).as_secs_f64()
        );
        self.last_mark = now;
    }

    /// Records a settled pass; true when the exploration should stop.
    pub(crate) fn settle(&mut self, bt: &Backtracker, result: GraphResult) -> bool {
        let node = bt.current.unwrap_or(0);
        self.settled.push(Settled {
            node,
            depth: bt.nodes[node].params.depth,
            score: score(&result),
            at: self.started.elapsed(),
        });
        if self.first.is_none() {
            self.first = Some(result);
        }
        self.settled.len() >= self.limit || bt.unexplored.is_empty()
    }

    /// Sum of pass times from the root down to `node`: the wall time a
    /// parallel exploration with unlimited workers needs to reach it.
    fn critical_path(&self, node: usize) -> Duration {
        let mut total = Duration::ZERO;
        let mut at = Some(node);
        while let Some((parent, took)) = at.and_then(|n| self.passes.get(&n)) {
            total += *took;
            at = *parent;
        }
        total
    }

    /// Stderr report, whatever way the search ended: one line per settle,
    /// then the totals every exit shares. `passes_sum` is what a sequential
    /// search spent in passes, `chain_max` the longest root-to-leaf chain
    /// of them (the wall time with unlimited workers), `outside` the rest
    /// (final re-pass, assembly).
    pub(crate) fn report(&self) {
        for (i, s) in self.settled.iter().enumerate() {
            eprintln!(
                "explore settle {i}: node {} depth {} at {:.3}s score {:?}",
                s.node,
                s.depth,
                s.at.as_secs_f64(),
                s.score,
            );
        }
        let total = self.started.elapsed();
        let loop_end = self.loop_end.unwrap_or(total);
        let passes_sum: Duration = self.passes.values().map(|(_, t)| *t).sum();
        let chain_max = self
            .passes
            .keys()
            .map(|n| self.critical_path(*n))
            .max()
            .unwrap_or_default();
        eprintln!(
            "explore: passes {} settles {} total {:.3}s passes_sum {:.3}s chain_max {:.3}s loop_overhead {:.3}s tail {:.3}s",
            self.passes.len(),
            self.settled.len(),
            total.as_secs_f64(),
            passes_sum.as_secs_f64(),
            chain_max.as_secs_f64(),
            loop_end.saturating_sub(passes_sum).as_secs_f64(),
            total.saturating_sub(loop_end).as_secs_f64(),
        );
        let Some(first) = self.settled.first() else {
            return;
        };
        let best = self
            .settled
            .iter()
            .min_by_key(|s| (s.score, s.at))
            .unwrap_or(first);
        eprintln!(
            "explore: t_first {:.3}s t_seq_best {:.3}s t_par_bound {:.3}s best_beats_first {}",
            first.at.as_secs_f64(),
            best.at.as_secs_f64(),
            self.critical_path(best.node).as_secs_f64(),
            best.score < first.score,
        );
    }
}

/// Scores a list of merge-list outcomes plus the counts the list cannot show.
pub(crate) fn score_outcomes<'a>(
    outcomes: impl IntoIterator<Item = &'a PretendOutcome>,
    unresolved: usize,
    skipped: usize,
) -> Score {
    let mut s = Score {
        unresolved,
        skipped,
        ..Score::default()
    };
    for o in outcomes {
        s.packages += 1;
        match o {
            PretendOutcome::NoVisibleCandidate => s.unresolved += 1,
            PretendOutcome::Uninstall { .. } => s.removals += 1,
            PretendOutcome::Downgrade { .. } => s.downgrades += 1,
            PretendOutcome::Reinstall { .. } => s.rebuilds += 1,
            _ => {}
        }
    }
    s
}

/// Scores one settled pass.
pub(crate) fn score(r: &GraphResult) -> Score {
    score_outcomes(
        r.entries.iter().map(|e| &e.outcome),
        r.slot_conflicts.len(),
        r.skipped_updates.len()
            + r.skipped_missing_deps.len()
            + r.held_back_use.len()
            + r.held_back_no_route.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(v: &str) -> PretendOutcome {
        PretendOutcome::New { version: v.into() }
    }
    fn down() -> PretendOutcome {
        PretendOutcome::Downgrade {
            from: "2".into(),
            to: "1".into(),
        }
    }

    #[test]
    fn counts_each_kind_of_cost() {
        let outs = [new("1"), new("2"), down()];
        let s = score_outcomes(&outs, 1, 2);
        assert_eq!(
            s,
            Score {
                unresolved: 1,
                skipped: 2,
                removals: 0,
                downgrades: 1,
                rebuilds: 0,
                packages: 3,
            }
        );
    }

    #[test]
    fn an_unresolved_conflict_outweighs_any_number_of_downgrades() {
        let many_downgrades = [down(), down(), down()];
        let a = score_outcomes(&many_downgrades, 0, 0);
        let b = score_outcomes(&[new("1")], 1, 0);
        assert!(a < b);
    }

    #[test]
    fn fewer_packages_wins_a_tie() {
        let a = score_outcomes(&[new("1")], 0, 0);
        let b = score_outcomes(&[new("1"), new("2")], 0, 0);
        assert!(a < b);
    }
}
