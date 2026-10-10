//! Installed packages read from the VDB.

use crate::*;

/// Lists every installed `(version, slot, sub_slot)` triple for
/// `category/package` found in the vdb under `root`
/// (`<root>/var/db/pkg/<category>/<package>-<version>/`), reading each
/// entry's `SLOT` file via `split_slot` (defaulting to `("0", "0")` if
/// missing, same fallback as `list_candidates`). Used for blocker
/// matching, which needs slots (sub-slot included, same "closes a real
/// gap" reasoning as `Candidate::sub_slot`'s own doc comment) to support
/// slotted blocker atoms -- `installed_versions` below doesn't need this
/// and stays a plain version list for its existing callers. `pub`
/// (unlike most of this crate's own internal helpers) so
/// `portuale/src/pretend.rs`'s own `--deselect`/`-W` implementation can
/// reuse it directly -- real `action_deselect` (`lib/_emerge/actions.py`)
/// only ever consults `vardb`/the world file, never repos/config at
/// all, so this is the one real feature in portuale with no need for
/// `portage-repo`'s own repo/config-resolution machinery. `run_deselect`
/// itself only ever uses `version`/`slot` from this (real
/// `Atom(f"{pkg.cp}:{pkg.slot}")` never includes sub-slot either), so
/// adding `sub_slot` here doesn't change its behavior at all.
pub fn installed_candidates(
    root: &Path,
    category: &str,
    package: &str,
) -> Vec<(String, String, String)> {
    // Per-`(root, cat, pkg)` memo of the scan below: a deep `emerge -pu`
    // asks for the same few thousand cps over and over (12,684 calls on
    // the reference workload), and every miss re-reads a whole category
    // directory plus one `SLOT` file per entry. Validated by the mtime of
    // each scanned category dir (1–2 `statx`, not ~80); negative results
    // are cached too. Thread-local so the lookup stays lock-free.
    type CandidatesCache =
        HashMap<(PathBuf, String, String), (Vec<(String, u64)>, Vec<(String, String, String)>)>;
    thread_local! {
        static CANDIDATES_CACHE: RefCell<CandidatesCache> = RefCell::new(HashMap::new());
    }
    let sources = installed_cp_sources(category, package);
    // feat#157 S1.2: the validity signal is the category directory's
    // generation (`st_mtime` in nanos, 0 when missing: every portuale vdb
    // mutation replaces or removes a package dir, so the *category* dir
    // mtime catches all of them), read through the registered backend.
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
    // Backlog #124 E1: compare inside the borrow and clone only the hit
    // -- the old `.get(&key).cloned()` deep-cloned `Vec<(PathBuf, u64)>`
    // solely to compare it and drop it, on every one of the 12,684 hits.
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
    // Real `profiles/updates/` package moves: `<category>/<package>` may
    // be the *post*-`move` name while the vdb still holds the package
    // under its pre-`move` `cat/pkg` dir. Scan every source name that
    // maps onto this one (`installed_cp_sources`); `slotmove` then
    // rewrites each hit's `(slot, sub_slot)`.
    let db = portage_vdb::for_root(root);
    let mut out = Vec::new();
    for (src_cat, src_pkg) in sources {
        // Live entries only (directories, no in-progress `-MERGING-<pf>`
        // entry -- real `vardbapi._excluded_dirs`), in listing order.
        let Ok(names) = db.category_entries(&src_cat) else {
            continue;
        };
        for name in names {
            let Some(version) = strip_version_prefix(&name, &src_pkg) else {
                continue;
            };
            let version = version.to_string();
            // #109 S3: the same `vdb_aux_get` seam the other vdb readers
            // use, so a validated consolidated snapshot serves `SLOT`
            // too. `resolve_vdb_entry` resolves this same entry back from
            // `(src_cat, src_pkg, version)`, so the value is unchanged on
            // the fallback path (the old read `.trim()`ed, the helper
            // normalises).
            let (slot, sub_slot) =
                split_slot(&vdb_aux_get(root, &src_cat, &src_pkg, &version, "SLOT"));
            let (slot, sub_slot) = apply_updates_to_slot(category, package, &slot, &sub_slot);
            out.push((version, slot, sub_slot));
        }
    }
    out
}

/// The repo an installed `category/package-version` was merged from --
/// its vdb `repository` file's first line, or `"__unknown__"` (real
/// `portage.versions._unknown_repo`) when that file is absent or empty.
pub fn installed_pkg_repo(root: &Path, category: &str, package: &str, version: &str) -> String {
    let repo = read_vdb_string(root, category, package, version, "repository");
    if repo.is_empty() {
        "__unknown__".to_string()
    } else {
        repo
    }
}

/// Every installed version of `category/package` as an `InstalledRef`
/// (version + slot/sub_slot from `SLOT`, repo from `installed_pkg_repo`).
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

/// The `SLOT` of the vdb entry named by its directory name `pf`
/// (`foo-bar-1.2.3-r1`), read through the same [`vdb_aux_get`] seam
/// every other installed-metadata consumer uses: the normalised
/// `"slot/sub_slot"` string, multi-line values collapsed to one
/// space-separated line, a present-but-invalid value translated to
/// `"0"` (real `aux_get`'s `_get_slot_re` rule, #115 S1), `""` for an
/// entry with no `SLOT` file or a `pf` [`split_pf`] cannot split. No
/// further splitting -- callers keep their own main-slot projection
/// and their own empty-case defaulting (#126 O5).
pub fn vdb_entry_slot(root: &Path, category: &str, pf: &str) -> String {
    let Some((package, version)) = split_pf(pf) else {
        return String::new();
    };
    vdb_aux_get(root, category, &package, &version, "SLOT")
}

/// Lists every installed version of `category/package` found in the vdb
/// under `root` (`<root>/var/db/pkg/<category>/<package>-<version>/`).
pub fn installed_versions(root: &Path, category: &str, package: &str) -> Vec<String> {
    installed_candidates(root, category, package)
        .into_iter()
        .map(|(version, _slot, _sub_slot)| version)
        .collect()
}

/// The ROOT-relative paths one installed `category/package-version`
/// owns, read from its vdb `CONTENTS` file (real `vartree`/`dblink`
/// shape: `obj <path> <md5> <mtime>`, `sym <path> -> <target> <mtime>`,
/// `dir <path>`; `dev`/`fif`/`bin` device-node lines the same way).
/// Only the path field is returned (the second whitespace token, with
/// one leading `/` stripped -- the vdb records `${ROOT}`-absolute
/// paths, the seam speaks `${ROOT}`-relative ones, matching the
/// `mrg-director` `MemoryDb` convention); other line kinds are skipped,
/// a missing/unreadable `CONTENTS` reads as empty -- the same tolerance
/// `read_vdb_string` gives a missing key. File order is preserved
/// (merge order), not sorted.
pub fn installed_contents_files(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    let key = resolve_vdb_entry(root, category, package, version);
    // `read_to_string` of old: invalid UTF-8 reads as empty, like a
    // missing file.
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

/// Every installed `category/package-version` whose vdb-recorded
/// `DEPEND`/`RDEPEND`/`BDEPEND`/`PDEPEND` (each flattened against that
/// package's own vdb `USE`) names an atom the installed
/// `consumer_category/consumer_package-consumer_version` satisfies --
/// real `vardb.match(atom)` direction, via `portage_dep::match_from_list`
/// against a `cat/pkg-ver:slot/sub_slot` candidate built from the
/// consumer's own installed slot (the same candidate shape
/// `best_installed_for_atom` matches against). Blocker atoms (`!`/`!!`)
/// never count (real `match` on a blocker is a conflict query, not a
/// dependency). Sorted, deduplicated; empty when nothing depends on it.
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
    // S8.1: a database backend narrows the entries with its dependency
    // index and hands over their raw records; the reduction and the match
    // below are the same code as the scan's. `files` (and a backend that
    // cannot answer) keeps the scan, with its exact syscall pattern.
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

/// Whether the (non-empty) dependency string `depstr`, flattened against
/// `use_flags`, has a non-blocker atom for the consumer that `candidate`
/// (`cat/pkg-ver:slot/subslot`) satisfies. The reduction and match shared
/// by both ways of [`installed_reverse_dependents`].
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

/// The database-backend half of [`installed_reverse_dependents`]: the raw
/// records `InstalledDb::reverse_dependents` returns for the consumer's
/// `category/package` (a superset: the backend's index drops only entries
/// that cannot name it), put through the same per-entry steps as the scan
/// (`split_pf`, the package-move identity, skip the consumer itself, the
/// entry's own `USE`, the four `*DEPEND` keys). `None` when the backend
/// cannot answer, so the caller falls back to the scan.
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

/// The highest installed version of `category/package` that satisfies
/// `atom_str`, or `None`. Real `_select_pkg_from_installed`
/// (`depgraph.py:8518`) -- the graph-or-installed fallback complete mode
/// uses so its deep re-walk of the required sets never merges a new
/// package.
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

/// Backlog #210: the installed version a complete-mode withhold keeps.
///
/// Real `_complete_graph` swaps package selection to
/// `_select_pkg_from_graph` (`depgraph.py:8662`) -- graph-or-installed,
/// never a new merge -- so a phase-1 merge an enforced parent-atom pin
/// (or a slot-conflict reconciliation) leaves with no mergeable
/// candidate stays installed when the installed instance satisfies the
/// requesting atom and every positive pin: real keeps it (nomerge) and
/// reports the skipped update, it does not fail the run. Returns the
/// highest such installed version, `None` when no installed instance
/// satisfies everything (a genuine absence -- the abort stands).
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

/// `--root-deps`'s own real `ESYSROOT`-vs-`ROOT` distinction, narrowed to
/// an "is it already there" existence check (see `enqueue_dependencies`'s
/// own doc comment for the full real grounding and why a fuller,
/// recursive second-root graph isn't attempted). Whether `atom_str` is
/// satisfied by anything installed under `running_root`'s own real vdb --
/// `installed_candidates`, keyed directly off the atom's own parsed
/// `category`/`package` (no wildcard-atom support needed: portuale's
/// own atom grammar never has an atom without an explicit category/
/// package, see `portage_dep::parse_atom`'s own doc comment), matched via
/// `portage_dep::match_from_list` the same way every other real
/// installed-package match in portuale works. Deliberately generic on
/// `running_root` (just like `installed_versions`/`owns_path_pf`
/// elsewhere): this function has no idea whether it's being pointed at a
/// real host `/` or a fixture's own fake vdb tree, and every automated
/// test in portuale uses the latter -- only `pretend.rs`'s own real CLI
/// boundary ever points this at real `/`, matching real portage's own
/// actual default (`SYSROOT` unset).
///
/// A version match alone is not enough: the matched instance's recorded
/// `USE` must also satisfy the atom's use-deps
/// (`portage_dep::use_deps_satisfied`, the same idiom
/// `best_installed_matching`'s own USE filter uses for the target root).
/// The version-only match is USE-blind by construction (the candidate
/// strings carry no USE at all), so without this an evaluated `[-flip]`
/// still "matched" an installed `+flip` instance and the dep was wrongly
/// dropped as running-root-satisfied (#133: real rebuilds it instead).
/// Conditional forms (`=`/`?`) stay vacuous here exactly as in
/// `match_from_list` -- they are the caller's to evaluate first (both
/// `root_deps_*` producers do, against the parent's own USE); what this
/// enforces is whatever unconditional form the atom already carries,
/// which is also what makes the `||`-closure call sites real-faithful
/// (real `dep_zapdeps` matches installed alternatives with USE).
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

/// One field of one installed entry, served by [`portage_vdb`]'s
/// `FilesDb::aux_get` (feat#157 S1.2; the code that used to live here --
/// the `stat` of the package dir, the consolidated `metadata` snapshot
/// check, the dir-mtime-keyed memo, the whitespace normalisation and the
/// invalid-`SLOT` -> `"0"` translation -- moved there unchanged).
///
/// What stays here is the package-move fallback (real global-updates
/// renames a moved package's vdb dir; portuale never writes, so when the
/// direct path is absent every `installed_cp_sources` name -- the
/// pre-`move` locations that map onto this `cat/pkg` -- is tried in
/// order, one `statx` each, the first real directory wins), and the two
/// behaviours audited at the seam:
///
/// - **No `KeyError`.** A missing or non-directory entry is `""`, today's
///   `unwrap_or_default()` behaviour; portuale has no caller that wants a
///   raise.
/// - **No `environment.bz2` search** (real bug 395463) for a key outside
///   the 23-set. Backlog #125: an out-of-set key (`CONTENTS`, `NEEDED.*`)
///   structurally bypasses the snapshot and the cache and reads the raw
///   file with no normalisation at all (real never `" ".join()`s a
///   line-oriented key), invalid UTF-8 decoded lossy.
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
        // Resolve the entry like `resolve_vdb_entry` (one `statx` per
        // candidate, the unresolved direct name when none is a
        // directory), then one `open` of the raw file.
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

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>`
/// (a vdb aux file, e.g. `USE` or `IUSE` -- same directory `SLOT`/
/// `CATEGORY` already come from) as a set of flag names, one per
/// whitespace-separated token, with any `+`/`-` IUSE default-marker
/// prefix stripped (irrelevant here: a reinstall check only cares which
/// flags exist/are enabled, not their declared defaults). A missing file
/// (e.g. an older vdb entry from before portuale's fixtures modeled
/// USE/IUSE at all) is an empty set, not an error -- same "absence is a
/// real, valid state" precedent `read_world_atoms` (pretend.rs) already
/// established. The value comes from [`vdb_aux_get`], so a validated
/// consolidated snapshot serves it without an `open()`.
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

/// `(IUSE, USE)` bare-name flag sets from an installed package's vdb --
/// its declared flags and its recorded enabled set. For a use-dep check
/// against an already-merged package (real `vardb.match` honouring a
/// `cat/pkg[flag]` atom): the recorded `USE` is already the final
/// resolved state, so `[flag]` is satisfied iff `flag ∈ USE`,
/// `[flag(+)]`/`[flag(-)]` fall back to the default only when
/// `flag ∉ IUSE`.
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

/// The `GraphEntry::use_flags_display` list (`(iuse_flag, enabled)`
/// pairs, `_alnum_sort_key`-sorted) for a tree ebuild
/// `<category>/<package>-<version>` -- the same per-flag resolution
/// `resolve_pretend_graph`'s own display loop does
/// (`effective_use_flags` over the candidate's `IUSE`).
///
/// Pulled out so `emerge --resume` can populate it on each reconstructed
/// source `GraphEntry`: the `mtimedb` resume list records only
/// `cat/pkg-ver`, so the resolver-loop population is skipped -- and
/// without it `emerge_build::build_use_env` produces an empty `USE=`,
/// which makes a `python-r1` / `distutils-r1` (or any USE-conditional)
/// ebuild `die` in `src_compile`. Empty when the candidate can't be
/// located or declares no `IUSE`. The `USE_EXPAND` re-grouping
/// (`use_expand_display`) is deliberately not recomputed -- `--resume
/// --pretend`'s bracket/USE line is already a documented divergence.
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
    let use_flags = effective_use_flags(
        config,
        iuse,
        &candidate.keywords,
        &candidate_str,
        category,
        package,
    );
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

/// The package's **full** effective `USE` set -- everything
/// `effective_use_flags` resolved enabled, not just the declared-IUSE
/// subset `candidate_use_flags_display` exposes. `bin/ebuild.sh`'s own
/// `use()` and the build-info `USE` file real `__dyn_install` writes
/// need the implicit profile flags (`abi_x86_64 amd64 elibc_glibc
/// kernel_linux`) too, which no package declares in `IUSE` (#37; the S0
/// recon's `metadata/USE` row, `TEST/findings/l2.md`).
///
/// The caller filters this through `portage_profile::portage_use` (real
/// `config.py:2261`'s own IUSE ∪ `IUSE_EFFECTIVE` narrowing) -- this
/// function deliberately returns the resolver's raw answer, the same set
/// `candidate_use_flags_display`'s `enabled` column is derived from.
/// Empty when the candidate or its md5-cache is unavailable, matching
/// every other resolver helper's degraded case.
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
    // A missing `IUSE` key is an empty `IUSE`, not missing metadata: the
    // real md5-cache writer omits empty fields (`porttest/emptydirs`
    // has no `IUSE=` line at all), and real portage still computes the
    // package's full effective USE -- the implicit profile flags -- for
    // it. Bailing here would leave `USE=""` on exactly the empty-IUSE
    // packages (`TEST/findings/l2.md` "S0 recon").
    let iuse = metadata.get("IUSE").map(String::as_str).unwrap_or("");
    let candidate_str = format!(
        "{category}/{package}-{version}:{}/{}::{}",
        candidate.slot, candidate.sub_slot, candidate.repo_name
    );
    let use_flags = effective_use_flags(
        config,
        iuse,
        &candidate.keywords,
        &candidate_str,
        category,
        package,
    );
    let mut sorted: Vec<String> = use_flags.iter().cloned().collect();
    sorted.sort_by_key(|f| alnum_sort_key(f));
    sorted
}

/// The vdb entry for `<category>/<package>-<version>`. Real
/// global-updates renames a moved package's vdb dir; portuale never
/// writes, so when the direct entry is absent it falls back to every
/// `installed_cp_sources` name (the pre-`move` locations that map onto
/// this `cat/pkg`), one `has_entry` (`statx`) per candidate, the first
/// real directory wins. Returns the direct key unchanged when nothing
/// matches, so a genuinely-missing entry still reads as empty/absent.
/// (feat#157 S1.3: was `vdb_pkg_dir`/`vdb_pkg_dir_meta`, which returned
/// the path; the disk access is `FilesDb::has_entry` now, same `statx`.)
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

/// `vdb_pkg_dir(..).is_dir()` of old: resolve the entry, then test the
/// resolved entry again (two `statx` when the direct entry exists, as
/// before).
pub(crate) fn vdb_entry_is_dir(root: &Path, category: &str, package: &str, version: &str) -> bool {
    let key = resolve_vdb_entry(root, category, package, version);
    portage_vdb::for_root(root).has_entry(&key).unwrap_or(false)
}

/// Reads `<root>/var/db/pkg/<category>/<package>-<version>/<filename>`
/// as a raw string (e.g. `DEPEND`/`RDEPEND`), unlike `read_vdb_flag_set`
/// which splits into a flag-name set -- a dependency string needs to
/// stay intact for `portage_use_reduce::use_reduce_flat` to parse
/// (`||`/USE-conditional groups, not just bare tokens). A missing file
/// is an empty string, not an error -- same "absence is a real, valid
/// state" precedent `read_vdb_flag_set` already established (a vdb
/// entry with nothing recorded for this key, e.g. `DEPEND=""` at merge
/// time, is indistinguishable from -- and handled the same as -- one
/// that's simply missing the file).
///
/// The value is whitespace-normalised exactly like real `_aux_get`
/// (`vartree.py:1044-1046`): `" ".join(myd.split())`, i.e.
/// `split_whitespace().join(" ")` here. Real applies this on the
/// individual-file path too, so the bytes are the same whether the
/// consolidated `metadata` snapshot validated or not. A line-oriented
/// key (`CONTENTS`, `NEEDED.*`) bypasses the normalisation structurally
/// -- [`vdb_aux_get`] routes anything outside real's 23-field
/// `_METADATA_FILE_FIELDS` to the raw, un-normalised read (backlog #125).
///
/// Since #109 S3 the value comes from [`vdb_aux_get`]: a validated
/// consolidated `metadata` snapshot serves it with no per-key `open()`,
/// falling back to the individual file otherwise.
pub(crate) fn read_vdb_string(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
    filename: &str,
) -> String {
    vdb_aux_get(root, category, package, version, filename)
}

/// Flattens `depstr` (one or more concatenated dependency-string keys)
/// against `use_flags`, into a `HashSet` of dependency-atom tokens
/// (`||` markers dropped) suitable for order-independent equality
/// comparison. `None` if `depstr` doesn't parse at all.
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

/// Real `find_libc_deps(vardb, realized=False)` (`portage.dep.libc`): the
/// `(category, package)` identity of every atom `virtual/libc`'s own
/// installed (vdb) `RDEPEND` names, once flattened against its own
/// installed `USE` -- empty if `virtual/libc` isn't installed at all,
/// same as real `vardb.match("virtual/libc")` finding nothing. A
/// simplified, one-level port of real `expand_new_virt`: real Gentoo's
/// own `virtual/libc` `RDEPEND` is always a flat `|| ( sys-libs/glibc
/// sys-libs/musl ... )` of real (non-virtual) packages, so this doesn't
/// replicate `expand_new_virt`'s own further case of recursing into a
/// *second* virtual reached this way, which real `virtual/libc` never
/// actually needs. Used by `deps_changed` to strip libc atoms out of
/// both sides of its own comparison before comparing -- real
/// `strip_libc_deps`'s whole purpose: practically every ebuild silently
/// gains/loses an implicit libc dependency across revisions, and that's
/// noise, not a real dependency change worth reporting.
///
/// `pub`: also the first half of real `_inject_libc_dep` (`doebuild.py:
/// 3026-3067`, backlog #39), which needs the realized provider versions
/// on top -- see `ebuild_phases::inject_libc_dep`.
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

/// One installed package in the vdb, `category/package-version` plus its
/// own recorded main `SLOT` -- real `vartree.dbapi.cpv_all()` / `_pkg_str`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub category: String,
    pub package: String,
    pub version: String,
    pub slot: String,
}

impl InstalledPackage {
    /// `category/package-version` -- the vdb cpv.
    pub fn cpv(&self) -> String {
        format!("{}/{}-{}", self.category, self.package, self.version)
    }
}

/// Every package recorded under `<root>/var/db/pkg` (real
/// `vartree.dbapi.cpv_all()`), each with its own main slot. Directory
/// names are split into `package`/`version` by finding the last
/// `-`-separated boundary whose right half `ververify`s -- the same
/// "a version always starts like a version, a package-name word may
/// not" disambiguation `strip_version_prefix` already makes, generalised
/// to a name whose own package isn't known ahead of time.
///
/// Memoised per `root`: a deep `emerge -pu` reads the whole installed set
/// (~2000 `SLOT` files) at ~10 resolver call sites, several inside loops,
/// and the vdb never changes mid-resolve. Keyed by `root` + a
/// `generation` so a merge/unmerge earlier in the same process
/// (test suites do this) is picked up -- real `vardbapi` invalidates its
/// own `cp_all` cache on category-dir mtime the same way.
pub fn all_installed_packages(root: &Path) -> Vec<InstalledPackage> {
    // root -> (vdb fingerprint at scan time, scanned set)
    type InstalledCache = HashMap<PathBuf, (u64, std::sync::Arc<Vec<InstalledPackage>>)>;
    static CACHE: OnceLock<RwLock<InstalledCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    // The cheap fingerprint of the vdb's directory structure (the vdb
    // dir's mtime plus every category dir's mtime, folded): `FilesDb::
    // generation` (feat#157 S1.2; was `vdb_fingerprint` here). ~30
    // `stat`s vs the ~2000 `SLOT` file reads a full scan does.
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
    // `FilesDb::entries` (feat#157 S1.3): the category/package directory
    // walk that used to be here, `-MERGING-` entries already skipped.
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
            // #109 S3: through the shared `vdb_aux_get` seam, so a valid
            // consolidated snapshot serves `SLOT` without the per-entry
            // `open()`. `resolve_vdb_entry` resolves the entry being scanned
            // back from `(category, name, version)`.
            let (slot, sub) = split_slot(&vdb_aux_get(root, &category, &name, &version, "SLOT"));
            // Real `profiles/updates/` package moves: present each vdb
            // entry under its post-`move`/`slotmove` identity, the way
            // real global-updates rewrites the vdb on sync (portuale
            // never writes -- see `resolve_vdb_entry` for the read-back side).
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

/// Every `category/package` present in any of `repos` -- real
/// `portdbapi.cp_all()` (`porttree.py`), used by `emerge --search`.
/// Sorted, deduplicated; a directory is a `cp` when it has at least one
/// `<package>-<version>.ebuild`.
pub fn all_cp(repos: &[RepoConfig]) -> Vec<String> {
    let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for repo in repos {
        let Ok(cats) = portage_util::read_dir_entries(&repo.location) else {
            continue;
        };
        for cat in cats.into_iter().filter(|e| e.path().is_dir()) {
            let category = cat.file_name().to_string_lossy().to_string();
            // Skip the non-category top-level dirs of a repo checkout.
            if matches!(
                category.as_str(),
                "eclass" | "profiles" | "metadata" | "licenses" | "scripts" | "distfiles" | ".git"
            ) {
                continue;
            }
            let Ok(pkgs) = portage_util::read_dir_entries(&cat.path()) else {
                continue;
            };
            for pkg in pkgs.into_iter().filter(|e| e.path().is_dir()) {
                let package = pkg.file_name().to_string_lossy().to_string();
                let has_ebuild = portage_util::read_dir_entries(&pkg.path())
                    .map(|d| {
                        d.into_iter().any(|e| {
                            e.file_name().to_str().is_some_and(|n| {
                                n.strip_suffix(".ebuild")
                                    .and_then(|s| strip_version_prefix(s, &package))
                                    .is_some()
                            })
                        })
                    })
                    .unwrap_or(false);
                if has_ebuild {
                    out.insert(format!("{category}/{package}"));
                }
            }
        }
    }
    out.into_iter().collect()
}
