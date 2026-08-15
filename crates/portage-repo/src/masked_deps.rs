//! Masked-dependency reports.

use crate::*;

/// One dependency atom's masked-only disclosure: the "All ebuilds that could
/// satisfy … have been masked" block for a *dependency*. Rendered in place of
/// the bare `!!! no visible ebuild for dependency` line, with the
/// `(dependency required by …)` chain and the docs footer.
// Portage: _show_unsatisfied_dep, depgraph.py
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom text (unevaluated, as queued).
    pub atom: String,
    /// `(cpv::repo, reasons)` descending version, cpv-deduped: the
    /// `- <cpv> (masked by: …)` lines the renderer prints.
    pub masked: Vec<(String, Vec<String>)>,
    /// `(node, type)` pairs, innermost parent first, up to the top-level
    /// argument(s). An installed intermediate renders its vdb repo with type
    /// `"installed"`; a binary parent renders `"binary"`. One chain per direct
    /// parent (Portage shows only its first abandon; portuale discloses all).
    // Portage: _get_dep_chain
    pub chain: Vec<(String, String)>,
}

/// The separate "no ebuilds built with USE flags to satisfy" disclosure for a
/// *dependency*'s `[use]`-unsatisfied atom: `Missing IUSE: …` rows (IUSE lacks
/// a required flag) or `Change USE: +… -…` rows (effective USE can't satisfy
/// the unconditional use-deps). Rendered instead of the bare no-visible line.
// Portage: _show_unsatisfied_dep
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseUnsatDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated, so a `[flag]` dep shows as written.
    pub atom: String,
    /// The `- <cpv>::<repo> (<reason>)` rows: the latest `Change USE:`-adjustable
    /// candidate only, else every `Missing IUSE:` candidate in descending order.
    /// Never empty (the report exists only when a row does).
    pub rows: Vec<(String, Vec<String>)>,
    /// Walked post-pass exactly like [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
}

/// The plain-miss disclosure for a dependency `NoVisibleCandidate` that is
/// neither masked-only nor `[use]`-unsatisfied. Rendered instead of the bare
/// `!!! no visible ebuild for dependency "<cp>"` line.
// Portage: _show_unsatisfied_dep, "there are no ebuilds to satisfy"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainMissDepReport {
    pub category: String,
    pub package: String,
    /// The requesting atom as queued, unevaluated, so a `[flag=]` dep shows as written.
    pub atom: String,
    /// Walked post-pass exactly like [`MaskedDepReport::chain`].
    pub chain: Vec<(String, String)>,
}
