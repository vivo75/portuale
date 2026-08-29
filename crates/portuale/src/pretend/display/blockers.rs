//! Blocker rows in the merge list.

use crate::pretend::*;

/// Where one blocker row is displayed.
/// Portage: output.py, Display.blockers
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BlockerRowDisposition {
    /// The trailing group after every package line: unresolved rows, plus
    /// satisfied rows not yet moved inline.
    Trailing,
    /// Not printed, not counted: a `Replacement` row whose replacement
    /// does not wait on its owner's merge.
    Hidden,
    /// Printed right after the display-list entry with this index, the
    /// replacement.
    Inline(usize),
}

/// The kept-`|| ( … )`-branch set the disposition calls share, derived once
/// per walk. Only a `Replacement` arm consults it, so a walk with none gets
/// an empty vector (a missing entry reads as suppressed, so no panic).
/// `owners` is the entries this walk dispositions: a whole-list walk passes
/// them all; `trailing_blocker_lines` passes only its own, since hoisting
/// its derivation to the list would pay it once per entry.
pub(crate) fn kept_alt_for_display(
    entries: &[GraphEntry],
    root: &Path,
    owners: impl IntoIterator<Item = usize>,
    // The settling pass's recorded cycle edges: a wait chain follows the
    // cycle-breaking branch the walk took.
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

/// The display disposition of one blocker row. `kept_alt` is the calling
/// walk's shared set from [`kept_alt_for_display`].
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
        // A row resolved by a removal prints inline after that removal's
        // `[uninstall]` row; a missing removal entry keeps the trailing group.
        // Portage: depgraph.py, _serialize_tasks
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

/// Mechanism predicate: does `b`'s replacement merge wait on its owner's
/// merge? Returns the replacement's index in `entries` when it does.
/// The traversal follows only edges Portage's leaf scan cannot ignore that
/// are not satisfied by an installed package, following kept `||` branches
/// exactly like plain edges. Intermediate hops match by cp, not instance.
/// Portage: depgraph.py, _serialize_tasks
/// The merge-bound entry a `Replacement` row resolves against: same cp and
/// slot, at a version other than the matched installed one.
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
    // A wait chain through a disjunctive edge is a wait exactly when that
    // edge's branch was the kept one; a missing entry reads as suppressed.
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
                    // The predicate implies the owner merges before the
                    // replacement; report disagreement rather than misplace.
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

/// One `[blocks <letter>] <resolved> ("<atom>" is {hard,soft} blocking
/// <owner cpv>)` line. Purely informational -- never refuses, never changes
/// the exit code. Red `B` = unresolved, teal `b` = satisfied.
/// Portage: output.py, ResolverOutput._blockers
pub(crate) fn format_blocker_row(
    // The row's owner cpv for the trailing text; the orphan's recorded cpv
    // for a scan-collected owner with no entry.
    owner_cpv: &str,
    include_mask: bool,
    // Tree-mode rows get the same `depth + 1`-space pad a package line
    // does; flat callers pass `""`.
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

/// Arguments of [`trailing_blocker_lines`].
pub(crate) struct TrailingBlockerLinesArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) owner_index: usize,
    pub(crate) include_mask: bool,
    pub(crate) columns: bool,
    pub(crate) color: &'a Colorizer,
    /// The settling pass's recorded cycle edges.
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// The `[blocks …]` lines for `entries[owner_index]`: `Trailing` rows only;
/// satisfied ones are dropped under `--columns`.
pub(crate) fn trailing_blocker_lines(args: TrailingBlockerLinesArgs<'_>) -> Vec<String> {
    let TrailingBlockerLinesArgs {
        entries,
        root,
        owner_index,
        include_mask,
        columns,
        color,
        circular,
    } = args;
    let entry = &entries[owner_index];
    // One kept-branch derivation for this owner's rows, skipped entirely
    // unless one is a `Replacement`.
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

/// Arguments of [`collect_inline_blocker_lines`].
pub(crate) struct CollectInlineBlockerLinesArgs<'a> {
    pub(crate) entries: &'a [GraphEntry],
    pub(crate) root: &'a Path,
    pub(crate) quiet: bool,
    pub(crate) columns: bool,
    pub(crate) color: &'a Colorizer,
    /// The settling pass's recorded cycle edges.
    pub(crate) circular: &'a HashMap<(bool, String, String), Vec<portage_repo::CircularDepChild>>,
}

/// The compiled inline rows for a whole display list: `(after, line)` pairs
/// keyed by the replacement entry's display index. Empty under `--columns`.
/// Portage: output.py, get_display_list
pub(crate) fn collect_inline_blocker_lines(
    args: CollectInlineBlockerLinesArgs<'_>,
) -> Vec<(usize, String)> {
    let CollectInlineBlockerLinesArgs {
        entries,
        root,
        quiet,
        columns,
        color,
        circular,
    } = args;
    if columns {
        return Vec::new();
    }
    // One kept-branch derivation for the whole display list.
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
