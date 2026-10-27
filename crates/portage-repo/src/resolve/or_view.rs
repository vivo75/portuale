//! The package set a `||` choice is judged against.

use crate::*;

/// One package of the post-merge set.
struct SetMember {
    version: String,
    slot: String,
    sub_slot: String,
    repo: String,
}

/// What the package tracker holds once the walk is done: the graph nodes plus
/// the installed packages no merge-bound node replaces.
// Portage: package_tracker.py, PackageTracker.match
pub(crate) struct PostMergeSet {
    root: PathBuf,
    nodes: HashMap<(String, String), Vec<SetMember>>,
    /// `(slot, version)` pairs of the merge-bound nodes, per package.
    replacing: HashMap<(String, String), Vec<(String, String)>>,
}

impl PostMergeSet {
    /// The set of one pass: `entries` are its graph nodes, `root` its installed packages.
    pub(crate) fn build(root: &Path, entries: &[GraphEntry]) -> Self {
        let mut nodes: HashMap<(String, String), Vec<SetMember>> = HashMap::new();
        let mut replacing: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
        for e in entries {
            let cp = (e.category.clone(), e.package.clone());
            let slot = e.slot.clone().unwrap_or_else(|| "0".to_string());
            let member = if let Some(version) = merge_bound_version(&e.outcome) {
                replacing
                    .entry(cp.clone())
                    .or_default()
                    .push((slot.clone(), version.clone()));
                SetMember {
                    version: version.clone(),
                    slot,
                    sub_slot: e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                    repo: e.repo_name.clone().unwrap_or_default(),
                }
            } else if let PretendOutcome::AlreadyInstalled { version } = &e.outcome {
                let held = installed_refs(root, &e.category, &e.package)
                    .into_iter()
                    .find(|r| &r.version == version);
                SetMember {
                    version: version.clone(),
                    slot: held.as_ref().map_or(slot, |r| r.slot.clone()),
                    sub_slot: held.as_ref().map_or_else(
                        || e.sub_slot.clone().unwrap_or_else(|| "0".to_string()),
                        |r| r.sub_slot.clone(),
                    ),
                    repo: held.map(|r| r.repo).unwrap_or_default(),
                }
            } else {
                continue;
            };
            nodes.entry(cp).or_default().push(member);
        }
        Self {
            root: root.to_path_buf(),
            nodes,
            replacing,
        }
    }

    /// The nodes of `category/package`, then the installed packages nothing replaces:
    /// a node replaces an installed package of its slot or of its version.
    fn members(&self, category: &str, package: &str) -> Vec<SetMember> {
        let cp = (category.to_string(), package.to_string());
        let mut out: Vec<SetMember> = Vec::new();
        for n in self.nodes.get(&cp).into_iter().flatten() {
            out.push(SetMember {
                version: n.version.clone(),
                slot: n.slot.clone(),
                sub_slot: n.sub_slot.clone(),
                repo: n.repo.clone(),
            });
        }
        let replacing = self.replacing.get(&cp).map_or(&[][..], Vec::as_slice);
        for r in installed_refs(&self.root, category, package) {
            let replaced = replacing
                .iter()
                .any(|(slot, version)| *slot == r.slot || *version == r.version);
            let listed = out
                .iter()
                .any(|m| m.version == r.version && m.slot == r.slot);
            if !replaced && !listed {
                out.push(SetMember {
                    version: r.version,
                    slot: r.slot,
                    sub_slot: r.sub_slot,
                    repo: r.repo,
                });
            }
        }
        out
    }

    /// The members of `category/package` that `atom_str` matches, USE deps ignored.
    fn matching(&self, category: &str, package: &str, atom_str: &str) -> Vec<SetMember> {
        let members = self.members(category, package);
        let strs: Vec<String> = members
            .iter()
            .map(|m| {
                format!(
                    // match-str-ok: post-merge set member (node or surviving vdb row), not a Candidate
                    "{category}/{package}-{}:{}/{}::{}",
                    m.version, m.slot, m.sub_slot, m.repo
                )
            })
            .collect();
        let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
        let Some(matched) = portage_dep::match_from_list(portage_dep::without_use(atom_str), &refs)
        else {
            return Vec::new();
        };
        members
            .into_iter()
            .zip(strs.iter())
            .filter(|(_, s)| matched.contains(&s.as_str()))
            .map(|(m, _)| m)
            .collect()
    }

    /// Whether any member of the set matches the atom (a blocker always does).
    pub(crate) fn matches(&self, atom_str: &str) -> bool {
        let Some(atom) = parse_dep_atom(atom_str, "or_view") else {
            return false;
        };
        atom.blocker != portage_dep::Blocker::None
            || !self
                .matching(&atom.category, &atom.package, atom_str)
                .is_empty()
    }

    /// Whether the set holds any version of the atom's package.
    pub(crate) fn has_cp(&self, atom_str: &str) -> bool {
        parse_dep_atom(atom_str, "or_view")
            .is_some_and(|a| !self.members(&a.category, &a.package).is_empty())
    }

    /// Whether a graph node (not a surviving installed package) matches the atom.
    pub(crate) fn node_matches(&self, atom_str: &str) -> bool {
        let Some(atom) = parse_dep_atom(atom_str, "or_view") else {
            return false;
        };
        let Some(nodes) = self
            .nodes
            .get(&(atom.category.clone(), atom.package.clone()))
        else {
            return false;
        };
        let strs: Vec<String> = nodes
            .iter()
            .map(|m| {
                format!(
                    // match-str-ok: post-merge graph node, not a Candidate
                    "{}/{}-{}:{}/{}::{}",
                    atom.category, atom.package, m.version, m.slot, m.sub_slot, m.repo
                )
            })
            .collect();
        let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
        portage_dep::match_from_list(portage_dep::without_use(atom_str), &refs)
            .is_some_and(|m| !m.is_empty())
    }

    /// The highest member matching the atom, as `(version, slot)`.
    pub(crate) fn best_matching(&self, atom_str: &str) -> Option<(String, String)> {
        let atom = parse_dep_atom(atom_str, "or_view")?;
        self.matching(&atom.category, &atom.package, atom_str)
            .into_iter()
            .max_by(|a, b| vercmp_ordering(&a.version, &b.version))
            .map(|m| (m.version, m.slot))
    }

    /// The highest version any member holds in `slot` of `category/package`.
    pub(crate) fn highest_in_slot(
        &self,
        category: &str,
        package: &str,
        slot: &str,
    ) -> Option<String> {
        self.members(category, package)
            .into_iter()
            .filter(|m| m.slot == slot)
            .map(|m| m.version)
            .max_by(|a, b| vercmp_ordering(a, b))
    }
}

/// The view a `||` choice is made in. `Normal` is the walk's own (the tree and the
/// vdb as they are); `PostMerge` is the package set after the merge.
// Portage: depgraph.py, _filtered_trees / _graph_trees
#[derive(Clone, Copy)]
pub(crate) enum OrView<'a> {
    Normal,
    PostMerge(&'a PostMergeSet),
}

/// What a view's probes read besides the set: the tree gate, the vdb root and the config.
#[derive(Clone, Copy)]
pub(crate) struct OrProbe<'a> {
    pub(crate) gate: CategoryGate<'a>,
    pub(crate) root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
}

impl OrView<'_> {
    /// Whether `atom_str` (USE deps already stripped) has an available package.
    pub(crate) fn satisfiable(
        self,
        gate: CategoryGate<'_>,
        atom_str: &str,
        config: &portage_profile::Config,
        extra: &[String],
    ) -> bool {
        match self {
            OrView::Normal => atom_currently_satisfiable(gate, atom_str, config, extra),
            OrView::PostMerge(set) => set.matches(atom_str),
        }
    }

    /// Whether the atom's `[use]` deps hold on an available package. In `PostMerge` a
    /// node carries no USE state yet, so the tree and the vdb stand in for it.
    pub(crate) fn use_satisfiable(self, p: OrProbe<'_>, atom_str: &str, extra: &[String]) -> bool {
        atom_currently_satisfiable(p.gate, atom_str, p.config, extra)
            || atom_matches_installed(p.root, atom_str, p.config)
    }

    /// Whether the atom is satisfied by what is installed.
    pub(crate) fn matches_installed(
        self,
        root: &Path,
        atom_str: &str,
        config: &portage_profile::Config,
    ) -> bool {
        match self {
            OrView::Normal => atom_matches_installed(root, atom_str, config),
            OrView::PostMerge(set) => set.matches(atom_str),
        }
    }

    /// The cp-level installed test (`all_installed`).
    pub(crate) fn cp_installed(self, root: &Path, atom_str: &str) -> bool {
        match self {
            OrView::Normal => atom_cp_installed(root, atom_str),
            OrView::PostMerge(set) => parse_dep_atom(atom_str, "or_view").is_some_and(|a| {
                a.blocker != portage_dep::Blocker::None
                    || a.category == "virtual"
                    || set.has_cp(atom_str)
            }),
        }
    }

    /// Whether the package has any installed version at all (the fuzzy cp-level fallback).
    pub(crate) fn any_cp_installed(self, root: &Path, category: &str, package: &str) -> bool {
        match self {
            OrView::Normal => !installed_candidates(root, category, package).is_empty(),
            OrView::PostMerge(set) => !set.members(category, package).is_empty(),
        }
    }

    /// Whether the slot the atom would pull in is installed (`all_installed_slots`).
    pub(crate) fn installed_in_slot_of(
        self,
        p: OrProbe<'_>,
        atom_str: &str,
        extra: &[String],
    ) -> bool {
        match self {
            OrView::Normal => atom_installed_in_slot_of(p.gate, p.root, p.config, atom_str, extra),
            OrView::PostMerge(set) => parse_dep_atom(atom_str, "or_view").is_some_and(|a| {
                a.blocker != portage_dep::Blocker::None
                    || a.category == "virtual"
                    || set.matches(atom_str)
            }),
        }
    }

    /// Whether every non-blocker atom is already a graph node (`all_in_graph`).
    pub(crate) fn all_in_graph(
        self,
        atoms: &[String],
        entries: &[GraphEntry],
        config: &portage_profile::Config,
        queued: &[QueueItem],
    ) -> bool {
        match self {
            OrView::Normal => atoms_all_in_graph(atoms, entries, config, queued),
            OrView::PostMerge(set) => {
                let non_blocker: Vec<&String> = atoms
                    .iter()
                    .filter(|a| {
                        parse_dep_atom(a, "or_view")
                            .is_some_and(|p| p.blocker == portage_dep::Blocker::None)
                    })
                    .collect();
                !non_blocker.is_empty() && non_blocker.iter().all(|a| set.node_matches(a))
            }
        }
    }

    /// The highest package the atom could pull in, as `(version, slot)`, USE deps ignored.
    pub(crate) fn highest_available(
        self,
        p: OrProbe<'_>,
        atom: &portage_dep::Atom,
        atom_str: &str,
        extra: &[String],
    ) -> Option<(String, String)> {
        match self {
            OrView::Normal => {
                highest_available_candidate_ignoring_use(HighestAvailableCandidateIgnoringUseArgs {
                    atom_str,
                    category: &atom.category,
                    package: &atom.package,
                    config: p.config,
                    extra_constraints: extra,
                    gate: p.gate,
                })
                .and_then(|(candidate_str, .., slot)| {
                    candidate_version(&candidate_str, &atom.category, &atom.package)
                        .map(|version| (version, slot))
                })
            }
            OrView::PostMerge(set) => set.best_matching(atom_str),
        }
    }
}
