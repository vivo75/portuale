//! `--binpkg-respect-use` checks and binary metadata readers.

use crate::*;

/// Arguments of [`binpkg_respect_use_ok`].
pub(crate) struct BinpkgRespectUseOkArgs<'a> {
    pub(crate) candidate: &'a Candidate,
    pub(crate) ebuild_at_version: Option<&'a Candidate>,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) respect_use: bool,
}

#[cfg(test)]
thread_local! {
    pub(crate) static BRU_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn binpkg_respect_use_ok_uncached_calls() -> u64 {
    BRU_UNCACHED_CALLS.with(std::cell::Cell::get)
}
/// Arguments of [`binpkg_respect_use_ok_uncached`].
pub(crate) struct BinpkgRespectUseOkUncachedArgs<'a> {
    pub(crate) candidate: &'a Candidate,
    pub(crate) ebuild_at_version: Option<&'a Candidate>,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) newuse: bool,
    pub(crate) changed_use: bool,
    pub(crate) respect_use: bool,
}

pub(crate) fn binpkg_respect_use_ok_uncached(args: BinpkgRespectUseOkUncachedArgs<'_>) -> bool {
    let BinpkgRespectUseOkUncachedArgs {
        candidate,
        ebuild_at_version,
        category,
        package,
        config,
        newuse,
        changed_use,
        respect_use,
    } = args;
    #[cfg(test)]
    BRU_UNCACHED_CALLS.with(|c| c.set(c.get() + 1));
    let Some(binary_use) = &candidate.binary_use else {
        return true;
    };
    let old_iuse: HashSet<String> = candidate
        .iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let (cur_iuse, cur_use, forced_flags) = match ebuild_at_version {
        Some(ebuild) if ebuild.source != CandidateSource::Binary => {
            let ebuild_str = format!(
                "{category}/{package}-{}:{}/{}::{}",
                ebuild.version, ebuild.slot, ebuild.sub_slot, ebuild.repo_name
            );
            let forced = forced_or_masked_flags_unfiltered(
                &ebuild.keywords,
                &ebuild_str,
                category,
                package,
                config,
            );
            let (iuse, use_flags) = candidate_iuse_and_use(ebuild, category, package, config)
                .unwrap_or_else(|| (old_iuse.clone(), Rc::new(binary_use.clone())));
            (iuse, use_flags, forced)
        }
        // Portage's no-`myeb` arm (Portage depgraph): the profile's
        // selection for the binary's own cpv over its own IUSE, never the
        // baked USE; `forced_flags` likewise. See the fn doc comment.
        _ => {
            let candidate_str = format!(
                "{category}/{package}-{}:{}/{}::{}",
                candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
            );
            let forced = forced_or_masked_flags_unfiltered(
                &candidate.keywords,
                &candidate_str,
                category,
                package,
                config,
            );
            let cur_use = effective_use_flags(EffectiveUseFlagsArgs {
                config,
                iuse: &candidate.iuse,
                keywords: &candidate.keywords,
                candidate_str: &candidate_str,
                category,
                package,
            });
            (old_iuse.clone(), cur_use, forced)
        }
    };
    let old_enabled: HashSet<String> = old_iuse.intersection(binary_use).cloned().collect();
    let cur_enabled: HashSet<String> = cur_iuse.intersection(&cur_use).cloned().collect();
    let mut flags: HashSet<String> = HashSet::new();
    if newuse || (respect_use && !changed_use) {
        // Portage's `flags -= forced_flags` (Portage depgraph): between
        // the IUSE difference and the enabled-set union. Not intersected
        // with the ebuild's IUSE (audit D1): a dropped masked flag stays
        // forgiven.
        let mut presence_diff: HashSet<String> =
            old_iuse.symmetric_difference(&cur_iuse).cloned().collect();
        for forced in &forced_flags {
            presence_diff.remove(forced);
        }
        flags.extend(presence_diff);
        flags.extend(old_enabled.symmetric_difference(&cur_enabled).cloned());
    } else if changed_use || respect_use {
        flags.extend(old_enabled.symmetric_difference(&cur_enabled).cloned());
    }
    flags.is_empty()
}

pub(crate) fn filter_usepkg_exclude_include(
    binary_candidates: Vec<Candidate>,
    category: &str,
    package: &str,
    usepkg_exclude: &[String],
    usepkg_include: &[String],
) -> Vec<Candidate> {
    if usepkg_exclude.is_empty() && usepkg_include.is_empty() {
        return binary_candidates;
    }
    binary_candidates
        .into_iter()
        .filter(|c| {
            let candidate_str = format!(
                "{category}/{package}-{}:{}/{}::{}",
                c.version, c.slot, c.sub_slot, c.repo_name
            );
            let is_excluded = usepkg_exclude
                .iter()
                .any(|ex| matches_config_entry(ex, &candidate_str, category, package));
            let is_included = usepkg_include.is_empty()
                || usepkg_include
                    .iter()
                    .any(|inc| matches_config_entry(inc, &candidate_str, category, package));
            !is_excluded && is_included
        })
        .collect()
}

/// Reads `<pkgdir>/Packages` for `category/package-version`'s entry -- the
/// binary-candidate counterpart to `read_md5_cache`, for dependency recursion
/// once a binary candidate is chosen. `None` if the version is gone (same
/// tolerance as a missing md5-cache entry).
pub fn read_binary_metadata(
    index: &BinaryIndex,
    category: &str,
    package: &str,
    version: &str,
) -> Option<HashMap<String, String>> {
    read_binary_metadata_instance(index, category, package, version, None)
}

/// [`read_binary_metadata`] for one specific build:
/// `Some(build_id)` returns exactly that `BUILD_ID`; `None` the newest build
/// (see [`pick_binary_instance`]) -- never merely the first entry, which is
/// usually the oldest under binpkg-multi-instance.
pub fn read_binary_metadata_instance(
    index: &BinaryIndex,
    category: &str,
    package: &str,
    version: &str,
    build_id: Option<&str>,
) -> Option<HashMap<String, String>> {
    let want = format!("{category}/{package}-{version}");
    pick_binary_instance(index.entries(), &want, build_id).cloned()
}
