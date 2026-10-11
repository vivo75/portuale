//! The `--tree` display.

use crate::pretend::*;

/// Arguments of [`print_tree`].
pub(crate) struct PrintTreeArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) display_order: &'a [usize],
    pub(crate) root: &'a Path,
    pub(crate) top_level_pkgs: &'a HashSet<(String, String)>,
    pub(crate) onlydeps: bool,
    pub(crate) oneshot: bool,
    pub(crate) unordered_display: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) alphabetical: bool,
    pub(crate) running_root: Option<&'a Path>,
    pub(crate) color: &'a Colorizer,
    pub(crate) system_atoms: &'a [String],
    pub(crate) world_atoms: &'a [String],
    pub(crate) force_reinstall_cps: &'a HashSet<(String, String)>,
    pub(crate) blocker_lines: &'a mut Vec<String>,
    pub(crate) masked_deps: &'a [portage_repo::MaskedDepReport],
    pub(crate) use_unsat_deps: &'a [portage_repo::UseUnsatDepReport],
    pub(crate) plain_miss_deps: &'a [portage_repo::PlainMissDepReport],
    /// Forwarded to `print_entry_line` (see its `force_sizes`).
    pub(crate) force_sizes: bool,
    /// The resolve's recorded cycle edges (see `kept_alt_for_display`).
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// `--tree`/`-t` and `--unordered-display`: a display-only port of Portage's
/// tree display. *Which* rows exist is still the flat predicate's call, so
/// a row hidden there stays hidden here. Blocker rows hang under their
/// owner (or the replacement, for a satisfied `Replacement` row); the
/// absent-owner row already lives on its removal entry. Dependency edges
/// come from `build_digraph`'s own narrowing, so `dep_zapdeps`' branch
/// selection is honoured. `ordered = false` (a node first met as an
/// ancestor) renders `nomerge` with no attr columns. Returns one
/// `(entry index, ordered)` pair per rendered row, in render order, for
/// the `Total:` recount (displayed merge rows, repeats included).
/// Portage: output_helpers.py, _tree_display
pub(crate) fn print_tree(args: PrintTreeArgs<'_>) -> Vec<(usize, bool)> {
    let PrintTreeArgs {
        entries,
        display_order,
        root,
        top_level_pkgs,
        onlydeps,
        oneshot,
        unordered_display,
        verbose,
        quiet,
        alphabetical,
        running_root,
        color,
        system_atoms,
        world_atoms,
        force_reinstall_cps,
        blocker_lines,
        masked_deps,
        use_unsat_deps,
        plain_miss_deps,
        force_sizes,
        circular,
    } = args;
    /// One node of the display graph: an entry, or a satisfied blocker
    /// row `entries[owner].blockers[index]`.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
    enum TreeNode {
        Entry(usize),
        Blocker { owner: usize, index: usize },
    }

    // `children` = Portage's `child_nodes`, `parents` = its `parent_nodes`.
    // Insertion order is edge order, which `--unordered-display` preserves.
    let mut node_order: Vec<TreeNode> = Vec::new();
    let mut children: HashMap<TreeNode, Vec<TreeNode>> = HashMap::new();
    let mut parents: HashMap<TreeNode, Vec<TreeNode>> = HashMap::new();
    let mut add_edge = |child: TreeNode, parent: TreeNode| {
        if !children.get(&parent).is_some_and(|v| v.contains(&child)) {
            children.entry(parent).or_default().push(child);
        }
        if !parents.get(&child).is_some_and(|v| v.contains(&parent)) {
            parents.entry(child).or_default().push(parent);
        }
    };

    // That narrowing is `build_digraph`'s own, reused through
    // `portage_repo::resolved_dep_targets` (merge-bound targets only).
    // Suppressed `|| ( … )` alternatives are already `None` there, so no
    // separate `kept_alt_branches` pass is needed.
    let dep_targets = portage_repo::resolved_dep_targets(entries, root, circular);
    // The walk consumes the tree-mode serialization order (merge direction;
    // the ordered walk below iterates it reversed) -- not the flat merge
    // order. `display_order` is a permutation of the entry indices, so every
    // entry keeps its node even when the scheduler never saw it.
    for &i in display_order {
        node_order.push(TreeNode::Entry(i));
    }
    // Display edges in resolver-discovery order, not merge-sorted array
    // position: the walk below picks the first untraversed parent. Stable
    // sort, so fabricated entries (all `discovery: 0`) keep array order.
    let mut by_discovery: Vec<usize> = (0..entries.len()).collect();
    by_discovery.sort_by_key(|&i| entries[i].discovery);
    for &i in &by_discovery {
        for &j in dep_targets[i].iter().flatten() {
            add_edge(TreeNode::Entry(j), TreeNode::Entry(i));
        }
    }
    // `required_by` fallback edges for a deps-less synthetic entry
    // (`--root-deps` build entries and their recursion): the resolver
    // records owners but no `deps`. Not applied to an entry with `deps`:
    // the forward walk already narrowed those edges and a second cp-keyed
    // pass would over-connect (multi-slot cps, `||` branches).
    //
    // `required_by` is cp-keyed, so an owner whose own atom already resolved
    // to one instance of this cp must not gain an edge to the others.
    for (i, e) in entries.iter().enumerate() {
        if !e.deps.is_empty() {
            continue;
        }
        for owner in &e.required_by {
            for (j, t) in entries.iter().enumerate() {
                if t.category != owner.0 || t.package != owner.1 || j == i {
                    continue;
                }
                let resolved_elsewhere = dep_targets[j].iter().flatten().any(|&k| {
                    k != i && entries[k].category == e.category && entries[k].package == e.package
                });
                if !resolved_elsewhere {
                    add_edge(TreeNode::Entry(i), TreeNode::Entry(j));
                }
                break;
            }
        }
    }
    // Blocker nodes: a satisfied row hangs its removal/replacement under
    // the blocker, and the blocker under the row's owner (unless the row
    // already lives on that same removal entry).
    // One kept-branch derivation for the whole node walk.
    let kept_alt = kept_alt_for_display(entries, root, 0..entries.len(), circular);
    for (owner, entry) in entries.iter().enumerate() {
        for (index, b) in entry.blockers.iter().enumerate() {
            // A flat-Hidden row the tree-mode simulation solves hangs under
            // the same replacement Portage's solved-blocker append follows.
            // Flat display never reaches this arm (disposition is Hidden).
            let stuck_after = if b.tree_scheduled_uninstall
                && matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. }))
                && blocker_row_disposition(entries, root, owner, b, &kept_alt)
                    != BlockerRowDisposition::Trailing
            {
                replacement_entry_index(entries, b)
            } else {
                None
            };
            let after = match blocker_row_disposition(entries, root, owner, b, &kept_alt) {
                BlockerRowDisposition::Inline(after) => Some(after),
                _ => stuck_after,
            };
            let Some(after) = after else {
                continue;
            };
            let node = TreeNode::Blocker { owner, index };
            // Insert after `after`: the walk meets the Blocker immediately
            // before its `after` entry and nests the replacement under it.
            // Appending every Blocker node last instead breaks that adjacency
            // and the prune then drops the Blocker.
            if let Some(pos) = node_order.iter().position(|&n| n == TreeNode::Entry(after)) {
                node_order.insert(pos + 1, node);
            } else {
                node_order.push(node);
            }
            if owner != after {
                add_edge(node, TreeNode::Entry(owner));
            }
            add_edge(TreeNode::Entry(after), node);
        }
    }

    // Ordered mode re-prints the suppressed root as a nomerge ancestor.
    let node_order: Vec<TreeNode> = node_order
        .into_iter()
        .filter(|n| {
            !matches!(n, TreeNode::Entry(i)
            if onlydeps && top_level_pkgs.contains(&(
                entries[*i].category.clone(),
                entries[*i].package.clone(),
            )))
        })
        .collect();

    let set_nodes: HashSet<TreeNode> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| top_level_pkgs.contains(&(e.category.clone(), e.package.clone())))
        .map(|(i, _)| TreeNode::Entry(i))
        .collect();

    fn children_of(children: &HashMap<TreeNode, Vec<TreeNode>>, n: TreeNode) -> &[TreeNode] {
        children.get(&n).map_or(&[][..], Vec::as_slice)
    }
    fn parents_of(parents: &HashMap<TreeNode, Vec<TreeNode>>, n: TreeNode) -> &[TreeNode] {
        parents.get(&n).map_or(&[][..], Vec::as_slice)
    }

    // Portage's `_unordered_tree_display`: DFS from every root (no parents)
    // in graph insertion order, each node once.
    fn unordered_walk(
        roots: &[TreeNode],
        children: &HashMap<TreeNode, Vec<TreeNode>>,
        seen: &mut HashSet<TreeNode>,
        out: &mut Vec<(TreeNode, usize, bool)>,
    ) {
        fn print_node(
            node: TreeNode,
            depth: usize,
            children: &HashMap<TreeNode, Vec<TreeNode>>,
            seen: &mut HashSet<TreeNode>,
            out: &mut Vec<(TreeNode, usize, bool)>,
        ) {
            if !seen.insert(node) {
                return;
            }
            out.push((node, depth, true));
            for &child in children_of(children, node) {
                print_node(child, depth + 1, children, seen, out);
            }
        }
        for &root in roots {
            print_node(root, 0, children, seen, out);
        }
    }

    // Portage's `_ordered_tree_display`: walk the reversed list; a node that
    // is a child of the current chain continues it, otherwise `add_parents`
    // fills the ancestor chain in first (`ordered = false` renders `nomerge`).
    /// Arguments of [`add_parents`].
    struct AddParentsArgs<'a> {
        current: TreeNode,
        ordered: bool,
        children: &'a HashMap<TreeNode, Vec<TreeNode>>,
        parents: &'a HashMap<TreeNode, Vec<TreeNode>>,
        set_nodes: &'a HashSet<TreeNode>,
        traversed: &'a mut HashSet<TreeNode>,
        shown_edges: &'a mut HashSet<(TreeNode, TreeNode)>,
        tree_nodes: &'a mut Vec<TreeNode>,
        out: &'a mut Vec<(TreeNode, usize, bool)>,
    }

    fn add_parents(args: AddParentsArgs<'_>) {
        let AddParentsArgs {
            current,
            ordered,
            children,
            parents,
            set_nodes,
            traversed,
            shown_edges,
            tree_nodes,
            out,
        } = args;
        let parent_nodes: &[TreeNode] = if !set_nodes.contains(&current) {
            parents_of(parents, current)
        } else {
            &[]
        };
        if !parent_nodes.is_empty() {
            let child_nodes: HashSet<TreeNode> =
                children_of(children, current).iter().copied().collect();
            let mut selected: Option<TreeNode> = None;
            for &node in parent_nodes {
                if !traversed.contains(&node) && !child_nodes.contains(&node) {
                    let edge = (current, node);
                    if shown_edges.contains(&edge) {
                        continue;
                    }
                    selected = Some(node);
                    break;
                }
            }
            if selected.is_none() {
                // A direct cycle is unavoidable.
                for &node in parent_nodes {
                    if !traversed.contains(&node) {
                        let edge = (current, node);
                        if shown_edges.contains(&edge) {
                            continue;
                        }
                        selected = Some(node);
                        break;
                    }
                }
            }
            if let Some(parent) = selected {
                shown_edges.insert((current, parent));
                traversed.insert(parent);
                add_parents(AddParentsArgs {
                    current: parent,
                    ordered: false,
                    children,
                    parents,
                    set_nodes,
                    traversed,
                    shown_edges,
                    tree_nodes,
                    out,
                });
            }
        }
        out.push((current, tree_nodes.len(), ordered));
        tree_nodes.push(current);
    }

    let mut display: Vec<(TreeNode, usize, bool)> = Vec::new();
    if unordered_display {
        let mut seen: HashSet<TreeNode> = HashSet::new();
        let roots: Vec<TreeNode> = node_order
            .iter()
            .copied()
            .filter(|&n| parents_of(&parents, n).is_empty())
            .collect();
        unordered_walk(&roots, &children, &mut seen, &mut display);
        // Any node the DFS never reached (e.g. a cycle's remainder) is
        // appended flat, so nothing is silently lost.
        for &n in &node_order {
            if !seen.contains(&n) {
                display.push((n, 0, true));
            }
        }
    } else {
        let mut tree_nodes: Vec<TreeNode> = Vec::new();
        let mut shown_edges: HashSet<(TreeNode, TreeNode)> = HashSet::new();
        for &x in node_order.iter().rev() {
            let mut depth = tree_nodes.len();
            while depth > 0 && !children_of(&children, tree_nodes[depth - 1]).contains(&x) {
                depth -= 1;
            }
            if depth > 0 {
                tree_nodes.truncate(depth);
                let parent = tree_nodes[depth - 1];
                tree_nodes.push(x);
                display.push((x, depth, true));
                shown_edges.insert((x, parent));
            } else {
                let mut traversed: HashSet<TreeNode> = HashSet::from([x]);
                tree_nodes.clear();
                add_parents(AddParentsArgs {
                    current: x,
                    ordered: true,
                    children: &children,
                    parents: &parents,
                    set_nodes: &set_nodes,
                    traversed: &mut traversed,
                    shown_edges: &mut shown_edges,
                    tree_nodes: &mut tree_nodes,
                    out: &mut display,
                });
            }
        }
    }

    // Portage's `_prune_tree_display`.
    let mut last_merge_depth = 0usize;
    let mut i = display.len();
    while i > 0 {
        i -= 1;
        let (node, depth, ordered) = display[i];
        if !ordered && depth == 0 && i > 0 && node == display[i - 1].0 && display[i - 1].1 == 0 {
            display.remove(i);
            continue;
        }
        let is_merge_or_uninstall = matches!(
            node,
            TreeNode::Entry(j)
                if merge_bound_version(&entries[j]).is_some()
                    || matches!(entries[j].outcome, PretendOutcome::Uninstall { .. })
        );
        if ordered && is_merge_or_uninstall {
            last_merge_depth = depth;
            continue;
        }
        if depth >= last_merge_depth || (i < display.len() - 1 && depth >= display[i + 1].1) {
            display.remove(i);
        }
    }

    let mut rendered: Vec<(usize, bool)> = Vec::new();
    for (node, depth, ordered) in display {
        // The indent is exactly `depth` spaces; every format string already
        // carries the one literal space after `]`.
        let indent = " ".repeat(depth);
        match node {
            TreeNode::Entry(i) => {
                rendered.push((i, ordered));
                print_entry_line(PrintEntryLineArgs {
                    entries,
                    root,
                    index: i,
                    indent: &indent,
                    ordered,
                    top_level_pkgs,
                    onlydeps,
                    oneshot,
                    verbose,
                    quiet,
                    alphabetical,
                    columns: false,
                    columnwidth: 130,
                    running_root,
                    color,
                    system_atoms,
                    world_atoms,
                    force_reinstall_cps,
                    blocker_lines,
                    masked_deps,
                    use_unsat_deps,
                    plain_miss_deps,
                    force_sizes,
                    circular,
                })
            }
            TreeNode::Blocker { owner, index } => {
                let owner_entry = &entries[owner];
                println!(
                    "{}",
                    format_blocker_row(
                        &format!(
                            "{}/{}-{}",
                            owner_entry.category,
                            owner_entry.package,
                            entry_display_version(owner_entry)
                        ),
                        !quiet,
                        &indent,
                        color,
                        &entries[owner].blockers[index],
                    )
                );
            }
        }
    }
    rendered
}
