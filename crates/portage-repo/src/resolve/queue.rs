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

/// One BFS-queued dependency-walk item: the atom text, its depth (see `Deep`),
/// the `(category, package)` that pushed it if any (`None` for a top-level atom,
/// consulted for `GraphEntry::required_by`), and the atom's own *unevaluated* text
/// when `evaluate_atom_conditionals` rewrote it (consulted only by
/// `--autounmask-use`'s `opt?`/REQUIRED_USE-conditional suggestion to recover the
/// original conditional form).
// Portage: _show_unsatisfied_dep atom.unevaluated_atom
#[derive(Clone)]
pub(crate) struct QueueItem {
    pub(crate) atom: String,
    pub(crate) depth: u32,
    pub(crate) owner: Option<(String, String)>,
    /// The *expanding* entry's own root flag: the owner half of the
    /// root-qualified edge this item records when it resolves. `false` for a
    /// directly-requested top-level atom.
    pub(crate) owner_targets_running_root: bool,
    pub(crate) unevaluated: Option<String>,
    /// Which root this atom resolves against: `true` only when the running root
    /// differs from the target root (otherwise a strict no-op); top-level atoms
    /// are always target-rooted.
    // Portage: depgraph.py, _add_pkg_deps five-group deps queue
    pub(crate) targets_running_root: bool,
    /// The edge from `owner` to this atom is an unsatisfied **build-time** dep
    /// (`DEPEND`/`BDEPEND`) with no run-time alternative: unbreakable in the
    /// cycle fallback, and an all-such cycle is the fatal `circular dependencies`
    /// error. `false` for top-level atoms, the `--deep` walk, and anything with
    /// a run-time alternative.
    // Portage: DepPriority buildtime && !runtime; _serialize_tasks ignore_priority scan
    pub(crate) buildtime_hard: bool,
    /// The atom was chosen out of an `|| ( ... )` group (see `or_group_universe`).
    /// `false` for plain deps, top-level atoms, and the auto-replace seeds.
    pub(crate) from_disjunction: bool,
}

/// Backtracking: `(cat, pkg)` targeted by an atom -> every puller this pass, as
/// `(parent_cat, parent_pkg, parent_version, atom_text)`. A top-level atom is
/// recorded with empty parent fields.
pub(crate) type SlotPullers = HashMap<(String, String), Vec<(String, String, String, String)>>;
