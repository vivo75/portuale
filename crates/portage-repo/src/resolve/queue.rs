//! The walk queue.

use crate::*;

/// `--deep`/`-D`: how far past an already-installed, already-satisfied package to
/// keep walking dependencies. The default (`NotRequested`) never walks them (the
/// pre-existing hardcoded behavior); a bare `--deep` is unlimited; `--deep=N`
/// bounds the depth (`--deep=0` is indistinguishable from absent, so `Bounded(0)`
/// is never constructed; negative values are rejected at CLI parsing).
// Portage: lib/_emerge/main.py --deep; create_depgraph_params.py myparams[deep];
// depgraph.py _too_deep/_add_pkg
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deep {
    #[default]
    NotRequested,
    Unlimited,
    Bounded(u32),
}
