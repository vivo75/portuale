//! Where a candidate's mask or keyword decision comes from.

use crate::*;

/// Which config entries, if any, were actually load-bearing for an
/// already-visible candidate to end up visible. `None` in every field
/// means "visible with no help needed". Computed once per finally-chosen
/// candidate, not in `is_visible`'s hot filtering loop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityProvenance {
    /// The `package.mask` entry that matched this candidate, if any --
    /// set even when `unmask_entry` goes on to cancel it.
    pub mask_entry: Option<String>,
    /// The `package.unmask` entry that cancelled `mask_entry`, if
    /// `mask_entry` is `Some` and something actually cancelled it. Always
    /// `None` when `mask_entry` is `None`.
    pub unmask_entry: Option<String>,
    /// The specific `package.accept_keywords` entry whose own tokens were
    /// needed for this candidate's `KEYWORDS` to be accepted, or `None`
    /// if the global `ACCEPT_KEYWORDS` set alone already sufficed (no
    /// package-scoped help needed at all).
    pub keyword_entry: Option<String>,
}
