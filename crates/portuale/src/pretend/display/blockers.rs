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
