//! Installed packages read from the VDB.

use crate::*;

/// Every installed `(version, slot, sub_slot)` triple for `category/package` in the vdb
/// under `root` (`SLOT` via `split_slot`, `("0", "0")` when missing). `pub` so `pretend.rs`'s
/// `--deselect` implementation reuses it directly. Only `version`/`slot` feed deselect.
// Portage: lib/_emerge/actions.py, action_deselect; Atom(f"{pkg.cp}:{pkg.slot}")
pub fn installed_candidates(
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<(String, String, String)> {
    // Memoized per `(root, cat, pkg)`, validated by category-dir mtime (negative
    // results cached too). Thread-local to stay lock-free.
    type CandidatesCache =
        HashMap<(PathBuf, String, String), (Vec<(String, u64)>, Vec<(String, String, String)>)>;
    thread_local! {
        static CANDIDATES_CACHE: RefCell<CandidatesCache> = RefCell::new(HashMap::new());
    }
    let sources = installed_cp_sources(category, package);
    // Validity signal: the category directory's generation (0 when missing), read
    // through the registered backend.
    let db = portage_vdb::for_root(root);
    let mut fps: Vec<(String, u64)> = Vec::with_capacity(sources.len());
    for (src_cat, _) in &sources {
        if !fps.iter().any(|(d, _)| d == src_cat) {
            let mtime = db.category_generation(src_cat).unwrap_or(0);
            fps.push((src_cat.clone(), mtime));
        }
    }
    let key = (
        root.to_path_buf(),
        category.to_string(),
        package.to_string(),
    );
    // Compare inside the borrow; clone only on a hit.
    if let Some(out) = CANDIDATES_CACHE.with(|c| {
        c.borrow()
            .get(&key)
            .filter(|(old, _)| *old == fps)
            .map(|(_, out)| out.clone())
    }) {
        return out;
    }
    let out = installed_candidates_uncached(root, sources, category, package);
    CANDIDATES_CACHE.with(|c| {
        c.borrow_mut().insert(key, (fps, out.clone()));
    });
    out
}

pub(crate) fn installed_candidates_uncached(
    root: &Path,
    sources: Vec<(String, String)>,
    category: &str,
    package: &str,
) -> Vec<(String, String, String)> {
    // `<category>/<package>` may be the post-`move` name while the vdb still holds
    // the pre-`move` dir: scan every source name mapping onto this one, then apply
    // `slotmove` to each hit's `(slot, sub_slot)`.
    let db = portage_vdb::for_root(root);
    let mut out = Vec::new();
    for (src_cat, src_pkg) in sources {
        // Live entries only (directories, no in-progress `-MERGING-<pf>` entries).
        let Ok(names) = db.category_entries(&src_cat) else {
            continue;
        };
        for name in names {
            let Some(version) = strip_version_prefix(&name, &src_pkg) else {
                continue;
            };
            let version = version.to_string();
            // Through the shared `vdb_aux_get` seam, so a validated snapshot serves
            // `SLOT` too.
            let (slot, sub_slot) =
                split_slot(&vdb_aux_get(root, &src_cat, &src_pkg, &version, "SLOT"));
            let (slot, sub_slot) = apply_updates_to_slot(category, package, &slot, &sub_slot);
            out.push((version, slot, sub_slot));
        }
    }
    out
}

/// The repo an installed `category/package-version` was merged from: its vdb
/// `repository` file's first line, or `"__unknown__"` when absent or empty.
// Portage: portage.versions._unknown_repo
pub fn installed_pkg_repo(root: &Path, category: &str, package: &str, version: &str) -> String {
    let repo = read_vdb_string(root, category, package, version, "repository");
    if repo.is_empty() {
        "__unknown__".to_string()
    } else {
        repo
    }
}

/// Every installed version of `category/package` as an `InstalledRef`
/// (version + slot/sub_slot + repo).
pub fn installed_refs(root: &Path, category: &str, package: &str) -> Vec<InstalledRef> {
    installed_candidates(root, category, package)
        .into_iter()
        .map(|(version, slot, sub_slot)| InstalledRef {
            repo: installed_pkg_repo(root, category, package, &version),
            version,
            slot,
            sub_slot,
        })
        .collect()
}

/// The `SLOT` of the vdb entry named by its directory name `pf`, through the shared
/// [`vdb_aux_get`] seam: normalised `"slot/sub_slot"`, present-but-invalid reads as `"0"`,
/// `""` with no `SLOT` file or an unsplittable `pf`. Callers keep their own defaulting.
// Portage: aux_get, _get_slot_re
pub fn vdb_entry_slot(root: &Path, category: &str, pf: &str) -> String {
    let Some((package, version)) = split_pf(pf) else {
        return String::new();
    };
    vdb_aux_get(root, category, &package, &version, "SLOT")
}

/// Every installed version of `category/package` under `root`.
pub fn installed_versions(root: &Path, category: &str, package: &str) -> Vec<String> {
    installed_candidates(root, category, package)
        .into_iter()
        .map(|(version, _slot, _sub_slot)| version)
        .collect()
}

/// The `${ROOT}`-relative paths one installed `category/package-version` owns, from its
/// vdb `CONTENTS` file. Only the path field is returned; other line kinds are skipped, a
/// missing/unreadable `CONTENTS` reads as empty, and file order is preserved (merge order).
// Portage: vartree, dblink CONTENTS shape
pub fn installed_contents_files(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    let key = resolve_vdb_entry(root, category, package, version);
    // Invalid UTF-8 reads as empty, like a missing file.
    let text = portage_vdb::for_root(root)
        .read_file(&key, "CONTENTS")
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match (words.next(), words.next()) {
                (Some("obj"), Some(path))
                | (Some("sym"), Some(path))
                | (Some("dir"), Some(path))
                | (Some("dev"), Some(path))
                | (Some("fif"), Some(path))
                | (Some("bin"), Some(path)) => {
                    Some(path.strip_prefix('/').unwrap_or(path).to_string())
                }
                _ => None,
            }
        })
        .collect()
}

/// Every installed `category/package-version` whose recorded `DEPEND`/`RDEPEND`/`BDEPEND`/
/// `PDEPEND` (each flattened against its own vdb `USE`) names an atom the installed consumer
/// satisfies. Blocker atoms never count. Sorted, deduplicated; empty when nothing depends on it.
// Portage: vardb.match(atom)
pub fn installed_reverse_dependents(
    root: &Path,
    consumer_category: &str,
    consumer_package: &str,
    consumer_version: &str,
) -> Vec<String> {
    let (slot, sub_slot) =
        read_vdb_slot(root, consumer_category, consumer_package, consumer_version);
    let candidate =
        format!("{consumer_category}/{consumer_package}-{consumer_version}:{slot}/{sub_slot}");
    let db = portage_vdb::for_root(root);
    // A database backend narrows the entries via its dependency index; the
    // reduction and match below are the same code as the scan's. `files` (and a
    // backend that cannot answer) keeps the scan.
    if db.kind() != portage_vdb::BackendKind::Files
        && let Some(found) = indexed_reverse_dependents(
            db.as_ref(),
            consumer_category,
            consumer_package,
            consumer_version,
            &candidate,
        )
    {
        return found;
    }
    let mut out = std::collections::BTreeSet::new();
    for pkg in all_installed_packages(root) {
        if pkg.category == consumer_category
            && pkg.package == consumer_package
            && pkg.version == consumer_version
        {
            continue;
        }
        let use_flags = read_vdb_flag_set(root, &pkg.category, &pkg.package, &pkg.version, "USE");
        for key in ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND"] {
            let depstr = read_vdb_string(root, &pkg.category, &pkg.package, &pkg.version, key);
            if depstr_names_consumer(
                &depstr,
                &use_flags,
                consumer_category,
                consumer_package,
                &candidate,
            ) {
                out.insert(format!("{}/{}-{}", pkg.category, pkg.package, pkg.version));
            }
        }
    }
    out.into_iter().collect()
}

/// Whether `depstr`, flattened against `use_flags`, has a non-blocker atom that
/// `candidate` (`cat/pkg-ver:slot/subslot`) satisfies. Shared by both halves of
/// [`installed_reverse_dependents`].
pub(crate) fn depstr_names_consumer(
    depstr: &str,
    use_flags: &HashSet<String>,
    consumer_category: &str,
    consumer_package: &str,
    candidate: &str,
) -> bool {
    if depstr.trim().is_empty() {
        return false;
    }
    let Some(atoms) = flat_dep_atoms(depstr, use_flags) else {
        return false;
    };
    atoms.iter().any(|atom_str| {
        let Some(atom) = portage_dep::parse_atom(atom_str) else {
            return false;
        };
        atom.blocker == portage_dep::Blocker::None
            && atom.category == consumer_category
            && atom.package == consumer_package
            && portage_dep::match_from_list(atom_str, &[candidate]).is_some_and(|m| !m.is_empty())
    })
}

/// The database-backend half of [`installed_reverse_dependents`]: the backend's
/// raw records for the consumer's `category/package` (a superset), put through
/// the same per-entry steps as the scan. `None` when the backend cannot answer,
/// so the caller falls back to the scan.
pub(crate) fn indexed_reverse_dependents(
    db: &dyn portage_vdb::InstalledDb,
    consumer_category: &str,
    consumer_package: &str,
    consumer_version: &str,
    candidate: &str,
) -> Option<Vec<String>> {
    use portage_vdb::DepClass;
    let cp = format!("{consumer_category}/{consumer_package}");
    let records = db
        .reverse_dependents(
            &cp,
            &[
                DepClass::Depend,
                DepClass::Rdepend,
                DepClass::Bdepend,
                DepClass::Pdepend,
            ],
        )
        .ok()?;
    let mut out = std::collections::BTreeSet::new();
    for rec in records {
        let Some((name, version)) = split_pf(&rec.key.pf) else {
            continue;
        };
        let (category, package) = apply_updates_to_cp(&rec.key.category, &name);
        if category == consumer_category
            && package == consumer_package
            && version == consumer_version
        {
            continue;
        }
        let use_flags: HashSet<String> = rec
            .use_flags
            .split_whitespace()
            .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
            .collect();
        if rec.deps.iter().any(|(_, depstr)| {
            depstr_names_consumer(
                depstr,
                &use_flags,
                consumer_category,
                consumer_package,
                candidate,
            )
        }) {
            out.insert(format!("{category}/{package}-{version}"));
        }
    }
    Some(out.into_iter().collect())
}

/// The highest installed version of `category/package` satisfying `atom_str`, or
/// `None`. The graph-or-installed fallback complete mode uses this so its deep
/// re-walk of the required sets never merges a new package.
// Portage: depgraph.py, _select_pkg_from_installed
pub(crate) fn best_installed_for_atom(
    root: &Path,
    atom_str: &str,
    category: &str,
    package: &str,
) -> Option<String> {
    let cands = installed_candidates(root, category, package);
    let strs: Vec<String> = cands
        .iter()
        .map(|(v, s, ss)| format!("{category}/{package}-{v}:{s}/{ss}::__installed__"))
        .collect();
    let refs: Vec<&str> = strs.iter().map(String::as_str).collect();
    let matched = portage_dep::match_from_list(atom_str, &refs)?;
    cands
        .iter()
        .zip(strs.iter())
        .filter(|(_, s)| matched.contains(&s.as_str()))
        .map(|((v, _, _), _)| v.clone())
        .max_by(|a, b| vercmp_ordering(a, b))
}

/// The installed version a complete-mode withhold keeps: the highest installed
/// version satisfying the requesting atom and every positive pin, or `None`
/// when no installed instance satisfies everything (a genuine absence).
// Portage: depgraph.py, _complete_graph via _select_pkg_from_graph
pub(crate) fn complete_mode_withheld_version(
    root: &Path,
    category: &str,
    package: &str,
    atom_str: &str,
    positives: &[String],
) -> Option<String> {
    installed_candidates(root, category, package)
        .iter()
        .map(|(v, s, ss)| {
            (
                v.clone(),
                format!("{category}/{package}-{v}:{s}/{ss}::__installed__"),
            )
        })
        .filter(|(_, s)| {
            portage_dep::match_from_list(atom_str, &[s.as_str()]).is_some_and(|m| !m.is_empty())
                && positives.iter().all(|p| {
                    portage_dep::match_from_list(p, &[s.as_str()]).is_some_and(|m| !m.is_empty())
                })
        })
        .map(|(v, _)| v)
        .max_by(|a, b| vercmp_ordering(a, b))
}

/// Whether `atom_str` is satisfied by anything installed under `running_root`'s vdb --
/// `--root-deps`'s `ESYSROOT`-vs-`ROOT` distinction, narrowed to an "is it already there"
/// existence check. Generic over `running_root`: only the CLI boundary points it at `/`.
/// A version match alone is not enough: the matched instance's recorded `USE` must also
/// satisfy the atom's use-deps (the version-only match is USE-blind). Conditional forms
/// (`=`/`?`) stay vacuous here as in `match_from_list` -- the caller evaluates those first.
// Portage: dep_zapdeps matching installed alternatives with USE
pub(crate) fn running_root_satisfies_atom(
    atom_str: &str,
    running_root: &Path,
    config: &portage_profile::Config,
) -> bool {
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return false;
    };
    let candidates = installed_candidates(running_root, &atom.category, &atom.package);
    let candidate_strs: Vec<String> = candidates
        .iter()
        .map(|(version, slot, sub_slot)| {
            format!(
                "{}/{}-{version}:{slot}/{sub_slot}",
                atom.category, atom.package
            )
        })
        .collect();
    let candidate_str_refs: Vec<&str> = candidate_strs.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(atom_str, &candidate_str_refs) else {
        return false;
    };
    if matched.is_empty() {
        return false;
    }
    let Some(use_deps) = atom.use_deps.as_ref().filter(|d| !d.is_empty()) else {
        return true;
    };
    let by_str: HashMap<&str, &(String, String, String)> = candidate_str_refs
        .iter()
        .copied()
        .zip(candidates.iter())
        .collect();
    matched.into_iter().any(|m| {
        let Some((version, _, _)) = by_str.get(m) else {
            return false;
        };
        let vdb_iuse =
            read_vdb_flag_set(running_root, &atom.category, &atom.package, version, "IUSE");
        let vdb_use =
            read_vdb_flag_set(running_root, &atom.category, &atom.package, version, "USE");
        let mut valid = valid_iuse(&vdb_iuse, config);
        valid.extend(vdb_use.iter().cloned());
        portage_dep::use_deps_satisfied(use_deps, &valid, &vdb_use)
    })
}

/// One field of one installed entry, served by [`portage_vdb`]'s `FilesDb::aux_get`. What
/// stays here is the package-move fallback (every `installed_cp_sources` name tried in order,
/// first existing directory wins), plus: a missing entry reads as `""`, and a key outside the
/// 23-set bypasses snapshot/cache/normalisation (raw read, lossy UTF-8).
// Portage: global-updates renaming the vdb dir; bug 395463
pub(crate) fn vdb_aux_get(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    key: &str,
) -> String {
    let db = portage_vdb::for_root(root);
    let pf = format!("{package}-{version}");
    if !in_metadata_file(key) {
        // Resolve like `resolve_vdb_entry`, then one `open` of the raw file.
        let direct = portage_vdb::EntryKey::new(category, pf.as_str());
        let mut found: Option<portage_vdb::EntryKey> = None;
        if db.has_entry(&direct).unwrap_or(false) {
            found = Some(direct.clone());
        } else if !global_package_updates().is_empty() {
            for (c, p) in installed_cp_sources(category, package) {
                let cand = portage_vdb::EntryKey::new(c, format!("{p}-{version}"));
                if db.has_entry(&cand).unwrap_or(false) {
                    found = Some(cand);
                    break;
                }
            }
        }
        let key_to_read = found.unwrap_or(direct);
        return match db.read_file(&key_to_read, key) {
            Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
            _ => String::new(),
        };
    }
    let direct = portage_vdb::EntryKey::new(category, pf);
    match db.aux_get(&direct, key) {
        Ok(Some(v)) => return v,
        Ok(None) => {}
        Err(_) => return String::new(),
    }
    if !global_package_updates().is_empty() {
        for (c, p) in installed_cp_sources(category, package) {
            let cand = portage_vdb::EntryKey::new(c, format!("{p}-{version}"));
            if let Ok(Some(v)) = db.aux_get(&cand, key) {
                return v;
            }
        }
    }
    String::new()
}

/// Reads a vdb aux file (e.g. `USE`/`IUSE`) as a flag-name set, one per
/// whitespace-separated token, `+`/`-` IUSE default markers stripped. A missing
/// file is an empty set, not an error. Served from [`vdb_aux_get`].
pub(crate) fn read_vdb_flag_set(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> HashSet<String> {
    vdb_aux_get(root, category, package, version, filename)
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .collect()
}

/// `(IUSE, USE)` bare-name flag sets from an installed package's vdb, for a use-dep check
/// against an already-merged package. The recorded `USE` is final: `[flag]` holds iff
/// `flag ∈ USE`; `[flag(+)]`/`[flag(-)]` fall back to the default only when `flag ∉ IUSE`.
// Portage: vardb.match honouring a cat/pkg[flag] atom
pub fn installed_pkg_iuse_and_use(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> (HashSet<String>, HashSet<String>) {
    (
        read_vdb_flag_set(root, category, package, version, "IUSE"),
        read_vdb_flag_set(root, category, package, version, "USE"),
    )
}

/// The `(iuse_flag, enabled)` display list for a tree ebuild -- the resolver's own per-flag
/// resolution (`effective_use_flags` over the candidate's `IUSE`). Lets `emerge --resume`
/// populate reconstructed entries (which record only `cat/pkg-ver`); without it the build env
/// gets an empty `USE=` and USE-conditional ebuilds `die`. Empty when unlocatable or IUSE-less.
// Portage: pkg_use_display
pub fn candidate_use_flags_display(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<(String, bool)> {
    let Some(candidate) = list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| cs.iter().find(|c| c.version == version).cloned())
    else {
        return Vec::new();
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&candidate.repo_location, category, &pf) else {
        return Vec::new();
    };
    let Some(iuse) = metadata.get("IUSE") else {
        return Vec::new();
    };
    let candidate_str = format!(
        "{category}/{package}-{version}:{}/{}::{}",
        candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse,
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
    });
    let mut seen: HashSet<String> = HashSet::new();
    let mut display: Vec<(String, bool)> = iuse
        .split_whitespace()
        .map(|tok| tok.trim_start_matches(['+', '-']).to_string())
        .filter(|flag| seen.insert(flag.clone()))
        .map(|flag| {
            let on = use_flags.contains(&flag);
            (flag, on)
        })
        .collect();
    display.sort_by_key(|p| alnum_sort_key(&p.0));
    display
}

/// The package's **full** effective `USE` set -- everything `effective_use_flags` resolved
/// enabled, including implicit profile flags no package declares in `IUSE`. The caller narrows
/// it through `portage_profile::portage_use`. Empty when the candidate/md5-cache is unavailable.
// Portage: bin/ebuild.sh use(), config.py; __dyn_install USE file
pub fn candidate_effective_use_flags(
    repos: &[RepoConfig],
    config: &portage_profile::Config,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    let Some(candidate) = list_candidates(repos, category, package)
        .ok()
        .and_then(|cs| cs.iter().find(|c| c.version == version).cloned())
    else {
        return Vec::new();
    };
    let pf = format!("{package}-{version}");
    let Ok(metadata) = repo_aux_metadata(&candidate.repo_location, category, &pf) else {
        return Vec::new();
    };
    // A missing `IUSE` key is an empty `IUSE`, not missing metadata: the md5-cache
    // writer omits empty fields, but the package still gets its implicit USE.
    let iuse = metadata.get("IUSE").map(String::as_str).unwrap_or("");
    let candidate_str = format!(
        "{category}/{package}-{version}:{}/{}::{}",
        candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(EffectiveUseFlagsArgs {
        config,
        iuse,
        keywords: &candidate.keywords,
        candidate_str: &candidate_str,
        category,
        package,
    });
    let mut sorted: Vec<String> = use_flags.iter().cloned().collect();
    sorted.sort_by_key(|f| alnum_sort_key(f));
    sorted
}

/// The vdb entry for `<category>/<package>-<version>`. Portuale never writes, so when the
/// direct entry is absent (a package move renamed it) every `installed_cp_sources` name is
/// tried, first existing directory wins. A missing entry returns the direct key (reads absent).
// Portage: global-updates renaming the vdb dir
pub(crate) fn resolve_vdb_entry(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> portage_vdb::EntryKey {
    let db = portage_vdb::for_root(root);
    let direct = portage_vdb::EntryKey::new(category, format!("{package}-{version}"));
    if db.has_entry(&direct).unwrap_or(false) || global_package_updates().is_empty() {
        return direct;
    }
    for (c, p) in installed_cp_sources(category, package) {
        let cand = portage_vdb::EntryKey::new(c, format!("{p}-{version}"));
        if db.has_entry(&cand).unwrap_or(false) {
            return cand;
        }
    }
    direct
}

/// Whether the resolved vdb entry exists (tests the resolved entry again).
pub(crate) fn vdb_entry_is_dir(root: &Path, category: &str, package: &str, version: &str) -> bool {
    let key = resolve_vdb_entry(root, category, package, version);
    portage_vdb::for_root(root).has_entry(&key).unwrap_or(false)
}

/// Reads a vdb aux file as a raw string (e.g. `DEPEND`/`RDEPEND`) -- unlike `read_vdb_flag_set`,
/// the value stays intact for `use_reduce_flat`. A missing file is an empty string.
/// Whitespace-normalised like `_aux_get`; line-oriented keys (`CONTENTS`, `NEEDED.*`) bypass
/// normalisation via [`vdb_aux_get`], whose validated snapshot also avoids per-key opens.
// Portage: vartree._aux_get
pub(crate) fn read_vdb_string(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> String {
    vdb_aux_get(root, category, package, version, filename)
}

/// Flattens `depstr` against `use_flags` into dependency-atom tokens (`||`
/// dropped) for order-independent comparison. `None` if `depstr` doesn't parse.
pub(crate) fn flat_dep_atoms(depstr: &str, use_flags: &HashSet<String>) -> Option<HashSet<String>> {
    let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
    let flat = portage_use_reduce::use_reduce_flat(
        &tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    Some(flat.into_iter().filter(|t| t != "||").collect())
}

/// The `(category, package)` of every atom `virtual/libc`'s installed `RDEPEND` names
/// (flattened against its installed `USE`) -- empty when `virtual/libc` isn't installed. One
/// level only. Used by `deps_changed` to strip libc noise, and by `inject_libc_dep` for providers.
// Portage: portage.dep.libc.find_libc_deps, strip_libc_deps, doebuild._inject_libc_dep
pub fn libc_provider_cps(root: &Path) -> HashSet<(String, String)> {
    let mut result = HashSet::new();
    for version in installed_versions(root, "virtual", "libc") {
        let use_flags = read_vdb_flag_set(root, "virtual", "libc", &version, "USE");
        let rdepend = read_vdb_string(root, "virtual", "libc", &version, "RDEPEND");
        let Some(atoms) = flat_dep_atoms(&rdepend, &use_flags) else {
            continue;
        };
        for atom_str in atoms {
            if let Some(atom) = portage_dep::parse_atom(&atom_str) {
                result.insert((atom.category, atom.package));
            }
        }
    }
    result
}

/// One installed package in the vdb, `category/package-version` plus its recorded
/// main `SLOT`.
// Portage: vartree.dbapi.cpv_all, _pkg_str
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub category: String,
    pub package: String,
    pub version: String,
    pub slot: String,
}

/// Every package recorded under `<root>/var/db/pkg`, each with its main slot. Directory names
/// split into `package`/`version` at the last `-` boundary whose right half verifies as a version.
/// Memoised per `root`, keyed by the vdb generation so a same-process merge/unmerge is picked up.
// Portage: vartree.dbapi.cpv_all
pub fn all_installed_packages(root: &Path) -> Vec<InstalledPackage> {
    // root -> (vdb fingerprint at scan time, scanned set)
    type InstalledCache = HashMap<PathBuf, (u64, std::sync::Arc<Vec<InstalledPackage>>)>;
    static CACHE: OnceLock<RwLock<InstalledCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    // ~30 `stat`s vs the ~2000 `SLOT` file reads a full scan does.
    let fingerprint = portage_vdb::for_root(root).generation().unwrap_or(0);
    if let Ok(guard) = cache.read()
        && let Some((fp, packages)) = guard.get(root)
        && *fp == fingerprint
    {
        return packages.as_ref().clone();
    }
    let packages = std::sync::Arc::new(all_installed_packages_uncached(root));
    if let Ok(mut guard) = cache.write() {
        guard.insert(
            root.to_path_buf(),
            (fingerprint, std::sync::Arc::clone(&packages)),
        );
    }
    packages.as_ref().clone()
}

pub(crate) fn all_installed_packages_uncached(root: &Path) -> Vec<InstalledPackage> {
    let mut out = Vec::new();
    // `-MERGING-` entries already skipped.
    let Ok(keys) = portage_vdb::for_root(root).entries() else {
        return out;
    };
    for key in keys {
        let category = key.category;
        let dirname = key.pf;
        {
            let Some((name, version)) = split_pf(&dirname) else {
                continue;
            };
            // Through the shared `vdb_aux_get` seam, so a validated snapshot serves
            // `SLOT` without per-entry opens.
            let (slot, sub) = split_slot(&vdb_aux_get(root, &category, &name, &version, "SLOT"));
            // Present each vdb entry under its post-`move`/`slotmove` identity, the
            // way global-updates rewrites the vdb on sync.
            let (category, name) = apply_updates_to_cp(&category, &name);
            let (slot, _sub) = apply_updates_to_slot(&category, &name, &slot, &sub);
            out.push(InstalledPackage {
                category: category.clone(),
                package: name,
                version,
                slot,
            });
        }
    }
    out
}
