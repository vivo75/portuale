//! Package removal: `ebuild <file> unmerge`.
//!
//! Runs `pkg_prerm`, deletes every `CONTENTS`-listed entry deepest-first,
//! runs `pkg_postrm`, then removes the vdb entry itself on success.
//! Portage: dbapi/vartree.py, dblink.unmerge.
//!
//! Gaps: no `bsd_chflags` handling (dead code on Linux); other I/O errors
//! fail hard where Portage counts per-file failures; the builddir is
//! recomputed fresh instead of reusing a prior merge's.

use crate::ebuild_merge;
use crate::ebuild_phases;
use crate::env_update;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Whether `command` is the unmerge command this module implements --
/// `ebuild.rs` checks this before routing to execution.
pub fn is_unmerge_command(command: &str) -> bool {
    command == "unmerge"
}

struct ContentsEntry {
    node_type: String,
    abs_path: String,
    /// Present for `obj`/`sym` lines; `None` for `dir` (no trailing field).
    mtime: Option<i64>,
}

/// Parses a `CONTENTS` file's lines (see `ebuild_merge::format_contents_line`).
fn parse_contents(text: &str) -> Vec<ContentsEntry> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let node_type = (*fields.first()?).to_string();
            let abs_path = (*fields.get(1)?).to_string();
            let mtime = fields.last().and_then(|s| s.parse::<i64>().ok());
            Some(ContentsEntry {
                node_type,
                abs_path,
                mtime,
            })
        })
        .collect()
}

/// Orphaned symlinks by live target directory `(dev, ino)`; grouped so
/// `remove_dirs` can look them up by inode.
/// Portage: dbapi/vartree.py, _unmerge_dirs.
type ProtectedSymlinks = BTreeMap<(u64, u64), Vec<String>>;

/// Arguments of [`remove_contents`].
struct RemoveContentsArgs<'a> {
    root: &'a Path,
    category: &'a str,
    others_in_slot: &'a [String],
    config_protect: &'a str,
    config_protect_mask: &'a str,
    unmerge_orphans: bool,
    infodirs_inodes: &'a BTreeSet<(u64, u64)>,
    cfgfiledict: &'a BTreeMap<String, String>,
    stale_confmem: &'a mut Vec<String>,
    preserved_paths: &'a BTreeSet<String>,
    contents_text: &'a str,
}

/// Deletes every `CONTENTS`-listed entry from `root`, deepest paths first.
/// Skips entries another same-slot instance owns, `obj`/`sym` entries whose
/// live mtime changed (unless `unmerge_orphans`), and defers literal `dir`
/// entries to `remove_dirs`. Removed paths still remembered by `cfgfiledict`
/// are collected into `stale_confmem`; `preserved_paths` are filtered first.
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles.
fn remove_contents(args: RemoveContentsArgs<'_>) -> Result<(), String> {
    let RemoveContentsArgs {
        root,
        category,
        others_in_slot,
        config_protect,
        config_protect_mask,
        unmerge_orphans,
        infodirs_inodes,
        cfgfiledict,
        stale_confmem,
        preserved_paths,
        contents_text,
    } = args;
    let mut entries = parse_contents(contents_text);
    entries.retain(|e| !preserved_paths.contains(&e.abs_path));
    entries.sort_by(|a, b| b.abs_path.cmp(&a.abs_path));

    let mut protected_symlinks: ProtectedSymlinks = BTreeMap::new();
    let mut dirs: Vec<(PathBuf, (u64, u64))> = Vec::new();

    for entry in entries {
        let is_owned = others_in_slot
            .iter()
            .any(|other_pf| ebuild_merge::owns_path_pf(root, category, other_pf, &entry.abs_path));

        let relative = entry.abs_path.trim_start_matches('/');
        let dest = root.join(relative);

        // A live symlink-to-directory claimed as a literal `dir` entry
        // elsewhere defers to `remove_dirs` (Portage: _unmerge_pkgfiles).
        if is_owned
            && (entry.node_type == "sym" || entry.node_type == "dir")
            && let Ok(link_meta) = std::fs::symlink_metadata(&dest)
            && link_meta.file_type().is_symlink()
            && let Ok(target_meta) = std::fs::metadata(&dest)
            && target_meta.is_dir()
        {
            let symlink_orphan = others_in_slot.iter().any(|other_pf| {
                ebuild_merge::owned_node_type_pf(root, category, other_pf, &entry.abs_path)
                    .as_deref()
                    == Some("dir")
            });
            if symlink_orphan {
                protected_symlinks
                    .entry((target_meta.dev(), target_meta.ino()))
                    .or_default()
                    .push(entry.abs_path.clone());
            }
        }

        if is_owned {
            // Another same-slot instance claims this path: skip before
            // the mtime check (most often an in-place upgrade).
            continue;
        } else if cfgfiledict.contains_key(&entry.abs_path) {
            // Going away and still remembered: its config-memory entry is stale too.
            stale_confmem.push(entry.abs_path.clone());
        }

        // `FEATURES=unmerge-orphans`: delete a non-protected `obj`/`sym`
        // entry unconditionally, except a symlink to a directory (which
        // falls through to the mtime-checked removal instead).
        if unmerge_orphans
            && matches!(entry.node_type.as_str(), "obj" | "sym")
            && !ebuild_merge::is_protected(root, config_protect, config_protect_mask, &dest)
        {
            let symlink_to_dir = entry.node_type == "sym"
                && std::fs::metadata(&dest)
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
            if !symlink_to_dir {
                if let Err(e) = std::fs::remove_file(&dest)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(format!("{}: {e}", dest.display()));
                }
                continue;
            }
        }

        match entry.node_type.as_str() {
            "obj" | "sym" => {
                if let (Some(recorded_mtime), Ok(meta)) =
                    (entry.mtime, std::fs::symlink_metadata(&dest))
                {
                    let current_mtime = ebuild_merge::mtime_secs(&meta)?;
                    if current_mtime != recorded_mtime {
                        // Locally modified since the merge that recorded
                        // this entry -- leave it in place.
                        continue;
                    }
                }
                if let Err(e) = std::fs::remove_file(&dest)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(format!("{}: {e}", dest.display()));
                }
            }
            "dir" => {
                if let Ok(meta) = std::fs::symlink_metadata(&dest) {
                    dirs.push((dest, (meta.dev(), meta.ino())));
                }
            }
            _ => {
                // `fif`/`dev` entries are never unlinked, matching Portage.
            }
        }
    }
    remove_dirs(root, dirs, &mut protected_symlinks, infodirs_inodes);
    // Entries `remove_dirs` didn't resolve stay in place with no warning.
    Ok(())
}

/// Filenames the install-info index leaves behind.
/// Portage: dbapi/vartree.py, _infodir_cleanup.
const INFODIR_CLEANUP: [&str; 2] = ["dir", "dir.old"];

/// Removes leftover install-info index files so the `rmdir` can succeed.
/// Fires for a literal `info` name or an `INFOPATH`/`INFODIR` inode match;
/// only regular files are deleted.
/// Portage: dbapi/vartree.py, _unmerge_dirs.
fn cleanup_info_dir(dest: &Path, inode_key: (u64, u64), infodirs_inodes: &BTreeSet<(u64, u64)>) {
    let is_info_dir = infodirs_inodes.contains(&inode_key)
        || dest.file_name().and_then(|n| n.to_str()) == Some("info");
    if !is_info_dir {
        return;
    }
    let Ok(entries) = portage_util::read_dir_entries(dest) else {
        return;
    };
    let names: Vec<String> = entries
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.is_empty()
        || names.len() > INFODIR_CLEANUP.len()
        || !names.iter().all(|n| INFODIR_CLEANUP.contains(&n.as_str()))
    {
        return;
    }
    for name in &names {
        let child = dest.join(name);
        if std::fs::symlink_metadata(&child).is_ok_and(|m| m.is_file()) {
            let _ = std::fs::remove_file(&child);
        }
    }
}

/// Removes this package's literal `dir` entries, tolerating "already gone"
/// and "not empty". Deleting a directory also deletes orphaned symlinks
/// pointing at it and re-queues newly-emptied parents. `dirs` is a LIFO
/// stack so a directory can be pushed back after popping.
/// Portage: dbapi/vartree.py, _unmerge_dirs.
fn remove_dirs(
    root: &Path,
    dirs: Vec<(PathBuf, (u64, u64))>,
    protected_symlinks: &mut ProtectedSymlinks,
    infodirs_inodes: &BTreeSet<(u64, u64)>,
) {
    // Sorted ascending so `pop()` visits deepest paths first (a path
    // sorts after its own ancestor).
    let mut stack = dirs;
    stack.sort_by(|a, b| a.0.cmp(&b.0));
    let mut revisit: BTreeMap<PathBuf, (u64, u64)> = BTreeMap::new();

    while let Some((dest, inode_key)) = stack.pop() {
        cleanup_info_dir(&dest, inode_key, infodirs_inodes);
        match std::fs::remove_dir(&dest) {
            Ok(()) => {
                let Some(unmerge_syms) = protected_symlinks.remove(&inode_key) else {
                    continue;
                };
                let mut parents: BTreeSet<PathBuf> = BTreeSet::new();
                for relative_path in &unmerge_syms {
                    let sym_dest = root.join(relative_path.trim_start_matches('/'));
                    if std::fs::remove_file(&sym_dest).is_ok()
                        && let Some(parent) = sym_dest.parent()
                    {
                        parents.insert(parent.to_path_buf());
                    }
                }
                // Walk each newly-emptied ancestor chain, re-queuing failed
                // rmdirs now that the symlink is gone.
                let mut recursive_parents: BTreeSet<PathBuf> = BTreeSet::new();
                for parent in parents {
                    let mut cur = parent;
                    while revisit.contains_key(&cur) {
                        recursive_parents.insert(cur.clone());
                        match cur.parent() {
                            Some(p) => cur = p.to_path_buf(),
                            None => break,
                        }
                    }
                }
                for parent in recursive_parents {
                    if let Some(key) = revisit.remove(&parent) {
                        stack.push((parent, key));
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already gone: tolerate, no revisit needed.
            }
            Err(_) => {
                // Any other tolerated failure (chiefly "not empty") is
                // tracked for a possible later revisit.
                revisit.insert(dest, inode_key);
            }
        }
    }
}

/// Options for `run_unmerge`. `config_protect`/`config_protect_mask`
/// are only consulted by `FEATURES=unmerge-orphans`; defaults match
/// `make.globals`, like `MergeOptions::default()`.
pub struct UnmergeOptions {
    pub debug: bool,
    pub shell: ebuild_phases::ShellBackend,
    pub config_protect: String,
    pub config_protect_mask: String,
    /// `unmerge-orphans` is a default-on `FEATURES` token, so `Default`
    /// is `true`. The env-var read only checks the literal value, not the
    /// accumulated default set -- setting `FEATURES` to any other token
    /// reads as `false` here, unlike Portage.
    pub unmerge_orphans: bool,
    /// Portage's `PORTAGE_CONFIGROOT`; only consulted by the masters-chain
    /// resolution reached via the `prerm`/`postrm` phase calls below.
    pub config_root: PathBuf,
    /// `pf`s of the same category the merge's replace loop already unmerged
    /// but a database backend keeps installed until the publishing commit.
    /// Left out of `others_in_slot`; empty everywhere else.
    pub already_unmerged: Vec<String>,
}

impl Default for UnmergeOptions {
    fn default() -> Self {
        Self {
            debug: false,
            shell: ebuild_phases::ShellBackend::default(),
            config_protect: "/etc".to_string(),
            config_protect_mask: "/etc/env.d".to_string(),
            unmerge_orphans: true,
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            already_unmerged: Vec::new(),
        }
    }
}

/// Arguments of [`unmerge_pkgfiles`].
#[cfg(test)]
pub(crate) struct UnmergePkgfilesArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) also_keep: &'a [String],
    pub(crate) options: &'a UnmergeOptions,
    pub(crate) is_replacement: bool,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
}

/// File-removal core of the unmerge: deletes every `CONTENTS` entry
/// deepest-first with the same skips as `remove_contents`, but no
/// `prerm`/`postrm` hooks and no vdb-directory removal. Split out so the
/// binpkg replace path can reuse it phase-free (a binpkg carries no
/// ebuild to source a phase from). `also_keep` folds into `others_in_slot`;
/// the `is_replacement`/`replacement_*` triple threads the replace-loop
/// preserved-libs state (empty on a standalone `emerge -C`).
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles.
#[cfg(test)]
pub(crate) fn unmerge_pkgfiles(args: UnmergePkgfilesArgs<'_>) -> Result<(), String> {
    let UnmergePkgfilesArgs {
        root,
        category,
        pn,
        pf,
        also_keep,
        options,
        is_replacement,
        replacement_preserved,
        replacement_needed,
    } = args;
    unmerge_pkgfiles_into(UnmergePkgfilesIntoArgs {
        root,
        category,
        pn,
        pf,
        also_keep,
        options,
        is_replacement,
        replacement_preserved,
        replacement_needed,
        retire: None,
    })
}

/// Arguments of [`unmerge_pkgfiles_into`].
pub(crate) struct UnmergePkgfilesIntoArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) category: &'a str,
    pub(crate) pn: &'a str,
    pub(crate) pf: &'a str,
    pub(crate) also_keep: &'a [String],
    pub(crate) options: &'a UnmergeOptions,
    pub(crate) is_replacement: bool,
    pub(crate) replacement_preserved: &'a BTreeMap<String, Vec<String>>,
    pub(crate) replacement_needed: &'a [(String, Vec<crate::needed_elf::NeededEntry>)],
    pub(crate) retire: Option<&'a mut ebuild_merge::RetireWrites>,
}

/// [`unmerge_pkgfiles`] collecting the registry/config-memory/other-entry
/// writes into `retire` instead of committing one by one; `None` commits
/// directly. See [`retire_entry`].
pub(crate) fn unmerge_pkgfiles_into(args: UnmergePkgfilesIntoArgs<'_>) -> Result<(), String> {
    let UnmergePkgfilesIntoArgs {
        root,
        category,
        pn,
        pf,
        also_keep,
        options,
        is_replacement,
        replacement_preserved,
        replacement_needed,
        mut retire,
    } = args;
    // Through the root's installed-database backend. The error keeps the
    // underlying text: the io error, `ENOENT` for a missing entry, the
    // UTF-8 text for a non-UTF-8 `CONTENTS`.
    let key = portage_vdb::EntryKey::new(category, pf);
    let db = portage_vdb::for_root(root);
    let not_installed = |e: &dyn std::fmt::Display| {
        format!(
            "{}: not installed ({e})",
            entry_path_for_message(root, category, pf).display()
        )
    };
    let contents_text = match db.read_file(&key, "CONTENTS") {
        Ok(Some(bytes)) => String::from_utf8(bytes).map_err(|_| {
            not_installed(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            ))
        })?,
        Ok(None) => return Err(not_installed(&std::io::Error::from_raw_os_error(2))),
        Err(portage_vdb::Error::Io { source, .. }) => return Err(not_installed(&source)),
        Err(e) => return Err(not_installed(&e)),
    };

    // Every other installed version of this same category/PN in the same
    // SLOT, excluding self.
    let version = &pf[pn.len() + 1..];
    let own_slot = ebuild_merge::read_installed_slot(root, category, pn, version);
    let mut others_in_slot: Vec<String> = match &own_slot {
        Some(slot) => portage_repo::installed_versions(root, category, pn)
            .into_iter()
            .map(|version| format!("{pn}-{version}"))
            .filter(|other_pf| other_pf != pf && !options.already_unmerged.contains(other_pf))
            .filter(|other_pf| {
                let version = &other_pf[pn.len() + 1..];
                ebuild_merge::read_installed_slot(root, category, pn, version).as_deref()
                    == Some(slot.as_str())
            })
            .collect(),
        None => Vec::new(),
    };
    for keep_pf in also_keep {
        if !others_in_slot.contains(keep_pf) {
            others_in_slot.push(keep_pf.clone());
        }
    }

    // `INFOPATH`/`INFODIR`, collated from `/etc/env.d/*`; see
    // `env_update::info_dirs_inodes` for why this stays a separate computation.
    let infodirs_inodes = env_update::info_dirs_inodes(root);

    // The `_conf_mem_file` memory `ebuild_merge` writes on merge, read once
    // up front, then pruned of every path this unmerge removes and rewritten.
    let cfgfiledict = ebuild_merge::read_cfgfiledict(root);
    let mut stale_confmem: Vec<String> = Vec::new();

    // Portage: _prune_plib_registry with unmerge=True (see
    // `ebuild_merge::preserve_libs_on_unmerge`). `own_slot` defaults to
    // `"0"` like the installed-slot fallback for a package with no slot.
    let preserved_paths =
        ebuild_merge::preserve_libs_on_unmerge_into(ebuild_merge::PreserveLibsOnUnmergeIntoArgs {
            root,
            category,
            pn,
            pf,
            slot: own_slot.as_deref().unwrap_or("0"),
            contents_text: &contents_text,
            is_replacement,
            retire: retire.as_deref_mut(),
        })?;

    remove_contents(RemoveContentsArgs {
        root,
        category,
        others_in_slot: &others_in_slot,
        config_protect: &options.config_protect,
        config_protect_mask: &options.config_protect_mask,
        unmerge_orphans: options.unmerge_orphans,
        infodirs_inodes: &infodirs_inodes,
        cfgfiledict: &cfgfiledict,
        stale_confmem: &mut stale_confmem,
        preserved_paths: &preserved_paths,
        contents_text: &contents_text,
    })?;
    if !stale_confmem.is_empty() {
        let mut updated = cfgfiledict;
        for filename in &stale_confmem {
            updated.remove(filename);
        }
        match retire.as_deref_mut() {
            Some(retire) => retire.set_config_memory(updated),
            None => ebuild_merge::write_cfgfiledict(root, &updated)?,
        }
    }

    // A preserved library whose last surviving consumer is the package now
    // being removed is orphaned: delete it and drop it from the registry.
    // In replacement mode a consumer owned by the old instance still counts,
    // the old instance's linkmap data is excluded, and the replacing
    // package's `NEEDED` lines feed explicitly (see
    // `ebuild_merge::find_unused_preserved_libs`).
    let being_unmerged: std::collections::BTreeSet<String> = contents_text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1).map(String::from))
        .collect();
    // The unmerged instance itself, plus (database backends) the same-slot
    // instances this replace loop already unmerged: their entries stay
    // installed until the publishing commit, so they must not feed the
    // linkage map of the later instances' prune.
    let exclude_cpvs: Vec<String> = if is_replacement {
        std::iter::once(format!("{category}/{pf}"))
            .chain(
                options
                    .already_unmerged
                    .iter()
                    .map(|gone| format!("{category}/{gone}")),
            )
            .collect()
    } else {
        Vec::new()
    };
    ebuild_merge::prune_unused_preserved_libs_into(
        ebuild_merge::PruneUnusedPreservedLibsIntoArgs {
            root,
            unmerge_no_replacement: !is_replacement,
            being_unmerged: &|p| being_unmerged.contains(p),
            exclude_cpvs: &exclude_cpvs,
            replacement_preserved,
            replacement_needed,
            retire,
        },
    )?;

    Ok(())
}

/// Removes the vdb entry directory itself, then best-effort `rmdir`s its
/// now-maybe-empty parent category directory (ignored on failure). Split
/// out so the binpkg replace path can drop an old entry phaselessly.
/// Portage: dbapi/vartree.py, dblink.delete.
pub(crate) fn delete_vdb_dir(root: &Path, category: &str, pf: &str) -> Result<(), String> {
    // Deletes the entry, then best-effort category `rmdir`.
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.delete_entry(&portage_vdb::EntryKey::new(category, pf))
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())
}

/// The last step of a standalone unmerge: with `retire` one transaction
/// holds the row deletion with the collected writes; without it the entry
/// directory is removed directly (see [`delete_vdb_dir`]).
pub(crate) fn retire_entry(
    root: &Path,
    category: &str,
    pf: &str,
    retire: Option<ebuild_merge::RetireWrites>,
) -> Result<(), String> {
    match retire {
        Some(writes) => writes.commit(root, &portage_vdb::EntryKey::new(category, pf)),
        None => delete_vdb_dir(root, category, pf),
    }
}

/// The directory of the installed entry `category/pf` as messages print
/// it; the bare `category/pf` on a backend with no such directory.
pub(crate) fn entry_path_for_message(root: &Path, category: &str, pf: &str) -> std::path::PathBuf {
    portage_vdb::for_root(root)
        .entry_path(&portage_vdb::EntryKey::new(category, pf))
        .unwrap_or_else(|| std::path::PathBuf::from(format!("{category}/{pf}")))
}

/// `prerm` -> delete files -> `postrm`, then the vdb entry itself, but
/// only on success.
/// Portage: top-level `unmerge()`.
pub fn run_unmerge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &UnmergeOptions,
) -> Result<i32, String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    // One `stat` that follows symlinks; any error is "not there".
    let installed = portage_vdb::for_root(root)
        .file_meta(
            &portage_vdb::EntryKey::new(env.category.as_str(), env.split.pf.as_str()),
            "CONTENTS",
        )
        .is_ok_and(|meta| meta.is_some());
    if !installed {
        return Err(format!(
            "{}: not installed",
            entry_path_for_message(root, &env.category, &env.split.pf).display()
        ));
    }

    let prerm_status = ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
        ebuild_path,
        phase: "prerm",
        root,
        portage_tmpdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &[],
        log_file: None,
    })?;
    if prerm_status != 0 {
        return Ok(prerm_status);
    }

    let mut retire = ebuild_merge::RetireWrites::for_root(root);
    unmerge_pkgfiles_into(UnmergePkgfilesIntoArgs {
        root,
        category: &env.category,
        pn: &env.split.pn,
        pf: &env.split.pf,
        also_keep: &[],
        options,
        is_replacement: false,
        replacement_preserved: &BTreeMap::new(), // Standalone `emerge -C`: no replacing package.
        replacement_needed: &[],
        retire: retire.as_mut(),
    })?;

    let postrm_status = ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
        ebuild_path,
        phase: "postrm",
        root,
        portage_tmpdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &[],
        log_file: None,
    })?;
    if postrm_status != 0 {
        return Ok(postrm_status);
    }

    retire_entry(root, &env.category, &env.split.pf, retire)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;

    fn tempdir() -> std::path::PathBuf {
        TempDir::new("portuale-ebuild-unmerge-test").keep()
    }

    #[test]
    fn parse_contents_reads_type_and_path_ignoring_trailing_fields() {
        let text = "dir /usr\nobj /usr/hello.txt abc123 100\nsym /usr/link -> hello.txt 100\n";
        let entries = parse_contents(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].node_type, "dir");
        assert_eq!(entries[0].abs_path, "/usr");
        assert_eq!(entries[1].node_type, "obj");
        assert_eq!(entries[1].abs_path, "/usr/hello.txt");
        assert_eq!(entries[2].node_type, "sym");
        assert_eq!(entries[2].abs_path, "/usr/link");
    }

    #[test]
    fn remove_contents_deletes_files_symlinks_and_empties_directories() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/x")).unwrap();
        std::fs::write(root.join("usr/share/x/hello.txt"), b"hello").unwrap();
        std::os::unix::fs::symlink("hello.txt", root.join("usr/share/x/link.txt")).unwrap();

        let file_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/x/hello.txt")).unwrap(),
        )
        .unwrap();
        let link_mtime = ebuild_merge::mtime_secs(
            &std::fs::symlink_metadata(root.join("usr/share/x/link.txt")).unwrap(),
        )
        .unwrap();
        let contents = format!(
            "dir /usr\n\
             dir /usr/share\n\
             dir /usr/share/x\n\
             obj /usr/share/x/hello.txt abc123 {file_mtime}\n\
             sym /usr/share/x/link.txt -> hello.txt {link_mtime}\n"
        );

        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
        })
        .expect("remove_contents succeeds");

        assert!(!root.join("usr/share/x/hello.txt").exists());
        assert!(
            root.join("usr/share/x/link.txt")
                .symlink_metadata()
                .is_err()
        );
        assert!(!root.join("usr/share/x").exists());
        assert!(!root.join("usr/share").exists());
        assert!(!root.join("usr").exists());
    }

    #[test]
    fn remove_contents_leaves_a_locally_modified_file_in_place() {
        // Changed mtime reads as locally modified and stays (this also
        // protects CONFIG_PROTECT'd files).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"user's own edits").unwrap();

        // A recorded mtime that can never match the file just written
        // above (created "now").
        let contents = "obj /etc.conf abc123 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("etc.conf").is_file(),
            "a locally-modified file must survive unmerge"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_deletes_a_locally_modified_file() {
        // Unlike the default (previous test), a locally-modified entry
        // is deleted anyway.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"user's own edits").unwrap();

        let contents = "obj /etc.conf abc123 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            !root.join("etc.conf").exists(),
            "unmerge-orphans deletes a locally-modified file too"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_still_respects_config_protect() {
        // Excludes a CONFIG_PROTECT'd path: only the `!mtime` check is
        // overridden, not CONFIG_PROTECT itself.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let contents = "obj /etc/foo.conf abc123 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("etc/foo.conf").is_file(),
            "unmerge-orphans must not delete a CONFIG_PROTECT'd path"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_leaves_a_symlink_to_a_directory_alone() {
        // Excludes a symlink to a directory; the mtime-checked removal
        // leaves it alone here too.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("real-lib")).unwrap();
        std::os::unix::fs::symlink(root.join("real-lib"), root.join("lib")).unwrap();

        let contents = "sym /lib -> whatever 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("lib").symlink_metadata().is_ok(),
            "unmerge-orphans must not delete a symlink-to-directory"
        );
    }

    #[test]
    fn remove_contents_tolerates_a_directory_still_shared_with_another_package() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        // A file this CONTENTS doesn't know about, simulating another
        // still-installed package's own file in the same shared dir.
        std::fs::write(root.join("usr/share/other.txt"), b"other").unwrap();

        let contents = "dir /usr\ndir /usr/share\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        // /usr/share survives (non-empty), and so does its parent.
        assert!(root.join("usr/share/other.txt").is_file());
    }

    #[test]
    fn remove_contents_tolerates_entries_already_gone() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let contents = "obj /usr/share/x/hello.txt abc123 100\ndir /usr/share/x\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("missing entries are not an error");
    }

    #[test]
    fn remove_contents_skips_a_path_another_same_slot_package_still_owns() {
        // A path another same-slot entry's `CONTENTS` claims is left alone
        // before the mtime check.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        std::fs::write(root.join("usr/share/shared.txt"), b"shared").unwrap();
        std::fs::write(root.join("usr/share/only-mine.txt"), b"mine").unwrap();

        // Matching mtimes, so only the ownership check can explain the skip.
        let shared_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/shared.txt")).unwrap(),
        )
        .unwrap();
        let mine_mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/only-mine.txt")).unwrap(),
        )
        .unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/otherpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(
            other_vdb.join("CONTENTS"),
            format!("obj /usr/share/shared.txt abc123 {shared_mtime}\n"),
        )
        .unwrap();

        let contents = format!(
            "obj /usr/share/shared.txt abc123 {shared_mtime}\n\
             obj /usr/share/only-mine.txt abc123 {mine_mtime}\n"
        );
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &["otherpkg-2.0".to_string()],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("usr/share/shared.txt").is_file(),
            "a path another same-slot package still owns must survive unmerge"
        );
        assert!(
            !root.join("usr/share/only-mine.txt").exists(),
            "a path no other same-slot package owns is still deleted normally"
        );
    }

    #[test]
    fn remove_contents_collects_a_removed_cfgfiledict_path_into_stale_confmem() {
        // A removed path the config memory still remembers is collected
        // for the caller to drop.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"content").unwrap();
        let mtime =
            ebuild_merge::mtime_secs(&std::fs::metadata(root.join("etc.conf")).unwrap()).unwrap();

        let mut cfgfiledict = BTreeMap::new();
        cfgfiledict.insert("/etc.conf".to_string(), "deadbeef".to_string());
        cfgfiledict.insert("/untouched.conf".to_string(), "cafebabe".to_string());

        let contents = format!("obj /etc.conf abc123 {mtime}\n");
        let mut stale_confmem = Vec::new();
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &cfgfiledict,
            stale_confmem: &mut stale_confmem,
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
        })
        .expect("remove_contents succeeds");

        assert_eq!(stale_confmem, vec!["/etc.conf".to_string()]);
    }

    #[test]
    fn remove_contents_does_not_mark_an_owned_path_stale_even_if_cfgfiledict_remembers_it() {
        // An owned path is never stale: the memory still serves the survivor.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        std::fs::write(root.join("usr/share/shared.txt"), b"shared").unwrap();
        let mtime = ebuild_merge::mtime_secs(
            &std::fs::metadata(root.join("usr/share/shared.txt")).unwrap(),
        )
        .unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/otherpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(
            other_vdb.join("CONTENTS"),
            format!("obj /usr/share/shared.txt abc123 {mtime}\n"),
        )
        .unwrap();

        let mut cfgfiledict = BTreeMap::new();
        cfgfiledict.insert("/usr/share/shared.txt".to_string(), "deadbeef".to_string());

        let contents = format!("obj /usr/share/shared.txt abc123 {mtime}\n");
        let mut stale_confmem = Vec::new();
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &["otherpkg-2.0".to_string()],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &cfgfiledict,
            stale_confmem: &mut stale_confmem,
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
        })
        .expect("remove_contents succeeds");

        assert!(stale_confmem.is_empty());
    }

    #[test]
    fn remove_contents_leaves_an_orphaned_symlink_alone_while_its_target_is_still_needed() {
        // The orphaned symlink's target is not this package's `dir` entry,
        // so both survive untouched.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("keep/target")).unwrap();
        std::fs::write(root.join("keep/target/other.txt"), b"other").unwrap();
        std::os::unix::fs::symlink(root.join("keep/target"), root.join("keep/link")).unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/orphanpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(other_vdb.join("CONTENTS"), "dir /keep/link\n").unwrap();

        let contents = "dir /keep\nsym /keep/link -> whatever 100\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &["orphanpkg-2.0".to_string()],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("keep/link").symlink_metadata().is_ok(),
            "the orphaned symlink itself must survive -- its target is never resolved"
        );
        assert!(
            root.join("keep/target/other.txt").is_file(),
            "the symlink's own target directory, owned by nobody in this removal, is untouched"
        );
    }

    #[test]
    fn remove_contents_deletes_an_orphaned_symlink_once_its_target_directory_empties_and_revisits_the_freed_parent()
     {
        // This time the target is this package's `dir` entry: both go,
        // and the parent gets a revisit.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("zzz-parent")).unwrap();
        std::fs::create_dir_all(root.join("aaa-target")).unwrap();
        std::os::unix::fs::symlink(root.join("aaa-target"), root.join("zzz-parent/compat-link"))
            .unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/orphanpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(other_vdb.join("CONTENTS"), "dir /zzz-parent/compat-link\n").unwrap();

        let contents =
            "dir /zzz-parent\nsym /zzz-parent/compat-link -> whatever 100\ndir /aaa-target\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &["orphanpkg-2.0".to_string()],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            !root
                .join("zzz-parent/compat-link")
                .symlink_metadata()
                .is_ok(),
            "the symlink is deleted once its own target directory actually empties"
        );
        assert!(
            !root.join("aaa-target").exists(),
            "the symlink's own target directory is removed normally"
        );
        assert!(
            !root.join("zzz-parent").exists(),
            "the parent, blocked only by the now-deleted symlink, is revisited and removed too (bug #640058)"
        );
    }

    #[test]
    fn cleanup_info_dir_removes_a_lone_leftover_index_file() {
        let tmp = tempdir();
        let info = tmp.join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("dir"), b"").unwrap();

        cleanup_info_dir(&info, (0, 0), &BTreeSet::new());

        assert!(!info.join("dir").exists());
    }

    #[test]
    fn cleanup_info_dir_removes_both_dir_and_dir_old() {
        let tmp = tempdir();
        let info = tmp.join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("dir"), b"").unwrap();
        std::fs::write(info.join("dir.old"), b"").unwrap();

        cleanup_info_dir(&info, (0, 0), &BTreeSet::new());

        assert!(!info.join("dir").exists());
        assert!(!info.join("dir.old").exists());
    }

    #[test]
    fn cleanup_info_dir_leaves_a_real_remaining_file_alone() {
        // Cleanup fires only when everything remaining is the index subset;
        // any other file keeps it all.
        let tmp = tempdir();
        let info = tmp.join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("dir"), b"").unwrap();
        std::fs::write(info.join("automake.info"), b"real content").unwrap();

        cleanup_info_dir(&info, (0, 0), &BTreeSet::new());

        assert!(info.join("dir").exists());
        assert!(info.join("automake.info").exists());
    }

    #[test]
    fn cleanup_info_dir_ignores_a_directory_not_named_info() {
        let tmp = tempdir();
        let other = tmp.join("not-info");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("dir"), b"").unwrap();

        cleanup_info_dir(&other, (0, 0), &BTreeSet::new());

        assert!(other.join("dir").exists());
    }

    #[test]
    fn cleanup_info_dir_fires_on_an_infodirs_inodes_match_even_when_not_named_info() {
        // The inode match fires independently of the `info` name: an
        // `INFOPATH` entry can name any directory.
        let tmp = tempdir();
        let other = tmp.join("not-info-at-all");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("dir"), b"").unwrap();

        let meta = std::fs::metadata(&other).unwrap();
        let inode_key = (meta.dev(), meta.ino());
        let infodirs_inodes = BTreeSet::from([inode_key]);

        cleanup_info_dir(&other, inode_key, &infodirs_inodes);

        assert!(!other.join("dir").exists());
    }

    #[test]
    fn remove_contents_removes_an_info_directory_blocked_only_by_a_leftover_index_file() {
        // Without the cleanup the info dir would never empty out (`dir`
        // is untracked).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/info")).unwrap();
        std::fs::write(root.join("usr/share/info/dir"), b"").unwrap();

        let contents = "dir /usr/share\ndir /usr/share/info\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            !root.join("usr/share/info").exists(),
            "the info directory is fully removed once its own leftover index is cleaned up"
        );
        assert!(!root.join("usr/share").exists());
    }

    #[test]
    fn remove_contents_removes_a_real_env_d_sourced_infopath_directory_not_named_info() {
        // End to end: the env.d collation and the `infodirs_inodes`
        // threading connect.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc/env.d")).unwrap();
        std::fs::create_dir_all(root.join("opt/pkg/docs")).unwrap();
        std::fs::write(root.join("opt/pkg/docs/dir"), b"").unwrap();
        std::fs::write(
            root.join("etc/env.d/50-pkg"),
            "INFOPATH=\"/opt/pkg/docs\"\n",
        )
        .unwrap();

        let infodirs_inodes = crate::env_update::info_dirs_inodes(&root);
        assert!(
            !infodirs_inodes.is_empty(),
            "the real env.d entry must resolve"
        );

        let contents = "dir /opt/pkg\ndir /opt/pkg/docs\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &infodirs_inodes,
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
        })
        .expect("remove_contents succeeds");

        assert!(
            !root.join("opt/pkg/docs").exists(),
            "a real env.d-sourced INFOPATH directory is cleaned up even though it isn't named info"
        );
    }

    /// With defaults a locally-modified file is deleted anyway
    /// (`unmerge-orphans` is default-on).
    #[test]
    fn real_unmerge_deletes_a_locally_modified_file_by_portage_default() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let merge_status = crate::ebuild_merge::run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_merge::MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(merge_status, 0);

        std::fs::write(
            root.join("usr/share/mergepkg/hello.txt"),
            b"hand-modified content",
        )
        .unwrap();

        let unmerge_status =
            run_unmerge(&ebuild, &root, &portage_tmpdir, &UnmergeOptions::default())
                .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(
            !root.join("usr/share/mergepkg/hello.txt").exists(),
            "unmerge-orphans is on by real default, so the modified file is deleted anyway"
        );
    }

    #[test]
    fn real_unmerge_removes_a_previously_real_merged_package() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let merge_status = crate::ebuild_merge::run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_merge::MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(merge_status, 0);
        assert!(root.join("usr/share/mergepkg/hello.txt").is_file());
        let vdb_dir = root.join("var/db/pkg/dev-libs/mergepkg-1.0");
        assert!(vdb_dir.is_dir());

        let unmerge_status =
            run_unmerge(&ebuild, &root, &portage_tmpdir, &UnmergeOptions::default())
                .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(!root.join("usr/share/mergepkg/hello.txt").exists());
        assert!(!root.join("usr/share/mergepkg/hello-link.txt").exists());
        assert!(!root.join("usr/share/mergepkg").exists());
        assert!(!vdb_dir.exists());
        assert!(!root.join("var/db/pkg/dev-libs").exists());
    }

    #[test]
    fn real_unmerge_drops_a_stale_conf_mem_entry_but_keeps_an_unrelated_one() {
        // End to end: the config-memory read, stale collection, and
        // rewrite connect through `run_unmerge`.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let merge_status = crate::ebuild_merge::run_merge(
            &ebuild,
            &root,
            &portage_tmpdir,
            &crate::ebuild_merge::MergeOptions::default(),
            None,
        )
        .expect("run_merge succeeds");
        assert_eq!(merge_status, 0);

        // One entry for a path about to be removed, one for an untouched path.
        let mut cfgfiledict = BTreeMap::new();
        cfgfiledict.insert(
            "/usr/share/mergepkg/hello.txt".to_string(),
            "deadbeef".to_string(),
        );
        cfgfiledict.insert("/etc/unrelated.conf".to_string(), "cafebabe".to_string());
        crate::ebuild_merge::write_cfgfiledict(&root, &cfgfiledict).unwrap();

        let unmerge_status =
            run_unmerge(&ebuild, &root, &portage_tmpdir, &UnmergeOptions::default())
                .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        let after = crate::ebuild_merge::read_cfgfiledict(&root);
        assert_eq!(
            after.get("/etc/unrelated.conf"),
            Some(&"cafebabe".to_string()),
            "an unrelated entry must survive untouched"
        );
        assert!(
            !after.contains_key("/usr/share/mergepkg/hello.txt"),
            "the removed package's own now-stale entry must be dropped"
        );
    }

    #[test]
    fn real_in_place_upgrade_via_ebuild_merge_replaces_the_old_slot_mate() {
        // End-to-end in-place upgrade: merging 2.0 removes 1.0 but keeps
        // the shared file; unmerging 2.0 removes it.
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild_v1 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-1.0.ebuild");
        let ebuild_v2 = repo_root.join("dev-libs/othersinslotpkg/othersinslotpkg-2.0.ebuild");

        for ebuild in [&ebuild_v1, &ebuild_v2] {
            let merge_status = crate::ebuild_merge::run_merge(
                ebuild,
                &root,
                &portage_tmpdir,
                &crate::ebuild_merge::MergeOptions::default(),
                None,
            )
            .expect("run_merge succeeds");
            assert_eq!(merge_status, 0);
        }
        let vdb_v1 = root.join("var/db/pkg/dev-libs/othersinslotpkg-1.0");
        let vdb_v2 = root.join("var/db/pkg/dev-libs/othersinslotpkg-2.0");

        assert!(
            root.join("usr/share/othersinslotpkg/shared.txt").is_file(),
            "2.0 owns shared.txt -- replacing 1.0 must not delete it"
        );
        assert!(
            !root
                .join("usr/share/othersinslotpkg/only-in-v1.txt")
                .exists(),
            "only-in-v1.txt has no other owner -- the replace unmerges it"
        );
        assert!(
            root.join("usr/share/othersinslotpkg/only-in-v2.txt")
                .is_file()
        );
        assert!(
            !vdb_v1.exists(),
            "1.0's vdb entry is dropped by the replace"
        );
        assert!(vdb_v2.is_dir());

        let unmerge_v2_status = run_unmerge(
            &ebuild_v2,
            &root,
            &portage_tmpdir,
            &UnmergeOptions::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_v2_status, 0);
        assert!(!root.join("usr/share/othersinslotpkg/shared.txt").exists());
        assert!(!vdb_v2.exists());
    }

    #[test]
    fn unmerge_of_a_never_installed_package_fails_clearly() {
        let tmp = tempdir();
        let root = tmp.join("root");
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&portage_tmpdir).unwrap();

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repo");
        let ebuild = repo_root.join("dev-libs/mergepkg/mergepkg-1.0.ebuild");

        let result = run_unmerge(&ebuild, &root, &portage_tmpdir, &UnmergeOptions::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not installed"));
    }
}
