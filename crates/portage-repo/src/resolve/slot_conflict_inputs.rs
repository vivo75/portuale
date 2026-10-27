//! Builds the slot-conflict handler's input from the slot-conflict records.

use crate::*;
use portage_profile::Config;

/// Arguments of [`solve_slot_conflict_records`].
pub(crate) struct SolveRecordsArgs<'a> {
    pub(crate) conflicts: &'a [SlotConflict],
    pub(crate) config: &'a Config,
    pub(crate) root: &'a Path,
    pub(crate) gate: CategoryGate<'a>,
}

/// Runs the handler over the records, one conflict per slot (records of one
/// slot are joined, as Portage keeps one conflict per slot).
// Portage: lib/_emerge/depgraph.py, _show_slot_collision_notice
pub(crate) fn solve_slot_conflict_records(args: SolveRecordsArgs<'_>) -> SlotConflictSolutions {
    let mut handler_conflicts: Vec<HandlerConflict> = Vec::new();
    let mut seen: Vec<(&str, &str, &str)> = Vec::new();
    for record in args.conflicts {
        let slot_key = (
            record.category.as_str(),
            record.package.as_str(),
            record.slot.as_str(),
        );
        if seen.contains(&slot_key) {
            continue;
        }
        seen.push(slot_key);
        let group: Vec<&SlotConflict> = args
            .conflicts
            .iter()
            .filter(|c| (c.category.as_str(), c.package.as_str(), c.slot.as_str()) == slot_key)
            .collect();
        handler_conflicts.push(conflict_for_group(&args, &group));
    }
    solve_slot_conflicts(&handler_conflicts)
}

fn conflict_for_group(args: &SolveRecordsArgs<'_>, group: &[&SlotConflict]) -> HandlerConflict {
    let first = group[0];
    let mut pkgs: Vec<HandlerPkg> = Vec::new();
    let mut parents: Vec<HandlerParent> = Vec::new();
    let mut parent_seen: Vec<(&str, &str, bool)> = Vec::new();
    for record in group {
        for inst in &record.instances {
            let cpv = format!("{}/{}-{}", first.category, first.package, inst.version);
            if pkgs
                .iter()
                .any(|p| p.key.cpv == cpv && p.key.installed == inst.installed)
            {
                continue;
            }
            let state = node_state(
                args,
                &NodeRef {
                    category: &first.category,
                    package: &first.package,
                    version: &inst.version,
                    installed: inst.installed,
                },
            );
            let match_str = format!(
                // match-str-ok: display line / conflict record, not a Candidate
                "{}/{}-{}:{}/{}::{}",
                first.category,
                first.package,
                inst.version,
                first.slot,
                inst.sub_slot,
                inst.repo_name
            );
            pkgs.push(HandlerPkg {
                key: PkgKey {
                    cpv,
                    installed: inst.installed,
                },
                match_str,
                iuse: state.0,
                use_enabled: state.1,
                required_use: state.2,
            });
        }
        for inst in &record.instances {
            for p in &inst.parents {
                let id = (p.parent_cpv.as_str(), p.atom.as_str(), p.installed);
                if parent_seen.contains(&id) {
                    continue;
                }
                parent_seen.push(id);
                parents.push(handler_parent(args, p));
            }
        }
    }
    HandlerConflict { pkgs, parents }
}

/// A package named by category, name and version.
struct NodeRef<'a> {
    category: &'a str,
    package: &'a str,
    version: &'a str,
    installed: bool,
}

/// `(IUSE, enabled USE, REQUIRED_USE)` of a node.
fn node_state(
    args: &SolveRecordsArgs<'_>,
    node: &NodeRef<'_>,
) -> (HashSet<String>, HashSet<String>, String) {
    if node.installed {
        let (iuse, use_flags) =
            installed_pkg_iuse_and_use(args.root, node.category, node.package, node.version);
        let required_use = vdb_aux_get(
            args.root,
            node.category,
            node.package,
            node.version,
            "REQUIRED_USE",
        );
        return (iuse, use_flags, required_use);
    }
    let (iuse, use_flags) = slot_conflict_flag_sets(
        args.gate,
        args.config,
        node.category,
        node.package,
        node.version,
    );
    let required_use = tree_metadata(args.gate, node)
        .and_then(|m| m.get("REQUIRED_USE").cloned())
        .unwrap_or_default();
    (iuse, use_flags, required_use)
}

fn tree_metadata(
    gate: CategoryGate<'_>,
    node: &NodeRef<'_>,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let cand = gate
        .repo_candidates(node.category, node.package)
        .ok()?
        .iter()
        .filter(|c| c.version == node.version)
        .max_by_key(|c| c.repo_priority)
        .cloned()?;
    let pf = format!("{}-{}", node.package, node.version);
    repo_aux_metadata(&cand.repo_location, node.category, &pf).ok()
}

fn handler_parent(args: &SolveRecordsArgs<'_>, p: &SlotConflictParent) -> HandlerParent {
    let Some((cat, pkg, ver)) = p
        .parent_cpv
        .split(':')
        .next()
        .filter(|s| !s.is_empty())
        .and_then(split_cpv)
    else {
        return HandlerParent {
            kind: HandlerParentKind::Argument,
            atom: p.atom.clone(),
            unevaluated: p.atom.clone(),
        };
    };
    let node = NodeRef {
        category: &cat,
        package: &pkg,
        version: &ver,
        installed: p.installed,
    };
    let (iuse, use_enabled, required_use) = node_state(args, &node);
    let unevaluated = unevaluated_atom(args, &node, &use_enabled, &p.atom);
    HandlerParent {
        kind: HandlerParentKind::Package(Box::new(HandlerPkg {
            key: PkgKey {
                cpv: format!("{cat}/{pkg}-{ver}"),
                installed: p.installed,
            },
            match_str: p.parent_cpv.clone(),
            iuse,
            use_enabled,
            required_use,
        })),
        atom: p.atom.clone(),
        unevaluated,
    }
}

/// The atom as written in the parent's dependency strings: the token whose
/// evaluation against the parent's USE gives `evaluated`; `evaluated` itself
/// when no token does.
fn unevaluated_atom(
    args: &SolveRecordsArgs<'_>,
    node: &NodeRef<'_>,
    parent_use: &HashSet<String>,
    evaluated: &str,
) -> String {
    let mut sources: Vec<String> = Vec::new();
    let tree = tree_metadata(args.gate, node);
    for key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"] {
        match (&tree, node.installed) {
            (Some(meta), _) => sources.extend(meta.get(key).cloned()),
            (None, true) => sources.push(vdb_aux_get(
                args.root,
                node.category,
                node.package,
                node.version,
                key,
            )),
            (None, false) => {}
        }
    }
    sources
        .iter()
        .flat_map(|s| s.split_whitespace())
        .find(|tok| {
            portage_dep::evaluate_atom_conditionals(tok, parent_use).as_deref() == Some(evaluated)
        })
        .map_or_else(|| evaluated.to_string(), str::to_string)
}
