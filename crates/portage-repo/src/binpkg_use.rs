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

/// `--usepkg-exclude`/`--usepkg-include` (real `main.py`: "a space
/// separated list of package names or slot atoms", real `WildcardSet`
/// grammar, same "plain atom or `*`-wildcard" two-tier matcher
/// `matches_config_entry` already backs `--exclude`/`.mask`/`.unmask`
/// with -- see `resolve_pretend`'s own doc comment on `excluded`).
/// Ports real `depgraph.py`'s own per-candidate binary-eligibility check
/// (`in_usepkg_exclude = have_usepkg_exclude and usepkg_exclude.
/// findAtomForPackage(pkg, ...)`; `in_usepkg_include = not
/// have_usepkg_include or usepkg_include.findAtomForPackage(pkg, ...)`;
/// `if in_usepkg_exclude or not in_usepkg_include: break` -- the
/// candidate is dropped from the binary pool entirely, never considered
/// alongside ebuilds at all) -- confirmed by reading it during the
/// original binary-package slice's own research, deliberately not acted
/// on then. Applied only to binary candidates: real `usepkg_exclude`/
/// `usepkg_include` gate binary-candidate eligibility specifically
/// (`built and not installed`), never ebuild candidates.
/// Real `_wrapped_select_pkg_highest_available_imp`'s binary-rejection
/// block (`depgraph.py:8259-8294`) with `_reinstall_for_flags`'s two
/// arms (`:3134-3162`): should this *binary* candidate be rejected so
/// the search falls back to the same-version ebuild?
///
/// The caller opens real's gate -- `reinstall_use or (not installed and
/// respect_use)` -- and this function implements the comparison. Real
/// compares the binary's own baked `IUSE`/`USE` (`old_*`) against the
/// **same-version ebuild's** profile-computed sets (`cur_*`), never
/// against the binary's own selection: a flag that only the ebuild
/// declares is visible.
///
/// Without an ebuild at that version the `cur_*` side is real's own
/// `else` arm (`depgraph.py:8265-8271`):
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
/// `pkgsettings.setcpv(pkg)` computes the *profile's* selection for the
/// binary's own cpv over its own IUSE -- **not** the binary's baked
/// USE. The #69 R0 oracle (real 3.0.82.2, `TEST/findings/l0.md`
/// "#69 R0") captured it: binary baked `USE=elibc_glibc r0flag`, the
/// profile selection after dropping the package's `r0flag` selection is
/// `['elibc_glibc']`, and real rejects (`{'r0flag'}`), while the old
/// "own sets stand in" fallback made the comparison empty. Reachability
/// is real's own: the respect-use half of the gate is off under
/// `--usepkgonly` (`create_depgraph_params.py:47-55`, modelled by
/// `pretend.rs`'s `binpkg_respect_use.unwrap_or(!usepkgonly)`).
///
/// Arms, in real's order:
/// - `newuse || (respect_use && !changed_use)`: the IUSE symmetric
///   difference plus the enabled-within-IUSE difference. This is why
///   plain `--binpkg-respect-use=auto` rejects a binary whose *IUSE* is
///   missing a flag the ebuild gained even when the two enabled sets
///   agree, and why `--newuse` rejects it regardless of respect-use.
/// - `changed_use || respect_use` (without `newuse`): enabled-only.
/// - neither (gate opened only by a non-`changed-use` `--reinstall`):
///   no comparison at all, the binary survives.
///
/// `forced_flags` (the ebuild's own `use.force`/`use.mask`, or -- in
/// the no-ebuild arm -- the binary cpv's, both via
/// [`forced_or_masked_flags_unfiltered`]) is subtracted from the IUSE
/// symmetric difference only (`depgraph.py:3149-3150`, the #69 R2
/// slice): a flag the profile forces or masks never rejects a binary by
/// its *presence* alone. It can still reject through the
/// enabled-within-IUSE term --
/// real's own n3b oracle cell (a newly added *forced* flag is enabled on
/// the ebuild side and absent from the binary's baked USE, so
/// `orig_iuse ∩ orig_use ^ cur_iuse ∩ cur_use` is non-empty).
///
/// The subtraction input is **not** intersected with the ebuild's IUSE
/// (audit D1, `LLM/02.68-74-review-through-B5.md`): real's
/// `forced_flags` is `chain(myeb.use.force, myeb.use.mask)` /
/// `chain(pkgsettings.useforce, pkgsettings.usemask)` verbatim, and
/// `myeb.use.force` comes from `config.setcpv`'s per-cpv
/// `getUseForce(cpv_slot)`/`getUseMask(cpv_slot)`
/// (`portage/package/ebuild/config.py:1973-1980`) with no IUSE filter.
/// A flag the profile masks but the new ebuild's IUSE has **dropped**
/// must still be forgiven: real keeps the binary (probed live against
/// 3.0.82.2; `_reinstall_for_flags` returns `None`), so the presence
/// term must lose it too.
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
    // #117: memoised per (config USE context, both candidates' full
    // metadata, the three flags). The function was the top named cost
    // after #105 (24.78 % with children) even though its children are the
    // already-memoised USE machinery: the residual is building each
    // downstream memo key plus the `old_iuse`/enabled-set allocations,
    // and the reference workload repeats it 10x (12,000 calls, 1,197
    // distinct inputs, measured). Pure in its arguments; thread-local so
    // the lookup stays lock-free (`EUF_CACHE`'s shape).
    //
    // Backlog #122: the `e.repo_location` hash (the uncached body reads
    // the ebuild's md5-cache entry through `candidate_iuse_and_use`, so
    // two `repo_location`s differing only behind identical candidate
    // `iuse` must not share), and the literal candidate strings bounding
    // any 64-bit digest collision to same-candidate inputs (`EUF_CACHE`'s
    // shape).
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
        // The baked USE set (backlog #124 E3): a `Some(BUILD_ID)` names
        // the build, and the baked USE is that build's recorded property
        // -- so two pool candidates sharing `(CPV, repo_name, build_id)`
        // are the same build with the same baked USE, while `build_id`
        // is precisely what distinguishes two binpkg-multi-instance
        // instances of one CPV (`dedup_binary_instances` groups by
        // `(version, slot, sub_slot, repo_name)` and ranks within by
        // `(BUILD_TIME, BUILD_ID)`). That makes the id an exact O(1)
        // stand-in for the set, replacing the old order-independent
        // xor-fold -- which built a fresh SipHash hasher per element
        // (~200 per call x 12,000 calls) on every call, hit or miss. (A
        // single shared hasher would not do: the set's iteration order
        // is not stable, which is why the fold existed.) A `None`
        // `build_id` (a `Packages` entry without `BUILD_ID`) names no
        // build, so those candidates keep the exact fold -- same cost as
        // before, never a wrong key.
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
                category,
                package,
                config,
            );
            let (iuse, use_flags) = candidate_iuse_and_use(ebuild, category, package, config)
                .unwrap_or_else(|| (old_iuse.clone(), Rc::new(binary_use.clone())));
            (iuse, use_flags, forced)
        }
        // Real's no-`myeb` arm (`depgraph.py:8265-8271`):
        // `pkgsettings.setcpv(pkg)` -- the profile's selection for the
        // binary's own cpv over its own IUSE, never the baked USE; its
        // `forced_flags` are the same settings object's
        // `useforce`/`usemask`. See this function's own doc comment for
        // the #69 R0 capture.
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
        // Real's `flags -= forced_flags` (`depgraph.py:3149-3150`) sits
        // between the IUSE symmetric difference and the enabled-set
        // union: a flag the profile force/masks never triggers a
        // rebuild *by its presence alone*, though it can still
        // contribute through the enabled-within-IUSE term below (the
        // #69 R0 n3b forced cell rejects exactly that way). Not
        // intersected with the ebuild's IUSE (audit D1): a masked/forced
        // flag the new ebuild's IUSE dropped must still be forgiven.
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

/// Re-reads `<pkgdir>/Packages` looking for `category/package-version`'s
/// own entry -- the binary-candidate counterpart to `read_md5_cache`,
/// used once a binary candidate has actually been chosen and its own
/// DEPEND/RDEPEND/BDEPEND/PDEPEND/IDEPEND (same key names real
/// `_pkgindex_aux_keys` uses, matching md5-cache's own) are needed for
/// dependency recursion. `None` if the exact version can't be found
/// (already-vanished binpkg, race with a concurrent build, etc.) --
/// same tolerance a missing/unreadable md5-cache entry already gets.
pub fn read_binary_metadata(
    index: &BinaryIndex,
    category: &str,
    package: &str,
    version: &str,
) -> Option<HashMap<String, String>> {
    read_binary_metadata_instance(index, category, package, version, None)
}

/// [`read_binary_metadata`] for one specific build (backlog #299):
/// `Some(build_id)` returns the entry carrying exactly that `BUILD_ID`
/// (`None` result if the index lacks it); `None` returns the newest build
/// of the version (see [`pick_binary_instance`]) -- never merely the first
/// entry, which under binpkg-multi-instance is usually the oldest.
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

/// `read_binary_metadata` extended to a `--getbinpkg` resolution: the
/// local `$PKGDIR/Packages` first, then each `config.binrepos` binrepo's
/// own cached `Packages` index (`BinRepo::packages_dir`). Used once a
/// binary candidate -- local *or* remote -- has been chosen and its own
/// dependency strings / `SIZE` are needed.
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

/// [`read_binary_metadata_any`] for the build the resolver selected
/// (backlog #299): with `Some(build_id)` every index (local, then each
/// binrepo) is searched for exactly that `BUILD_ID` before falling back
/// to the newest build of the version, so a chosen instance never walks
/// another instance's dependency strings.
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
