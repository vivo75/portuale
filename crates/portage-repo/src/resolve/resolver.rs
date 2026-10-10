//! The resolver trait and solver selection.

use crate::*;

/// Which dependency-solving algorithm answers a [`ResolveRequest`].
///
/// Portuale-only (real `emerge` has no `--solver`): `Portage` is the
/// default and the backtracking walk real Portage runs;
/// `PubGrub`/`Resolvo` drive lu-zero's `portage-atom-pubgrub` /
/// `portage-atom-resolvo` bridges (see `solver_bridge.rs`) and are
/// Rust-only. The selection rides inside [`ResolveRequest`] so a
/// [`Resolver`] stays a `Box<dyn Resolver>` picked at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SolverKind {
    /// Portuale's own backtracking graph walk (`BacktrackingResolver`).
    #[default]
    Portage,
    /// PubGrub version solving over the same repo facts.
    PubGrub,
    /// resolvo CDCL SAT solving over the same repo facts.
    Resolvo,
}

impl SolverKind {
    /// Parse a `--solver=<value>` token. `None` for anything outside the
    /// three accepted values (the caller reports the usage error).
    pub fn parse(value: &str) -> Option<SolverKind> {
        match value {
            "portage" => Some(SolverKind::Portage),
            "pubgrub" => Some(SolverKind::PubGrub),
            "resolvo" => Some(SolverKind::Resolvo),
            _ => None,
        }
    }

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

/// A dependency-resolution strategy: [`ResolveRequest`] in, merge-ordered
/// [`GraphResult`] out (or a hard-error string). The default implementation
/// is [`BacktrackingResolver`]; alternate architectures plug in wholesale
/// via [`active_resolver_for`] without touching a call site.
pub trait Resolver {
    fn resolve(&self, req: &ResolveRequest) -> Result<GraphResult, Error>;
}

/// The default resolver: a single-pass BFS graph walk wrapped in real
/// `_emerge/resolver/backtracking.py`'s retry loop. See `backtracking_resolve`.
pub struct BacktrackingResolver;

impl Resolver for BacktrackingResolver {
    fn resolve(&self, req: &ResolveRequest) -> Result<GraphResult, Error> {
        backtracking_resolve(req)
    }
}

/// The resolver portuale uses -- a hook for runtime selection between
/// resolver implementations, currently always [`BacktrackingResolver`].
pub fn active_resolver() -> Box<dyn Resolver> {
    active_resolver_for(SolverKind::Portage)
}

/// Pick the [`Resolver`] for one [`SolverKind`] (`--solver=`): the
/// backtracking walk for `Portage`, lu-zero's PubGrub / resolvo bridges
/// (see `solver_bridge.rs`) for the other two. The parked engines only
/// implement version solving over repo facts: they ignore `--onlydeps`
/// and its `--onlydeps-with-rdeps` / `--onlydeps-with-ideps` root
/// filtering (like `--nodeps` / `--buildpkgonly`) -- only the
/// backtracking walk above applies them.
pub fn active_resolver_for(kind: SolverKind) -> Box<dyn Resolver> {
    match kind {
        SolverKind::Portage => Box::new(BacktrackingResolver),
        SolverKind::PubGrub => Box::new(solver_bridge::PubGrubResolver),
        SolverKind::Resolvo => Box::new(solver_bridge::ResolvoResolver),
    }
}
