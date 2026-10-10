//! Where a candidate's mask or keyword decision comes from.

use crate::*;

/// `--json`'s own "state-change trace" (portuale's own feature -- see
/// the `--json` module doc comment; not a port of any real emerge
/// output): which config entries, if any, were actually load-bearing for
/// an already-`is_visible` candidate to end up visible. `None` in every
/// field means "visible with no help needed" (no matching package.mask,
/// no matching package.accept_keywords entry). Computed once per
/// finally-chosen candidate in `resolve_pretend_graph`, not in
/// `is_visible`'s own hot filtering loop over every candidate -- same
/// "duplicate a small, stable chunk of `is_visible` rather than thread a
/// reason out of its own filtering fast path" precedent `keyword_masked_
/// only` above already set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityProvenance {
    /// The `package.mask` entry that matched this candidate, if any --
    /// set even when `unmask_entry` goes on to cancel it (the point is
    /// to show the mask was there at all, not just that it didn't end up
    /// mattering).
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
/// to call on a candidate already known `is_visible` (an invisible one
/// would just report "no entry helped," which isn't a useful trace of
/// anything).
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

    let mask_entry =
        config_entries_matching(&config.package_mask, &candidate_str, category, package)
            .into_iter()
            .next()
            .cloned();
    let unmask_entry = if mask_entry.is_some() {
        config_entries_matching(&config.package_unmask, &candidate_str, category, package)
            .into_iter()
            .next()
            .cloned()
    } else {
        None
    };
    let keyword_entry = keyword_provenance(KeywordProvenanceArgs {
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
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
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) accept_keywords: &'a HashSet<String>,
    pub(crate) package_accept_keywords: &'a [(String, Vec<String>)],
}

/// The specific `package.accept_keywords` entry (if any) responsible for
/// `keywords` being accepted: `None` if the plain global `accept_keywords`
/// set alone already accepts it (checked by calling `keywords_accepted`
/// with no package entries at all -- reuses its exact matching logic
/// rather than a second implementation). Otherwise walks
/// `package_accept_keywords` in the same least-to-most-specific order
/// `specificity_ordered_flags` itself applies them in, accumulating onto
/// a copy of the global set exactly like `specificity_ordered_flags`
/// does, and reports the first entry whose own addition flips
/// `keywords_accepted` from false to true -- the one actually load-
/// bearing for this candidate, not merely the most specific matching
/// entry (a less-specific entry earlier in the accumulation may already
/// have been enough, e.g. a `*` wildcard reached before a slot-scoped
/// `~arch` grant). `None` if even every matching entry together somehow
/// isn't enough -- shouldn't happen for a candidate already confirmed
/// `is_visible`, but a safe fallback rather than a panic.
pub(crate) fn keyword_provenance(args: KeywordProvenanceArgs<'_>) -> Option<String> {
    let KeywordProvenanceArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords,
    } = args;
    if keywords_accepted(KeywordsAcceptedArgs {
        keywords,
        candidate_str,
        category,
        package,
        accept_keywords,
        package_accept_keywords: &[],
    }) {
        return None;
    }
    // Same source-order fold `keywords_accepted` -> `specificity_ordered_
    // flags` itself uses (see its doc comment): walk matches in file
    // order and report the first entry whose own contribution flips
    // visibility on.
    let matching =
        config_entries_matching(package_accept_keywords, candidate_str, category, package);
    let mut seed = accept_keywords.clone();
    for (entry, tokens) in matching {
        portage_profile::apply_incremental_iter(tokens, &mut seed);
        if keywords_accepted(KeywordsAcceptedArgs {
            keywords,
            candidate_str,
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

/// Real `output.py::gen_mask_str` + `Package.get_keyword_mask` /
/// `isHardMasked`, for the one-character bracket-mask column
/// (`GraphEntry::keyword_mask`, present at plain `-p` and `-pv` alike --
/// see that field's own doc comment). Only ever called on a candidate already
/// resolved into a `GraphEntry`, i.e. one that *is* visible -- so this
/// only classifies *why* it needed help, never re-checks visibility.
///
///   - `Some('#')` -- real `if pkg.isHardMasked(): "#"`, checked first:
///     `mask_entry` was set (some `package.mask` matched), even though a
///     `package.unmask` went on to cancel it (real `isHardMasked` /
///     `_getRawMaskAtom` deliberately ignores `package.unmask`).
///   - `None` -- the candidate's own `KEYWORDS` are accepted by the
///     *global* `ACCEPT_KEYWORDS` alone (real `get_keyword_mask`'s
///     `missing` is empty): an ordinary stable install, no marker.
///   - `Some('~')` -- not globally accepted, but the candidate carries a
///     `~<arch>` keyword whose `<arch>` *is* in `ACCEPT_KEYWORDS` (real
///     `keyword.lstrip("~") in global_accept_keywords` -> `"unstable"`):
///     a testing keyword for our own arch, made visible by
///     `package.accept_keywords`.
///   - `Some('*')` -- not globally accepted and no such `~<our-arch>`
///     keyword: visible only via `**` or a different arch's keyword
///     (real `"missing"`).
///
/// Deliberately narrower than real `getRawMissingKeywords`: the `~`-vs-
/// `*` split is decided straight off the candidate's own `KEYWORDS`
/// tokens rather than reconstructing the exact `missing` list, which is
/// sufficient for every realistic single-arch case.
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
