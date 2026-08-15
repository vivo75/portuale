//! Which alternative of an `||` group to prefer.

use crate::*;

/// The `version` field of one `highest_available_candidate_ignoring_use` candidate
/// string: the per-atom seed of an alternative's `cp_map`.
pub(crate) fn candidate_version(
    candidate_str: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let rest = candidate_str.strip_prefix(&format!("{category}/{package}-"))?;
    rest.split(':').next().map(str::to_string)
}
