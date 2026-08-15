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
    if keywords_accepted(KeywordsAcceptedArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
        accept_keywords: &config.accept_keywords,
        package_accept_keywords: &[],
    }) {
        return None;
    }
    let testing_for_our_arch = candidate.keywords.iter().any(|k| {
        k.strip_prefix('~')
            .is_some_and(|arch| config.accept_keywords.contains(arch))
    });
    Some(if testing_for_our_arch { '~' } else { '*' })
}
