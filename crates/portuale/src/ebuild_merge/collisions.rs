//! Blockers against installed packages and file collisions.

use crate::ebuild_merge::*;

/// Real `mypkglist = others_in_slot + blockers` (`dblink.merge()`'s own
/// blocker half -- `others_in_slot` is already `find_collisions`'s own
/// `own_versions`). Real `dblink._blockers` is never computed by
/// `dblink` itself: it's injected by the real depgraph resolver, which
/// already knows the full dependency graph by the time a merge runs.
/// Portuale's own `ebuild <file> merge` has no depgraph at all (a
/// standalone, single-ebuild real-execution path, unlike `emerge
/// --pretend`) -- so this is a new, self-contained computation:
/// resolves real `repos.conf`/profile/USE config for the merging
/// package's own repo (`portage_repo::find_repos` +
/// `portage_profile::resolve_config`, the exact same machinery
/// `pretend.rs` already uses, including its own real `masters =`
/// resolution), computes the merging package's own effective USE the
/// same way `portage_repo`'s own (now `pub`) `effective_use_flags`
/// always has, flattens its own real `DEPEND`+`RDEPEND`+`BDEPEND`+
/// `PDEPEND`+`IDEPEND` (`portage_use_reduce::use_reduce_flat`) against
/// it, and matches every blocker atom found (`!atom`/`!!atom`,
/// `portage_dep::parse_atom`'s own `.blocker`) against every real
/// installed package (`portage_dep::match_from_list`, which -- real,
/// verified behavior already relied on elsewhere in portuale --
/// ignores an atom's blocker marker entirely when matching, so the
/// blocker atom string can be passed in as-is). Real weak vs. strong
/// blockers are not distinguished (`dblink.merge()`'s own `mypkglist`
/// construction doesn't either -- both kinds exclude a collision the
/// same way). Returns every matched installed package as a bare
/// `(category, pf)` pair.
///
/// Degrades gracefully to an empty set on any resolution failure
/// (missing `repos.conf`, unreadable md5-cache, an ebuild path outside
/// any real repo, etc.) -- config resolution isn't guaranteed to
/// succeed in every context `ebuild <file> merge` is used from (unlike
/// `emerge --pretend`, portuale's own real-execution CLI has never
/// required it before this slice), and a collision that would have been
/// excluded here just gets reported as an ordinary one instead: never a
/// false negative in the direction that could silently corrupt a real
/// merge.
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

/// The installed-package scan + blocker-atom match half of
/// `blocked_installed_packages` (real `mypkglist`'s `blockers` term),
/// split out so `merge_binpkg` can reuse it: a binary package's
/// `*DEPEND` build-info files are already USE-reduced at build time, so
/// the merge side has a flat token list in hand without ever resolving
/// config/USE against a repo (which a binpkg has no path to). Matches
/// every `!atom`/`!!atom` (`portage_dep::parse_atom`'s own `.blocker`)
/// against every installed vdb entry (`category/pf:slot/sub_slot`, so a
/// slot-restricted blocker like `!dev-libs/foo:0` matches correctly).
/// Weak vs. strong blockers are not distinguished (`dblink.merge()`'s
/// own `mypkglist` construction doesn't either).
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
                        // #116: through the vdb seam, so this scan sees the
                        // same normalised `SLOT` every other consumer does
                        // (a missing field is `""` -> `("", "")` here).
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

/// Real PMS 13.4's own symlink-over-directory ban (checked
/// unconditionally, regardless of `FEATURES`, but only for the symlinks
/// real actually checks -- file-target/dangling ones; a symlink whose
/// target is a directory is in real `os.walk`'s `dirs`, never in
/// `file_list`/`symlink_list`, so it is skipped here, like real) plus real `FEATURES=
/// collision-protect`'s own ordinary-collision detection, plus real
/// preserve-libs collision exclusion and real blocker exclusion (`mypkglist
/// = others_in_slot + blockers` -- see `blocked_installed_packages`'s own
/// doc comment for the full real grounding; `FEATURES=protect-owned` is
/// real too, but decided by the caller, `run_merge`, using this
/// function's own `collisions` result together with `find_owners`, not
/// inside this function itself). Walks `d` (the real install image,
/// `${D}`) the same way `merge_tree` does,
/// but read-only and file/symlink-only (real `_collision_protect` never
/// checks directories at all -- a directory merging into an existing
/// directory is normal, not a collision). Returns `(collisions,
/// symlink_collisions, plib_collisions)` as real, `ROOT`-relative
/// absolute paths (`plib_collisions` keyed by the preserved lib's own
/// owning cpv); the caller decides whether `collisions` alone should
/// abort the merge (gated on `FEATURES=collision-protect`) --
/// `symlink_collisions` always should, and `plib_collisions` never does
/// (real `_collision_protect` excludes those from `collisions`
/// unconditionally, regardless of `FEATURES`).
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

            // Real `os.walk(srcroot)` (which builds real `filelist`/
            // `linklist`, `vartree.py:4625-4681`) puts a symlink that
            // points to a directory into the `dirs` list, not `files` --
            // so it never reaches `filelist`/`linklist` and is never
            // collision-checked at all (verified: `link_to_dir` lands in
            // `dirs`, `link_to_file`/dangling land in `files`). A
            // directory-target symlink colliding with a real directory
            // (e.g. linux-firmware's `nvidia/ad10x` symlinks over
            // installed directories) is therefore ignored here, matching
            // real: no `symlink_collisions` entry, no abort. Only
            // file-target/dangling symlinks are checked below. `metadata`
            // follows the link exactly like `os.walk`'s own `is_dir()`
            // does (image-relative for relative targets); a dangling
            // link errors here and falls through to the check, like real.
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

/// Real `vardbapi._owners.get_owners()`, narrowed: for each of
/// `collisions`, walks every installed package under `<root>/var/db/
/// pkg` (all categories, all packages -- real portage keeps a
/// persistent reverse index for this; portuale just scans fresh every
/// time, acceptable for a real, but not performance-critical, error-
/// reporting path only reached when a merge is about to abort anyway)
/// and returns the `category/pf` -> claimed-paths map for whichever
/// ones actually claim it.
///
/// The scan is [`portage_vdb::InstalledDb::owners`] (S1.5: the old
/// two-level walk and per-entry `CONTENTS` read moved into `FilesDb`
/// unchanged). Two things the old loop did stay here because they live
/// above `portage-vdb`: an entry whose directory name does not split as
/// `<package>-<version>` (`portage_repo::split_pf`) claims nothing (the
/// old walk skipped it before reading its `CONTENTS`; it is now read and
/// its claims dropped), and the result is regrouped as `category/pf` ->
/// the matched collision strings (the same absolute strings the old
/// direct read pushed). `owns_path` keeps reading through the
/// `mrg_director::PackagesDb` seam.
pub(crate) fn find_owners(root: &Path, collisions: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let paths: Vec<&[u8]> = collisions.iter().map(|c| c.as_bytes()).collect();
    let Ok(claims) = portage_vdb::for_root(root).owners(&paths) else {
        return owners;
    };
    for (path, key) in claims {
        // The old walk skipped an entry name that is not
        // `<package>-<version>` before reading its `CONTENTS`.
        if portage_repo::split_pf(&key.pf).is_none() {
            continue;
        }
        owners
            .entry(format!("{}/{}", key.category, key.pf))
            .or_default()
            .push(String::from_utf8_lossy(&path).into_owned());
    }
    owners
}

/// Real "package NOT merged due to file collisions" abort message,
/// narrowed to what portuale can cheaply compute (see this module's
/// own module doc comment): every colliding path, annotated with
/// whichever other real installed package(s) `find_owners` found
/// actually claiming it (`(unclaimed)` when none did -- a real,
/// possible outcome: a stray file on disk with no owner at all, real
/// portage's own "None of the installed packages claim the file(s)"
/// case).
pub(crate) fn collision_message(
    root: &Path,
    cpv: &str,
    collisions: &[String],
    symlink_collisions: &[String],
) -> String {
    let mut lines = Vec::new();
    if !symlink_collisions.is_empty() {
        lines.push(format!(
            "Package '{cpv}' NOT merged: one or more collisions between \
             symlinks and directories, forbidden by PMS section 13.4:"
        ));
        for f in symlink_collisions {
            lines.push(format!("\t{f}"));
        }
    }
    if !collisions.is_empty() {
        lines.push(
            "This package will overwrite one or more files that may belong \
             to other packages:"
                .to_string(),
        );
        let owners = find_owners(root, collisions);
        for (owner, paths) in &owners {
            lines.push(format!("{owner}:"));
            for f in paths {
                lines.push(format!("\t{f}"));
            }
        }
        let claimed: std::collections::HashSet<&String> = owners.values().flatten().collect();
        let unclaimed: Vec<&String> = collisions.iter().filter(|f| !claimed.contains(f)).collect();
        if !unclaimed.is_empty() {
            lines.push("(unclaimed):".to_string());
            for f in unclaimed {
                lines.push(format!("\t{f}"));
            }
        }
        lines.push(format!(
            "Package '{cpv}' NOT merged due to file collisions."
        ));
    }
    lines.join("\n")
}
