//! The resolver trait and solver selection.

use crate::*;

/// Which dependency-solving algorithm answers a [`ResolveRequest`].
/// Portuale-only (Portage's `emerge` has no `--solver`): `Portage` is the
/// backtracking walk; `PubGrub`/`Resolvo` drive lu-zero's bridges (see
/// `solver_bridge.rs`). The selection rides inside [`ResolveRequest`] so a
/// [`Resolver`] stays a `Box<dyn Resolver>` picked at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SolverKind {
    /// Portuale's backtracking graph walk (`BacktrackingResolver`).
    #[default]
    Portage,
    /// PubGrub version solving over the same repo facts.
    PubGrub,
    /// resolvo CDCL SAT solving over the same repo facts.
    Resolvo,
}

impl SolverKind {
    /// The canonical `--solver=<value>` spelling, for help text and argv
    /// forwarding.
    pub fn as_str(self) -> &'static str {
        match self {
            SolverKind::Portage => "portage",
            SolverKind::PubGrub => "pubgrub",
            SolverKind::Resolvo => "resolvo",
        }
    }
}

impl std::fmt::Display for SolverKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
