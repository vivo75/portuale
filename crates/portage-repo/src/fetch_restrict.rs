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

/// Arguments of [`evaluated_metadata_tokens`].
pub(crate) struct EvaluatedMetadataTokensArgs<'a> {
    pub(crate) value_str: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate_str: &'a str,
    pub(crate) config: &'a portage_profile::Config,
}

/// A candidate's `PROPERTIES` (or `RESTRICT`) tokens after USE-conditional
/// evaluation against its own effective USE (flat `use_reduce`: neither key has
/// `||`-group semantics). An unparsable value yields an empty set.
// Portage: _PackageMetadataWrapper.__getitem__, config.py
pub(crate) fn evaluated_metadata_tokens(args: EvaluatedMetadataTokensArgs<'_>) -> HashSet<String> {
    let EvaluatedMetadataTokensArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
    } = args;
    if value_str.trim().is_empty() {
        return HashSet::new();
    }
    let use_flags = use_flags_if_conditional(UseFlagsIfConditionalArgs {
        value_str,
        candidate,
        category,
        package,
        candidate_str,
        config,
    });
    let tokens: Vec<String> = value_str.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(&tokens, &use_flags, portage_use_reduce::MatchMode::Normal)
        .map(|flat| flat.into_iter().filter(|t| t != "||").collect())
        .unwrap_or_default()
}
