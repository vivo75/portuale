//! Graph entries and their edges.

use crate::*;

/// What resolves a blocker row that is not unsolvable. Lets the renderer decide
/// placement without re-deriving it. Portage: depgraph.py, _serialize_tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerSatisfiedBy {
    /// A merge-bound entry replaces the installed match in its own slot.
    Replacement { cp: (String, String), slot: String },
    /// An uninstall task resolves the block: `cpv` is the removed installed package
    /// (the `Uninstall` entry's own cpv); `anchor` is the side selected first -- the
    /// owner for a merging owner, the merge-bound blocked instance for an installed one.
    Uninstall {
        cpv: String,
        anchor: (String, String),
    },
}

/// A blocker atom matching an installed package or another package this run would
/// merge. Purely informational: v1 never resolves or refuses on account of a blocker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerConflict {
    /// The raw blocker atom text, e.g. `"!!dev-libs/foo"`.
    pub atom_str: String,
    /// `true` for a strong (`!!`) blocker, `false` for a weak (`!`) one.
    pub strong: bool,
    pub matched_category: String,
    pub matched_package: String,
    pub matched_version: String,
    /// The block cannot be resolved (installed, unreplaced, unmergable): counted as
    /// unsatisfied, rendered as an error, exit 1. Portage: _serialize_tasks.
    pub unsolvable: bool,
    /// What resolves this row. Only `Replacement` is classified so far; `None` covers
    /// unsolvable and not-yet-classified rows alike, hidden until B2 models scheduling.
    pub satisfied_by: Option<BlockerSatisfiedBy>,
    /// Whether tree-mode serialization solves this row. Set resolve-side; read only by
    /// `--tree` display and counters, never by flat display, `--json` or exit codes.
    pub tree_scheduled_uninstall: bool,
}

/// One installed package named in an `[old-ver]` column. `repo` is `"__unknown__"`
/// when absent. Portage: output.py, convert_myoldbest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRef {
    pub version: String,
    pub slot: String,
    pub sub_slot: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEntry {
    pub category: String,
    pub package: String,
    pub outcome: PretendOutcome,
    /// Only ever non-empty for entries that would newly merge or upgrade.
    pub blockers: Vec<BlockerConflict>,
    /// Only `Some` for entries a slot can be tracked for; else `None`.
    pub slot: Option<String>,
    /// The candidate's own sub-slot and repo name, for the `-pv` decoration.
    /// Portage: output.py, _append_slot / _append_repository.
    pub sub_slot: Option<String>,
    pub repo_name: Option<String>,
    /// The installed version(s) this entry replaces (see notes). Portage: convert_myoldbest.
    pub oldbest: Vec<InstalledRef>,
    /// This package's own IUSE flags with resolved enabled state, sorted. Only ever
    /// non-empty for entries with readable md5-cache metadata. Always computed.
    pub use_flags_display: Vec<(String, bool)>,
    /// The same flags as `-pv` shows them: plain `USE` group plus per-`USE_EXPAND`
    /// groups (prefix stripped, hidden groups omitted); only non-empty groups appear.
    /// Portage: output.py, _display_use / map_to_use_expand / _create_use_string.
    ///
    /// The `-pv` form (every flag shown); see `use_expand_display_p` for `-p`.
    pub use_expand_display: Vec<(String, String)>,
    /// The plain `-p` form: unchanged and removed-from-IUSE flags omitted, so often
    /// empty for non-`New`; equals `use_expand_display` for a `New`.
    pub use_expand_display_p: Vec<(String, String)>,
    /// The one-character mask column (`#` hard-masked, `~` testing-only, `*` other).
    /// `None` for an ordinary candidate. Always rendered. Portage: gen_mask_str.
    pub keyword_mask: Option<char>,
    /// The `S` column: `true` for a `New` entry installed in some other slot; always
    /// `false` otherwise. Portage: output.py, _get_installed_best.
    pub new_slot: bool,
    /// The `I` column: evaluated `PROPERTIES` contains `interactive`, merge-bound only.
    pub interactive: bool,
    /// The `f`/`F` column: merge-bound *ebuild* entry with `fetch` in evaluated
    /// `RESTRICT`; only ever `false` for a binary. See `fetch_restrict_satisfied`.
    pub fetch_restrict: bool,
    /// Only meaningful with `fetch_restrict`: every distfile already in `DISTDIR` at
    /// its `Manifest` size (`f`); unparsable `SRC_URI` / missing entry counts as `F`.
    pub fetch_restrict_satisfied: bool,
    /// Per-file `(filename, size)` of distfiles missing from `DISTDIR`, so shared
    /// distfiles dedup once. Empty for non-merge-bound entries, binaries, and
    /// unparsable `SRC_URI` / incomplete `Manifest`. Portage: output.py, _calc_size.
    pub download_files: Vec<(String, u64)>,
    /// Every `(category, package)` that reached this entry via its own deps (sorted,
    /// deduplicated). Diamonds list every parent: recording is independent of the
    /// BFS's resolve-again dedup.
    pub required_by: Vec<(String, String)>,
    /// Ebuild or binary, meaningful for merge-bound entries only. See `CandidateSource`.
    pub source: CandidateSource,
    /// The `--json` state-change trace. `Default` for entries that never pick a fresh
    /// candidate. See `VisibilityProvenance`.
    pub provenance: VisibilityProvenance,
    /// `--autounmask` keyword suggestion for a dependency's own `NoVisibleCandidate`:
    /// `(version, keyword)` of the best `KEYWORDS`-masked candidate. Else `None`.
    pub keyword_suggestion: Option<(String, String)>,
    /// `--autounmask-use` suggestion: `(version, [(flag, desired_state)])` from the
    /// best plain-USE-mismatch candidate, under the same gating. Else `None`.
    pub use_suggestion: Option<(String, Vec<(String, bool)>)>,
    /// Flipping the *requesting parent's* own flag, not the candidate's. Doubles as the
    /// abort renderer's skip signal and `--json` provenance. See `suggested_parent_use_candidate`.
    pub parent_use_suggestion: Option<ParentUseSuggestion>,
    /// `true` for an entry needing building under the running root, not the target
    /// `ROOT`. `blockers` stay empty for it (a cut); USE displays repaint at display
    /// time. Residual: top-level `IDEPEND` still resolves against `ROOT`. Portage:
    /// depgraph.py, running_root_satisfies_atom.
    pub targets_running_root: bool,
    /// The `g` column: merge-bound `--getbinpkg` *remote* binary not yet in `$PKGDIR`.
    pub remote_binary: bool,
    /// The binary candidate's `BUILD_ID`, appended to the displayed version. Else `None`.
    pub build_id: Option<String>,
    /// This entry's own direct dependencies in discovery order (`RDEPEND`, `IDEPEND`,
    /// `PDEPEND`, `DEPEND`, `BDEPEND`), each with its key-implied `DepPriority`. The
    /// whole input to merge-order serialization (edges plus `.order`). Empty for
    /// `NoVisibleCandidate` and synthetic entries (position + `required_by` fallback).
    /// Portage: depgraph.py, _add_pkg_deps / _serialize_tasks.
    pub deps: Vec<DepEdge>,
    /// Index in pre-sort BFS discovery order, stamped just before sorting; display
    /// edges enumerate in this order. `0` for entries fabricated outside a resolve.
    pub discovery: usize,
}

/// A root-qualified `cat/pkg`: the same cp merged in both roots is two nodes, so every
/// node-keyed map uses this. Portage: depgraph.py (Package.root_config).
pub type RootCp = (bool, String, String);

/// Per walked dependency edge, whether it was ever pulled build-time-only and/or via
/// a softer route. Unbreakable in a cycle when hard-only (or under an unsatisfied
/// non-optional slot-operator priority the token text cannot see). Root-qualified on
/// both ends. Built in `resolve_pretend_graph`; read by merge-order and cycles.
/// Portage: resolver/package_tracker.py (PackageTracker).
pub(crate) type EdgeKindMap = HashMap<(RootCp, RootCp), (bool, bool)>;

/// Which root an edge's child lands in: `RDEPEND`/`PDEPEND` stay with the owner;
/// `IDEPEND`/`DEPEND`/`BDEPEND` go to the running root. Key ordinals are
/// `DepEdge::key`'s: 0=`RDEPEND`, 1=`IDEPEND`, 2=`PDEPEND`, 3=`DEPEND`, 4=`BDEPEND`.
/// Portage: depgraph.py, _add_pkg_deps.
pub(crate) fn dep_child_root(owner_targets_running_root: bool, key: u8) -> bool {
    match key {
        // `RDEPEND` / `PDEPEND`: the owner's own root (`myroot`).
        0 | 2 => owner_targets_running_root,
        // `IDEPEND` / `DEPEND` / `BDEPEND`: the running root.
        1 | 3 | 4 => true,
        // Unreachable; kept owner-local like the run-time keys.
        _ => owner_targets_running_root,
    }
}

/// Attribute a skipped-update row's missed node and merge parent to their resolving
/// roots, for the per-node `to '<root>'` suffix and per-root USE. The parent sits in
/// its entry's root (first merge-match wins); the miss sits where the rejecting
/// parent's edge points (buildtime off a target parent: the running root). Both are
/// `false` on a single-root run; installed parents attribute target, not derived.
pub(crate) fn skipped_update_roots(
    entries: &[GraphEntry],
    missed: (&str, &str),
    parent: (&str, &str, &str),
    atom: &str,
) -> (bool, bool) {
    fn merge_version(e: &GraphEntry) -> Option<&str> {
        match &e.outcome {
            PretendOutcome::New { version } => Some(version.as_str()),
            PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => {
                Some(to.as_str())
            }
            PretendOutcome::Reinstall { version, .. } => Some(version.as_str()),
            _ => None,
        }
    }
    let (cc, cp2, cv) = parent;
    let parent = entries
        .iter()
        .find(|e| {
            e.category.as_str() == cc
                && e.package.as_str() == cp2
                && merge_version(e).is_some_and(|v| v == cv)
        })
        .or_else(|| {
            entries.iter().find(|e| {
                e.category.as_str() == cc && e.package.as_str() == cp2 && merge_version(e).is_some()
            })
        });
    let Some(parent) = parent else {
        return (false, false);
    };
    let consumer_running = parent.targets_running_root;
    let edge = parent
        .deps
        .iter()
        .find(|d| d.category == missed.0 && d.package == missed.1 && d.atom == atom)
        .or_else(|| {
            parent
                .deps
                .iter()
                .find(|d| d.category == missed.0 && d.package == missed.1)
        });
    let cross_active = entries.iter().any(|e| e.targets_running_root);
    let missed_running = match edge {
        Some(d) => cross_active && dep_child_root(parent.targets_running_root, d.key),
        None => false,
    };
    (missed_running, consumer_running)
}
