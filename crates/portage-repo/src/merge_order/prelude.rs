//! Per-run lookups shared by the digraph build.

use crate::merge_order::*;

/// Shared prelude of `build_digraph` and `kept_alt_branches`: the `cat/pkg`
/// index, installed flags, and per-entry candidate strings.
pub(crate) struct DigraphPrelude<'a> {
    pub(crate) cp_indices: HashMap<(&'a str, &'a str), Vec<usize>>,
    pub(crate) installed: Vec<bool>,
    pub(crate) entry_candidate: Vec<Option<String>>,
}

impl DigraphPrelude<'_> {
    /// Narrow every edge to the entries its atom actually matches (version /
    /// slot / repo); keep the edge when the match cannot be decided.
    /// Portage: depgraph.py, _create_graph (_select_pkg_highest_available)
    pub(crate) fn edge_matches(&self, atom: &str, j: usize) -> bool {
        let Some(cand) = self.entry_candidate[j].as_deref() else {
            return true;
        };
        match portage_dep::match_from_list(atom, &[cand]) {
            Some(m) => !m.is_empty(),
            None => true,
        }
    }
}
