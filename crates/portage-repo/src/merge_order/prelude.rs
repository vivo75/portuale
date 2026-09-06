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

    /// The per-atom candidate entries: same-cp entries the atom matches, minus
    /// removals (and, under `merge_bound_only`, the parent and installed nodes).
    pub(crate) fn match_candidates(
        &self,
        entries: &[GraphEntry],
        from: usize,
        edge: &DepEdge,
        merge_bound_only: bool,
    ) -> Vec<usize> {
        let Some(idxs) = self
            .cp_indices
            .get(&(edge.category.as_str(), edge.package.as_str()))
        else {
            return Vec::new();
        };
        // Uninstall entries are skipped, so "merge-bound" is "not installed".
        let merge_bound = |j: usize| !self.installed[j];
        let cands: Vec<usize> = idxs
            .iter()
            .copied()
            .filter(|&j| {
                if !self.edge_matches(&edge.atom, j) {
                    return false;
                }
                if matches!(entries[j].outcome, PretendOutcome::Uninstall { .. }) {
                    return false;
                }
                if merge_bound_only && (j == from || !merge_bound(j)) {
                    return false;
                }
                true
            })
            .collect();
        // Edges land on the resolving-root instance, never a same-cp other-root sibling.
        let want = crate::dep_child_root(entries[from].targets_running_root, edge.key);
        let rooted: Vec<usize> = cands
            .iter()
            .copied()
            .filter(|&j| entries[j].targets_running_root == want)
            .collect();
        let cands = if rooted.is_empty() { cands } else { rooted };
        self.narrow_by_use(entries, &edge.atom, cands)
    }

    /// USE-aware narrowing of multi-instance matches: an atom with use-deps never
    /// edges to an instance that cannot satisfy them. Only ever removes a
    /// provably-wrong edge; anything undecidable keeps the unfiltered set.
    pub(crate) fn narrow_by_use(
        &self,
        entries: &[GraphEntry],
        atom: &str,
        cands: Vec<usize>,
    ) -> Vec<usize> {
        if cands.len() < 2 {
            return cands;
        }
        let Some(parsed) = portage_dep::parse_atom(atom) else {
            return cands;
        };
        let use_deps = parsed.use_deps.unwrap_or_default();
        if use_deps.is_empty() {
            return cands;
        }
        let mut kept = Vec::new();
        let mut carriers = Vec::new();
        for &j in &cands {
            let display = &entries[j].use_flags_display;
            if display.is_empty() {
                return cands;
            }
            let declared: HashSet<String> = display.iter().map(|(flag, _)| flag.clone()).collect();
            let enabled: HashSet<String> = display
                .iter()
                .filter(|(_, on)| *on)
                .map(|(flag, _)| flag.clone())
                .collect();
            if portage_dep::use_deps_satisfied(&use_deps, &declared, &enabled) {
                kept.push(j);
            }
            // An instance declaring every named flag keeps the edge even when the
            // current USE mismatches (the walk may have landed the atom there).
            if use_deps
                .iter()
                .all(|d| d.default.is_some() || declared.contains(&d.flag))
            {
                carriers.push(j);
            }
        }
        if !kept.is_empty() {
            kept
        } else if !carriers.is_empty() {
            carriers
        } else {
            cands
        }
    }

    /// Ranking over a fixed candidate set: merge-bound first, then highest version.
    pub(crate) fn rank_best(
        entries: &[GraphEntry],
        installed: &[bool],
        cands: impl Iterator<Item = usize>,
    ) -> Option<usize> {
        let merge_bound = |j: usize| !installed[j];
        let mut best: Option<usize> = None;
        for j in cands {
            best = Some(match best {
                None => j,
                Some(b) => {
                    let better = match (merge_bound(j), merge_bound(b)) {
                        (true, false) => true,
                        (false, true) => false,
                        _ => {
                            let bv = outcome_version(&entries[b]).unwrap_or("");
                            let jv = outcome_version(&entries[j]).unwrap_or("");
                            portage_versions::vercmp(jv, bv).is_some_and(|o| o > 0)
                        }
                    };
                    if better { j } else { b }
                }
            });
        }
        best
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
