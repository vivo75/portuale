//! A dependency with no version to merge: feedback for backtracking, and its unsatisfied entry.

use crate::resolve::pass::*;
use portage_dep::Atom;
use portage_profile::Config;
use std::ops::ControlFlow;

/// What [`compute_nvc_suggestions`] hands back to its caller.
pub(crate) struct NvcSuggestionsOut {
    pub(crate) keyword_suggestion: Option<(String, String)>,
    pub(crate) use_suggestion: Option<(String, Vec<(String, bool)>)>,
    pub(crate) parent_use_suggestion: Option<ParentUseSuggestion>,
}

/// The values [`compute_nvc_suggestions`] reads from its caller.
pub(crate) struct ComputeNvcSuggestionsArgs<'a> {
    pub(crate) outcome: &'a PretendOutcome,
    pub(crate) bp: &'a BacktrackParams,
    pub(crate) ctx: &'a ResolveCtx<'a>,
    pub(crate) key: &'a (String, String),
    pub(crate) atom: &'a Atom,
    pub(crate) config: &'a Config,
    pub(crate) owner: Option<&'a (String, String)>,
    pub(crate) unevaluated_atom: &'a Option<String>,
    pub(crate) entries: &'a [GraphEntry],
}

/// The values [`push_unresolved_entry`] reads from its caller.
pub(crate) struct PushUnresolvedEntryArgs<'a> {
    pub(crate) key: (String, String),
    pub(crate) outcome: PretendOutcome,
    pub(crate) already_installed_deps: Vec<DepEdge>,
    pub(crate) keyword_suggestion: Option<(String, String)>,
    pub(crate) use_suggestion: Option<(String, Vec<(String, bool)>)>,
    pub(crate) parent_use_suggestion: Option<ParentUseSuggestion>,
    pub(crate) item_targets_running_root: bool,
    pub(crate) state: &'a mut PassState,
}

/// Push the unsatisfied entry carrying its resolving root and suggestions.
pub(crate) fn push_unresolved_entry(args: PushUnresolvedEntryArgs<'_>) {
    let PushUnresolvedEntryArgs {
        key,
        outcome,
        already_installed_deps,
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
        item_targets_running_root,
        state,
    } = args;
    state.entries.push(GraphEntry {
        discovery: 0,
        category: key.0,
        package: key.1,
        outcome,
        blockers: Vec::new(),
        slot: None,
        sub_slot: None,
        repo_name: None,
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion,
        use_suggestion,
        parent_use_suggestion,
        // The node carries its resolving root (Portage's `Package.root_config`).
        targets_running_root: item_targets_running_root,
        remote_binary: false,
        build_id: None,
        deps: already_installed_deps,
    });
}
