//! Blockers against installed packages and file collisions.

use crate::ebuild_merge::*;

/// `mypkglist = others_in_slot + blockers`: the blocker half. Portage's
/// `dblink._blockers` is injected by the depgraph resolver, which knows
/// the full graph by merge time; standalone `ebuild <file> merge` has no
/// depgraph, so this resolves config, computes effective USE, flattens
/// all five `*DEPEND` keys and matches every blocker atom against every
/// installed package. Weak vs. strong blockers are not distinguished
/// (Portage's `mypkglist` construction doesn't either).
///
/// Degrades to an empty set on any resolution failure: a missed
/// exclusion only reports an ordinary collision, never a silent
/// corruption.
// Portage: vartree.py, dblink.merge
pub(crate) fn blocked_installed_packages(
    root: &Path,
    config_root: &Path,
    env: &ebuild_phases::Environment,
    slot: &str,
    repository: &str,
) -> HashSet<(String, String)> {
    (|| -> Option<HashSet<(String, String)>> {
        let repo_root = ebuild_phases::repo_root_for(&env.pkg_dir)?;
        let metadata =
            portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok()?;

        let repos = portage_repo::find_repos(config_root).ok()?;
        let main_repo = repos.iter().find(|r| r.is_main)?;
        let overlay_repos: Vec<(String, PathBuf)> = repos
            .iter()
            .filter(|r| !r.is_main)
            .map(|r| (r.name.clone(), r.location.clone()))
            .collect();
        let repo_masters: HashMap<String, Vec<PathBuf>> = repos
            .iter()
            .map(|r| (r.name.clone(), r.masters.clone()))
            .collect();
        let repo_aliases: Vec<(String, PathBuf)> = repos
            .iter()
            .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
            .collect();
        let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
            config_root,
            main_repo_location: &main_repo.location,
            overlay_repos: &overlay_repos,
            repo_aliases: &repo_aliases,
            main_repo_name: &main_repo.name,
            repo_masters: &repo_masters,
            eroot: root,
        })
        .ok()?;

        let iuse = metadata.get("IUSE").map(String::as_str).unwrap_or_default();
        let keywords: Vec<String> = metadata
            .get("KEYWORDS")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let candidate_str = format!(
            "{}/{}-{}:{slot}/{slot}::{repository}",
            env.category, env.split.pn, env.split.pvr
        );
        let use_flags = portage_repo::effective_use_flags(portage_repo::EffectiveUseFlagsArgs {
            config: &config,
            iuse,
            keywords: &keywords,
            candidate_str: &candidate_str,
            category: &env.category,
            package: &env.split.pn,
        });

        let dep_keys = ["DEPEND", "RDEPEND", "BDEPEND", "PDEPEND", "IDEPEND"];
        let mut depstr = String::new();
        for dep_key in dep_keys {
            if let Some(d) = metadata.get(dep_key) {
                depstr.push_str(d);
                depstr.push(' ');
            }
        }
        let tokens: Vec<String> = depstr.split_whitespace().map(String::from).collect();
        let flat_deps = portage_use_reduce::use_reduce_flat(
            &tokens,
            &use_flags,
            portage_use_reduce::MatchMode::Normal,
        )
        .ok()?;
        Some(blockers_from_flat_deps(root, &flat_deps))
    })()
    .unwrap_or_default()
}

/// The installed-package scan + blocker-atom match half, split out so
/// `merge_binpkg` can reuse it: a binpkg's `*DEPEND` files are already
/// USE-reduced at build time. Matches every `!atom`/`!!atom` against
/// every installed entry (`category/pf:slot/sub_slot`).
// Portage: vartree.py, dblink.merge
pub(crate) fn blockers_from_flat_deps(
    root: &Path,
    flat_deps: &[String],
) -> HashSet<(String, String)> {
    (|| -> Option<HashSet<(String, String)>> {
        // Category by category, like the old walk: list the category,
        // then read each entry's `SLOT`, then the next category.
        let db = portage_vdb::for_root(root);
        let installed: Vec<(String, String, String)> = db
            .categories()
            .ok()?
            .into_iter()
            .flat_map(|category_name| {
                db.category_entries(&category_name)
                    .into_iter()
                    .flatten()
                    .map(move |pf| {
                        // Through the vdb seam, so the scan sees the same
                        // normalised `SLOT` every consumer does.
                        let slot = portage_repo::vdb_entry_slot(root, &category_name, &pf);
                        let (slot, sub_slot) = slot
                            .split_once('/')
                            .map(|(s, ss)| (s.to_string(), ss.to_string()))
                            .unwrap_or_else(|| (slot.clone(), slot.clone()));
                        let candidate_str = format!("{category_name}/{pf}:{slot}/{sub_slot}");
                        (category_name.clone(), pf, candidate_str)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let installed_strs: Vec<&str> = installed.iter().map(|(_, _, s)| s.as_str()).collect();
        let by_str: HashMap<&str, &(String, String, String)> = installed_strs
            .iter()
            .copied()
            .zip(installed.iter())
            .collect();

        let mut blocked: HashSet<(String, String)> = HashSet::new();
        for tok in flat_deps {
            let Some(dep_atom) = portage_dep::parse_atom(tok) else {
                continue;
            };
            if dep_atom.blocker == portage_dep::Blocker::None {
                continue;
            }
            if let Some(matched) = portage_dep::match_from_list(tok, &installed_strs) {
                for m in matched {
                    if let Some((category, pf, _)) = by_str.get(m) {
                        blocked.insert((category.clone(), pf.clone()));
                    }
                }
            }
        }
        Some(blocked)
    })()
    .unwrap_or_default()
}

/// PMS 13.4's symlink-over-directory ban (unconditional, but only for
/// the checked file-target/dangling symlinks) plus ordinary-collision
/// detection, preserve-libs exclusion and blocker exclusion.
/// `FEATURES=protect-owned` is decided by the caller from this
/// function's `collisions` plus `find_owners`, not inside. Walks `d`
/// read-only and file/symlink-only (directories merging into existing
/// directories are normal). Returns `(collisions, symlink_collisions,
/// plib_collisions)` as `ROOT`-relative absolute paths; only the caller
/// decides whether `collisions` aborts, `symlink_collisions` always
/// should, `plib_collisions` never does.
// Portage: vartree.py, dblink._collision_protect
pub(crate) type CollisionsResult =
    Result<(Vec<String>, Vec<String>, BTreeMap<String, BTreeSet<String>>), String>;
/// Arguments of [`find_collisions`].
pub(crate) struct FindCollisionsArgs<'a> {
    pub(crate) d: &'a Path,
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) package: &'a str,
    pub(crate) slot: &'a str,
    pub(crate) config_protect: &'a str,
    pub(crate) config_protect_mask: &'a str,
    pub(crate) plib_inodes: &'a HashMap<(u64, u64), Vec<(String, String)>>,
    pub(crate) blocked: &'a HashSet<(String, String)>,
}

pub(crate) fn find_collisions(args: FindCollisionsArgs<'_>) -> CollisionsResult {
    let FindCollisionsArgs {
        d,
        root,
        category,
        package,
        slot,
        config_protect,
        config_protect_mask,
        plib_inodes,
        blocked,
    } = args;
    let own_versions: Vec<String> = portage_repo::installed_versions(root, category, package)
        .into_iter()
        .filter(|version| {
            read_installed_slot(root, category, package, version).as_deref() == Some(slot)
        })
        .collect();

    let mut collisions = Vec::new();
    let mut symlink_collisions = Vec::new();
    let mut plib_collisions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::new()];
    while let Some(relative_dir) = stack.pop() {
        let src_dir = d.join(&relative_dir);
        let children: Vec<PathBuf> = portage_util::read_dir_entries(&src_dir)
            .map_err(|e| format!("{}: {e}", src_dir.display()))?
            .into_iter()
            .map(|e| relative_dir.join(e.file_name()))
            .collect();

        for relative_path in children {
            let src = d.join(&relative_path);
            let dest = root.join(&relative_path);
            let abs_path = format!("/{}", relative_path.display());
            let file_type = std::fs::symlink_metadata(&src)
                .map_err(|e| format!("{}: {e}", src.display()))?
                .file_type();

            if file_type.is_dir() {
                stack.push(relative_path);
                continue;
            }

            // `os.walk` puts a directory-target symlink into `dirs`, not
            // `files`, so it is never collision-checked; only
            // file-target/dangling symlinks are checked below.
            // `metadata` follows the link like `os.walk`'s `is_dir()`
            // does; a dangling link errors and falls through to the
            // check.
            if file_type.is_symlink()
                && std::fs::metadata(&src).map(|m| m.is_dir()).unwrap_or(false)
            {
                continue;
            }

            let Ok(dest_meta) = std::fs::symlink_metadata(&dest) else {
                continue;
            };

            if file_type.is_symlink() && dest_meta.is_dir() {
                symlink_collisions.push(abs_path);
                continue;
            }

            if let Some(plibs) = plib_inodes.get(&(dest_meta.dev(), dest_meta.ino())) {
                for (cpv, path) in plibs {
                    plib_collisions
                        .entry(cpv.clone())
                        .or_default()
                        .insert(path.clone());
                }
                continue;
            }

            let owned = own_versions
                .iter()
                .any(|version| owns_path(root, category, package, version, &abs_path))
                || blocked
                    .iter()
                    .any(|(bcat, bpf)| owns_path_pf(root, bcat, bpf, &abs_path));
            if owned || is_protected(root, config_protect, config_protect_mask, &dest) {
                continue;
            }
            collisions.push(abs_path);
        }
    }
    Ok((collisions, symlink_collisions, plib_collisions))
}
