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

/// Whether `candidate`'s own declared LICENSE is fully accepted -- real
/// `Package.py`'s own `settings._getMissingLicenses` check (via
/// `LicenseManager.getMissingLicenses`/`_getPkgAcceptLicense`).
///
/// `config.accept_license` (global, already `@group`-expanded but still
/// symbolic -- see that field's own doc comment, portage-profile) is
/// layered with every matching `package.license` entry's own tokens, in
/// atom-specificity order -- real `_getPkgAcceptLicense`'s own
/// `accept_license.extend(x)` loop over `ordered_by_atom_specificity`
/// matches, ported the same way `package.use.mask`/`.force` already
/// order multiple matches in portuale (see `effective_use_flags`).
/// The resulting symbolic token list is resolved into a *concrete*
/// per-candidate acceptable-license set via the same `*`/`-*`/
/// `-license`/`license` algorithm real `getMissingLicenses`/
/// `get_pruned_accept_license` both use -- `*` needs "every license
/// LICENSE could possibly mention" (real `matchall=1`: every USE-
/// conditional forced active, ported here by reusing
/// `portage_use_reduce::use_reduce_flat` with `MatchMode::All`, since
/// group boundaries don't matter for this flat "what license names
/// exist at all" question the way they do for the real masking check
/// below).
///
/// A `LICENSE` string portuale's own bespoke parser can't make sense
/// of is treated as masked (not visible) rather than accepted --
/// matching the "can't tell, so exclude" precedent portuale's own
/// `reinstall_flags_for_use_change`/`candidate_iuse_and_use` already
/// establish for an unreadable candidate, rather than real portage's own
/// (differently-plumbed) `InvalidDependString` handling, which routes a
/// malformed `LICENSE` to a wholly separate "invalid metadata" masking
/// reason portuale has no equivalent pathway for.
/// This candidate's own effective USE, only actually resolved if
/// `value_str` (a `LICENSE`/`PROPERTIES`/`RESTRICT` string) contains a
/// `?` at all -- real `use_reduce`'s own "if '?' in license_str"
/// optimization, shared by every metadata key that needs this same
/// "resolve USE, but only when it could possibly matter" treatment.
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
    // Owned (not the shared `Rc`): cold path, one clone per call -- the
    // same cost as before #103.
    effective_use_flags(
        config,
        &candidate.iuse,
        &candidate.keywords,
        candidate_str,
        category,
        package,
    )
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

/// A candidate's own `PROPERTIES` (or `RESTRICT`) tokens after real
/// USE-conditional evaluation against this candidate's own effective USE
/// -- real `_PackageMetadataWrapper.__getitem__`'s own `use_reduce(...)`
/// pass over a `_use_conditional_keys` value (`local_config and "?" in
/// v`), which is exactly what `pkg.properties`/`pkg.restrict` then
/// `.split()`. Used for the display-only `interactive` bracket-column
/// check (and available for the `fetch` one, if that lands later).
/// `PROPERTIES`/`RESTRICT` have no `||`-group semantics (real
/// `config.py`: "ACCEPT_PROPERTIES works like ACCEPT_LICENSE, without
/// groups"), so a flat `use_reduce` is faithful -- same reasoning
/// `metadata_key_accepted` already documents. An unparsable value yields
/// an empty set (the token simply won't be found), the "can't tell, so
/// don't claim it" precedent this crate already sets elsewhere.
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

/// Real `output.py:636`'s own `not getfetchsizes(cpv, useflags=…,
/// only_restricted=True)`: whether every distfile this candidate's own
/// `SRC_URI` names -- flattened against its effective USE (`use_flags`),
/// exactly the `useflags=pkg_info.use` real portage passes -- is already
/// present in `distdir` at the byte size its repo `Manifest` records.
/// (`RESTRICT=fetch` is package-wide, so every `SRC_URI` file is
/// "restricted" -- there's no per-file `fetch+` syntax -- which makes
/// `only_restricted=True` a no-op filter here.) An unparsable `SRC_URI`,
/// or a distfile with no `Manifest` `DIST` line, counts as *not*
/// satisfied -- the loud `F` column, same "can't tell, so don't claim
/// it's fine" precedent as `evaluated_metadata_tokens` above. An empty
/// `SRC_URI` (no distfiles at all) is trivially satisfied.
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

/// `portage_fetch::flatten_src_uri` with the `active` closure this crate
/// always wants: a real USE membership check (positive `flag?` group
/// active iff the flag is on; `!flag?` iff off), the `useflags=pkg.use`
/// real portage passes `getfetchsizes`/`getFetchMap`.
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

/// Real `output.py:300-332`'s own per-package `_calc_size` contribution
/// to `counters.totalsize` -- `db.getfetchsizes(cpv, useflags=pkg.use,
/// myrepo=…)` (no `only_restricted`): every one of this candidate's own
/// `SRC_URI` distfiles whose on-disk size in `distdir` isn't already the
/// size its repo `Manifest` records, paired with that recorded size (the
/// bytes still to download). Returned as `(filename, size)` pairs so the
/// caller can dedup a shared distfile across the whole graph exactly as
/// real portage's own `myfetchlist` does. An unparsable `SRC_URI`, or a
/// distfile with no `Manifest` `DIST` line, yields an empty list -- real
/// `getfetchsizes` returns `None` for an incomplete digest map and
/// `_calc_size` then adds nothing (`"[empty/missing/bad digest]"`).
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
