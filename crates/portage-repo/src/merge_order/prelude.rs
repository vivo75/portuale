//! Per-run lookups shared by the digraph build.

use crate::merge_order::*;

/// Shared prelude of `build_digraph` and the public
/// [`kept_alt_branches`]: `cat/pkg` -> entry indices, the installed-node
/// flags (`AlreadyInstalled`/`NoVisibleCandidate`), and the per-entry
/// candidate strings `edge_matches` narrows atoms against.
pub(crate) struct DigraphPrelude<'a> {
    pub(crate) cp_indices: HashMap<(&'a str, &'a str), Vec<usize>>,
    pub(crate) installed: Vec<bool>,
    pub(crate) entry_candidate: Vec<Option<String>>,
}

/// Arguments of [`DigraphPrelude::select_dep_target`].
pub(crate) struct SelectDepTargetArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) from: usize,
    pub(crate) from_deps: &'a [DepEdge],
    pub(crate) ei: usize,
    pub(crate) suppressed: &'a HashSet<usize>,
    pub(crate) merge_bound_only: bool,
}

impl DigraphPrelude<'_> {
    /// Real `_create_graph` resolves every dep atom to a single package
    /// (`_select_pkg_highest_available`) before `_add_pkg` records the
    /// edge. Portuale looked each atom's `cat/pkg` up in `cp_indices` and
    /// connected it to *every* scheduled instance of that `cp` -- so a
    /// slot-qualified atom (`app-text/docbook-sgml-dtd:3.0`) wrongly
    /// gained an edge to a sibling slot also being merged, and the extra
    /// parent skewed `_merge_order_bias`'s parent-count ordering. Narrow
    /// every edge to the entries its atom actually matches (version /
    /// slot / repo); keep the edge whenever the candidate string can't be
    /// built or the atom won't parse, so this only ever removes a
    /// provably-wrong edge.
    pub(crate) fn edge_matches(&self, atom: &str, j: usize) -> bool {
        let Some(cand) = self.entry_candidate[j].as_deref() else {
            return true;
        };
        match portage_dep::match_from_list(atom, &[cand]) {
            Some(m) => !m.is_empty(),
            None => true,
        }
    }

    /// B2: real resolves every dep atom to a *single* package
    /// (`_select_pkg_highest_available`) before `_add_pkg` records the
    /// edge. A bare multi-slot atom (`llvm-runtimes/clang-runtime[...]`
    /// in clang-common's PDEPEND) matches every scheduled slot, and
    /// edging it to all of them gave the older slot an extra
    /// `runtime_post` parent -- which promoted it into `asap` and split
    /// the clang-runtime drain. Among the matches prefer a merge-bound
    /// entry (the node real's scheduler graph actually edges to when the
    /// cp is being rebuilt/updated), then the highest version by
    /// `vercmp`; first on ties. A `Uninstall` removal (#72 B3) is never a
    /// merge target: its only ordering edge is `build_digraph`'s
    /// dedicated one.
    ///
    /// `merge_bound_only` is the **one** difference between this
    /// function's two callers (#82). `build_digraph` says `false`: real's
    /// scheduler digraph holds nomerge nodes, and an installed target is
    /// a legitimate edge there. `pretend.rs::print_tree` says `true`: it
    /// renders no line at all for an `AlreadyInstalled` /
    /// `NoVisibleCandidate` entry, so an edge to one would open a hole in
    /// the tree. Everything else -- the candidate strings, the atom
    /// narrowing and the ranking -- is literally this code for both, so
    /// the ranking cannot drift between them again (#82: `print_tree`'s
    /// private copy ranked with a **string** compare, under which `1.9`
    /// outranks `1.10` and slot `9` outranks slot `10`).
    ///
    /// #85: the ranking above runs *after* real's `_minimize_children`
    /// (`depgraph.py:4751-4856`). When several of one parent's atoms
    /// select different instances of the same cp, real eliminates the
    /// redundant selections first: installed instances first, then
    /// ascending version, dropping a package every one of whose atoms is
    /// also matched by another surviving package. The surviving target
    /// per atom is then the highest-ranked remainder -- which is NOT
    /// always the vercmp-highest match. Live shape: `treeslotuser`'s
    /// `:0` selects 1.9 while the bare `treeslotpkg` atom selects 1.10;
    /// real eliminates 1.10 (`:0` matches only 1.9, the bare atom matches
    /// both), so the scheduler graph holds no `user → 1.10` edge at all
    /// (verified in real's own `--debug` digraph dump) and the bias
    /// parent-count ties 1-1, settling by discovery order `1.9, 1.10`.
    /// Without the elimination the extra edge doubles 1.10's parent
    /// count and the bias flips the pair. Fully-redundant atom sets
    /// collapse to the highest version, exactly today's ranking, so only
    /// partially-overlapping shapes like this one move.
    pub(crate) fn select_dep_target(&self, args: SelectDepTargetArgs<'_>) -> Option<usize> {
        let SelectDepTargetArgs {
            entries,
            from,
            from_deps,
            ei,
            suppressed,
            merge_bound_only,
        } = args;
        let edge = &from_deps[ei];
        // Sibling edges selecting the same cp: the minimize universe.
        // Suppressed `||` branches never become edges, so they don't
        // participate (same exclusion the edge loops apply).
        let cp = (edge.category.as_str(), edge.package.as_str());
        let sib_eis: Vec<usize> = from_deps
            .iter()
            .enumerate()
            .filter(|(si, se)| {
                (se.category.as_str(), se.package.as_str()) == cp && !suppressed.contains(si)
            })
            .map(|(si, _)| si)
            .collect();
        let sib_sets: Vec<Vec<usize>> = sib_eis
            .iter()
            .map(|&si| self.match_candidates(entries, from, &from_deps[si], merge_bound_only))
            .collect();
        let own = sib_eis
            .iter()
            .position(|&si| si == ei)
            .map(|pos| &sib_sets[pos]);
        let own = own?;
        if own.len() < 2 {
            return Self::rank_best(entries, &self.installed, own.iter().copied());
        }
        // A `NoVisibleCandidate` entry is never a *selected* package:
        // real's `_select_package` returns None for its atom and
        // `_minimize_children` yields `(atom, None)` without any
        // elimination. So NVC entries neither eliminate other
        // candidates nor are eliminated themselves here -- but they
        // stay rankable below, preserving the pre-#85 fallback that
        // picked them (e.g. the `opartlya` NVC disclosure row, whose
        // dep-string order the contract suite pins). Without this,
        // the ascending-version elimination order drops an NVC entry
        // before a same-cp installed one and flips disclosure order.
        let is_nvc = |j: &usize| matches!(entries[*j].outcome, PretendOutcome::NoVisibleCandidate);
        // Real's elimination order (bug 631894 determinism note):
        // installed instances first, then ascending version.
        let mut union: Vec<usize> = Vec::new();
        for set in &sib_sets {
            for &j in set {
                if !union.contains(&j) {
                    union.push(j);
                }
            }
        }
        union.sort_by(|&a, &b| {
            // Installed sorts before merge-bound so installed
            // candidates are eliminated first, like real.
            match (self.installed[a], self.installed[b]) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let av = outcome_version(&entries[a]).unwrap_or("");
                    let bv = outcome_version(&entries[b]).unwrap_or("");
                    portage_versions::vercmp(av, bv)
                        .map(|o| o.cmp(&0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });
        let mut alive: HashSet<usize> = union.iter().copied().collect();
        for &p in &union {
            if is_nvc(&p) || !alive.contains(&p) {
                continue;
            }
            let mut shared_by_all = false;
            for set in &sib_sets {
                if set.contains(&p) {
                    if set
                        .iter()
                        .filter(|q| alive.contains(q) && !is_nvc(q))
                        .count()
                        < 2
                    {
                        shared_by_all = false;
                        break;
                    }
                    shared_by_all = true;
                }
            }
            if shared_by_all {
                alive.remove(&p);
            }
        }
        Self::rank_best(
            entries,
            &self.installed,
            own.iter().copied().filter(|j| alive.contains(j)),
        )
    }

    /// The per-atom candidate entries `select_dep_target` narrows and
    /// ranks: every same-cp entry the atom matches (version / slot /
    /// repo -- plus the #148 USE narrowing below), minus `Uninstall`
    /// removals, minus (under `merge_bound_only`) the parent itself
    /// and installed nodes.
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
        // Uninstall entries are skipped below, so among the survivors
        // "merge-bound" is exactly "not an installed node".
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
        // Track X Slice C (#242): real resolves every dep atom against
        // its group's root (`_dep_expand`'s `root_config`,
        // `depgraph.py:4928`), so an edge lands on the resolving-root
        // instance -- never on a same-cp sibling merged into the other
        // root. Without this, every edge onto a both-roots cp collapses
        // onto the first scheduled instance (a phantom self-edge on the
        // target instance, and a missing buildtime edge that strands the
        // running instance). The filter only ever removes a provably
        // same-cp other-root sibling: when no resolving-root instance
        // survives, the unfiltered set stands (same rule as
        // `edge_matches`), so single-root graphs resolve exactly as
        // before.
        let want = crate::dep_child_root(entries[from].targets_running_root, edge.key);
        let rooted: Vec<usize> = cands
            .iter()
            .copied()
            .filter(|&j| entries[j].targets_running_root == want)
            .collect();
        let cands = if rooted.is_empty() { cands } else { rooted };
        self.narrow_by_use(entries, &edge.atom, cands)
    }

    /// Backlog #148: USE-aware narrowing of multi-instance matches.
    /// Real matches dep atoms against pkgs *with* use (`atom.match(pkg.
    /// with_use(...))`), so `>=slotusetarget-1.0[x]` never edges to an
    /// x-less instance -- without this, the x/y parents all edge to the
    /// vercmp-highest instance and `_merge_order_bias`'s parent-count
    /// ordering prints the surviving conflict's rows in the wrong order
    /// (2.0 before real's 1.0-first). Applies only when it can decide
    /// fairly, and only ever removes a provably-wrong edge (same rule
    /// as `edge_matches`): unparseable atoms, atoms without use-deps,
    /// singleton sets, any candidate without display USE, and a
    /// narrowing that would empty the set all keep the unfiltered set.
    ///
    /// Deliberate narrowing vs the walk: `valid_iuse`'s
    /// profile-effective union is not applied (no config reaches this
    /// seam), so a use-dep on an effective-only flag over sibling
    /// versions with split IUSE declarations could mis-drop. The
    /// empty-fallback keeps the worst case at status quo; the
    /// contract suite and the L0 bed gate the rest.
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
            // Backlog #244: an instance whose IUSE declares every flag
            // the atom names is the one real's walk landed this atom on
            // before an autounmask change flipped it the other way for
            // another parent -- real keeps that edge (the slot-conflict
            // block lists the violated parent under it). A flag with a
            // `(+)`/`(-)` default need not be declared.
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

    /// `select_dep_target`'s ranking over a fixed candidate set: prefer a
    /// merge-bound entry, then the highest version by `vercmp`; first on
    /// ties.
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
