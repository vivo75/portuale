//! Installed packages' metadata as the resolver sees it.

use crate::*;

/// The vdb record verbatim (`Raw`) vs what Portage's dynamic-deps overlay leaves
/// behind (`Effective`: live ebuild `*DEPEND` plus the vdb's own built slot-operator
/// atoms). With `--dynamic-deps=n` the two collapse into one.
/// Portage: FakeVartree.py, _apply_dynamic_deps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InstalledMetaLayer {
    Raw,
    Effective,
}

/// The metadata view an installed package's own deps walk against: the live
/// ebuild metadata while the version still has an ebuild, else the
/// vdb-recorded snapshot (moves applied) for an installed-only package.
/// Portage walks the vardb in both cases (the S0 `s0-e-gbapp-pvuD`
/// capture walks installed-only `pg34-gone`'s `RDEPEND`).
/// Portage: FakeVartree.py, _aux_get_wrapper.
pub(crate) fn installed_walk_metadata(
    repos: &[RepoConfig],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> std::sync::Arc<HashMap<String, String>> {
    if let Some(live) = live_metadata_for_installed(repos, root, category, package, version) {
        return live;
    }
    let mut map = HashMap::new();
    for key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND", "EAPI"] {
        let raw = read_vdb_string(root, category, package, version, key);
        if raw.trim().is_empty() {
            continue;
        }
        map.insert(
            key.to_string(),
            apply_updates_to_dep_string(&raw).unwrap_or(raw),
        );
    }
    std::sync::Arc::new(map)
}

/// The live ebuild metadata for one installed `cat/pkg-version`, always from the
/// repo the vdb says it was built from, never from a repo-priority search. `None`
/// when the version is gone from the recorded repo. One shared helper, so the
/// installed-metadata readers agree on which repo is authoritative
/// ([`installed_walk_metadata`] falls back to the vdb snapshot on `None`).
/// Portage: FakeVartree.py, _aux_get_wrapper.
pub(crate) fn live_metadata_for_installed(
    repos: &[RepoConfig],
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Option<std::sync::Arc<HashMap<String, String>>> {
    let pf = format!("{package}-{version}");
    let recorded = read_vdb_string(root, category, package, version, "repository");
    if !recorded.is_empty()
        && let Some(repo) = repos.iter().find(|r| r.name == recorded)
        && let Ok(meta) = repo_aux_metadata(&repo.location, category, &pf)
    {
        return Some(meta);
    }
    // Fallback narrowing: a missing `repository` file (minimal fixture vdbs) or a
    // removed repo falls back to a priority search instead of stranding the package.
    list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| {
            cs.iter()
                .filter(|c| c.version == version)
                .max_by_key(|c| c.repo_priority)
                .and_then(|c| repo_aux_metadata(&c.repo_location, category, &pf).ok())
        })
}

/// Every atom in one raw (vdb) `*DEPEND` string that is a *built* slot-operator dep
/// (`:=` with a sub-slot, flattened against the recorded USE). A `use_reduce`
/// failure reads as no built atoms. Portage: _slot_operator.py, find_built_slot_operator_atoms.
pub(crate) fn built_slot_operator_atoms(
    raw_depstr: &str,
    use_flags: &HashSet<String>,
) -> Vec<String> {
    let tokens: Vec<String> = raw_depstr.split_whitespace().map(String::from).collect();
    let Ok(flat) = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return Vec::new();
    };
    flat.into_iter()
        .filter(|tok| {
            parse_dep_atom(tok, "installed_meta").is_some_and(|a| {
                a.slot_operator == Some(portage_dep::SlotOperator::Equals) && a.sub_slot.is_some()
            })
        })
        .collect()
}

/// Arguments of [`installed_dep_string`].
pub(crate) struct InstalledDepStringArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) dynamic_deps: bool,
    pub(crate) ignore_built_slot_operator_deps: bool,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) version: &'a str,
    /// The repo's current metadata for this exact version; `None` when the version is
    /// gone from every repo.
    pub(crate) live: Option<&'a HashMap<String, String>>,
    pub(crate) key: &'a str,
    pub(crate) layer: InstalledMetaLayer,
    /// Caches the Effective result only, per pass, keyed `(category, package, version,
    /// key)`. The key stays valid with the overlay inputs: the ignore flag is a
    /// run-constant CLI flag, and both EAPIs are stable per package within a run.
    pub(crate) memo: &'a mut HashMap<(String, String, String, String), String>,
}

/// One installed package's `*DEPEND` string for `key`, at `layer`. One shared view for
/// the four installed-metadata readers, so there is no second copy. Per arm:
/// - no live metadata, or either EAPI unsupported: the raw record stands, with global
///   package moves applied (a missing vdb `EAPI` file reads as `"0"`);
/// - `--ignore-built-slot-operator-deps`, or an installed EAPI without `:=` support:
///   the live ebuild string stands alone;
/// - `IDEPEND` from a live EAPI without `idepend` support: `""`;
/// - otherwise the vdb's own built `:=` atoms for this key are appended to the live
///   string; when the live EAPI itself lacks `:=` support the overlay is void.
///
/// Portage: FakeVartree.py, _apply_dynamic_deps.
pub(crate) fn installed_dep_string(args: InstalledDepStringArgs<'_>) -> String {
    let InstalledDepStringArgs {
        root,
        dynamic_deps,
        ignore_built_slot_operator_deps,
        category,
        package,
        version,
        live,
        key,
        layer,
        memo,
    } = args;
    let raw = read_vdb_string(root, category, package, version, key);
    if layer == InstalledMetaLayer::Raw || !dynamic_deps {
        return raw;
    }
    let memo_key = (
        category.to_string(),
        package.to_string(),
        version.to_string(),
        key.to_string(),
    );
    if let Some(cached) = memo.get(&memo_key) {
        return cached.clone();
    }
    let result = match live {
        // No live ebuild metadata, so the raw record stands, with moves applied.
        None => apply_updates_to_dep_string(&raw).unwrap_or(raw),
        Some(live) => {
            let live_value = live
                .get(key)
                .map(String::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let live_eapi = live
                .get("EAPI")
                .map(String::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let inst_eapi_raw = read_vdb_string(root, category, package, version, "EAPI");
            let inst_eapi = inst_eapi_raw.trim();
            let inst_eapi = if inst_eapi.is_empty() { "0" } else { inst_eapi };
            if !(md5_dict::eapi_is_supported(&live_eapi) && md5_dict::eapi_is_supported(inst_eapi))
            {
                apply_updates_to_dep_string(&raw).unwrap_or_else(|| raw.clone())
            } else if key == "IDEPEND" && !md5_dict::eapi_has_idepend(&live_eapi) {
                // Only the live-ebuild overlay needs the gate (an older build never
                // wrote an `IDEPEND` file); installed-metadata readers see `""`.
                String::new()
            } else if ignore_built_slot_operator_deps
                || !md5_dict::eapi_has_slot_operator(inst_eapi)
            {
                live_value
            } else {
                let use_flags = read_vdb_flag_set(root, category, package, version, "USE");
                let built = built_slot_operator_atoms(&raw, &use_flags);
                if built.is_empty() {
                    live_value
                } else if !md5_dict::eapi_has_slot_operator(&live_eapi) {
                    apply_updates_to_dep_string(&raw).unwrap_or_else(|| raw.clone())
                } else if live_value.is_empty() {
                    built.join(" ")
                } else {
                    format!("{live_value} {}", built.join(" "))
                }
            }
        }
    };
    memo.insert(memo_key, result.clone());
    result
}
