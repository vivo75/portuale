//! The world and `package.*` half of `_do_global_updates`: `update_dbentry`
//! over the world list (with `_world_repo_match` and a sorted atomic
//! write) and over every `package.*` file (into `._cfgNNNN_` siblings
//! under `CONFIG_PROTECT`).
// Portage: lib/portage/_global_updates.py, _do_global_updates

use crate::ebuild_merge::{is_protected, md5_hex_bytes, new_protect_filename};
use crate::global_updates::{RepoMatch, repo_update_map};
use portage_repo::{RepoConfig, UpdateCmd};
use portage_vdb::{EntryKey, InstalledDb};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

/// `etc/portage` under the root: the only tree `update_config_files` touches.
// Portage: lib/portage/const.py, USER_CONFIG_PATH
const USER_CONFIG_PATH: &str = "etc/portage";

/// The top-level `package.*` names, in Portage's order.
const TOP_LEVEL_UPDATE_FILES: &[&str] = &[
    "package.accept_keywords",
    "package.env",
    "package.keywords",
    "package.license",
    "package.mask",
    "package.properties",
    "package.unmask",
    "package.use",
    "sets",
];

/// The `profile/` names appended after the top level, in Portage's order.
const PROFILE_UPDATE_FILES: &[&str] = &[
    "packages",
    "package.accept_keywords",
    "package.keywords",
    "package.mask",
    "package.unmask",
    "package.use",
    "package.use.force",
    "package.use.mask",
    "package.use.stable.force",
    "package.use.stable.mask",
];

/// What [`update_world`] needs: the root whose world file moves, and the
/// repos (in `portdb` order) whose updates apply to it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct UpdateWorldArgs<'a> {
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
}

/// What [`update_config_files`] needs: the root holding
/// `etc/portage`, the repos whose updates apply, the settings halves
/// (`CONFIG_PROTECT`, `CONFIG_PROTECT_MASK`, `case-insensitive-fs`),
/// and the progress hook the output driver passes (`None` in tests that
/// do not watch progress): one call per rewritten line, where Portage
/// writes `'p'`.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct UpdateConfigFilesArgs<'a> {
    pub root: &'a Path,
    pub repos: &'a [RepoConfig],
    pub config_protect: &'a str,
    pub config_protect_mask: &'a str,
    pub case_insensitive: bool,
    pub on_progress: Option<&'a dyn Fn()>,
}

/// Rewrite every world entry the updates move, gated by
/// `_world_repo_match`, and write the list back sorted. Returns whether
/// anything changed.
///
/// # Errors
///
/// Returns `Err` when the world store cannot be read or written.
// Portage: lib/portage/_global_updates.py, _do_global_updates
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn update_world(args: UpdateWorldArgs<'_>) -> Result<bool, String> {
    let db = portage_vdb::for_root(args.root);
    let map = repo_update_map(args.repos);
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(&map, master);
    let mut world_list = db.world().map_err(|e| e.to_string())?.atoms;
    let mut world_modified = false;
    for (repo_name, cmds) in &map {
        if repo_name == "DEFAULT" || cmds.is_empty() {
            continue;
        }
        let repo_match = |repository: &str| matcher.matches(repo_name, repository);
        for cmd in cmds {
            for atom in world_list.iter_mut() {
                let new_atom = apply_update_to_atom(cmd, atom);
                if new_atom == *atom {
                    continue;
                }
                if !world_repo_match(&db, &repo_match, atom, &new_atom)? {
                    continue;
                }
                *atom = new_atom;
                world_modified = true;
            }
        }
    }
    if world_modified {
        world_list.sort();
        let mut txn = db.begin_write().map_err(|e| e.to_string())?;
        txn.set_world(&portage_vdb::World { atoms: world_list })
            .map_err(|e| e.to_string())?;
        txn.commit().map_err(|e| e.to_string())?;
    }
    Ok(world_modified)
}

/// Rewrite every `package.*` line the updates move, leaving the
/// original files in place and writing the rewritten text to fresh
/// `._cfgNNNN_` siblings when protected.
///
/// # Errors
///
/// Returns `Err` when the vdb cannot be read or a `._cfgNNNN_` name
/// cannot be allocated; an unreadable file is skipped and an unwritable
/// one warns and continues, never fatal.
// Portage: lib/portage/update.py, update_config_files
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn update_config_files(args: UpdateConfigFilesArgs<'_>) -> Result<(), String> {
    let db = portage_vdb::for_root(args.root);
    let map = repo_update_map(args.repos);
    let master = args
        .repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.name.clone());
    let matcher = RepoMatch::new(&map, master);
    let user_config = args.root.join(USER_CONFIG_PATH);
    let mut rels: Vec<String> = Vec::new();
    for name in TOP_LEVEL_UPDATE_FILES {
        let path = user_config.join(name);
        if path.is_dir() {
            rels.extend(walk_config_dir(&user_config, &path));
        } else {
            rels.push((*name).to_string());
        }
    }
    for name in PROFILE_UPDATE_FILES {
        let path = user_config.join("profile").join(name);
        if path.is_dir() {
            rels.extend(walk_config_dir(&user_config, &path));
        } else {
            rels.push(format!("profile/{name}"));
        }
    }
    rels.sort();
    rels.dedup();
    let mut file_contents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rel in &rels {
        let Ok(bytes) = std::fs::read(user_config.join(rel)) else {
            continue;
        };
        file_contents.insert(rel.clone(), split_lines(&String::from_utf8_lossy(&bytes)));
    }
    let mut updated: BTreeSet<String> = BTreeSet::new();
    for (repo_name, cmds) in &map {
        if repo_name == "DEFAULT" || cmds.is_empty() {
            continue;
        }
        let repo_match = |repository: &str| matcher.matches(repo_name, repository);
        for cmd in cmds {
            for (rel, contents) in file_contents.iter_mut() {
                let mut pos = 0;
                while pos < contents.len() {
                    let line = contents[pos].clone();
                    let Some(first) = line.split_whitespace().next() else {
                        pos += 1;
                        continue;
                    };
                    // `package.mask` incrementals and `packages`
                    // system markers rewrite like the bare atom.
                    let mut atom = first.to_string();
                    if let Some(rest) = atom.strip_prefix('-') {
                        atom = rest.to_string();
                    }
                    if let Some(rest) = atom.strip_prefix('*') {
                        atom = rest.to_string();
                    }
                    if portage_dep::parse_atom(&atom).is_none() {
                        pos += 1;
                        continue;
                    }
                    let new_atom = apply_update_to_atom(cmd, &atom);
                    if new_atom == atom || !config_repo_match(&db, &repo_match, &atom, &new_atom)? {
                        pos += 1;
                        continue;
                    }
                    contents[pos] = format!("# {}\n", update_comment(cmd));
                    contents.insert(pos + 1, line.replacen(&atom, &new_atom, 1));
                    updated.insert(rel.clone());
                    if let Some(on_progress) = args.on_progress {
                        on_progress();
                    }
                    pos += 2;
                }
            }
        }
    }
    // The folded lists keep `is_protected`'s whitespace split working;
    // the lowered destination only matters with the flag set.
    let folded_protect;
    let folded_mask;
    let (protect, mask) = if args.case_insensitive {
        folded_protect = args.config_protect.to_lowercase();
        folded_mask = args.config_protect_mask.to_lowercase();
        (folded_protect.as_str(), folded_mask.as_str())
    } else {
        (args.config_protect, args.config_protect_mask)
    };
    for rel in &updated {
        let body: String = file_contents[rel].concat();
        let updating = user_config.join(rel);
        let compared = if args.case_insensitive {
            Path::new(&updating.to_string_lossy().to_lowercase()).to_path_buf()
        } else {
            updating.clone()
        };
        let dest = if is_protected(args.root, protect, mask, &compared) {
            new_protect_filename(&updating, &md5_hex_bytes(body.as_bytes()))?
        } else {
            updating.clone()
        };
        if let Err(e) = write_config_file_atomic(&dest, &body) {
            eprintln!(
                "\n!!! {e}\n!!! An error occurred while updating a config file: '{}'",
                dest.display()
            );
        }
    }
    Ok(())
}

/// One `move`/`slotmove` applied to one atom token, in command order.
fn apply_update_to_atom(cmd: &UpdateCmd, atom: &str) -> String {
    match cmd {
        UpdateCmd::Move { old, new } => portage_repo::apply_move_to_token(atom, old, new),
        UpdateCmd::SlotMove {
            cp,
            old_slot,
            new_slot,
        } => portage_repo::apply_slotmove_to_token(atom, cp, old_slot, new_slot),
    }
}

/// The `# move …` / `# slotmove …` comment heading each rewritten line.
fn update_comment(cmd: &UpdateCmd) -> String {
    match cmd {
        UpdateCmd::Move { old, new } => {
            format!("move {}/{} {}/{}", old.0, old.1, new.0, new.1)
        }
        UpdateCmd::SlotMove {
            cp,
            old_slot,
            new_slot,
        } => format!("slotmove {}/{} {old_slot} {new_slot}", cp.0, cp.1),
    }
}

/// `_world_repo_match`: the best `vardb.match` hit for the old atom (or
/// the new one) comes from the update's repo; the `portdb` hit only
/// feeds a warning Portage never prints.
fn world_repo_match(
    db: &Arc<dyn InstalledDb>,
    repo_match: &dyn Fn(&str) -> bool,
    atoma: &str,
    atomb: &str,
) -> Result<bool, String> {
    let mut matches = match_keys(db, atoma)?;
    if matches.is_empty() {
        matches = match_keys(db, atomb)?;
    }
    let Some(best) = best_key(&matches) else {
        return Ok(false);
    };
    Ok(repo_match(&entry_repository(db, &best)?))
}

/// `_config_repo_match`: like the world one, except both misses refuse
/// the rewrite instead of falling through.
fn config_repo_match(
    db: &Arc<dyn InstalledDb>,
    repo_match: &dyn Fn(&str) -> bool,
    atoma: &str,
    atomb: &str,
) -> Result<bool, String> {
    let mut matches = match_keys(db, atoma)?;
    if matches.is_empty() {
        matches = match_keys(db, atomb)?;
        if matches.is_empty() {
            return Ok(false);
        }
    }
    let Some(best) = best_key(&matches) else {
        return Ok(false);
    };
    Ok(repo_match(&entry_repository(db, &best)?))
}

/// Installed keys matching `atom`, with the stored `SLOT` appended so
/// slot-qualified atoms match the way `vardb.match` matches them. An
/// unparseable atom matches nothing.
fn match_keys(db: &Arc<dyn InstalledDb>, atom: &str) -> Result<Vec<EntryKey>, String> {
    let mut pairs: Vec<(String, EntryKey)> = Vec::new();
    for key in db.entries().map_err(|e| e.to_string())? {
        let slot = db
            .aux_get(&key, "SLOT")
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut cpv = format!("{}/{}", key.category, key.pf);
        if !slot.is_empty() {
            cpv.push(':');
            cpv.push_str(&slot);
        }
        pairs.push((cpv, key));
    }
    let refs: Vec<&str> = pairs.iter().map(|(cpv, _)| cpv.as_str()).collect();
    let hits: BTreeSet<String> = portage_dep::match_from_list(atom, &refs)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect();
    Ok(pairs
        .into_iter()
        .filter(|(cpv, _)| hits.contains(cpv))
        .map(|(_, key)| key)
        .collect())
}

/// Portage `best` over the hits: the highest version by `vercmp`.
fn best_key(keys: &[EntryKey]) -> Option<EntryKey> {
    keys.iter()
        .max_by(|a, b| {
            portage_versions::vercmp(&version_of(a), &version_of(b))
                .unwrap_or(0)
                .cmp(&0)
        })
        .cloned()
}

/// The version part of an installed `pf`, `""` when it never parses.
fn version_of(key: &EntryKey) -> String {
    portage_repo::split_pf(&key.pf)
        .map(|(_, version)| version)
        .unwrap_or_default()
}

/// The `repository` file of an entry, `""` when absent.
fn entry_repository(db: &Arc<dyn InstalledDb>, key: &EntryKey) -> Result<String, String> {
    db.aux_get(key, "repository")
        .map_err(|e| e.to_string())
        .map(|v| v.unwrap_or_default())
}

/// Every config file under a `package.*` directory, relative to
/// `etc/portage`: hidden and `VCS_DIRS` directories are never descended
/// into, hidden files are skipped, and an unreadable directory reads as
/// empty. Sorted by the caller for determinism.
fn walk_config_dir(user_config: &Path, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(top) = stack.pop() {
        let Ok(entries) = portage_util::read_dir_entries(&top) else {
            continue;
        };
        for entry in entries {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if portage_util::VCS_DIRS.contains(&name) {
                    continue;
                }
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(user_config) {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    out
}

/// Lines that keep their endings, like `readlines`: the final line
/// stays unterminated when the file has no trailing newline.
fn split_lines(body: &str) -> Vec<String> {
    body.split_inclusive('\n').map(str::to_string).collect()
}

/// Portage `write_atomic` for the `package.*` write-back: a fresh
/// temporary in the same directory, then `rename` over the target. A
/// leftover temporary is removed; the target keeps its mode when it
/// already exists.
// Portage: lib/portage/util/__init__.py, write_atomic
fn write_config_file_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: bad file name", path.display()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mode = std::fs::metadata(path).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let candidate = parent.join(format!(".{base}.tmp.{}.{}", std::process::id(), n));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(format!("{}: {e}", candidate.display())),
        }
    };
    let result = (|| {
        use std::io::Write as _;
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))
                .map_err(|e| format!("{}: {e}", tmp.display()))?;
        }
        file.write_all(body.as_bytes())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        drop(file);
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("{} -> {}: {e}", tmp.display(), path.display()))
    })();
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Portage 3.0.83 rewrites the fixture's world to
    /// `dev-libs/{deppkg,newpkg}` (sorted) and its `package.use` into a
    /// fresh `._cfg0000_package.use` (`_global_updates.py:182-214` for the
    /// world loop, `update.py:241-415` for the config files). The
    /// expected bytes are the committed Portage capture.
    #[test]
    fn world_and_package_use_match_the_portage_capture() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let capture = manifest.join("../../LLM/evidence/2026-10-25-review/t5/after");
        let scratch = portage_util::TempDir::new("global_updates_config");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [(fixture.join("root"), &root), (fixture.join("repo"), &repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        let repos = test_repos(&repo);
        let world_changed = update_world(UpdateWorldArgs {
            root: &root,
            repos: &repos,
        })
        .expect("world updates apply");
        assert!(world_changed, "the move rewrites one world entry");
        assert_eq!(
            std::fs::read(root.join("var/lib/portage/world")).unwrap(),
            std::fs::read(capture.join("var/lib/portage/world")).unwrap(),
            "world is byte-identical to the capture"
        );

        update_config_files(UpdateConfigFilesArgs {
            root: &root,
            repos: &repos,
            config_protect: "/etc",
            config_protect_mask: "/etc/env.d",
            case_insensitive: false,
            on_progress: None,
        })
        .expect("config updates apply");
        assert_eq!(
            std::fs::read(root.join("etc/portage/package.use")).unwrap(),
            std::fs::read(fixture.join("root/etc/portage/package.use")).unwrap(),
            "the original package.use is byte-identical to the fixture"
        );
        assert_eq!(
            std::fs::read(root.join("etc/portage/._cfg0000_package.use")).unwrap(),
            std::fs::read(capture.join("etc/portage/._cfg0000_package.use")).unwrap(),
            "._cfg0000_package.use is byte-identical to the capture"
        );
        let leftovers: Vec<_> = std::fs::read_dir(root.join("etc/portage"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("._cfg") && n != "._cfg0000_package.use")
            .collect();
        assert!(leftovers.is_empty(), "no extra ._cfg files: {leftovers:?}");
    }

    /// The one `gutest` main repo the fixture's updates live in.
    fn test_repos(repo: &Path) -> Vec<RepoConfig> {
        vec![RepoConfig {
            name: "gutest".to_string(),
            location: repo.to_path_buf(),
            priority: 0,
            is_main: true,
            masters: Vec::new(),
            profile_formats: Vec::new(),
            cache_formats: Vec::new(),
            aliases: Vec::new(),
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: Vec::new(),
        }]
    }

    /// Portage 3.0.83 sorts the world list before the atomic write
    /// (`_global_updates.py:208-210`): a move that leaves the list
    /// unsorted still lands sorted on disk.
    #[test]
    fn unsorted_world_is_sorted_by_the_move() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture = manifest.join("../../fixtures/global-updates");
        let scratch = portage_util::TempDir::new("global_updates_world_sort");
        let root = scratch.join("root");
        let repo = scratch.join("repo");
        for (from, to) in [(fixture.join("root"), &root), (fixture.join("repo"), &repo)] {
            let ok = std::process::Command::new("cp")
                .args(["-a", &from.to_string_lossy(), &to.to_string_lossy()])
                .status()
                .is_ok_and(|s| s.success());
            assert!(ok, "cp -a {} failed", from.display());
        }
        std::fs::write(
            root.join("var/lib/portage/world"),
            "dev-libs/zzz\ndev-libs/oldpkg\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("profiles/updates/1Q-2026"),
            "move dev-libs/oldpkg dev-libs/aaa\n",
        )
        .unwrap();
        let repos = test_repos(&repo);
        let world_changed = update_world(UpdateWorldArgs {
            root: &root,
            repos: &repos,
        })
        .expect("world updates apply");
        assert!(world_changed, "the move rewrites one world entry");
        assert_eq!(
            std::fs::read(root.join("var/lib/portage/world")).unwrap(),
            b"dev-libs/aaa\ndev-libs/zzz\n",
            "the rewritten world is sorted, not in original order"
        );
    }
}
