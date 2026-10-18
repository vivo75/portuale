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

/// Computes `candidate`'s own `VisibilityProvenance` -- only meaningful
/// on a candidate already known `is_visible`.
pub(crate) fn visibility_provenance(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> VisibilityProvenance {
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );

    // The mask/unmask match sees the candidate's binary build-id (S9), so
    // an `=`+build-id entry provenances to the right instance.
    let mask_entry = config_entries_matching_with_build_id(
        &config.package_mask,
        &candidate_str,
        candidate.build_id.as_deref(),
        category,
        package,
    )
    .into_iter()
    .next()
    .cloned();
    let unmask_entry = if mask_entry.is_some() {
        config_entries_matching_with_build_id(
            &config.package_unmask,
            &candidate_str,
            candidate.build_id.as_deref(),
            category,
            package,
        )
        .into_iter()
        .next()
        .cloned()
    } else {
        None
    };
    let keyword_entry = keyword_provenance(KeywordProvenanceArgs {
        keywords: &effective_keywords(
            config,
            &candidate_str,
            category,
            package,
            &candidate.keywords,
        ),
        candidate_str: &candidate_str,
        build_id: candidate.build_id.as_deref(),
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &config.package_accept_keywords,
    });

    VisibilityProvenance {
        mask_entry,
        unmask_entry,
        keyword_entry,
    }
}

/// Arguments of [`keyword_provenance`].
pub(crate) struct KeywordProvenanceArgs<'a> {
    pub(crate) keywords: &'a [String],
    pub(crate) candidate_str: &'a str,
    /// The binary `BUILD_ID` (see [`EffectiveUseFlagsArgs::build_id`).
    pub(crate) build_id: Option<&'a str>,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// The specific `package.accept_keywords` entry (if any) responsible for
/// `keywords` being accepted: `None` if the global `accept_keywords` set
/// alone already accepts it. Reports the first entry whose own addition
/// flips acceptance from false to true -- the actually load-bearing one,
/// not merely the most specific match.
pub(crate) fn keyword_provenance(args: KeywordProvenanceArgs<'_>) -> Option<String> {
    let KeywordProvenanceArgs {
        keywords,
        candidate_str,
        build_id,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    } = args;
    if keywords_accepted(KeywordsAcceptedArgs {
        keywords,
        candidate_str,
        build_id,
        category,
        package,
        accept_keywords,
        package_accept_keywords: &[],
    }) {
        return None;
    }
    // Same source-order fold `specificity_ordered_flags` uses: walk matches
    // in file order and report the first entry that flips visibility on.
    let matching = config_entries_matching_with_build_id(
        package_accept_keywords,
        candidate_str,
        build_id,
        category,
        package,
    );
    let mut seed = accept_keywords.clone();
    for (entry, tokens) in matching {
        portage_profile::apply_incremental_iter(tokens, &mut seed);
        if keywords_accepted(KeywordsAcceptedArgs {
            keywords,
            candidate_str,
            build_id,
            category,
            package,
            accept_keywords: &seed,
            package_accept_keywords: &[],
        }) {
            return Some(entry.clone());
        }
    }
    None
}

/// The one-character bracket-mask column (`GraphEntry::keyword_mask`).
/// Only ever called on a visible candidate -- classifies *why* it needed
/// help, never re-checks visibility.
/// Portage: output.py, gen_mask_str + Package.get_keyword_mask / isHardMasked.
///
///   - `Some('#')` -- a `package.mask` matched, even though a
///     `package.unmask` went on to cancel it.
///   - `None` -- the global `ACCEPT_KEYWORDS` alone accepts the candidate.
///   - `Some('~')` -- a testing `~<arch>` keyword for our own arch, made
///     visible by `package.accept_keywords`.
///   - `Some('*')` -- visible only via `**` or a different arch's keyword.
///
/// The `~`-vs-`*` split reads straight off the candidate's own `KEYWORDS`
/// tokens, sufficient for every realistic single-arch case.
pub(crate) fn keyword_mask_marker(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
    mask_entry: &Option<String>,
) -> Option<char> {
    if mask_entry.is_some() {
        return Some('#');
    }
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    // The effective KEYWORDS (profile `package.keywords` applied): a stable
    // keyword added by a profile node reads as globally accepted here.
    let eff_keywords = effective_keywords(
        config,
        &candidate_str,
        category,
        package,
        &candidate.keywords,
    );
    if keywords_accepted(KeywordsAcceptedArgs {
        keywords: &eff_keywords,
        candidate_str: &candidate_str,
        build_id: candidate.build_id.as_deref(),
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &[],
    }) {
        return None;
    }
    let testing_for_our_arch = eff_keywords.iter().any(|k| {
        k.strip_prefix('~')
            .is_some_and(|arch| config.accept_keywords.contains(arch))
    });
    Some(if testing_for_our_arch { '~' } else { '*' })
}
