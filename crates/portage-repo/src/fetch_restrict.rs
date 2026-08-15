//! SRC_URI and RESTRICT=fetch helpers.

use crate::*;

/// Arguments of [`use_flags_if_conditional`].
pub(crate) struct UseFlagsIfConditionalArgs<'a> {
    pub(crate) value_str: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
}

/// This candidate's effective USE, resolved lazily and only when `value_str`
/// (a `LICENSE`/`PROPERTIES`/`RESTRICT` string) contains a `?` at all.
pub(crate) fn use_flags_if_conditional(args: UseFlagsIfConditionalArgs<'_>) -> HashSet<String> {
    let UseFlagsIfConditionalArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
    } = args;
    if !value_str.contains('?') {
        return HashSet::new();
    }
    // Owned, not the shared `Rc`: cold path, one clone per call.
    effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: &candidate.iuse,
        keywords: &candidate.keywords,
        candidate_str,
        category,
        package,
    })
    .as_ref()
    .clone()
}
