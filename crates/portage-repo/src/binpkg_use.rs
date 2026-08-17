//! `--binpkg-respect-use` checks and binary metadata readers.

use crate::*;

#[cfg(test)]
thread_local! {
    pub(crate) static BRU_UNCACHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn binpkg_respect_use_ok_uncached_calls() -> u64 {
    BRU_UNCACHED_CALLS.with(std::cell::Cell::get)
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
