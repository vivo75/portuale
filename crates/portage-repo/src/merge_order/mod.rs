//! Portage's `_serialize_tasks` merge-order scheduler over a typed digraph.
//!
//! Portage's scheduler is not a topological sort: it is a repeated "pop every
//! currently-eligible leaf" loop over edges carrying a `DepPriority`, with a
//! priority-relaxation ladder, a `_merge_order_bias` pre-sort, and an
//! `asap_nodes` fast path for freshly-unblocked `PDEPEND` children.
//! Portage: _emerge/depgraph.py, _serialize_tasks
//!
//! The graph holds only the forward transitive closure of the merge-bound
//! packages, which reproduces Portage's merge list exactly (see notes).
//!
//! The selection loop runs over an incrementally-maintained leaf frontier
//! (`SerializeFrontier`, a port of `_serialize_frontier.py`); the frontier
//! is pure plumbing, identical with it on or off (pinned by unit tests).

use std::cmp::Reverse;
mod prelude;
pub(crate) use prelude::*;

use std::collections::{BinaryHeap, HashMap, HashSet};

use std::path::Path;
