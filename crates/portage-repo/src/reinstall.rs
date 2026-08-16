//! When an installed package needs a reinstall (changed deps, slot, repo, USE).

use crate::*;

/// `--changed-deps`: whether `version`'s vdb-recorded dependency strings
/// differ from the repo's *current* ebuild for that exact version, once both
/// are flattened against the installed package's own recorded `USE` (so a
/// purely USE-driven difference never triggers this) and compared key by key
/// (`DEPEND` moving to `RDEPEND` registers as changed), structured (a plain
/// reorder registers as changed; redundant brackets don't), with
/// `strip_slot_operator_slots` and top-level-only `strip_libc_deps`.
/// A vdb-side string that fails to parse counts as "changed"; a repo-side one
/// reports "unchanged" (tolerant fallback).
/// Portage: depgraph.py, _changed_deps.
/// One `*DEPEND` string reduced to a canonical token stream (`(`/`)`
/// around every group, `||` markers kept). `None` if the string -- or any
/// atom in it -- doesn't parse. Shared by `deps_changed` (installed side)
/// and `binary_deps_changed` (a binhost binary's baked deps).
pub(crate) fn canonical_dep_key(
    depstr: &str,
    use_flags: &HashSet<String>,
    libc_cps: &HashSet<(String, String)>,
) -> Option<Vec<String>> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let reduced = portage_use_reduce::use_reduce_structured(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    let mut out: Vec<String> = Vec::with_capacity(reduced.len());
    let mut depth: usize = 0;
    for tok in reduced {
        match tok.as_str() {
            "(" => {
                depth += 1;
                out.push(tok);
            }
            ")" => {
                depth = depth.saturating_sub(1);
                out.push(tok);
            }
            "||" => out.push(tok),
            _ => {
                let evaluated = portage_dep::evaluate_atom_conditionals(&tok, use_flags)?;
                let stripped = strip_slot_operator_one(&evaluated);
                if depth == 0
                    && !libc_cps.is_empty()
                    && let Some(atom) = portage_dep::parse_atom(&stripped)
                    && libc_cps.contains(&(atom.category, atom.package))
                {
                    continue;
                }
                out.push(stripped);
            }
        }
    }
    Some(out)
}
/// Arguments of [`deps_changed`].
pub(crate) struct DepsChangedArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) repos: &'a [RepoConfig],
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    pub(crate) with_bdeps: bool,
}

pub(crate) fn deps_changed(args: DepsChangedArgs<'_>) -> bool {
    let DepsChangedArgs {
        root,
        repos,
        category,
        package,
        version,
        with_bdeps,
    } = args;
    let dep_keys: &[&str] = if with_bdeps {
        &["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"]
    } else {
        &["RDEPEND", "PDEPEND", "IDEPEND"]
    };

    let installed_use = read_vdb_flag_set(root, category, package, version, "USE");

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };

    let libc_cps = libc_provider_cps(root);
    let canonical_key = |depstr: &str| canonical_dep_key(depstr, &installed_use, &libc_cps);

    // vdb side first: an unparsable vdb string is unconditionally "changed".
    let mut vdb_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_key(&read_vdb_string(root, category, package, version, key)) {
            Some(canon) => vdb_by_key.push(canon),
            None => return true,
        }
    }

    // repo side: the ebuild is assumed well-formed, so an unparsable one
    // stays the tolerant "can't tell, don't crash" `false`.
    let mut repo_by_key: Vec<Vec<String>> = Vec::with_capacity(dep_keys.len());
    for key in dep_keys {
        match canonical_key(metadata.get(*key).map(String::as_str).unwrap_or_default()) {
            Some(canon) => repo_by_key.push(canon),
            None => return false,
        }
    }

    // Per-key comparison, element-wise like Portage's `built_deps` vs
    // `unbuilt_deps`.
    vdb_by_key != repo_by_key
}

/// Splits a raw `SLOT` string into `(slot, sub_slot)`: `SLOT="main/sub"`,
/// `sub_slot` defaulting to the slot itself when no `/` is present. An
/// empty string (missing `SLOT` file/key) defaults to `("0", "0")`, the
/// same `"0"` fallback `list_candidates`/`installed_candidates` use.
/// Portage: portage.versions slot parsing.
pub(crate) fn split_slot(raw: &str) -> (String, String) {
    if raw.is_empty() {
        return ("0".to_string(), "0".to_string());
    }
    match raw.split_once('/') {
        Some((slot, sub_slot)) => (slot.to_string(), sub_slot.to_string()),
        None => (raw.to_string(), raw.to_string()),
    }
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/SLOT` and
/// splits it via `split_slot` (the sub-slot kept, not discarded).
pub(crate) fn read_vdb_slot(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> (String, String) {
    split_slot(&read_vdb_string(root, category, package, version, "SLOT"))
}

/// `--changed-slot`: whether `version`'s vdb-recorded `SLOT` (main+sub)
/// differs from the repo's *current* ebuild for that exact version.
/// Portage: depgraph.py, _changed_slot.
///
/// Scope cut: Portage's consumers of `_changed_slot` live in
/// binary-package/slot-operator-rebuild scheduling portuale has none of;
/// ported as another independent `PretendOutcome::Reinstall` trigger
/// instead -- the same "report a reinstall" simplification
/// `--changed-deps` established. Re-reads the repo's raw `SLOT` value
/// directly (not `Candidate::slot`, which keeps the main component only).
/// A repo-side lookup that fails reports "unchanged" (`false`), matching
/// Portage's `_equiv_ebuild(pkg) is None` -> `False`.
pub(crate) fn slot_changed(
    root: &Path,
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
) -> bool {
    let vdb_slot = read_vdb_slot(root, category, package, version);

    let Ok(repo_candidates) = list_candidates(repos, category, package) else {
        return false;
    };
    let Some(resolved) = repo_candidates
        .iter()
        .filter(|c| c.version == version)
        .max_by_key(|c| c.repo_priority)
    else {
        return false;
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&resolved.repo_location, category, &pf) else {
        return false;
    };
    let repo_slot = split_slot(
        metadata
            .get("SLOT")
            .map(String::as_str)
            .unwrap_or("")
            .trim(),
    );

    vdb_slot != repo_slot
}

/// `--newrepo`: whether `version`'s vdb-recorded `repository` file differs
/// from `current_repo_name` (already resolved by the caller -- no md5-cache
/// re-read needed, unlike `slot_changed`). A vdb entry with no
/// `repository` file at all is `"__unknown__"`, a distinct value that
/// either equals `current_repo_name` or doesn't.
/// Portage: depgraph.py's straight repo-name comparison.
pub(crate) fn new_repo_changed(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    current_repo_name: &str,
) -> bool {
    let vdb_repo = read_vdb_string(root, category, package, version, "repository");
    let vdb_repo = if vdb_repo.is_empty() {
        "__unknown__"
    } else {
        &vdb_repo
    };
    vdb_repo != current_repo_name
}

/// Whether a *visible* ebuild still exists in the tree at `version`
/// (no slot comparison). Queried from the tree independently of the
/// resolver's candidate pool, so it stays meaningful under `--usepkgonly`.
/// Portage: depgraph.py, _equiv_ebuild_visible.
pub fn ebuild_visible_at(
    repos: &[RepoConfig],
    category: &str,
    package: &str,
    version: &str,
    config: &portage_profile::Config,
) -> bool {
    list_candidates(repos, category, package)
        .map(|tree| {
            tree.iter()
                .any(|c| c.version == version && is_visible(c, category, package, config))
        })
        .unwrap_or(false)
}

/// A candidate's own `is_valid_flag` domain: its declared `IUSE` unioned
/// with the profile's EAPI 5+ `IUSE_EFFECTIVE` (`config.iuse_effective`).
/// Used only for a USE-dep's `.required`/`(+)`/`(-)` check -- deliberately
/// NOT for `--newuse`'s IUSE-*presence* diff, which must stay strictly
/// declared-IUSE.
/// Portage: pkg.iuse.is_valid_flag.
pub(crate) fn valid_iuse(
    declared: &HashSet<String>,
    config: &portage_profile::Config,
) -> HashSet<String> {
    if config.iuse_effective.is_empty() {
        return declared.clone();
    }
    declared.union(&config.iuse_effective).cloned().collect()
}

/// `candidate`'s current IUSE (fresh from its md5-cache entry) and its
/// effective (computed) USE set. Shared by `reinstall_flags_for_use_change`
/// and `resolve_pretend`'s USE-dep filtering. Returns `None` if the
/// candidate's metadata can't be read at all.
pub(crate) fn candidate_iuse_and_use(
    candidate: &Candidate,
    category: &str,
    package: &str,
    config: &portage_profile::Config,
) -> Option<(HashSet<String>, Rc<HashSet<String>>)> {
    // A binary candidate carries the flags it was *actually built with*
    // over its own recorded `IUSE`, not a profile-recomputed set.
    if candidate.source == CandidateSource::Binary {
        let iuse: HashSet<String> = candidate
            .iuse
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        // Fresh `Rc`: no shared source exists for the baked set.
        let use_flags = Rc::new(candidate.binary_use.clone().unwrap_or_default());
        return Some((iuse, use_flags));
    }
    let pf = format!("{package}-{}", candidate.version);
    let metadata = repo_aux_metadata(&candidate.repo_location, category, &pf).ok()?;
    // A missing IUSE key means "declares no USE flags at all", not
    // "unreadable" -- unlike a missing md5-cache entry (the `?` above).
    let iuse: HashSet<String> = metadata
        .get("IUSE")
        .map(|s| s.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect();
    let candidate_str = format!(
        "{category}/{package}-{}:{}/{}::{}",
        candidate.version, candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse: metadata.get("IUSE").map(String::as_str).unwrap_or_default(),
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
    });
    Some((iuse, use_flags))
}

/// Arguments of [`reinstall_flags_for_use_change`].
pub(crate) struct ReinstallFlagsForUseChangeArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) candidate: &'a Candidate,
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) newuse: bool,
}

/// `--newuse`/`--changed-use`: whether `candidate` (a version already
/// installed) needs reinstalling because its currently-effective USE
/// differs from what the vdb recorded at merge time. Returns the sorted
/// list of flags that triggered it, or `None` if nothing did. If both
/// flags are given, `newuse` wins.
/// Portage: depgraph.py, _reinstall_for_flags.
///
/// Both branches share one term -- the enablement change of a flag present
/// in IUSE on both sides -- which alone is the *entire* `--changed-use`
/// formula. `--newuse` adds IUSE presence changes on top, minus profile-
/// forced flags (`config.use_force ∪ config.use_mask`).
pub(crate) fn reinstall_flags_for_use_change(
    args: ReinstallFlagsForUseChangeArgs<'_>,
) -> Option<Vec<String>> {
    let ReinstallFlagsForUseChangeArgs {
        root,
        category,
        package,
        candidate,
        config,
        newuse,
    } = args;
    let version = &candidate.version;
    let orig_use = read_vdb_flag_set(root, category, package, version, "USE");
    let orig_iuse = read_vdb_flag_set(root, category, package, version, "IUSE");

    let (cur_iuse, cur_use) = candidate_iuse_and_use(candidate, category, package, config)?;

    let orig_enabled: HashSet<String> = orig_iuse.intersection(&orig_use).cloned().collect();
    let cur_enabled: HashSet<String> = cur_iuse.intersection(&cur_use).cloned().collect();
    let mut flags: HashSet<String> = orig_enabled
        .symmetric_difference(&cur_enabled)
        .cloned()
        .collect();

    if newuse {
        let mut presence_diff: HashSet<String> =
            orig_iuse.symmetric_difference(&cur_iuse).cloned().collect();
        for forced in config.use_force.union(&config.use_mask) {
            presence_diff.remove(forced);
        }
        flags.extend(presence_diff);
    }

    if flags.is_empty() {
        return None;
    }
    let mut sorted: Vec<String> = flags.into_iter().collect();
    sorted.sort();
    Some(sorted)
}
