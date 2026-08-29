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

pub(crate) fn digraph_prelude<'a>(entries: &'a [GraphEntry], root: &Path) -> DigraphPrelude<'a> {
    let mut cp_indices: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        cp_indices
            .entry((e.category.as_str(), e.package.as_str()))
            .or_default()
            .push(i);
    }
    let installed: Vec<bool> = entries
        .iter()
        .map(|e| {
            matches!(
                e.outcome,
                PretendOutcome::AlreadyInstalled { .. } | PretendOutcome::NoVisibleCandidate
            )
        })
        .collect();
    let entry_candidate: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            let ver = entry_version(e)?;
            let (slot, sub_slot) = match (e.slot.as_deref(), e.sub_slot.as_deref()) {
                (Some(s), Some(ss)) => (s.to_string(), ss.to_string()),
                _ => read_vdb_slot(root, &e.category, &e.package, ver),
            };
            let repo = e.repo_name.as_deref().unwrap_or("gentoo");
            Some(format!(
                "{}/{}-{ver}:{slot}/{sub_slot}::{repo}",
                e.category, e.package
            ))
        })
        .collect();
    DigraphPrelude {
        cp_indices,
        installed,
        entry_candidate,
    }
}
