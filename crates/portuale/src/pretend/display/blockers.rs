//! Blocker rows in the merge list.

use crate::pretend::*;

/// #68/#72 B2: where one blocker row is displayed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BlockerRowDisposition {
    /// The trailing group after every package line (real
    /// `Display.blockers`, `output.py:573-592`): unresolved rows, plus
    /// the satisfied rows #72 B4 has not moved inline yet.
    Trailing,
    /// Not printed, not counted: a `Replacement` row whose replacement
    /// does not wait on its owner's merge. Real's serializer never
    /// scheduled the blocker's uninstall, so the solved blocker never
    /// entered the merge list (`depgraph.py:10351-10358`) and no
    /// `Conflict:` entry exists for it (flat p2b, B0b).
    Hidden,
    /// Printed right after the display-list entry with this index, the
    /// replacement (`output.py:119-121` appends a satisfied blocker to
    /// `print_msg` at its `_serialize_tasks` position): B0b's
    /// p2c/p2d/p2e shape.
    Inline(usize),
}

/// #84: the kept-`|| ( … )`-branch set the disposition calls below
/// share, derived once per *walk* instead of once per row.
/// `replacement_wait_index` used to derive it on every call, so each
/// `Replacement` row paid the O(graph) derivation again in each of the
/// three list walks (`collect_inline_blocker_lines`,
/// `trailing_blocker_lines`, `count_blocker_rows`) and in the `--tree`
/// node walk. The derivation itself is untouched -- still
/// `portage_repo::kept_alt_branches`, the one `suppressed_alt_edges`
/// ranking `build_digraph` shares, so the #53 self-branch exclusion
/// still cannot drift -- only its placement moved.
///
/// `owners` is the entries whose blocker rows this walk will disposition:
/// a whole-list walk passes them all, while `trailing_blocker_lines`
/// passes only its own, because it is called **once per display entry**
/// (`print_entry_line`) and hoisting the derivation to the list would
/// have paid it once per entry -- measured 12.4 s -> 15.8 s on
/// `emerge -p @world` here, a pessimization, not a saving. Only a
/// `Replacement` arm can consult the set, so a walk that dispositions
/// none gets the empty vector and no derivation at all;
/// `replacement_wait_index` reads a missing entry as "suppressed"
/// through `get`, so the empty case cannot panic.
pub(crate) fn kept_alt_for_display(
    entries: &[GraphEntry],
    root: &Path,
    owners: impl IntoIterator<Item = usize>,
    // Backlog #221: the settling pass's recorded cycle edges. Both the
    // `--tree` walk and the flat-list blocker walks pass the resolve's
    // own map, so a wait chain follows the cycle-breaking branch the
    // walk took (backlogs #216/#221 reconciliation, I5: the flat walks
    // used to pass an empty map, which could mis-disposition a wait
    // chain through a demoted branch when Replacement rows coexist with
    // a backtrack).
    circular: &HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
) -> Vec<HashSet<usize>> {
    let has_replacement_row = owners.into_iter().any(|i| {
        entries[i]
            .blockers
            .iter()
            .any(|b| matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. })))
    });
    if has_replacement_row {
        portage_repo::kept_alt_branches(entries, root, circular)
    } else {
        Vec::new()
    }
}

/// #68/#72 B2: the display disposition of one blocker row. `entries` is
/// the display-ordered list, `owner_index` the entry carrying `b`, and
/// `root` the vdb root the `satisfied` check reads. `kept_alt` is the
/// calling walk's shared set from [`kept_alt_for_display`].
pub(crate) fn blocker_row_disposition(
    entries: &[GraphEntry],
    root: &Path,
    owner_index: usize,
    b: &BlockerConflict,
    kept_alt: &[HashSet<usize>],
) -> BlockerRowDisposition {
    match &b.satisfied_by {
        Some(BlockerSatisfiedBy::Replacement { .. }) => {
            match replacement_wait_index(entries, root, owner_index, b, kept_alt) {
                Some(index) => BlockerRowDisposition::Inline(index),
                None => BlockerRowDisposition::Hidden,
            }
        }
        // #72 B4: a row resolved by a removal prints inline right after
        // that removal's `[uninstall]` row (real appends the solved
        // blocker when the uninstall node is selected, `depgraph.py:
        // 10351-10358`; B0 u6 has the `b` between the uninstall row and
        // the trailing unsatisfied group). If the removal entry is
        // missing (an owner the bridge dropped) the row keeps the
        // trailing group, the pre-B4 shape.
        Some(BlockerSatisfiedBy::Uninstall { cpv, .. }) => {
            match entries.iter().position(|e| {
                matches!(&e.outcome, PretendOutcome::Uninstall { version }
                    if format!("{}/{}-{version}", e.category, e.package) == *cpv)
            }) {
                Some(index) => BlockerRowDisposition::Inline(index),
                None => BlockerRowDisposition::Trailing,
            }
        }
        // Unresolved rows keep the trailing group.
        None => BlockerRowDisposition::Trailing,
    }
}

/// #68/#72 B2's mechanism predicate: does `b`'s replacement merge wait on
/// its owner's merge? Returns the replacement's index in `entries` when
/// it does.
///
/// Real `_serialize_tasks` appends a satisfied blocker to the merge list
/// only when it was stuck -- no merge node selectable in any priority
/// range -- and scheduled the blocker's uninstall (`depgraph.py:9998`,
/// `:10190`); the replacement's own selection then removes that uninstall
/// and appends the solved blocker after itself (`:10315-10331`,
/// `:10351-10358`). B0b pinned the shapes: the wait exists when the
/// replacement depends on the owner's merge through a chain of
/// merge-bound entries (`BDEPEND` p2c, `BDEPEND -> RDEPEND` p2d, plain
/// `RDEPEND` p2e), and not when the dependency is satisfied by an
/// installed package (flat p2b, no row), nor when the owner is not
/// merging at all (cell g).
///
/// The traversal follows only edges real's leaf scan cannot ignore
/// (`DepPriorityNormalRange`: optional and `runtime_post` edges are
/// ignored at the first two relaxed rungs, build-time and runtime are
/// not) that are **not** satisfied by an installed package
/// (`dep_edge_satisfied_by_installed`, the same rule `build_digraph` uses
/// for `DepPriority::satisfied`; `GraphEntry::deps` itself never carries
/// that computed bit).
///
/// `|| ( … )` branches: `dep_zapdeps` collapses each group during graph
/// construction, so the serializer operates on the one branch this run
/// kept -- the predicate must therefore follow kept alternatives exactly
/// like plain edges and skip only the suppressed ones
/// (`portage_repo::kept_alt_branches`, the same `choice_bins` ranking
/// `build_digraph` uses; #76 B0 captured real waiting through a
/// disjunctive `BDEPEND` and an `RDEPEND` group, q1/q2/q3 in
/// `TEST/findings/l0.md` "#76 B0").
///
/// Intermediate hops are matched by **cp**, not per instance (T10):
/// `DepEdge` carries no resolved slot, real's `_create_graph` resolves
/// each atom to one package and `build_digraph` narrows with its own
/// `edge_matches`, so a cp with two merging slots could over-connect
/// here -- i.e. the predicate can only *over-find* a wait (show a row
/// real might hide), never miss one. B2's pinned shapes have no
/// multi-slot intermediates; the `debug_assert!` below still guards the
/// ordering implication.
/// The merge-bound entry a `Replacement` row's blocker resolves against:
/// same cp and slot as the row's `satisfied_by`, merge-bound, at a
/// version other than the matched installed one. Shared by
/// `replacement_wait_index` (the flat wait predicate) and the #81
/// tree-stuck row placement (which hangs the row under the same
/// replacement real's solved-blocker append follows).
pub(crate) fn replacement_entry_index(
    entries: &[GraphEntry],
    b: &BlockerConflict,
) -> Option<usize> {
    let (cp, slot) = match &b.satisfied_by {
        Some(BlockerSatisfiedBy::Replacement { cp, slot }) => (cp, slot),
        _ => return None,
    };
    entries.iter().position(|e| {
        (e.category.as_str(), e.package.as_str()) == (cp.0.as_str(), cp.1.as_str())
            && e.slot.as_deref() == Some(slot.as_str())
            && merge_bound_version(e).is_some_and(|v| v != b.matched_version)
    })
}

pub(crate) fn replacement_wait_index(
    entries: &[GraphEntry],
    root: &Path,
    owner_index: usize,
    b: &BlockerConflict,
    kept_alt: &[HashSet<usize>],
) -> Option<usize> {
    let replacement = replacement_entry_index(entries, b)?;
    let owner = &entries[owner_index];
    let owner_cp = (owner.category.as_str(), owner.package.as_str());
    // "The owner's merge" must exist: a nomerge (installed-only) owner has
    // no merge to wait on (B0b cell g's row stays hidden).
    if !entries.iter().any(|e| {
        (e.category.as_str(), e.package.as_str()) == owner_cp && merge_bound_version(e).is_some()
    }) {
        return None;
    }
    // #76 B2: the alternatives real's serializer actually sees are the
    // display list's shared kept set (#84 `kept_alt_for_display`) -- a
    // wait chain through a disjunctive edge is a wait exactly when that
    // edge's branch was the kept one. A missing entry reads as
    // suppressed, so an empty set hides the row.
    let mut stack = vec![replacement];
    let mut seen: HashSet<usize> = HashSet::from([replacement]);
    while let Some(i) = stack.pop() {
        for (ei, edge) in entries[i].deps.iter().enumerate() {
            if edge.priority.optional || edge.priority.runtime_post {
                continue;
            }
            if edge.alt.is_some() && !kept_alt.get(i).is_some_and(|kept| kept.contains(&ei)) {
                continue;
            }
            for (j, candidate) in entries.iter().enumerate() {
                if (candidate.category.as_str(), candidate.package.as_str())
                    != (edge.category.as_str(), edge.package.as_str())
                {
                    continue;
                }
                if merge_bound_version(candidate).is_none() {
                    continue;
                }
                if portage_repo::dep_edge_satisfied_by_installed(root, edge, Some(candidate)) {
                    continue;
                }
                if (candidate.category.as_str(), candidate.package.as_str()) == owner_cp {
                    // The plan's guard (docs/history/02.68-74.md §4 D/B2): the
                    // predicate implies the owner merges before the
                    // replacement; if portuale's merge order disagrees,
                    // stop and report (a `merge_order.rs` item) rather
                    // than print a row real would put elsewhere.
                    let (rpcp, rpslot) = match &b.satisfied_by {
                        Some(BlockerSatisfiedBy::Replacement { cp, slot }) => {
                            (cp.0.as_str(), slot.as_str())
                        }
                        _ => ("?", "?"),
                    };
                    debug_assert!(
                        owner_index < replacement,
                        "B2 predicate fired with the owner (#{owner_index} {}/{} {}) \
                         not before the replacement (#{replacement} {}/{} {}): \
                         portuale's merge order disagrees with the dependency chain",
                        owner.category,
                        owner.package,
                        entry_display_version(owner),
                        rpcp,
                        rpslot,
                        b.matched_version,
                    );
                    return Some(replacement);
                }
                if seen.insert(j) {
                    stack.push(j);
                }
            }
        }
    }
    None
}

/// One `[blocks <letter>     ] <resolved> ("<atom>" is {hard,soft}
/// blocking <owner cpv>)` line -- real `ResolverOutput._blockers`
/// (`output.py:75-123`). Purely informational (see
/// `resolve_pretend_graph`'s doc comment) -- v1 neither refuses nor
/// changes the exit code for a blocker match.
///
/// Since #68 S2/S3 a row can be *satisfied* (real `blocker.satisfied`):
/// the red `B` / `PKG_BLOCKER` branch is the unresolved one
/// (`BlockerConflict::unsolvable`), the teal `b` / `PKG_BLOCKER_SATISFIED`
/// branch the satisfied one. `resolved` is real
/// `dep_expand(str(atom).lstrip("!"))` -- a category-qualification only,
/// and every portuale blocker atom is already `cat/pkg[...]`, so it
/// reduces to stripping the leading `!`/`!!`. Real's `(is <desc>
/// <parents>)` alternative (`self.resolved == blocker.atom`) is
/// unreachable -- `resolved` drops the `!` while `blocker.atom` keeps it.
/// Real `_blockers` appends `empty_space_in_brackets()` after the
/// five-space pad, and that adds the mask column's own space whenever
/// `verbosity > 1` -- true at real portage's default `emerge -p`
/// verbosity of 2, dropped only under `--quiet` (verbosity 1), which
/// `include_mask` carries.
pub(crate) fn format_blocker_row(
    // The row's owner cpv (`cat/pkg-ver`) for the trailing
    // `("…" is … blocking <owner>)` text -- an entry's own cpv at the
    // four entry-owned call sites, the orphan's recorded cpv for a
    // scan-collected owner with no entry (backlog #80).
    owner_cpv: &str,
    include_mask: bool,
    // #75 C1: real `_blockers` (`output.py:93-96`) places `self.indent`
    // between the bracket and the resolved atom, so a tree-mode row gets
    // the same `depth + 1`-space pad a package line does; flat callers
    // pass `""`.
    indent: &str,
    color: &Colorizer,
    b: &BlockerConflict,
) -> String {
    let pad = if include_mask { "      " } else { "     " };
    let (style, letter) = if b.unsolvable {
        ("PKG_BLOCKER", "B")
    } else {
        ("PKG_BLOCKER_SATISFIED", "b")
    };
    let resolved = b.atom_str.trim_start_matches('!');
    let desc = if b.strong {
        "hard blocking"
    } else {
        "soft blocking"
    };
    let parents = owner_cpv;
    format!(
        "[{} {}{pad}] {indent}{}{}",
        color.c(style, "blocks"),
        color.c(style, letter),
        color.c(style, resolved),
        color.c(style, &format!(" (\"{resolved}\" is {desc} {parents})")),
    )
}

/// The `[blocks …]` lines `print_entry_line` collects into the trailing
/// group for `entries[owner_index]`: `Trailing` rows only, and satisfied
/// ones are dropped under `--columns` (real `output.py:120`, `_blockers`
/// appends a satisfied line to `print_msg` only when `not
/// self.conf.columns`). Unresolved lines are never column-suppressed.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn trailing_blocker_lines(
    entries: &[GraphEntry],
    root: &Path,
    owner_index: usize,
    include_mask: bool,
    columns: bool,
    color: &Colorizer,
    // Backlog #221 (I5): the settling pass's recorded cycle edges (see
    // `kept_alt_for_display`): a wait chain through a demoted branch
    // dispositions like the walk took it.
    circular: &HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
) -> Vec<String> {
    let entry = &entries[owner_index];
    // #84: one kept-branch derivation for this owner's rows -- and none
    // at all unless one of them is a `Replacement`, which is what keeps
    // the per-entry call cheap.
    let kept_alt = kept_alt_for_display(entries, root, [owner_index], circular);
    entry
        .blockers
        .iter()
        .filter(|b| {
            blocker_row_disposition(entries, root, owner_index, b, &kept_alt)
                == BlockerRowDisposition::Trailing
                && !(columns && !b.unsolvable)
        })
        .map(|b| {
            format_blocker_row(
                &format!(
                    "{}/{}-{}",
                    entry.category,
                    entry.package,
                    entry_display_version(entry)
                ),
                include_mask,
                "",
                color,
                b,
            )
        })
        .collect()
}

/// The compiled inline rows for a whole display list: `(after, line)`
/// pairs keyed by the display index of the replacement entry the line
/// follows. Sorted by that index (stable, so several owners waiting on
/// the same replacement keep display order). Empty under `--columns`
/// (satisfied lines are suppressed; the counters still count them -- real
/// counts every `Blocker` node in the merge list, `get_display_list`
/// `output.py:575-584`).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn collect_inline_blocker_lines(
    entries: &[GraphEntry],
    root: &Path,
    quiet: bool,
    columns: bool,
    color: &Colorizer,
    // Backlog #221 (I5): the settling pass's recorded cycle edges (see
    // `kept_alt_for_display`).
    circular: &HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
) -> Vec<(usize, String)> {
    if columns {
        return Vec::new();
    }
    // #84: one kept-branch derivation for the whole display list.
    let kept_alt = kept_alt_for_display(entries, root, 0..entries.len(), circular);
    let mut out: Vec<(usize, String)> = Vec::new();
    for (owner_index, entry) in entries.iter().enumerate() {
        for b in &entry.blockers {
            if let BlockerRowDisposition::Inline(after) =
                blocker_row_disposition(entries, root, owner_index, b, &kept_alt)
            {
                out.push((
                    after,
                    format_blocker_row(
                        &format!(
                            "{}/{}-{}",
                            entry.category,
                            entry.package,
                            entry_display_version(entry)
                        ),
                        !quiet,
                        "",
                        color,
                        b,
                    ),
                ));
            }
        }
    }
    out.sort_by_key(|(after, _)| *after);
    out
}

/// The `blocks`/`blocks_satisfied` counters real `_PackageCounters`
/// accumulates from the merge list: every `Blocker` node in it counts,
/// satisfied or not (`get_display_list`, `output.py:575-584`), which is
/// exactly the rows `Trailing` and `Inline` describe. Hidden rows never
/// entered the list -- except in tree mode (`tree == true`), where a
/// flat-Hidden Replacement row the #81 simulation solves DOES enter it
/// (C0 p2b: `Conflict: 1 block (all satisfied)` under `--tree -v` and
/// nothing under flat `-v`).
pub(crate) fn count_blocker_rows(
    entries: &[GraphEntry],
    root: &Path,
    tree: bool,
    // Backlog #221 (I5): the settling pass's recorded cycle edges (see
    // `kept_alt_for_display`).
    circular: &HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
) -> (u64, u64) {
    let mut blocks = 0u64;
    let mut unsolvable = 0u64;
    // #84: one kept-branch derivation for the whole counter scan.
    let kept_alt = kept_alt_for_display(entries, root, 0..entries.len(), circular);
    for (owner_index, entry) in entries.iter().enumerate() {
        for b in &entry.blockers {
            // Backlog #81: in tree mode a flat-Hidden Replacement row
            // the simulation solves counts like a shown satisfied row.
            // This mirrors `print_tree`'s Blocker-node set exactly (same
            // flag, same arm): anything counted here has a node there.
            let tree_shown = tree
                && b.tree_scheduled_uninstall
                && matches!(b.satisfied_by, Some(BlockerSatisfiedBy::Replacement { .. }))
                && blocker_row_disposition(entries, root, owner_index, b, &kept_alt)
                    == BlockerRowDisposition::Hidden;
            if !tree_shown
                && blocker_row_disposition(entries, root, owner_index, b, &kept_alt)
                    == BlockerRowDisposition::Hidden
            {
                continue;
            }
            blocks += 1;
            if b.unsolvable {
                unsolvable += 1;
            }
        }
    }
    (blocks, unsolvable)
}
