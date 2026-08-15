//! Ebuild candidates for a package.

use crate::*;

/// Which kind of package this `Candidate` is -- Portage's `pkg.type_name`.
/// An ebuild's USE is computed dynamically; a binary's was baked at build
/// time (`binary_use`). `--pretend` prints `[ebuild` vs. `[binary`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    Ebuild,
    Binary,
}
