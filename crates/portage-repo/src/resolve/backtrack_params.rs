//! Parameters a backtracking pass runs with.

use crate::*;

/// One in-cycle dependency of a `circular_dependency` parent: the child's
/// own `cat/pkg-version` plus the slot/sub-slot an atom's `:slot` pin would
/// match against. `pub` (fields private) so the CLI crate can carry the
/// settling pass's map into its own display derivations, which must demote
/// exactly like the walk did.
/// Portage: `circular_child` package object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CircularDepChild {
    pub(crate) category: String,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) slot: Option<String>,
    pub(crate) sub_slot: Option<String>,
}
