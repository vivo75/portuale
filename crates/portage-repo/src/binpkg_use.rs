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

/// Should this *binary* candidate be rejected for the same-version ebuild?
/// Ports Portage's binary-eligibility check (`in_usepkg_exclude`/`include`),
/// its `--usepkg-exclude`/`--usepkg-include` lists, and
/// `_wrapped_select_pkg_highest_available_imp`'s rejection block with
/// `_reinstall_for_flags`'s arms. Binary candidates only. The caller opens
/// Portage's gate; Portage compares the baked `IUSE`/`USE` against the
/// **same-version ebuild's** profile-computed sets.
/// Without an ebuild at that version the `cur_*` side is Portage's own
/// `else` arm (Portage depgraph no-ebuild arm):
///
/// ```python
///             if myeb:
///                 now_use = self._pkg_use_enabled(myeb)
///                 forced_flags = set(chain(myeb.use.force, myeb.use.mask))
///             else:
///                 pkgsettings.setcpv(pkg)
///                 now_use = pkgsettings["PORTAGE_USE"].split()
///                 forced_flags = set(
///                     chain(pkgsettings.useforce, pkgsettings.usemask)
///                 )
///             cur_iuse = iuses
/// ```
///
/// `pkgsettings.setcpv(pkg)` is the *profile's* selection for the binary's cpv,
/// not the baked USE. The respect-use half of the gate is off under
/// `--usepkgonly` (modelled by `pretend.rs`'s `unwrap_or(!usepkgonly)`).
///
/// Arms: `newuse || (respect_use && !changed_use)` compares IUSE difference
/// plus enabled-within-IUSE; `changed_use || respect_use` enabled-only;
/// neither (non-`changed-use` `--reinstall` only) compares nothing.
///
/// `forced_flags` (via [`forced_or_masked_flags_unfiltered`]) leaves only the
/// IUSE difference (Portage depgraph): forced/masked flags never reject
/// by *presence*, but can via the enabled term. Not intersected with the
/// ebuild's IUSE (audit D1): a dropped masked flag stays forgiven.
pub(crate) fn binpkg_respect_use_ok(args: BinpkgRespectUseOkArgs<'_>) -> bool {
    let BinpkgRespectUseOkArgs {
        candidate,
        ebuild_at_version,
        category,
        package,
        config,
        newuse,
        changed_use,
        respect_use,
    } = args;
    let Some(binary_use) = &candidate.binary_use else {
        return true;
    };
    // Memoised per (config USE context, both candidates' metadata, the three
    // flags): the top named cost, pure in its arguments.
    // Thread-local so the lookup stays lock-free (`EUF_CACHE`'s shape).
    type BruCache = HashMap<(u64, String, Option<String>, bool, bool, bool), bool>;
    thread_local! {
        static BRU_CACHE: RefCell<BruCache> = RefCell::new(HashMap::new());
    }
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let ebuild_str = ebuild_at_version.map(|e| {
        format!(
            "{category}/{package}-{}:{}/{}::{}",
            e.version, e.slot, e.sub_slot, e.repo_name
        )
    });
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use_context_fingerprint(config).hash(&mut h);
        category.hash(&mut h);
        package.hash(&mut h);
        candidate.version.hash(&mut h);
        candidate.slot.hash(&mut h);
        candidate.sub_slot.hash(&mut h);
        candidate.repo_name.hash(&mut h);
        candidate.iuse.hash(&mut h);
        candidate.keywords.hash(&mut h);
        // The baked USE set: a `Some(BUILD_ID)` names the
        // build, so the id is an exact O(1) stand-in for the set. A `None`
        // `build_id` names no build, so those keep the order-independent
        // xor-fold (same cost as before, never a wrong key).
        match &candidate.build_id {
            Some(id) => id.hash(&mut h),
            None => {
                binary_use.len().hash(&mut h);
                let mut acc: u64 = 0;
                for v in binary_use {
                    let mut e = std::collections::hash_map::DefaultHasher::new();
                    v.hash(&mut e);
                    acc ^= e.finish();
                }
                acc.hash(&mut h);
            }
        }
        match ebuild_at_version {
            Some(e) => {
                1u8.hash(&mut h);
                e.version.hash(&mut h);
                e.slot.hash(&mut h);
                e.sub_slot.hash(&mut h);
                e.repo_name.hash(&mut h);
                e.repo_location.hash(&mut h);
                e.iuse.hash(&mut h);
                e.keywords.hash(&mut h);
                (e.source == CandidateSource::Binary).hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
        (
            h.finish(),
            candidate_str.clone(),
            ebuild_str.clone(),
            newuse,
            changed_use,
            respect_use,
        )
    };
    if let Some(hit) = BRU_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return hit;
    }
    let result = binpkg_respect_use_ok_uncached(BinpkgRespectUseOkUncachedArgs {
        candidate,
        ebuild_at_version,
        category,
        package,
        config,
        newuse,
        changed_use,
        respect_use,
    });
    BRU_CACHE.with(|c| {
        c.borrow_mut().insert(key, result);
    });
    result
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
                ebuild.build_id.as_deref(),
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
                candidate.build_id.as_deref(),
                category,
                package,
                config,
            );
            let cur_use = effective_use_flags(EffectiveUseFlagsArgs {
                config,
                iuse: &candidate.iuse,
                keywords: &candidate.keywords,
                candidate_str: &candidate_str,
                build_id: candidate.build_id.as_deref(),
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
                .any(|ex| matches_cli_option_atom(ex, &candidate_str));
            let is_included = usepkg_include.is_empty()
                || usepkg_include
                    .iter()
                    .any(|inc| matches_cli_option_atom(inc, &candidate_str));
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

/// Arguments of [`read_binary_metadata_any`].
pub struct ReadBinaryMetadataAnyArgs<'a> {
    pub config: &'a portage_profile::Config,
    pub root: &'a Path,
    pub local: &'a BinaryIndex,
    pub category: &'a str,
    pub package: &'a str,
    pub version: &'a str,
}

/// `read_binary_metadata` extended to a `--getbinpkg` resolution: the local
/// `$PKGDIR/Packages` first, then each binrepo's cached index. Used once a
/// local *or* remote binary candidate is chosen.
pub fn read_binary_metadata_any(
    args: ReadBinaryMetadataAnyArgs<'_>,
) -> Option<HashMap<String, String>> {
    let ReadBinaryMetadataAnyArgs {
        config,
        root,
        local,
        category,
        package,
        version,
    } = args;
    read_binary_metadata_any_instance(ReadBinaryMetadataAnyInstanceArgs {
        config,
        root,
        local,
        category,
        package,
        version,
        build_id: None,
    })
}

/// Arguments of [`read_binary_metadata_any_instance`].
pub struct ReadBinaryMetadataAnyInstanceArgs<'a> {
    pub config: &'a portage_profile::Config,
    pub root: &'a Path,
    pub local: &'a BinaryIndex,
    pub category: &'a str,
    pub package: &'a str,
    pub version: &'a str,
    pub build_id: Option<&'a str>,
}

/// [`read_binary_metadata_any`] for the build the resolver selected:
/// `Some(build_id)` searches every index for exactly that build first,
/// so a chosen instance never walks another's dep strings.
pub fn read_binary_metadata_any_instance(
    args: ReadBinaryMetadataAnyInstanceArgs<'_>,
) -> Option<HashMap<String, String>> {
    let ReadBinaryMetadataAnyInstanceArgs {
        config,
        root,
        local,
        category,
        package,
        version,
        build_id,
    } = args;
    let lookup = |build_id: Option<&str>| {
        if let Some(m) = read_binary_metadata_instance(local, category, package, version, build_id)
        {
            return Some(m);
        }
        config.binrepos.iter().find_map(|binrepo| {
            let index = cached_binary_index(&binrepo.packages_dir(root));
            read_binary_metadata_instance(&index, category, package, version, build_id)
        })
    };
    build_id
        .and_then(|id| lookup(Some(id)))
        .or_else(|| lookup(None))
}
