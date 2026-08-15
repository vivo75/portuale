//! LICENSE parsing and ACCEPT_LICENSE acceptance.

use crate::*;

/// LICENSE's PMS 7.3.2 structure: plain tokens, `||` any-of groups, and the
/// bundle a conditional or plain sub-group contributes inside a `||` alternative
/// list (`AllOf`). A flat `use_reduce` can't be reused here: group boundaries
/// decide masking, unlike DEPEND/RDEPEND recursion.
// Portage: portage.dep.use_reduce with opconvert=True
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LicenseNode {
    License(String),
    AnyOf(Vec<LicenseNode>),
    AllOf(Vec<LicenseNode>),
}

/// What opened the bracket being collected; carried explicitly since the stack
/// holds `LicenseNode`s, which can't carry a pending marker like a flat token stack.
pub(crate) enum PendingBracket {
    None,
    Conditional(String),
    AnyOf,
}
