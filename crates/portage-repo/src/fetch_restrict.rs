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

/// Arguments of [`fetch_restrict_files_all_present`].
pub(crate) struct FetchRestrictFilesAllPresentArgs<'a> {
    pub(crate) src_uri: &'a str,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repo_location: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) distdir: &'a Path,
}

/// Whether every distfile this candidate's `SRC_URI` names (flattened against its
/// effective USE) is already present in `distdir` at its Manifest-recorded size.
/// Unparsable `SRC_URI` or a distfile with no `Manifest` `DIST` line counts as
/// *not* satisfied; empty `SRC_URI` is trivially satisfied.
// Portage: output.py, getfetchsizes with only_restricted=True
pub(crate) fn fetch_restrict_files_all_present(args: FetchRestrictFilesAllPresentArgs<'_>) -> bool {
    let FetchRestrictFilesAllPresentArgs {
        src_uri,
        use_flags,
        repo_location,
        category,
        package,
        distdir,
    } = args;
    let Ok(files) = flatten_src_uri_with_use(src_uri, use_flags) else {
        return false;
    };
    if files.is_empty() {
        return true;
    }
    let digests =
        portage_fetch::parse_manifest(&repo_location.join(category).join(package).join("Manifest"))
            .unwrap_or_default();
    files.iter().all(|f| {
        let Some(recorded) = digests.get(&f.filename) else {
            return false;
        };
        std::fs::metadata(distdir.join(&f.filename))
            .map(|m| m.len() == recorded.size)
            .unwrap_or(false)
    })
}

/// `portage_fetch::flatten_src_uri` with a USE-membership check: `flag?` is active
/// iff the flag is on (`!flag?` iff off).
// Portage: getfetchsizes, getFetchMap with useflags=pkg.use
pub(crate) fn flatten_src_uri_with_use(
    src_uri: &str,
    use_flags: &HashSet<String>,
) -> Result<Vec<portage_fetch::SrcUriEntry>, Error> {
    portage_fetch::flatten_src_uri(src_uri, |negated, flag| {
        let on = use_flags.contains(flag);
        if negated { !on } else { on }
    })
    .map_err(Error::Fetch)
}

/// Arguments of [`fetch_bytes_to_download`].
pub(crate) struct FetchBytesToDownloadArgs<'a> {
    pub(crate) src_uri: &'a str,
    pub(crate) use_flags: &'a HashSet<String>,
    pub(crate) repo_location: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) distdir: &'a Path,
}

/// Per-package download size: every `SRC_URI` distfile whose on-disk size isn't
/// already the Manifest-recorded size, as `(filename, size)` pairs (the caller
/// dedupes shared distfiles). Unparsable `SRC_URI` or missing `DIST` yields empty.
// Portage: output.py, _calc_size via db.getfetchsizes
pub(crate) fn fetch_bytes_to_download(args: FetchBytesToDownloadArgs<'_>) -> Vec<(String, u64)> {
    let FetchBytesToDownloadArgs {
        src_uri,
        use_flags,
        repo_location,
        category,
        package,
        distdir,
    } = args;
    let Ok(files) = flatten_src_uri_with_use(src_uri, use_flags) else {
        return Vec::new();
    };
    if files.is_empty() {
        return Vec::new();
    }
    let digests =
        portage_fetch::parse_manifest(&repo_location.join(category).join(package).join("Manifest"))
            .unwrap_or_default();
    let mut out = Vec::new();
    for f in &files {
        let Some(recorded) = digests.get(&f.filename) else {
            return Vec::new();
        };
        let on_disk = std::fs::metadata(distdir.join(&f.filename))
            .map(|m| m.len())
            .ok();
        if on_disk != Some(recorded.size) {
            out.push((f.filename.clone(), recorded.size));
        }
    }
    out
}
