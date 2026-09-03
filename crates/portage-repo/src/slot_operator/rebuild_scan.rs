//! Rebuilds triggered by slot-operator dependencies.

use crate::*;

/// [`slot_operator_rebuild_scan`]'s result: the consumer `(cat, pkg)`
/// replace set, the `(provider_cpv, consumer_cpv)` display pairs, and the
/// stale-binary `(category, package, version)` triples.
pub(crate) type SlotOpRebuildScan = (
    BTreeSet<(String, String)>,
    Vec<(String, String)>,
    BTreeSet<(String, String, String)>,
);
