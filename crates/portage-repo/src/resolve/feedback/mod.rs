//! Turning a pass's problems into feedback for the next pass.

use crate::*;
use std::ops::ControlFlow;

/// Why one `cat/pkg` version is masked on retry. Readers see the union across
/// reasons; the backtracker consumes them. Portage: backtracking.py, `runtime_pkg_mask`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MaskReason {
    /// The masked version collided in its slot. `parents` are the pulling
    /// `((cat, pkg, ver), atom)` triples whose atom does NOT match it.
    SlotConflict {
        parents: Vec<((String, String, String), String)>,
    },
    /// The masked parent's subtree contained an unsatisfiable dep; `atom` is
    /// that dep's text. Portage also records the root; portuale resolves one root.
    MissingDependency {
        parent: (String, String),
        atom: String,
    },
    /// A non-installed binary whose built slot-operator dep cannot be satisfied the
    /// way it was built. The rendered `neg` carries a `[binary]` marker, so only the
    /// binary-candidate pool filter honours it; every string-constraint reader skips
    /// marked entries. Only the slot-change arm is ported; the update and
    /// unsatisfied probe arms steer a parent the graph already replaces or fails on.
    SlotOperatorBuilt,
}
