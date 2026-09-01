//! Replaying `_create_graph` for fail-fast passes.

use crate::merge_order::*;

/// The USE state `replay_create_graph` needs for one entry: declared `IUSE`, the
/// enabled flags before any autounmask change, and the flags no change can
/// touch. `None` for entries whose USE cannot change (installed, binary, missing).
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayUse {
    pub iuse: HashSet<String>,
    pub enabled: HashSet<String>,
    pub fixed: HashSet<String>,
}
