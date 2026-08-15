//! Process-wide switches the CLI sets once per run.

use crate::*;

/// `--package-moves`: whether `profiles/updates/` `move`/`slotmove` directives apply (default on).
/// Process-global so it needn't thread through the resolver.
// Portage: main.py, actions.py
pub(crate) static PACKAGE_MOVES_ENABLED: AtomicBool = AtomicBool::new(true);
