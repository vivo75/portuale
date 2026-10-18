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
use std::os::unix::fs::{FileTypeExt, MetadataExt};
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
    /// Present for `obj` lines; compared by the `!md5` arm.
    md5: Option<String>,
}

/// Parses a `CONTENTS` file's lines: a thin map over
/// [`portage_vdb::parse_line`], keeping only the kind, the absolute path
/// and (for `obj`/`sym`) the mtime `remove_contents` compares.
fn parse_contents(text: &str) -> Vec<ContentsEntry> {
    text.lines()
        .filter_map(|line| {
            let entry = portage_vdb::parse_line(line)?;
            let (node_type, mtime, md5) = match entry {
                portage_vdb::ContentsLine::Obj { mtime, md5, .. } => {
                    ("obj", Some(mtime), Some(md5.to_string()))
                }
                portage_vdb::ContentsLine::Sym { mtime, .. } => ("sym", Some(mtime), None),
                portage_vdb::ContentsLine::Dir { .. } => ("dir", None, None),
                portage_vdb::ContentsLine::Dev { .. } => ("dev", None, None),
                portage_vdb::ContentsLine::Fif { .. } => ("fif", None, None),
                portage_vdb::ContentsLine::Bin { .. } => ("bin", None, None),
            };
            Some(ContentsEntry {
                node_type: node_type.to_string(),
                abs_path: entry.path().to_string(),
                mtime,
                md5,
            })
        })
        .collect()
}

/// `unmerge-orphans` from the calling env alone: the default-on token
/// folded with the incremental semantics, so `-unmerge-orphans` removes
/// it and `-*` clears it (a bare `FEATURES=buildpkg` keeps it on).
/// The fallback wherever no resolved config is available.
/// Portage: const.py:130, incremental FEATURES
pub fn unmerge_orphans_from_env() -> bool {
    let env_tokens: Vec<String> = std::env::var("FEATURES")
        .map(|f| f.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let base = ["unmerge-orphans".to_string()];
    let layers: Vec<&[String]> = vec![&base, &env_tokens];
    portage_profile::fold_token_layers(&layers)
        .iter()
        .any(|t| t == "unmerge-orphans")
}

/// `PACKDEBUG_PATH` (`const.py:113`): under `FEATURES=packdebug` Portage
/// keeps serving old debuginfo tarballs to binhost clients instead of
/// unmerging them.
pub(crate) const PACKDEBUG_TARBALL_IGNORE: &str = "/usr/lib/debug/.tarball/*";

/// The `UNINSTALL_IGNORE` list with the packdebug tarball dir appended
/// exactly when `features` carries `packdebug` (a stale copy is dropped
/// first, so re-resolving is idempotent).
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles; const.py:113
pub(crate) fn uninstall_ignore_with_packdebug(base: &str, features: &str) -> String {
    let mut patterns: Vec<String> = shlex_split(base)
        .into_iter()
        .filter(|p| p != PACKDEBUG_TARBALL_IGNORE)
        .collect();
    if features.split_whitespace().any(|t| t == "packdebug") {
        patterns.push(PACKDEBUG_TARBALL_IGNORE.to_string());
    }
    patterns.join(" ")
}

/// POSIX-shell word splitting for `UNINSTALL_IGNORE` (quotes group,
/// backslash escapes the next char), as Portage's `shlex.split` does.
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles (`shlex.split`)
pub(crate) fn shlex_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else if q == '"' && c == '\\' {
                    match chars.next() {
                        Some(n) if n == '"' || n == '\\' => cur.push(n),
                        Some(n) => {
                            cur.push('\\');
                            cur.push(n);
                        }
                        None => cur.push('\\'),
                    }
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '\\' => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                    in_word = true;
                }
                c if c.is_whitespace() => {
                    if in_word {
                        out.push(std::mem::take(&mut cur));
                        in_word = false;
                    }
                }
                _ => {
                    cur.push(c);
                    in_word = true;
                }
            },
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

/// Whether the root-relative `abs_path` (leading `/`) matches any
/// `UNINSTALL_IGNORE` pattern, with Python-fnmatch semantics.
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles (`fnmatch.fnmatch`)
fn uninstall_ignore_matches(patterns: &[String], abs_path: &str) -> bool {
    patterns
        .iter()
        .any(|p| crate::install_mask::fnmatch(abs_path, p))
}

/// Greedy 72-column word wrap, Portage's `textwrap.wrap(msg, 72)` for a
/// message with no long words (the preserved-symlink notice).
fn wrap_72(msg: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in msg.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
        } else if cur.len() + 1 + word.len() <= 72 {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Live `(dev, ino)` of every recorded path's parent directory, each with
/// the recorded parents behind it. Portage builds the same map per dblink
/// to match a path through a symlinked directory by inode.
/// Portage: dbapi/vartree.py, dblink._match_contents (`_contents_inodes`)
fn recorded_parent_inodes(root: &Path, text: &str) -> BTreeMap<(u64, u64), Vec<String>> {
    let mut map: BTreeMap<(u64, u64), Vec<String>> = BTreeMap::new();
    let mut stat_done: BTreeSet<String> = BTreeSet::new();
    for (_, recorded) in portage_vdb::paths(text) {
        let parent = Path::new(recorded.trim_start_matches('/'))
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !stat_done.insert(parent.clone()) {
            continue;
        }
        if let Ok(meta) = std::fs::metadata(root.join(&parent)) {
            map.entry((meta.dev(), meta.ino()))
                .or_default()
                .push(parent);
        }
    }
    map
}

/// Whether the package whose `CONTENTS` is `text` owns `abs_path`: the
/// exact path, or Portage's narrow parent-inode form — the live parent
/// directory's `(dev, ino)` equals a recorded parent's, and the path
/// rebuilt under that recorded parent is itself recorded.
/// Portage: dbapi/vartree.py, dblink._match_contents.
fn contents_isowner(
    root: &Path,
    text: &str,
    abs_path: &str,
    inodes: &BTreeMap<(u64, u64), Vec<String>>,
) -> bool {
    if ebuild_merge::contents_text_owns(text, abs_path) {
        return true;
    }
    let rel = abs_path.trim_start_matches('/');
    let path = Path::new(rel);
    let (Some(parent), Some(base)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return false;
    };
    let Ok(parent_meta) = std::fs::metadata(root.join(parent)) else {
        return false;
    };
    let Some(parents) = inodes.get(&(parent_meta.dev(), parent_meta.ino())) else {
        return false;
    };
    parents.iter().any(|dir| {
        let rebuilt = if dir.is_empty() {
            format!("/{base}")
        } else {
            format!("/{dir}/{base}")
        };
        ebuild_merge::contents_text_owns(text, &rebuilt)
    })
}

/// Whether `text` owns `abs_path`: the exact path, or any recorded path
/// holding it as a proper prefix component. Portage's `getcontents`
/// materialises implicit parent `dir` entries, so a file installed
/// through a directory symlink owns the symlink too; the prefix form is
/// that mechanism without materialising the entries.
/// Portage: dbapi/vartree.py, getcontents (`dir_entry`); _owners.
fn contents_owns_or_under(text: &str, abs_path: &str) -> bool {
    let prefix = format!("{abs_path}/");
    portage_vdb::paths(text).any(|(_, path)| path == abs_path || path.starts_with(prefix.as_str()))
}

/// Orphaned symlinks by live target directory `(dev, ino)`; grouped so
/// `remove_dirs` can look them up by inode.
/// Portage: dbapi/vartree.py, _unmerge_dirs.
type ProtectedSymlinks = BTreeMap<(u64, u64), Vec<String>>;

/// Arguments of [`remove_contents`].
struct RemoveContentsArgs<'a> {
    root: &'a Path,
    category: &'a str,
    pf: &'a str,
    others_in_slot: &'a [String],
    /// The members of `others_in_slot` that are the package being merged
    /// over this one; their ownership comes from the new image's `CONTENTS`.
    replacing: &'a [String],
    config_protect: &'a str,
    config_protect_mask: &'a str,
    unmerge_orphans: bool,
    /// Raw `UNINSTALL_IGNORE` (the resolved scalar with the packdebug fold);
    /// split shell-style once per call.
    uninstall_ignore: &'a str,
    infodirs_inodes: &'a BTreeSet<(u64, u64)>,
    cfgfiledict: &'a BTreeMap<String, String>,
    stale_confmem: &'a mut Vec<String>,
    preserved_paths: &'a BTreeSet<String>,
    contents_text: &'a str,
    /// The direct-path listing printer; a disabled listing is a no-op
    /// (the `emerge -C` path and silent unit tests).
    listing: &'a mut ebuild_merge::MergeListing,
}

/// Deletes every `CONTENTS`-listed entry from `root`, deepest paths first,
/// in Portage's arm order: missing lstat skips, the `UNINSTALL_IGNORE` and
/// libdir-symlink ignores, the `is_owned`/symlink-orphan check, the orphan
/// arm for any non-directory (any recorded kind), the mtime check for kinds
/// other than `dir`/`fif`/`dev`, then the `!dir`, `!sym`, `!obj`+`!md5`,
/// `!fif` type guards. A `dir` entry that is a live symlink takes the sym
/// branch (R3); a symlink to a non-empty directory whose children are not
/// all owned regular files waits in `protected_symlinks` (R4) for the
/// owners search. Removed paths still remembered by `cfgfiledict` are
/// collected into `stale_confmem`; `preserved_paths` are filtered first.
/// Returns the R4b elog lines (empty when nothing was protected).
/// Portage: dbapi/vartree.py, _unmerge_pkgfiles.
fn remove_contents(args: RemoveContentsArgs<'_>) -> Result<Vec<String>, String> {
    let RemoveContentsArgs {
        root,
        category,
        pf,
        others_in_slot,
        replacing,
        config_protect,
        config_protect_mask,
        unmerge_orphans,
        uninstall_ignore,
        infodirs_inodes,
        cfgfiledict,
        stale_confmem,
        preserved_paths,
        contents_text,
        listing,
    } = args;
    let mut entries = parse_contents(contents_text);
    entries.retain(|e| !preserved_paths.contains(&e.abs_path));
    entries.sort_by(|a, b| b.abs_path.cmp(&a.abs_path));
    let ignore_patterns = shlex_split(uninstall_ignore);
    let self_inodes = recorded_parent_inodes(root, contents_text);

    let mut protected_symlinks: ProtectedSymlinks = BTreeMap::new();
    let mut dirs: Vec<(PathBuf, (u64, u64))> = Vec::new();

    // Each other instance's `CONTENTS`, read once.
    let owner_texts: Vec<Option<String>> = others_in_slot
        .iter()
        .map(|other_pf| {
            if replacing.contains(other_pf) {
                ebuild_merge::read_replacing_contents_pf(root, category, other_pf)
            } else {
                ebuild_merge::read_contents_pf(root, category, other_pf)
            }
        })
        .collect();

    for entry in entries {
        let relative = entry.abs_path.trim_start_matches('/');
        let dest = root.join(relative);

        // A missing lstat skips before anything else (a path under a
        // replaced directory reports ENOTDIR here too, not only ENOENT).
        // Portage: dbapi/vartree.py, `--- !found`.
        let lstat = match std::fs::symlink_metadata(&dest) {
            Ok(meta) => meta,
            Err(_) => {
                listing.unmerge_kept("!found", &entry.node_type, &dest);
                continue;
            }
        };
        let islink = lstat.file_type().is_symlink();
        // The followed stat: `None` includes a dangling symlink.
        let stat = std::fs::metadata(&dest).ok();
        let stat_is_dir = stat.as_ref().is_some_and(|m| m.is_dir());

        let is_owned = owner_texts
            .iter()
            .flatten()
            .any(|text| ebuild_merge::contents_text_owns(text, &entry.abs_path));
        // A live symlink-to-directory claimed as a literal `dir` entry
        // elsewhere defers to `remove_dirs` (Portage: _unmerge_pkgfiles).
        if is_owned
            && (entry.node_type == "sym" || entry.node_type == "dir")
            && let Ok(link_meta) = std::fs::symlink_metadata(&dest)
            && link_meta.file_type().is_symlink()
            && let Ok(target_meta) = std::fs::metadata(&dest)
            && target_meta.is_dir()
        {
            let symlink_orphan = owner_texts.iter().flatten().any(|text| {
                ebuild_merge::contents_text_node_type(text, &entry.abs_path).as_deref()
                    == Some("dir")
            });
            if symlink_orphan {
                protected_symlinks
                    .entry((target_meta.dev(), target_meta.ino()))
                    .or_default()
                    .push(entry.abs_path.clone());
            }
        }

        // R1 (`UNINSTALL_IGNORE` fnmatch, packdebug folded) and R2 (live
        // `/lib`, `/usr/lib`, `/usr/local/lib` symlinks never go): both
        // skip before ownership, like Portage's `cfgpro` arm.
        // Portage: dbapi/vartree.py, _unmerge_pkgfiles.
        if uninstall_ignore_matches(&ignore_patterns, &entry.abs_path)
            || (islink
                && matches!(
                    entry.abs_path.as_str(),
                    "/lib" | "/usr/lib" | "/usr/local/lib"
                ))
        {
            listing.unmerge_kept("cfgpro", &entry.node_type, &dest);
            continue;
        }

        if is_owned {
            // Another same-slot instance claims this path: skip before
            // the mtime check (most often an in-place upgrade).
            listing.unmerge_replaced(&entry.node_type, &dest);
            continue;
        } else if cfgfiledict.contains_key(&entry.abs_path) {
            // Going away and still remembered: its config-memory entry is stale too.
            stale_confmem.push(entry.abs_path.clone());
        }

        // `FEATURES=unmerge-orphans`: delete any non-directory that is
        // not a symlink to a directory and not protected, whatever the
        // recorded kind (`obj`/`sym`, but also `fif`/`dev` nodes and a
        // `dir` entry that is live a regular file).
        // Portage: dbapi/vartree.py, _unmerge_pkgfiles.
        if unmerge_orphans
            && !lstat.is_dir()
            && !(islink && stat_is_dir)
            && !ebuild_merge::is_protected(root, config_protect, config_protect_mask, &dest)
        {
            unlink_tolerant(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            listing.unmerge_removed(&entry.node_type, &dest);
            continue;
        }

        // The mtime check covers every kind but `dir`/`fif`/`dev`.
        if !matches!(entry.node_type.as_str(), "dir" | "fif" | "dev")
            && let Some(recorded_mtime) = entry.mtime
        {
            let current_mtime = ebuild_merge::mtime_secs(&lstat)?;
            if current_mtime != recorded_mtime {
                // Locally modified since the merge that recorded
                // this entry -- leave it in place.
                listing.unmerge_kept("!mtime", &entry.node_type, &dest);
                continue;
            }
        }

        match entry.node_type.as_str() {
            "obj" | "bin" => {
                // `!obj`: the followed target must be a regular file
                // (an `obj` replaced by a directory skips instead of
                // aborting the whole unmerge with EISDIR).
                if !stat.as_ref().is_some_and(|m| m.is_file()) {
                    listing.unmerge_kept("!obj", &entry.node_type, &dest);
                    continue;
                }
                // `!md5`: an `obj` that survives this far (protected, or
                // `-unmerge-orphans`) is removed only when its md5 still
                // matches `CONTENTS`. A file that vanished mid-unmerge
                // reads as `!obj`.
                if entry.node_type == "obj"
                    && let Some(recorded_md5) = entry.md5.as_deref()
                {
                    let live_md5 = match ebuild_merge::md5_hex(&dest) {
                        Ok(md5) => md5,
                        Err(_) => {
                            listing.unmerge_kept("!obj", &entry.node_type, &dest);
                            continue;
                        }
                    };
                    if live_md5 != recorded_md5.to_lowercase() {
                        listing.unmerge_kept("!md5", &entry.node_type, &dest);
                        continue;
                    }
                }
                unlink_tolerant(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                listing.unmerge_removed(&entry.node_type, &dest);
            }
            "sym" | "dir" => {
                // R3: a `dir` entry that is a live symlink takes the sym
                // branch (unlinked, subject to R4). A `dir` that is neither
                // a live symlink nor a live directory is `!dir`-kept.
                // Portage: dbapi/vartree.py, _unmerge_pkgfiles.
                if entry.node_type == "dir" && !islink {
                    // `!dir`: a recorded directory that is not a live
                    // directory stays (the mtime arm never ran for it).
                    if lstat.is_dir() {
                        dirs.push((dest, (lstat.dev(), lstat.ino())));
                    } else {
                        listing.unmerge_kept("!dir", &entry.node_type, &dest);
                    }
                    continue;
                }
                // `!sym`: a recorded symlink that is not a live symlink
                // stays.
                if !islink {
                    listing.unmerge_kept("!sym", &entry.node_type, &dest);
                    continue;
                }
                // R4: a symlink to a non-empty directory is kept unless
                // every child is a regular file this package owns; then it
                // waits in `protected_symlinks` for the owners search.
                // Portage: dbapi/vartree.py, _unmerge_pkgfiles (`!empty`).
                if let Some(target_meta) = stat.as_ref().filter(|m| m.is_dir()) {
                    let names: Vec<String> = portage_util::read_dir_entries(&dest)
                        .map(|entries| {
                            entries
                                .into_iter()
                                .map(|e| e.file_name().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    if !names.is_empty() {
                        let target_key = (target_meta.dev(), target_meta.ino());
                        let mut all_owned = true;
                        for name in &names {
                            let child_abs = format!("{}/{name}", entry.abs_path);
                            if !contents_isowner(root, contents_text, &child_abs, &self_inodes) {
                                all_owned = false;
                                break;
                            }
                            match std::fs::symlink_metadata(
                                root.join(child_abs.trim_start_matches('/')),
                            ) {
                                // Vanished mid-unmerge: skip, like Portage.
                                Err(_) => continue,
                                Ok(meta) if !meta.file_type().is_file() => {
                                    // Nested symlinks or directories make
                                    // the issue complex, so the symlink is
                                    // preserved to be on the safe side.
                                    all_owned = false;
                                    break;
                                }
                                Ok(_) => {}
                            }
                        }
                        if !all_owned {
                            protected_symlinks
                                .entry(target_key)
                                .or_default()
                                .push(entry.abs_path.clone());
                            listing.unmerge_kept("!empty", &entry.node_type, &dest);
                            continue;
                        }
                    }
                }
                unlink_tolerant(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                listing.unmerge_removed(&entry.node_type, &dest);
            }
            "fif" if !lstat.file_type().is_fifo() => {
                // `!fif`: a recorded fifo that is not a live fifo stays.
                listing.unmerge_kept("!fif", &entry.node_type, &dest);
            }
            "fif" => {
                // A live fifo was already unlinked by the orphan arm under
                // default FEATURES; reaching here keeps it.
                listing.unmerge_kept("", &entry.node_type, &dest);
            }
            _ => {
                // `dev` entries are kept, matching Portage.
                listing.unmerge_kept("", &entry.node_type, &dest);
            }
        }
    }
    remove_dirs(
        root,
        dirs,
        &mut protected_symlinks,
        infodirs_inodes,
        &mut *listing,
    );
    let mut elog = unmerge_protected_symlinks(UnmergeProtectedArgs {
        root,
        category,
        pf,
        others_in_slot: &owner_texts,
        infodirs_inodes,
        protected_symlinks: &mut protected_symlinks,
        listing,
    })?;
    // Whatever R4b left behind stays, with the preserved notice.
    // Portage: dbapi/vartree.py, _unmerge_pkgfiles (trailing elog).
    if !protected_symlinks.is_empty() {
        let flat: BTreeSet<&String> = protected_symlinks
            .values()
            .flat_map(|syms| syms.iter())
            .collect();
        elog.extend(wrap_72(
            "One or more symlinks to directories have been preserved in order to \
             ensure that files installed via these symlinks remain accessible. \
             This indicates that the mentioned symlink(s) may be obsolete remnants \
             of an old install, and it may be appropriate to replace a given symlink \
             with the directory that it points to.",
        ));
        elog.push(String::new());
        for f in flat {
            elog.push(format!(
                "\t{}",
                root.join(f.trim_start_matches('/')).display()
            ));
        }
        elog.push(String::new());
    }
    Ok(elog)
}

/// Arguments of [`unmerge_protected_symlinks`].
struct UnmergeProtectedArgs<'a> {
    root: &'a Path,
    category: &'a str,
    pf: &'a str,
    others_in_slot: &'a [Option<String>],
    infodirs_inodes: &'a BTreeSet<(u64, u64)>,
    protected_symlinks: &'a mut ProtectedSymlinks,
    listing: &'a mut ebuild_merge::MergeListing,
}

/// Portage's `_unmerge_protected_symlinks`: for the leftover
/// `protected_symlinks`, return early when a same-slot instance owns the
/// path; otherwise search every installed package for owners of the
/// symlink path (an exact record, or a record under it — Portage's
/// `getcontents` materialises implicit parent dirs) and unlink it when
/// only this package owns it, re-running `remove_dirs` over its ancestors.
/// Returns the search elog lines (empty when the early return fires).
/// Portage: dbapi/vartree.py, _unmerge_protected_symlinks.
fn unmerge_protected_symlinks(args: UnmergeProtectedArgs<'_>) -> Result<Vec<String>, String> {
    let UnmergeProtectedArgs {
        root,
        category,
        pf,
        others_in_slot,
        infodirs_inodes,
        protected_symlinks,
        listing,
    } = args;
    if protected_symlinks.is_empty() {
        return Ok(Vec::new());
    }
    let flat: Vec<String> = {
        let mut set = BTreeSet::new();
        for syms in protected_symlinks.values() {
            set.extend(syms.iter().cloned());
        }
        set.into_iter().collect()
    };

    for f in &flat {
        for other in others_in_slot.iter().flatten() {
            let inodes = recorded_parent_inodes(root, other);
            if contents_isowner(root, other, f, &inodes) {
                // Another same-slot instance owns a file via this
                // symlink: skip the search (the trailing preserved
                // notice still fires in the caller).
                return Ok(Vec::new());
            }
        }
    }

    let mut elog = vec![
        String::new(),
        "Directory symlink(s) may need protection:".to_string(),
        String::new(),
    ];
    for f in &flat {
        elog.push(format!(
            "\t{}",
            root.join(f.trim_start_matches('/')).display()
        ));
    }
    elog.extend(
        [
            "",
            "Use the UNINSTALL_IGNORE variable to exempt specific symlinks",
            "from the following search (see the make.conf man page).",
            "",
            "Searching all installed packages for files installed via above symlink(s)...",
            "",
        ]
        .into_iter()
        .map(String::from),
    );

    let db = portage_vdb::for_root(root);
    let all = db.read_file_all("CONTENTS").map_err(|e| e.to_string())?;
    let mut owned = false;
    'scan: for (key, bytes) in &all {
        if key.category == category && key.pf == pf {
            continue;
        }
        let Some(bytes) = bytes else { continue };
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        for f in &flat {
            if contents_owns_or_under(text, f) {
                owned = true;
                break 'scan;
            }
        }
    }

    if !owned {
        elog.push(
            "The above directory symlink(s) are all safe to remove. Removing them now..."
                .to_string(),
        );
        elog.push(String::new());
        let mut dirs = Vec::new();
        for syms in protected_symlinks.values() {
            for relative_path in syms {
                let obj = root.join(relative_path.trim_start_matches('/'));
                let mut parent = obj.parent().map(Path::to_path_buf);
                while let Some(p) = parent {
                    if p == root || !p.starts_with(root) {
                        break;
                    }
                    if let Ok(meta) = std::fs::symlink_metadata(&p) {
                        dirs.push((p.clone(), (meta.dev(), meta.ino())));
                    }
                    parent = p.parent().map(Path::to_path_buf);
                }
                unlink_tolerant(&obj).map_err(|e| format!("{}: {e}", obj.display()))?;
                listing.unmerge_removed("sym", &obj);
            }
        }
        protected_symlinks.clear();
        remove_dirs(
            root,
            dirs,
            protected_symlinks,
            infodirs_inodes,
            &mut *listing,
        );
    }
    Ok(elog)
}

/// Echo the R4b lines (` * `, Portage's echo module) and append them to the
/// elog summary log; both best-effort around a successful unmerge.
/// Portage: dbapi/vartree.py, dblink._elog("elog", "postrm", ...).
pub(crate) fn emit_unmerge_elog(root: &Path, category: &str, pf: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    if crate::elog::echo_enabled() {
        for line in lines {
            println!(" * {line}");
        }
    }
    if crate::elog::module_enabled("save_summary") {
        let _ = crate::elog::save_summary_process(
            &crate::elog::logdir(root),
            &format!("{category}/{pf}"),
            &lines.join("\n"),
        );
    }
}

/// `remove_file` tolerating what Portage's `unlink()` tolerates beyond
/// "already gone": the type-confusion pair (a path that changed kind
/// mid-unmerge), so a stale `CONTENTS` line can never abort the removal.
/// Portage: dbapi/vartree.py, `_ignored_unlink_errnos`.
fn unlink_tolerant(dest: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) if matches!(e.raw_os_error(), Some(n) if n == libc::EISDIR || n == libc::ENOTDIR) => {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Filenames the install-info index leaves behind.
/// Portage: dbapi/vartree.py, _infodir_cleanup.
const INFODIR_CLEANUP: [&str; 2] = ["dir", "dir.old"];

/// Removes leftover install-info index files so the `rmdir` can succeed.
/// Fires for a literal `info` name or an `INFOPATH`/`INFODIR` inode match;
/// only regular files are deleted.
/// Portage: dbapi/vartree.py, _unmerge_dirs.
fn cleanup_info_dir(
    dest: &Path,
    inode_key: (u64, u64),
    infodirs_inodes: &BTreeSet<(u64, u64)>,
    listing: &mut ebuild_merge::MergeListing,
) {
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
            match std::fs::remove_file(&child) {
                Ok(()) => listing.unmerge_removed("obj", &child),
                Err(_) => listing.show_unmerge("!!!", "", "obj", &child),
            }
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
    listing: &mut ebuild_merge::MergeListing,
) {
    // Sorted ascending so `pop()` visits deepest paths first (a path
    // sorts after its own ancestor).
    let mut stack = dirs;
    stack.sort_by(|a, b| a.0.cmp(&b.0));
    let mut revisit: BTreeMap<PathBuf, (u64, u64)> = BTreeMap::new();

    while let Some((dest, inode_key)) = stack.pop() {
        cleanup_info_dir(&dest, inode_key, infodirs_inodes, &mut *listing);
        match std::fs::remove_dir(&dest) {
            Ok(()) => {
                listing.unmerge_removed("dir", &dest);
                let Some(unmerge_syms) = protected_symlinks.remove(&inode_key) else {
                    continue;
                };
                let mut parents: BTreeSet<PathBuf> = BTreeSet::new();
                for relative_path in &unmerge_syms {
                    let sym_dest = root.join(relative_path.trim_start_matches('/'));
                    match std::fs::remove_file(&sym_dest) {
                        Ok(()) => {
                            listing.unmerge_removed("sym", &sym_dest);
                            if let Some(parent) = sym_dest.parent() {
                                parents.insert(parent.to_path_buf());
                            }
                        }
                        Err(_) => listing.show_unmerge("!!!", "", "sym", &sym_dest),
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
                listing.unmerge_kept("!empty", "dir", &dest);
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
    /// `UNINSTALL_IGNORE` paths the unmerge skips (the resolved scalar with
    /// the packdebug fold); mirrors `MergeOptions::uninstall_ignore`, which
    /// is where the three entry points resolve it.
    /// Portage: dbapi/vartree.py, _unmerge_pkgfiles; cnf/make.globals:93
    pub uninstall_ignore: String,
    /// Portage's `PORTAGE_CONFIGROOT`; only consulted by the masters-chain
    /// resolution reached via the `prerm`/`postrm` phase calls below.
    pub config_root: PathBuf,
    /// `pf`s of the same category the merge's replace loop already unmerged
    /// but a database backend keeps installed until the publishing commit.
    /// Left out of `others_in_slot`; empty everywhere else.
    pub already_unmerged: Vec<String>,
    /// Print the per-file unmerge listing to stdout: set only on the
    /// direct `ebuild <file> unmerge` path (and the direct merge's
    /// replace loop, via `MergeOptions.direct`). The `emerge -C` path
    /// stays silent.
    /// Portage: vartree.py, `dblink._display_merge`.
    pub direct: bool,
}

impl Default for UnmergeOptions {
    fn default() -> Self {
        Self {
            debug: false,
            shell: ebuild_phases::ShellBackend::default(),
            config_protect: "/etc".to_string(),
            config_protect_mask: "/etc/env.d".to_string(),
            unmerge_orphans: true,
            uninstall_ignore: "/lib/modules/*".to_string(),
            config_root: PathBuf::from("/dev/null/no-config-root-configured"),
            already_unmerged: Vec::new(),
            direct: false,
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
        listing: &mut ebuild_merge::MergeListing::new(false),
    })
    .map(|_| ())
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
    /// The direct-path listing printer; a disabled listing is a no-op.
    pub(crate) listing: &'a mut ebuild_merge::MergeListing,
}

/// [`unmerge_pkgfiles`] collecting the registry/config-memory/other-entry
/// writes into `retire` instead of committing one by one; `None` commits
/// directly. Returns the R4b elog lines for the caller to echo and log.
/// See [`retire_entry`].
pub(crate) fn unmerge_pkgfiles_into(
    args: UnmergePkgfilesIntoArgs<'_>,
) -> Result<Vec<String>, String> {
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
        listing,
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
        // A replaced instance with no `CONTENTS` (a resolve-fixture seed,
        // never a real merge) owns no recorded files: walk an empty list
        // instead of failing, like Portage's `getcontents()` returning
        // `{}` on `ENOENT` (vartree.py, `dblink.getcontents`). Scoped to
        // the replace loop (`is_replacement`) and to missing files only:
        // a standalone unmerge keeps the "not installed" error
        // `emerge -C` relies on (#348 S6 M1).
        Ok(None) if is_replacement => String::new(),
        Err(portage_vdb::Error::Io { source, .. })
            if is_replacement && source.raw_os_error() == Some(2) =>
        {
            String::new()
        }
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

    let elog = remove_contents(RemoveContentsArgs {
        root,
        category,
        pf,
        others_in_slot: &others_in_slot,
        replacing: also_keep,
        config_protect: &options.config_protect,
        config_protect_mask: &options.config_protect_mask,
        unmerge_orphans: options.unmerge_orphans,
        uninstall_ignore: &options.uninstall_ignore,
        infodirs_inodes: &infodirs_inodes,
        cfgfiledict: &cfgfiledict,
        stale_confmem: &mut stale_confmem,
        preserved_paths: &preserved_paths,
        contents_text: &contents_text,
        listing,
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
    let being_unmerged: std::collections::BTreeSet<String> = portage_vdb::paths(&contents_text)
        .map(|(_, p)| p.to_string())
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

    Ok(elog)
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
        log_file: crate::ebuild_phases::PhaseLog::Inherit,
    })?;
    if prerm_status != 0 {
        return Ok(prerm_status);
    }

    let mut retire = ebuild_merge::RetireWrites::for_root(root);
    let mut listing = ebuild_merge::MergeListing::new(options.direct);
    let elog_lines = unmerge_pkgfiles_into(UnmergePkgfilesIntoArgs {
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
        listing: &mut listing,
    })?;
    emit_unmerge_elog(root, &env.category, &env.split.pf, &elog_lines);

    let postrm_status = ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
        ebuild_path,
        phase: "postrm",
        root,
        portage_tmpdir,
        debug: options.debug,
        config_root: &options.config_root,
        shell: options.shell,
        build_env: &[],
        log_file: crate::ebuild_phases::PhaseLog::Inherit,
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
    fn is_unmerge_command_covers_exactly_unmerge() {
        assert!(is_unmerge_command("unmerge"));
        assert!(!is_unmerge_command("merge"));
        assert!(!is_unmerge_command("qmerge"));
        assert!(!is_unmerge_command("install"));
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
    fn parse_contents_keeps_spaces_in_paths_and_skips_junk() {
        // Portage 3.0.82.2, S0 cell S1 shapes (#357): the shared parser
        // keeps the full path; a non-matching line is skipped.
        let text = "obj /usr/share/t355/a b.txt abc123 100\n\
                    sym /usr/share/t355/c d -> a b.txt 100\n\
                    junk line here\n";
        let entries = parse_contents(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].node_type, "obj");
        assert_eq!(entries[0].abs_path, "/usr/share/t355/a b.txt");
        assert_eq!(entries[0].mtime, Some(100));
        assert_eq!(entries[1].node_type, "sym");
        assert_eq!(entries[1].abs_path, "/usr/share/t355/c d");
        assert_eq!(entries[1].mtime, Some(100));
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
        // Real md5s: the `!md5` arm keeps an entry whose content drifted.
        let hello_md5 = ebuild_merge::md5_hex(&root.join("usr/share/x/hello.txt")).unwrap();
        let contents = format!(
            "dir /usr\n\
             dir /usr/share\n\
             dir /usr/share/x\n\
             obj /usr/share/x/hello.txt {hello_md5} {file_mtime}\n\
             sym /usr/share/x/link.txt -> hello.txt {link_mtime}\n"
        );

        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("etc/foo.conf").is_file(),
            "unmerge-orphans must not delete a CONFIG_PROTECT'd path"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_leaves_a_symlink_to_a_directory_alone() {
        // R2 skips a live `/lib` symlink before anything else (this also
        // exercises the orphan arm's symlink-to-directory exclusion, which
        // would leave it alone here too).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("real-lib")).unwrap();
        std::os::unix::fs::symlink(root.join("real-lib"), root.join("lib")).unwrap();

        let contents = "sym /lib -> whatever 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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

        // Real md5s: the `!md5` arm keeps an entry whose content drifted.
        let shared_md5 = ebuild_merge::md5_hex(&root.join("usr/share/shared.txt")).unwrap();
        let mine_md5 = ebuild_merge::md5_hex(&root.join("usr/share/only-mine.txt")).unwrap();
        let contents = format!(
            "obj /usr/share/shared.txt {shared_md5} {shared_mtime}\n\
             obj /usr/share/only-mine.txt {mine_md5} {mine_mtime}\n"
        );
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &["otherpkg-2.0".to_string()],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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

        // A real md5: the path is genuinely removed (not `!md5`-kept).
        let md5 = ebuild_merge::md5_hex(&root.join("etc.conf")).unwrap();
        let contents = format!("obj /etc.conf {md5} {mtime}\n");
        let mut stale_confmem = Vec::new();
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &cfgfiledict,
            stale_confmem: &mut stale_confmem,
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &cfgfiledict,
            stale_confmem: &mut stale_confmem,
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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

        cleanup_info_dir(
            &info,
            (0, 0),
            &BTreeSet::new(),
            &mut ebuild_merge::MergeListing::new(false),
        );

        assert!(!info.join("dir").exists());
    }

    #[test]
    fn cleanup_info_dir_removes_both_dir_and_dir_old() {
        let tmp = tempdir();
        let info = tmp.join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("dir"), b"").unwrap();
        std::fs::write(info.join("dir.old"), b"").unwrap();

        cleanup_info_dir(
            &info,
            (0, 0),
            &BTreeSet::new(),
            &mut ebuild_merge::MergeListing::new(false),
        );

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

        cleanup_info_dir(
            &info,
            (0, 0),
            &BTreeSet::new(),
            &mut ebuild_merge::MergeListing::new(false),
        );

        assert!(info.join("dir").exists());
        assert!(info.join("automake.info").exists());
    }

    #[test]
    fn cleanup_info_dir_ignores_a_directory_not_named_info() {
        let tmp = tempdir();
        let other = tmp.join("not-info");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("dir"), b"").unwrap();

        cleanup_info_dir(
            &other,
            (0, 0),
            &BTreeSet::new(),
            &mut ebuild_merge::MergeListing::new(false),
        );

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

        cleanup_info_dir(
            &other,
            inode_key,
            &infodirs_inodes,
            &mut ebuild_merge::MergeListing::new(false),
        );

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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &infodirs_inodes,
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
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
    fn an_obj_whose_md5_changed_but_mtime_restored_survives_without_unmerge_orphans() {
        // Portage 3.0.82.2, S0 cells M1/M2 (not from portuale output): the
        // content edited with its mtime put back differs only in md5, so
        // Portage keeps it (`--- !md5`).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc/t355")).unwrap();
        let live = root.join("etc/t355/mod.conf");
        std::fs::write(&live, b"v1").unwrap();
        let mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&live).unwrap()).unwrap();
        let md5_v1 = ebuild_merge::md5_hex(&live).unwrap();
        std::fs::write(&live, b"admin edit").unwrap();
        filetime::set_file_mtime(&live, filetime::FileTime::from_unix_time(mtime, 0)).unwrap();

        let contents = format!("obj /etc/t355/mod.conf {md5_v1} {mtime}\n");
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            live.is_file(),
            "an md5-only drift must survive without unmerge-orphans"
        );
    }

    #[test]
    fn an_obj_replaced_by_a_directory_is_skipped_not_fatal() {
        // Portage 3.0.82.2, S0 cell T1b (`--- !obj`, rc 0): the recorded
        // mtime matches, so the `!obj` guard skips instead of aborting the
        // whole unmerge with EISDIR.
        let tmp = tempdir();
        let root = tmp.join("root");
        let dir = root.join("usr/share/t355/o");
        std::fs::create_dir_all(&dir).unwrap();
        let mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&dir).unwrap()).unwrap();
        std::fs::write(dir.join("f"), b"admin file").unwrap();
        filetime::set_file_mtime(&dir, filetime::FileTime::from_unix_time(mtime, 0)).unwrap();

        let contents = format!("obj /usr/share/t355/o abc123 {mtime}\n");
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("a type-changed obj must skip, not fail");

        assert!(
            dir.join("f").is_file(),
            "the replacement directory must survive"
        );
    }

    #[test]
    fn a_sym_replaced_by_a_regular_file_is_kept_without_unmerge_orphans() {
        // Portage 3.0.82.2, S0 cell T2 (`--- !sym`, kept): the recorded
        // mtime restored, unmerged with `-unmerge-orphans`.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355")).unwrap();
        std::fs::write(root.join("usr/share/t355/keep"), b"keep").unwrap();
        std::os::unix::fs::symlink("keep", root.join("usr/share/t355/s")).unwrap();
        let link = root.join("usr/share/t355/s");
        let mtime = ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, b"admin file").unwrap();
        filetime::set_file_mtime(&link, filetime::FileTime::from_unix_time(mtime, 0)).unwrap();
        let keep_mtime =
            ebuild_merge::mtime_secs(&std::fs::metadata(root.join("usr/share/t355/keep")).unwrap())
                .unwrap();
        let keep_md5 = ebuild_merge::md5_hex(&root.join("usr/share/t355/keep")).unwrap();

        let contents = format!(
            "obj /usr/share/t355/keep {keep_md5} {keep_mtime}\n\
             sym /usr/share/t355/s -> keep {mtime}\n"
        );
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            link.is_file() && !link.symlink_metadata().unwrap().file_type().is_symlink(),
            "a sym replaced by a file must survive without unmerge-orphans"
        );
        assert!(
            !root.join("usr/share/t355/keep").exists(),
            "the untouched entry still unmerges normally"
        );
    }

    #[test]
    fn a_sym_replaced_by_a_regular_file_is_unlinked_by_the_orphan_arm() {
        // Default FEATURES: the live regular file is a non-directory that
        // is not protected, so the orphan arm takes it whatever the
        // recorded kind (S0 cell T2's default-FEATURES shape).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355")).unwrap();
        std::fs::write(root.join("usr/share/t355/keep"), b"keep").unwrap();
        std::os::unix::fs::symlink("keep", root.join("usr/share/t355/s")).unwrap();
        let link = root.join("usr/share/t355/s");
        let mtime = ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::fs::write(&link, b"admin file").unwrap();
        filetime::set_file_mtime(&link, filetime::FileTime::from_unix_time(mtime, 0)).unwrap();

        let contents = format!("sym /usr/share/t355/s -> keep {mtime}\n");
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            others_in_slot: &[],
            replacing: &[],
            pf: "",
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            link.symlink_metadata().is_err(),
            "the orphan arm unlinks any non-directory under default FEATURES"
        );
    }

    #[test]
    fn a_dir_replaced_by_a_regular_file_follows_the_orphan_arm() {
        // Portage 3.0.82.2, S0 cells T3a/T3b: default FEATURES unlinks the
        // file through the orphan arm; `-unmerge-orphans` keeps it (`--- !dir`).
        for unmerge_orphans in [true, false] {
            let tmp = tempdir();
            let root = tmp.join("root");
            let dir = root.join("usr/share/t355/dd");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::remove_dir(&dir).unwrap();
            std::fs::write(&dir, b"admin file").unwrap();

            let contents = "dir /usr/share/t355/dd\n";
            remove_contents(RemoveContentsArgs {
                root: &root,
                category: "app-misc",
                others_in_slot: &[],
                replacing: &[],
                pf: "",
                uninstall_ignore: "",
                config_protect: "/etc",
                config_protect_mask: "",
                unmerge_orphans,
                infodirs_inodes: &BTreeSet::new(),
                cfgfiledict: &BTreeMap::new(),
                stale_confmem: &mut Vec::new(),
                preserved_paths: &BTreeSet::new(),
                contents_text: contents,
                listing: &mut ebuild_merge::MergeListing::new(false),
            })
            .expect("a type-changed dir must never abort the unmerge");

            assert_eq!(
                dir.symlink_metadata().is_err(),
                unmerge_orphans,
                "unmerge_orphans={unmerge_orphans}: the file goes exactly then"
            );
        }
    }

    #[test]
    fn a_fifo_is_unlinked_under_unmerge_orphans_and_kept_without_them() {
        // Portage 3.0.82.2, S0 cells F1a/F1b: default FEATURES unlinks the
        // fifo (`<<< fif`); `-unmerge-orphans` keeps it (`--- fif`).
        for unmerge_orphans in [true, false] {
            let tmp = tempdir();
            let root = tmp.join("root");
            std::fs::create_dir_all(root.join("usr/share/t355")).unwrap();
            let fifo = root.join("usr/share/t355/p");
            let status = std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .expect("mkfifo runs unprivileged");
            assert!(status.success());

            let contents = "fif /usr/share/t355/p\n";
            remove_contents(RemoveContentsArgs {
                root: &root,
                category: "app-misc",
                others_in_slot: &[],
                replacing: &[],
                pf: "",
                uninstall_ignore: "",
                config_protect: "/etc",
                config_protect_mask: "",
                unmerge_orphans,
                infodirs_inodes: &BTreeSet::new(),
                cfgfiledict: &BTreeMap::new(),
                stale_confmem: &mut Vec::new(),
                preserved_paths: &BTreeSet::new(),
                contents_text: contents,
                listing: &mut ebuild_merge::MergeListing::new(false),
            })
            .expect("remove_contents succeeds");

            assert_eq!(
                fifo.symlink_metadata().is_err(),
                unmerge_orphans,
                "unmerge_orphans={unmerge_orphans}: the fifo goes exactly then"
            );
        }
    }

    #[test]
    fn a_dev_entry_is_always_kept() {
        // Portage keeps `dev` nodes under both FEATURES modes (`---`); a
        // live node would go through the orphan arm under default
        // FEATURES (the F2 source verdict), so a live directory proves
        // the `dev` arm itself keeps in both modes (creating a real node
        // needs privileges).
        for unmerge_orphans in [true, false] {
            let tmp = tempdir();
            let root = tmp.join("root");
            let live = root.join("dev/node");
            std::fs::create_dir_all(&live).unwrap();

            let contents = "dev /dev/node\n";
            remove_contents(RemoveContentsArgs {
                root: &root,
                category: "app-misc",
                others_in_slot: &[],
                replacing: &[],
                pf: "",
                uninstall_ignore: "",
                config_protect: "/etc",
                config_protect_mask: "",
                unmerge_orphans,
                infodirs_inodes: &BTreeSet::new(),
                cfgfiledict: &BTreeMap::new(),
                stale_confmem: &mut Vec::new(),
                preserved_paths: &BTreeSet::new(),
                contents_text: contents,
                listing: &mut ebuild_merge::MergeListing::new(false),
            })
            .expect("remove_contents succeeds");

            assert!(
                live.is_dir(),
                "unmerge_orphans={unmerge_orphans}: a dev entry is always kept"
            );
        }
    }

    #[test]
    fn merged_usr_dir_entries_survive_a_standalone_unmerge() {
        // Portage 3.0.82.2, S0 cell C1a (not from portuale output): on a
        // merged-usr ROOT, `emerge -C` of a package recording `dir /bin`
        // must leave `/bin` and `/lib64` in place. Failing-first from S4:
        // green at every commit.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::create_dir_all(root.join("usr/lib64")).unwrap();
        std::os::unix::fs::symlink("usr/bin", root.join("bin")).unwrap();
        std::os::unix::fs::symlink("usr/lib64", root.join("lib64")).unwrap();
        std::fs::write(root.join("usr/bin/u1"), b"untracked").unwrap();
        std::fs::write(root.join("usr/lib64/u2"), b"untracked").unwrap();
        let live = root.join("bin/t355");
        std::fs::write(&live, b"a").unwrap();
        let mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&live).unwrap()).unwrap();
        let md5 = ebuild_merge::md5_hex(&live).unwrap();

        let vdb = root.join("var/db/pkg/app-misc/t355-c1a-1");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(
            vdb.join("CONTENTS"),
            format!("dir /bin\nobj /bin/t355 {md5} {mtime}\n"),
        )
        .unwrap();
        // A second package records `dir /bin` too, so the R4b owners
        // search keeps the symlink (the C1a shape: Portage 3.0.82.2 keeps
        // `/bin` with `--- !empty`, then the preserved notice).
        let vdb_b = root.join("var/db/pkg/app-misc/t355-c1b-1");
        std::fs::create_dir_all(&vdb_b).unwrap();
        std::fs::write(vdb_b.join("CONTENTS"), "dir /bin\nobj /bin/oth abc 1\n").unwrap();
        std::fs::write(root.join("usr/bin/oth"), b"other package").unwrap();

        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        crate::ebuild_merge::unmerge_one_installed(crate::ebuild_merge::UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-c1a",
            pf: "t355-c1a-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &crate::ebuild_merge::MergeOptions::default(),
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        })
        .expect("unmerge_one_installed succeeds");

        assert!(
            !root.join("bin/t355").exists(),
            "the package's own file is removed"
        );
        assert!(
            root.join("bin/oth").is_file(),
            "the other package's file survives"
        );
        assert!(
            root.join("bin")
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink()),
            "/bin must survive"
        );
        assert!(
            root.join("lib64")
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink()),
            "/lib64 must survive"
        );
        assert!(root.join("usr/bin/u1").is_file());
        assert!(root.join("usr/lib64/u2").is_file());
    }

    #[test]
    fn unmerge_one_installed_honours_a_disabled_orphan_arm() {
        // Portage 3.0.82.2, S0 cell B1b: with `FEATURES=-unmerge-orphans`
        // an md5-only drift and an mtime drift both survive `emerge -C`,
        // while an untouched file still goes.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355")).unwrap();
        let keep = root.join("usr/share/t355/keep");
        let dropmod = root.join("usr/share/t355/dropmod");
        let droptouch = root.join("usr/share/t355/droptouch");
        std::fs::write(&keep, b"keep").unwrap();
        std::fs::write(&dropmod, b"v1").unwrap();
        std::fs::write(&droptouch, b"touch").unwrap();
        let keep_stat = || {
            let mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&keep).unwrap()).unwrap();
            (mtime, ebuild_merge::md5_hex(&keep).unwrap())
        };
        let (keep_mtime, keep_md5) = keep_stat();
        let dropmod_mtime =
            ebuild_merge::mtime_secs(&std::fs::metadata(&dropmod).unwrap()).unwrap();
        let dropmod_md5 = ebuild_merge::md5_hex(&dropmod).unwrap();
        let droptouch_mtime =
            ebuild_merge::mtime_secs(&std::fs::metadata(&droptouch).unwrap()).unwrap();
        let droptouch_md5 = ebuild_merge::md5_hex(&droptouch).unwrap();
        // dropmod: content edited, mtime restored (md5-only drift).
        std::fs::write(&dropmod, b"admin edit").unwrap();
        filetime::set_file_mtime(
            &dropmod,
            filetime::FileTime::from_unix_time(dropmod_mtime, 0),
        )
        .unwrap();
        // droptouch: mtime moved.
        filetime::set_file_mtime(
            &droptouch,
            filetime::FileTime::from_unix_time(droptouch_mtime + 100, 0),
        )
        .unwrap();

        let vdb = root.join("var/db/pkg/app-misc/t355-drop-1");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(
            vdb.join("CONTENTS"),
            format!(
                "dir /usr/share\n\
                 dir /usr/share/t355\n\
                 obj /usr/share/t355/keep {keep_md5} {keep_mtime}\n\
                 obj /usr/share/t355/dropmod {dropmod_md5} {dropmod_mtime}\n\
                 obj /usr/share/t355/droptouch {droptouch_md5} {droptouch_mtime}\n"
            ),
        )
        .unwrap();

        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        let options = crate::ebuild_merge::MergeOptions {
            unmerge_orphans: false,
            ..crate::ebuild_merge::MergeOptions::default()
        };
        crate::ebuild_merge::unmerge_one_installed(crate::ebuild_merge::UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-drop",
            pf: "t355-drop-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        })
        .expect("unmerge_one_installed succeeds");

        assert!(
            !keep.exists(),
            "an untouched file still unmerges without the orphan arm"
        );
        assert!(
            dropmod.is_file(),
            "an md5-only drift survives without the orphan arm"
        );
        assert!(
            droptouch.is_file(),
            "an mtime drift survives without the orphan arm"
        );
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

    #[test]
    fn unmerge_pkgfiles_tolerates_missing_contents_only_as_replacement() {
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/seed")).unwrap();
        std::fs::write(root.join("usr/share/seed/orphan.txt"), b"orphan").unwrap();
        // A resolve-fixture seed: an entry directory with no `CONTENTS`.
        let vdb = root.join("var/db/pkg/dev-libs/seedpkg-1.0");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(vdb.join("SLOT"), "0\n").unwrap();
        let options = UnmergeOptions::default();
        // Replace loop (#348 S6 M1): a replaced instance with no recorded
        // files walks an empty list instead of failing, like Portage's
        // `getcontents()` returning `{}` on `ENOENT`.
        unmerge_pkgfiles(UnmergePkgfilesArgs {
            root: &root,
            category: "dev-libs",
            pn: "seedpkg",
            pf: "seedpkg-1.0",
            also_keep: &[],
            options: &options,
            is_replacement: true,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
        })
        .expect("a replacement unmerge without CONTENTS succeeds");
        assert!(
            root.join("usr/share/seed/orphan.txt").is_file(),
            "with no file list nothing is deleted"
        );
        // Standalone `emerge -C` keeps the "not installed" error.
        let err = unmerge_pkgfiles(UnmergePkgfilesArgs {
            root: &root,
            category: "dev-libs",
            pn: "seedpkg",
            pf: "seedpkg-1.0",
            also_keep: &[],
            options: &options,
            is_replacement: false,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
        })
        .expect_err("without CONTENTS must fail");
        assert!(err.contains("not installed"), "{err}");
    }

    #[test]
    fn shlex_split_follows_shell_quoting() {
        assert_eq!(shlex_split(""), Vec::<String>::new());
        assert_eq!(
            shlex_split("/lib/modules/* /a/b"),
            vec!["/lib/modules/*", "/a/b"]
        );
        assert_eq!(shlex_split("\"/x/*\" '/y/*'"), vec!["/x/*", "/y/*"]);
        assert_eq!(shlex_split("a\"b c\"d"), vec!["ab cd"]);
        assert_eq!(shlex_split("/x/\\ foo"), vec!["/x/ foo"]);
    }

    #[test]
    fn uninstall_ignore_with_packdebug_appends_exactly_under_packdebug() {
        // Portage 3.0.82.2, vartree.py:2926-2931 (S0 cell U2 is source-only:
        // portuale never writes the tarball, so only the fold is pinned).
        assert_eq!(
            uninstall_ignore_with_packdebug("/lib/modules/*", "packdebug"),
            "/lib/modules/* /usr/lib/debug/.tarball/*"
        );
        assert_eq!(
            uninstall_ignore_with_packdebug("/lib/modules/*", "buildpkg"),
            "/lib/modules/*"
        );
        // Re-resolving is idempotent, never stacking the pattern.
        assert_eq!(
            uninstall_ignore_with_packdebug(
                "/lib/modules/* /usr/lib/debug/.tarball/*",
                "packdebug"
            ),
            "/lib/modules/* /usr/lib/debug/.tarball/*"
        );
        // Leaving `packdebug` drops a stale copy again.
        assert_eq!(
            uninstall_ignore_with_packdebug("/lib/modules/* /usr/lib/debug/.tarball/*", "buildpkg"),
            "/lib/modules/*"
        );
    }

    #[test]
    fn wrap_72_matches_portage_textwrap_on_the_preserved_notice() {
        // Portage 3.0.82.2, S0 cell C1a-pg (`textwrap.wrap(msg, 72)`): the
        // trailing preserved-symlink notice wraps to exactly these lines.
        assert_eq!(
            wrap_72(
                "One or more symlinks to directories have been preserved in order to \
                 ensure that files installed via these symlinks remain accessible. \
                 This indicates that the mentioned symlink(s) may be obsolete remnants \
                 of an old install, and it may be appropriate to replace a given symlink \
                 with the directory that it points to."
            ),
            vec![
                "One or more symlinks to directories have been preserved in order to",
                "ensure that files installed via these symlinks remain accessible. This",
                "indicates that the mentioned symlink(s) may be obsolete remnants of an",
                "old install, and it may be appropriate to replace a given symlink with",
                "the directory that it points to.",
            ]
        );
    }

    #[test]
    fn contents_owns_or_under_counts_a_record_under_the_symlink() {
        // Portage's `getcontents` materialises implicit parent dirs, so a
        // file installed through a directory symlink owns the symlink.
        let text = "obj /usr/share/t355/link/b.txt abc 1\nsym /other -> x 1\n";
        assert!(contents_owns_or_under(text, "/usr/share/t355/link"));
        assert!(contents_owns_or_under(text, "/usr/share/t355/link/b.txt"));
        assert!(!contents_owns_or_under(text, "/usr/share/t355/link2"));
        // The prefix form owns every ancestor of a recorded path too
        // (Portage's implicit parent dirs).
        assert!(contents_owns_or_under(text, "/usr/share/t355"));
        assert!(!contents_owns_or_under(text, "/other/sibling"));
    }

    #[test]
    fn remove_contents_skips_uninstall_ignore_paths() {
        // Portage 3.0.82.2, S0 cell U1a (not from portuale output): the
        // literal `UNINSTALL_IGNORE="/usr/share/t355/ign/*"` drops the
        // make.globals default, so `ign/x` is kept (`--- cfgpro`) while
        // `m.ko` goes.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355/ign")).unwrap();
        std::fs::create_dir_all(root.join("lib/modules/t355")).unwrap();
        std::fs::write(root.join("usr/share/t355/ign/x"), b"x").unwrap();
        std::fs::write(root.join("lib/modules/t355/m.ko"), b"mod").unwrap();
        let x_stat = || {
            let live = root.join("usr/share/t355/ign/x");
            (
                ebuild_merge::mtime_secs(&std::fs::metadata(&live).unwrap()).unwrap(),
                ebuild_merge::md5_hex(&live).unwrap(),
            )
        };
        let m_stat = || {
            let live = root.join("lib/modules/t355/m.ko");
            (
                ebuild_merge::mtime_secs(&std::fs::metadata(&live).unwrap()).unwrap(),
                ebuild_merge::md5_hex(&live).unwrap(),
            )
        };
        let (x_mtime, x_md5) = x_stat();
        let (m_mtime, m_md5) = m_stat();
        let contents = format!(
            "obj /usr/share/t355/ign/x {x_md5} {x_mtime}\n\
             obj /lib/modules/t355/m.ko {m_md5} {m_mtime}\n"
        );
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "/usr/share/t355/ign/*",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("usr/share/t355/ign/x").is_file(),
            "an ignored path must survive unmerge"
        );
        assert!(
            !root.join("lib/modules/t355/m.ko").exists(),
            "the dropped default no longer protects m.ko"
        );
    }

    #[test]
    fn remove_contents_never_removes_live_libdir_symlinks() {
        // Portage 3.0.82.2, S0 cell L1 (not from portuale output): live
        // symlinks `/lib` and `/usr/lib` are kept (`--- cfgpro`), exactly
        // these two (plus `/usr/local/lib`); a recorded `/lib64` symlink
        // is not on the list and still goes.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr")).unwrap();
        std::os::unix::fs::symlink("lib64", root.join("lib")).unwrap();
        std::os::unix::fs::symlink("lib64", root.join("usr/lib")).unwrap();
        std::os::unix::fs::symlink("lib64", root.join("lib64")).unwrap();
        let contents = "sym /lib -> lib64 1\nsym /usr/lib -> lib64 1\nsym /lib64 -> lib64 1\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: true,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        for guarded in ["lib", "usr/lib"] {
            assert!(
                root.join(guarded)
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink()),
                "{guarded} must survive"
            );
        }
        assert!(
            root.join("lib64").symlink_metadata().is_err(),
            "a recorded symlink outside the R2 list still goes"
        );
    }

    #[test]
    fn remove_contents_unlinks_a_dir_entry_that_is_a_live_symlink_to_an_empty_dir() {
        // Portage 3.0.82.2, S0 cell D1 (not from portuale output): a `dir`
        // entry that is a live symlink takes the sym branch (`<<< dir`).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("opt/t355/other")).unwrap();
        std::os::unix::fs::symlink("other", root.join("opt/t355/d")).unwrap();
        let contents = "dir /opt/t355/d\n";
        remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            root.join("opt/t355/d").symlink_metadata().is_err(),
            "the symlink a dir entry points at must go (S0 D1)"
        );
        assert!(root.join("opt/t355/other").is_dir());
    }

    #[test]
    fn remove_contents_unlinks_a_symlink_whose_children_are_all_owned() {
        // Portage 3.0.82.2, S0 cell E3 (not from portuale output): every
        // child is an owned regular file, so the link goes with no elog.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355/a-real")).unwrap();
        std::fs::write(root.join("usr/share/t355/a-real/f"), b"f").unwrap();
        std::os::unix::fs::symlink("a-real", root.join("usr/share/t355/z-link")).unwrap();
        let live_f = root.join("usr/share/t355/a-real/f");
        let f_mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&live_f).unwrap()).unwrap();
        let f_md5 = ebuild_merge::md5_hex(&live_f).unwrap();
        let link = root.join("usr/share/t355/z-link");
        let link_mtime =
            ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        let contents = format!(
            "dir /usr/share/t355/a-real\n\
             obj /usr/share/t355/a-real/f {f_md5} {f_mtime}\n\
             sym /usr/share/t355/z-link -> a-real {link_mtime}\n"
        );
        let elog = remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(link.symlink_metadata().is_err());
        assert!(!live_f.exists());
        assert!(
            elog.is_empty(),
            "all-owned links go silently (S0 E3): {elog:?}"
        );
    }

    #[test]
    fn remove_contents_protects_a_symlink_with_an_unowned_child_and_unlinks_it_when_nobody_owns_it()
    {
        // Portage 3.0.82.2, S0 cell E2 (not from portuale output): `--- !empty`
        // plus the search elog, then `<<< sym` when the owners search finds
        // nobody.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355/real")).unwrap();
        std::fs::write(root.join("usr/share/t355/real/u.txt"), b"admin").unwrap();
        std::os::unix::fs::symlink("real", root.join("usr/share/t355/link")).unwrap();
        let link = root.join("usr/share/t355/link");
        let link_mtime =
            ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        let contents =
            format!("dir /usr/share/t355/real\nsym /usr/share/t355/link -> real {link_mtime}\n");
        let elog = remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "t355-ea-1",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(link.symlink_metadata().is_err());
        assert!(root.join("usr/share/t355/real/u.txt").is_file());
        let text = elog.join("\n");
        assert!(text.contains("Directory symlink(s) may need protection:"));
        assert!(text.contains("Searching all installed packages"));
        assert!(text.contains("safe to remove. Removing them now"));
        assert!(!text.contains("One or more symlinks"));
    }

    #[test]
    fn remove_contents_keeps_a_protected_symlink_another_package_owns() {
        // Portage 3.0.82.2, S0 cells C1a (keep half) and E2 (search
        // wording), not from portuale output: E1-as-designed never reaches
        // R4 (the merge backs the symlink aside), so this pins the
        // redesigned probe — a second package recording the same symlink
        // path. The link stays with the trailing preserved notice and no
        // "safe to remove".
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/t355/real")).unwrap();
        std::fs::write(root.join("usr/share/t355/real/u.txt"), b"admin").unwrap();
        std::os::unix::fs::symlink("real", root.join("usr/share/t355/link")).unwrap();
        let link = root.join("usr/share/t355/link");
        let link_mtime =
            ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        let contents =
            format!("dir /usr/share/t355/real\nsym /usr/share/t355/link -> real {link_mtime}\n");
        let other_vdb = root.join("var/db/pkg/app-misc/t355-eb2-1");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(other_vdb.join("CONTENTS"), &contents).unwrap();
        let elog = remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "t355-ea-1",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(
            link.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink()),
            "another owner keeps the link"
        );
        let text = elog.join("\n");
        assert!(text.contains("Directory symlink(s) may need protection:"));
        assert!(!text.contains("safe to remove"));
        assert!(text.contains("One or more symlinks to directories have been preserved"));
    }

    #[test]
    fn remove_contents_skips_the_owners_search_when_a_same_slot_instance_owns_the_link() {
        // The R4b early return, in the bug-#326685 shape: another same-slot
        // instance records the path as a literal `dir`, so the symlink is
        // orphaned into `protected_symlinks` before the `is_owned` skip;
        // R4b then skips the search elog, while the trailing preserved
        // notice still fires.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("keep/target")).unwrap();
        std::fs::write(root.join("keep/target/other.txt"), b"other").unwrap();
        std::os::unix::fs::symlink(root.join("keep/target"), root.join("keep/link")).unwrap();
        let link = root.join("keep/link");
        let link_mtime =
            ebuild_merge::mtime_secs(&std::fs::symlink_metadata(&link).unwrap()).unwrap();
        let other_vdb = root.join("var/db/pkg/dev-libs/orphanpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(other_vdb.join("CONTENTS"), "dir /keep/link\n").unwrap();
        let contents = format!("sym /keep/link -> whatever {link_mtime}\n");
        let elog = remove_contents(RemoveContentsArgs {
            root: &root,
            category: "dev-libs",
            pf: "seedpkg-1.0",
            others_in_slot: &["orphanpkg-2.0".to_string()],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(link.symlink_metadata().is_ok());
        let text = elog.join("\n");
        assert!(
            !text.contains("may need protection"),
            "the early return skips the search: {text:?}"
        );
        assert!(text.contains("One or more symlinks to directories have been preserved"));
    }

    #[test]
    fn contents_isowner_matches_a_child_through_a_live_symlink_by_inode() {
        // Portage's narrow `_match_contents` form: the child is addressed
        // through the live symlink, but the rebuilt path under the
        // same-inode recorded parent is recorded too.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("t/a-real")).unwrap();
        std::os::unix::fs::symlink("a-real", root.join("t/z-link")).unwrap();
        let text = "dir /t/a-real\nobj /t/a-real/f abc 1\nsym /t/z-link -> a-real 1\n";
        let inodes = recorded_parent_inodes(&root, text);
        assert!(contents_isowner(&root, text, "/t/a-real/f", &inodes));
        assert!(contents_isowner(&root, text, "/t/z-link/f", &inodes));
        assert!(!contents_isowner(&root, text, "/t/z-link/u.txt", &inodes));
        assert!(!contents_isowner(&root, text, "/t/a-real/u.txt", &inodes));
    }

    #[test]
    fn merged_usr_bin_is_removed_when_no_other_package_records_it() {
        // Portage 3.0.82.2, S0 cell C1b (not from portuale output): with no
        // other recorder Portage removes `/bin` (`<<< sym` after the search
        // elog). The S4 C1 test covers the keep half; this covers R4b's
        // remove half at merged-usr scale.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::os::unix::fs::symlink("usr/bin", root.join("bin")).unwrap();
        std::fs::write(root.join("usr/bin/u1"), b"untracked").unwrap();
        let live = root.join("bin/t355");
        std::fs::write(&live, b"a").unwrap();
        let mtime = ebuild_merge::mtime_secs(&std::fs::metadata(&live).unwrap()).unwrap();
        let md5 = ebuild_merge::md5_hex(&live).unwrap();
        let contents = format!("dir /bin\nobj /bin/t355 {md5} {mtime}\n");
        let elog = remove_contents(RemoveContentsArgs {
            root: &root,
            category: "app-misc",
            pf: "t355-c1a-1",
            others_in_slot: &[],
            replacing: &[],
            uninstall_ignore: "",
            config_protect: "/etc",
            config_protect_mask: "",
            unmerge_orphans: false,
            infodirs_inodes: &BTreeSet::new(),
            cfgfiledict: &BTreeMap::new(),
            stale_confmem: &mut Vec::new(),
            preserved_paths: &BTreeSet::new(),
            contents_text: &contents,
            listing: &mut ebuild_merge::MergeListing::new(false),
        })
        .expect("remove_contents succeeds");

        assert!(!root.join("bin").symlink_metadata().is_ok());
        assert!(root.join("usr/bin/u1").is_file());
        let text = elog.join("\n");
        assert!(text.contains("safe to remove. Removing them now"));
    }

    /// Hand-built merged-usr pair: A owns `sym link`, B records the same
    /// symlink path; an untracked file makes R4 protect it and R4b keep it.
    fn seed_dir_symlink_pair(root: &Path, with_owner_b: bool) {
        std::fs::create_dir_all(root.join("usr/share/t355/real")).unwrap();
        std::fs::write(root.join("usr/share/t355/real/u.txt"), b"admin").unwrap();
        std::os::unix::fs::symlink("real", root.join("usr/share/t355/link")).unwrap();
        let link_mtime = ebuild_merge::mtime_secs(
            &std::fs::symlink_metadata(root.join("usr/share/t355/link")).unwrap(),
        )
        .unwrap();
        let contents = format!(
            "dir /usr/share\n\
             dir /usr/share/t355\n\
             dir /usr/share/t355/real\n\
             sym /usr/share/t355/link -> real {link_mtime}\n"
        );
        let vdb_a = root.join("var/db/pkg/app-misc/t355-ea-1");
        std::fs::create_dir_all(&vdb_a).unwrap();
        std::fs::write(vdb_a.join("CONTENTS"), &contents).unwrap();
        if with_owner_b {
            let vdb_b = root.join("var/db/pkg/app-misc/t355-eb2-1");
            std::fs::create_dir_all(&vdb_b).unwrap();
            std::fs::write(vdb_b.join("CONTENTS"), &contents).unwrap();
        }
    }

    #[test]
    fn unmerge_one_installed_keeps_a_dir_symlink_another_package_records() {
        // Portage 3.0.82.2, S0 cells C1a (keep half) and E2 (search
        // wording), not from portuale output: the E1 e2e through
        // `unmerge_one_installed`, with two installed packages.
        let tmp = tempdir();
        let root = tmp.join("root");
        seed_dir_symlink_pair(&root, true);
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        crate::ebuild_merge::unmerge_one_installed(crate::ebuild_merge::UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-ea",
            pf: "t355-ea-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &crate::ebuild_merge::MergeOptions::default(),
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        })
        .expect("unmerge_one_installed succeeds");

        assert!(
            root.join("usr/share/t355/link")
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink()),
            "another owner keeps the link (S0 C1a shape)"
        );
        assert!(root.join("usr/share/t355/real/u.txt").is_file());
        assert!(!root.join("var/db/pkg/app-misc/t355-ea-1").exists());
        assert!(root.join("var/db/pkg/app-misc/t355-eb2-1").is_dir());
    }

    #[test]
    fn unmerge_one_installed_removes_a_dir_symlink_nobody_else_records() {
        // Portage 3.0.82.2, S0 cell E2 (not from portuale output): the E2
        // e2e through `unmerge_one_installed` — one package, untracked
        // child, nobody owns the link, so it goes.
        let tmp = tempdir();
        let root = tmp.join("root");
        seed_dir_symlink_pair(&root, false);
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        crate::ebuild_merge::unmerge_one_installed(crate::ebuild_merge::UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-ea",
            pf: "t355-ea-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &crate::ebuild_merge::MergeOptions::default(),
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_needed: &[],
            replacement_preserved: &BTreeMap::new(),
            deferred_delete: None,
        })
        .expect("unmerge_one_installed succeeds");

        assert!(root.join("usr/share/t355/link").symlink_metadata().is_err());
        assert!(root.join("usr/share/t355/real/u.txt").is_file());
    }

    #[test]
    fn unmerge_one_installed_unlinks_a_dir_entry_replaced_by_a_symlink() {
        // Portage 3.0.82.2, S0 cell D1 (not from portuale output): the D1
        // e2e through `unmerge_one_installed` — the recorded dir is live a
        // symlink to an empty dir, so R3 unlinks it.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("opt/t355/other")).unwrap();
        std::os::unix::fs::symlink("other", root.join("opt/t355/d")).unwrap();
        let vdb = root.join("var/db/pkg/app-misc/t355-d1-1");
        std::fs::create_dir_all(&vdb).unwrap();
        std::fs::write(
            vdb.join("CONTENTS"),
            "dir /opt\ndir /opt/t355\ndir /opt/t355/d\n",
        )
        .unwrap();
        let portage_tmpdir = tmp.join("tmp");
        std::fs::create_dir_all(&portage_tmpdir).unwrap();
        crate::ebuild_merge::unmerge_one_installed(crate::ebuild_merge::UnmergeOneInstalledArgs {
            root: &root,
            category: "app-misc",
            package: "t355-d1",
            pf: "t355-d1-1",
            also_keep: &[],
            scratch_dir: &tmp.join("scratch"),
            portage_tmpdir: &portage_tmpdir,
            options: &crate::ebuild_merge::MergeOptions::default(),
            backup: None,
            is_replacement: false,
            replaced_by_version: None,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        })
        .expect("unmerge_one_installed succeeds");

        assert!(root.join("opt/t355/d").symlink_metadata().is_err());
        assert!(root.join("opt/t355/other").is_dir());
    }
}
