// Real package removal (task #55's own natural complement): `ebuild
// <file> unmerge` mirrors real `dblink.unmerge()` plus the top-level
// `unmerge()` function's own success-gated `dblink.delete()` call
// (`lib/portage/dbapi/vartree.py`): runs `pkg_prerm`, deletes every
// file/dir/symlink the vdb entry's own `CONTENTS` lists from `${ROOT}`
// (in real `_unmerge_pkgfiles()`'s own reverse-sorted order -- deepest
// paths first, so a directory always empties out before its own removal
// is attempted -- `mykeys.sort(); mykeys.reverse()`), runs `pkg_postrm`,
// and -- only if every step above succeeded -- removes the vdb entry
// itself (real `dblink.delete()`: `shutil.rmtree(self.dbdir)` plus an
// `rmdir` of the parent `<category>` directory if it's now empty).
// `pkg_prerm`/`pkg_postrm` run via `ebuild_phases::run_single_phase`, the
// same way `ebuild_merge::run_merge`'s own `pkg_preinst`/`pkg_postinst`
// do -- real `unmerge()` invokes them directly
// (`EbuildPhase(phase="prerm"/"postrm")`), not through `doebuild()`'s own
// `actionmap_deps` chain.
//
// Without this, `merge` alone can never be exercised through a real
// install/reinstall/removal cycle -- every merge would just accumulate
// vdb entries and files forever.
//
// Locally-modified files are protected on removal too, real `_unmerge_
// pkgfiles()`'s own actual mechanism for it: an `obj`/`sym` entry whose
// live, on-disk mtime no longer matches what `CONTENTS` recorded at
// merge time is left in place instead of deleted (real `!mtime` skip --
// broader than `CONFIG_PROTECT` alone, since it applies to *every*
// unmerge regardless of path, and it's what actually protects a
// CONFIG_PROTECT'd file on removal too: real `dblink._protect()`
// diverts a changed write to a `._cfgNNNN_` sibling while `CONTENTS`
// still records the *original* file's own now-stale mtime, so the real
// `/etc/foo.conf` a user edited never matches and is never touched).
//
// `others_in_slot` reverse-dependency checking is real too: real
// `_unmerge_pkgfiles()`'s own `is_owned` check (`vartree.py:2893-2916`,
// via `dblink.isowner()` == `bool(self._match_contents(filename))`) --
// before a `CONTENTS` entry is even considered for the mtime check,
// every *other* installed version of the same `category/PN` in the same
// `SLOT` (excluding self) is asked whether its own `CONTENTS` also
// claims that exact path; if so, the entry is left alone entirely
// (real `"replaced"` skip) regardless of node type. This is what makes
// an in-place upgrade not delete files the new version also owns --
// without it, `merge`-then-`merge` (a reinstall/upgrade) followed by
// `unmerge`ing the *old* vdb entry would have deleted files the new
// installation still depends on. Reuses `ebuild_merge::owns_path_pf`
// (already built for real blocker exclusion's own `CONTENTS`-ownership
// check) and `ebuild_merge::read_installed_slot`, both promoted to
// `pub(crate)` for this. Real `isowner`'s own path-ambiguity-via-
// symlinked-directories inode-cache mechanism isn't reproduced --
// this pilot's own `owns_path_pf` is a plain string scan, matching the
// same simplification `find_owners`/blocker exclusion already made.
//
// The real "symlink orphan" refinement (bug #326685) is real too: when
// a live symlink-to-directory this package's own CONTENTS recorded as
// `sym` or `dir` is `is_owned` by another same-slot instance that
// itself now records that exact path as a literal `dir` entry (the
// directory the symlink pointed to got "promoted" to a real directory
// across the upgrade), the symlink is genuinely orphaned: neither this
// package's own removal pass nor the survivor's own files actually
// still need it as a symlink. Real `_unmerge_pkgfiles()`
// (`vartree.py:2895-2926`) detects this and defers the decision --
// `protected_symlinks`, keyed by the symlink's own *target* directory's
// `(dev, ino)` -- to a second pass over this package's own literal
// `dir` entries (real `_unmerge_dirs()`, `vartree.py:3209-3332`,
// `remove_dirs` below): when that target directory is later actually
// removed (because nothing else needs it as a real directory either),
// the now-truly-orphaned symlink is deleted too, and its own
// newly-emptied parent directories are recursively revisited (real bug
// #640058) in case removing the symlink itself finally empties them.
// Real `_unmerge_protected_symlinks()` (`vartree.py:3114-3207`, called
// on whatever `protected_symlinks` entries *don't* get resolved by
// `_unmerge_dirs()`) is deliberately NOT ported: its own first loop
// re-checks the exact same `others_in_slot`/`isowner` condition already
// required to populate `protected_symlinks` in the first place -- since
// that fact cannot change between the two passes within one real
// `unmerge()` call, its own `return` fires unconditionally, making the
// real system-wide `get_owners()`-gated delete-or-elog-warn logic after
// it genuinely unreachable dead code in current portage (confirmed by
// tracing the exact call graph, not a simplification -- there is no
// real behavior there to be unfaithful to). The real elog warning text
// for symlinks that *do* survive (`vartree.py:3085-3103`) is also not
// reproduced: this module has no message-printing output anywhere else
// either, only the behavioral effect (leave the symlink in place) --
// see the "Failure handling is coarser" gap below for the same
// no-message-output pattern elsewhere in this module.
//
// `FEATURES=unmerge-orphans` is real too (`vartree.py:2934-2950`):
// despite the name, not untracked-orphan scanning -- for a
// non-`CONFIG_PROTECT`'d `obj`/`sym` entry (excluding a symlink whose
// live target itself resolves to a directory, real comment: "Don't
// unlink symlinks to directories here since that can remove /lib and
// /usr/lib symlinks"), it bypasses the ordinary `!mtime` staleness
// check entirely and deletes the entry unconditionally, even if
// locally modified. Reuses `ebuild_merge::is_protected` (promoted to
// `pub(crate)` for this), the exact same real `ConfigProtect.
// isprotected()` check `ebuild_merge`'s own `CONFIG_PROTECT` handling
// already established -- `UnmergeOptions`'s own `config_protect`/
// `config_protect_mask` fields mirror `MergeOptions`'s exactly, same
// env-var-sourced defaults.
//
// Real `INFOPATH` cleanup is real too (`vartree.py:3226-3251`, inside
// `_unmerge_dirs()`/`remove_dirs` below): a directory literally named
// `"info"` (real comment: "since it might have been in INFOPATH
// previously even though it may not be there now") whose only
// remaining content is a subset of `{"dir", "dir.old"}` (real
// `_infodir_cleanup`, GNU `install-info`'s own auto-generated index
// files) has those removed first, before the ordinary `rmdir` attempt
// -- without this, a stray leftover index file would keep such a
// directory from ever emptying out and being removed at all. The other
// real trigger for this same cleanup, `inode_key in infodirs_inodes`
// (a real, `INFOPATH`/`INFODIR` env-var-driven inode set, covering an
// info directory that *isn't* literally named `"info"`) is threaded
// through too now: `env_update::info_dirs_inodes` collates real
// `INFOPATH`/`INFODIR` values from `/etc/env.d/*` the same way
// `env_update::run_env_update` collates every other real
// `COLON_SEPARATED` key, and `run_unmerge` below computes that set once
// and threads it down through `remove_contents`/`remove_dirs` into
// `cleanup_info_dir`.
//
// Real `stale_confmem` cleanup is real too (`vartree.py:2747`/
// `2931-2932`/`3106-3109`): `cfgfiledict`, the real `_conf_mem_file`
// "already offered this MD5" memory `ebuild_merge` writes on merge
// (`ebuild_merge::read_cfgfiledict`/`write_cfgfiledict`, promoted
// `pub(crate)` for this), is read once up front by `run_unmerge` below;
// any path `remove_contents` actually removes -- not `is_owned` by
// another same-slot instance -- that `cfgfiledict` still remembers is
// collected into `stale_confmem` and dropped from the persisted memory
// afterward, matching real ordering exactly (the elif is off the same
// `is_owned` check as the "replaced" skip).
//
// KNOWN, DOCUMENTED GAPS (v1 scope, matching `ebuild_merge`'s own
// "narrow v1, document the cut" pattern):
//   - No `bsd_chflags` handling -- confirmed dead code on Linux, not a
//     real gap: `lib/portage/__init__.py:311` sets `bsd_chflags = None`
//     unconditionally on non-BSD, and this pilot's own hard goal is
//     Linux-only/musl-static.
//   - Failure handling is coarser: real `_unmerge_pkgfiles()` counts
//     per-file failures and keeps going regardless (overall success is
//     governed by the `prerm`/`postrm` phase exit codes, not by
//     individual file-removal failures); this pilot's own removal loop
//     tolerates "already gone" (`NotFound`, matching real
//     `_ignored_unlink_errnos`/`_ignored_rmdir_errnos`) and "directory
//     not empty" (matching real `!empty` tolerance) but treats any
//     other I/O error as a hard failure.
//   - `${T}`/`${D}`/etc. are recomputed fresh via `ebuild_phases::
//     compute_environment` for every `unmerge` call, the same as
//     `merge` -- real `unmerge()` reuses whatever `PORTAGE_BUILDDIR` a
//     prior merge left behind when present; this pilot doesn't attempt
//     that distinction.

use crate::ebuild_merge;
use crate::ebuild_phases;
use crate::env_update;
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Whether `command` is the one real unmerge command this module
/// implements -- `ebuild.rs` checks this alongside `ebuild_phases::
/// is_real_phase_command`/`ebuild_merge::is_real_merge_command` before
/// routing to real execution.
pub fn is_real_unmerge_command(command: &str) -> bool {
    command == "unmerge"
}

struct ContentsEntry {
    node_type: String,
    abs_path: String,
    /// Present for `obj`/`sym` lines (real `ebuild_merge::
    /// format_contents_line`'s own trailing field) -- `None` for `dir`
    /// (which has no trailing field at all: its own last
    /// whitespace-separated token is `abs_path` itself, non-numeric, so
    /// parsing it as an mtime naturally fails into `None`).
    mtime: Option<i64>,
}

/// Parses a real `CONTENTS` file's own lines (see `ebuild_merge::
/// format_contents_line` for the exact format written).
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

/// Real `protected_symlinks`: the live target directory's own `(dev,
/// ino)` a bug-#326685 orphaned symlink points to -> every such
/// symlink's own `abs_path`. Grouped by target inode (not by symlink
/// path) because that's how `remove_dirs` below looks entries up --
/// real `_unmerge_dirs()`'s own `protected_symlinks.pop(inode_key)`.
type ProtectedSymlinks = BTreeMap<(u64, u64), Vec<String>>;

/// Deletes every `CONTENTS`-listed entry from `root`, deepest paths
/// first (see this module's own doc comment for why, and for the v1
/// failure-tolerance simplification) -- except: an entry another
/// same-`category/PN`-and-`SLOT` installed package (`others_in_slot`,
/// bare `PF` strings) also owns, real `is_owned`/`"replaced"` skip (see
/// this module's own doc comment); or an `obj`/`sym` entry whose live
/// mtime no longer matches what `CONTENTS` recorded, left in place
/// instead (real `!mtime` skip -- see this module's own doc comment for
/// why this is also what protects a CONFIG_PROTECT'd file on removal) --
/// unless `unmerge_orphans` is set, real `FEATURES=unmerge-orphans`
/// (see this module's own doc comment), which bypasses that `!mtime`
/// check entirely for a non-`CONFIG_PROTECT`'d `obj`/`sym` entry (a
/// symlink whose live target is itself a directory excluded, real
/// comment: "Don't unlink symlinks to directories here since that can
/// remove /lib and /usr/lib symlinks"). This package's own literal
/// `dir` entries are deferred to a second pass (`remove_dirs`, real
/// `_unmerge_dirs()`) instead of removed inline here (real `mydirs`) --
/// see this module's own doc comment on bug #326685 for why the
/// deferral matters. `cfgfiledict`/`stale_confmem`: real `_unmerge_
/// pkgfiles()`'s own `stale_confmem` cleanup (`vartree.py:2931-2932`) --
/// a removed, not-`is_owned` entry that `cfgfiledict` (the real
/// `_conf_mem_file` "already offered this MD5" memory `ebuild_merge`
/// writes on merge) still remembers has its own path collected into
/// `stale_confmem`, so `run_unmerge` can drop it from the persisted
/// memory afterward -- otherwise a future merge could wrongly treat a
/// long-gone package's own previously-offered update as "already
/// offered" for a path nothing installs anymore. `preserved_paths`: real
/// "remove the preserved files from our contents so that they won't be
/// unmerged" (`vartree.py:2293-2294`, see `ebuild_merge::preserve_libs_
/// on_unmerge`'s own doc comment for the real preserve-libs registration
/// this mirrors) -- filtered out of `entries` immediately, the same
/// point real portage itself strips them from the CONTENTS dict, before
/// the per-file removal loop below ever sees them at all.
#[allow(clippy::too_many_arguments)]
fn remove_contents(
    root: &Path,
    category: &str,
    others_in_slot: &[String],
    config_protect: &str,
    config_protect_mask: &str,
    unmerge_orphans: bool,
    infodirs_inodes: &BTreeSet<(u64, u64)>,
    cfgfiledict: &BTreeMap<String, String>,
    stale_confmem: &mut Vec<String>,
    preserved_paths: &BTreeSet<String>,
    contents_text: &str,
) -> Result<(), String> {
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

        // Real bug #326685 detection (`vartree.py:2898-2926`): a live
        // symlink-to-directory this package's own CONTENTS recorded as
        // `sym` or `dir`, whose exact path another same-slot instance
        // now claims *as a literal `dir` entry* (not `sym`) -- see this
        // module's own doc comment for the full real grounding.
        if is_owned && (entry.node_type == "sym" || entry.node_type == "dir") {
            if let Ok(link_meta) = std::fs::symlink_metadata(&dest) {
                if link_meta.file_type().is_symlink() {
                    if let Ok(target_meta) = std::fs::metadata(&dest) {
                        if target_meta.is_dir() {
                            let symlink_orphan = others_in_slot.iter().any(|other_pf| {
                                ebuild_merge::owned_node_type_pf(
                                    root,
                                    category,
                                    other_pf,
                                    &entry.abs_path,
                                )
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
                    }
                }
            }
        }

        if is_owned {
            // Real "replaced" skip: another still-installed version of
            // this same cp:slot also claims this path -- most commonly
            // an in-place upgrade sharing files with the version being
            // unmerged. Checked before the mtime check, matching real
            // `_unmerge_pkgfiles()`'s own ordering.
            continue;
        } else if cfgfiledict.contains_key(&entry.abs_path) {
            // Real `stale_confmem` (`vartree.py:2931-2932`): this path
            // is actually going away and nothing else owns it, so its
            // `_conf_mem_file` memory entry is now stale too.
            stale_confmem.push(entry.abs_path.clone());
        }

        // Real `FEATURES=unmerge-orphans` (`vartree.py:2934-2950`):
        // deletes a non-`CONFIG_PROTECT`'d `obj`/`sym` entry
        // unconditionally, bypassing the `!mtime` staleness check below
        // entirely -- even a locally-modified file is deleted. Excludes
        // a symlink whose live target itself resolves to a directory
        // (real comment: "Don't unlink symlinks to directories here
        // since that can remove /lib and /usr/lib symlinks"), which
        // falls through to the ordinary mtime-checked removal below
        // instead. Checked before the mtime check, matching real
        // ordering exactly.
        if unmerge_orphans
            && matches!(entry.node_type.as_str(), "obj" | "sym")
            && !ebuild_merge::is_protected(root, config_protect, config_protect_mask, &dest)
        {
            let symlink_to_dir = entry.node_type == "sym"
                && std::fs::metadata(&dest)
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
            if !symlink_to_dir {
                if let Err(e) = std::fs::remove_file(&dest) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(format!("{}: {e}", dest.display()));
                    }
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
                if let Err(e) = std::fs::remove_file(&dest) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(format!("{}: {e}", dest.display()));
                    }
                }
            }
            "dir" => {
                if let Ok(meta) = std::fs::symlink_metadata(&dest) {
                    dirs.push((dest, (meta.dev(), meta.ino())));
                }
            }
            _ => {
                // Real `_unmerge_pkgfiles()`'s own `"fif"`/`"dev"` branches
                // (`vartree.py:3062-3068`) never call `unlink()` at all --
                // both just report `show_unmerge("---", "", ...)` (real
                // portage's own conservative "leave a device/fifo node in
                // place" behavior, unlike `obj`/`sym`/`dir` above). This
                // matches that exactly: a real no-op, not merely "nothing
                // to remove yet" (see `ebuild_merge::merge_tree`'s own
                // `fif`/`dev` branch for the merge-time counterpart that
                // creates these nodes in the first place).
            }
        }
    }
    remove_dirs(root, dirs, &mut protected_symlinks, infodirs_inodes);
    // Real trailing `if protected_symlinks:` elog warning
    // (`vartree.py:3085-3103`) for whatever entries `remove_dirs`
    // didn't resolve is deliberately not reproduced -- see this
    // module's own doc comment (no message-printing output anywhere
    // else in this module either). The behavioral effect -- those
    // symlinks are left in place -- already holds: `remove_dirs` only
    // ever deletes a `protected_symlinks` entry's own symlinks when it
    // actually removes their target directory.
    Ok(())
}

/// Real `_infodir_cleanup` (`vartree.py:1794`): the only filenames GNU
/// `install-info`'s own auto-generated index is ever known to leave
/// behind.
const INFODIR_CLEANUP: [&str; 2] = ["dir", "dir.old"];

/// Real `_unmerge_dirs()`'s own INFOPATH cleanup (`vartree.py:3226-
/// 3251`): real `inode_key in infodirs_inodes or os.path.basename(obj)
/// == "info"` -- a directory whose own `(dev, ino)` matches a real,
/// live `INFOPATH`/`INFODIR` entry (`env_update::info_dirs_inodes`,
/// covering an info directory reached via a different path than the
/// literal `"info"` name, e.g. a symlink or a non-standard install
/// location) *or* is literally named `"info"` (the more common case,
/// shipped separately first). If its only remaining content is a
/// non-empty subset of `INFODIR_CLEANUP` (real `remaining and
/// len(remaining) <= len(infodir_cleanup) and not
/// set(remaining).difference(infodir_cleanup)`), those files are
/// deleted -- real regular-file-only (`stat.S_ISREG`), so a same-named
/// symlink or subdirectory is left alone -- clearing the way for the
/// caller's own subsequent `rmdir` attempt to actually succeed.
fn cleanup_info_dir(dest: &Path, inode_key: (u64, u64), infodirs_inodes: &BTreeSet<(u64, u64)>) {
    let is_info_dir = infodirs_inodes.contains(&inode_key)
        || dest.file_name().and_then(|n| n.to_str()) == Some("info");
    if !is_info_dir {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dest) else {
        return;
    };
    let names: Vec<String> = entries
        .filter_map(|e| e.ok())
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

/// Real `_unmerge_dirs()` (`vartree.py:3209-3332`): removes this
/// package's own literal `dir` entries (`dirs`, real `mydirs`),
/// tolerating "already gone" and "not empty" the same way the main
/// per-entry loop above does for everything else. The one added
/// wrinkle over a plain removal loop: when removing one of these
/// directories actually succeeds, and its own `(dev, ino)` matches a
/// `protected_symlinks` entry (a bug-#326685 orphaned symlink pointing
/// at exactly this directory -- see `remove_contents`'s own doc
/// comment), that symlink is now genuinely safe to delete too (nothing
/// needs it as a real directory after all) -- deleted here, with its
/// own newly-emptied parent directories recursively re-queued for
/// another removal attempt (real bug #640058), since a directory that
/// failed to rmdir earlier only because this exact symlink still
/// occupied it deserves a second chance. Uses `dirs` as a LIFO stack,
/// mirroring real `_unmerge_dirs()`'s own `dirs.pop()`/`dirs.append()`
/// usage of a plain Python list -- removal order itself has no real
/// semantic meaning (this module's own doc comment), but the stack
/// shape (a directory can be pushed back after being popped) is
/// load-bearing for the bug-#640058 revisit to work at all.
fn remove_dirs(
    root: &Path,
    dirs: Vec<(PathBuf, (u64, u64))>,
    protected_symlinks: &mut ProtectedSymlinks,
    infodirs_inodes: &BTreeSet<(u64, u64)>,
) {
    // Real `_unmerge_dirs()`'s own `dirs = sorted(dirs)`: `mydirs` is
    // built as a Python *set* during the caller's own traversal (order
    // not guaranteed), so real code always re-sorts ascending before
    // relying on `pop()` (removes-from-the-end) to visit deepest paths
    // first -- sorted ascending, a path is always lexicographically
    // greater than its own ancestor (`/usr` < `/usr/share`), so the
    // last element is always the deepest.
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
                    if std::fs::remove_file(&sym_dest).is_ok() {
                        if let Some(parent) = sym_dest.parent() {
                            parents.insert(parent.to_path_buf());
                        }
                    }
                }
                // Real bug #640058: walk each newly-emptied symlink's
                // own ancestor chain while each successive ancestor is
                // itself a directory that previously failed to rmdir
                // (`revisit`), re-queuing all of them for another
                // removal attempt now that this symlink is gone.
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
                // Already gone -- tolerate, no revisit needed (real:
                // `errno == ENOENT` is excluded from revisit tracking).
            }
            Err(_) => {
                // Real: any other tolerated rmdir failure (chiefly
                // "not empty") is tracked for a possible later revisit.
                revisit.insert(dest, inode_key);
            }
        }
    }
}

/// Options for `run_unmerge`, bundled into a struct rather than more
/// positional parameters -- this pilot already relearned the
/// "positional-parameter pain" lesson once, in `--newrepo`'s own
/// bulk-fix saga (see `ebuild_merge::MergeOptions`'s own doc comment,
/// the precedent this mirrors). `config_protect`/`config_protect_mask`
/// are only consulted by `FEATURES=unmerge-orphans`
/// (`unmerge_orphans`) -- `Default` matches real `make.globals`'s own
/// values exactly, the same defaults `MergeOptions::default()` uses.
pub struct UnmergeOptions {
    pub debug: bool,
    pub shell: ebuild_phases::ShellBackend,
    pub config_protect: String,
    pub config_protect_mask: String,
    /// Real `"unmerge-orphans" in self.settings.features`. Real
    /// `unmerge-orphans` *is* one of real `make.globals`'s own default
    /// `FEATURES` tokens (`cnf/make.globals:77-84`) -- confirmed by
    /// reading it directly (a real, previously-undiscovered mismatch:
    /// this field's own `Default` used to be `false` with a doc comment
    /// incorrectly claiming "not in FEATURES by default", the same
    /// mistake `ebuild_merge::MergeOptions::protect_owned` had). `Default`
    /// is now `true`, matching real portage's own actual out-of-the-box
    /// behavior. This pilot's own env-var read still only checks
    /// whether the literal `FEATURES` value (when set at all) contains
    /// the `"unmerge-orphans"` token -- it doesn't *accumulate* onto the
    /// real default set the way real portage's own `+`/`-`-prefixed
    /// `make.conf` `FEATURES` merging does, so setting `FEATURES` to any
    /// *other* token still reads as `unmerge_orphans: false` here, unlike
    /// real portage -- a pre-existing simplification this fix doesn't
    /// attempt to also resolve.
    pub unmerge_orphans: bool,
    /// Real `PORTAGE_CONFIGROOT` -- see `ebuild_merge::MergeOptions::
    /// config_root`'s own doc comment for the exact real default/`Default`
    /// split this mirrors (only consulted by `ebuild_phases::
    /// eclass_locations_value`'s own masters-chain resolution, reached
    /// via `prerm`/`postrm`'s own `run_single_phase` call below).
    pub config_root: PathBuf,
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
        }
    }
}

/// Real top-level `unmerge()`: `dblink.unmerge()` (`prerm` -> delete
/// files -> `postrm`), then -- only on success -- `dblink.delete()`
/// (remove the vdb entry itself).
pub fn run_unmerge(
    ebuild_path: &Path,
    root: &Path,
    portage_tmpdir: &Path,
    options: &UnmergeOptions,
) -> Result<i32, String> {
    let env = ebuild_phases::compute_environment(ebuild_path, portage_tmpdir)?;
    let vdb_dir = root
        .join("var/db/pkg")
        .join(&env.category)
        .join(&env.split.pf);
    let contents_path = vdb_dir.join("CONTENTS");
    let contents_text = std::fs::read_to_string(&contents_path)
        .map_err(|e| format!("{}: not installed ({e})", vdb_dir.display()))?;

    // Real `others_in_slot`: every other installed version of this same
    // category/PN in the same SLOT, excluding self -- see this module's
    // own doc comment.
    let own_slot = ebuild_merge::read_installed_slot(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf[env.split.pn.len() + 1..],
    );
    let others_in_slot: Vec<String> = match &own_slot {
        Some(slot) => portage_repo::installed_versions(root, &env.category, &env.split.pn)
            .into_iter()
            .map(|version| format!("{}-{version}", env.split.pn))
            .filter(|pf| pf != &env.split.pf)
            .filter(|pf| {
                let version = &pf[env.split.pn.len() + 1..];
                ebuild_merge::read_installed_slot(root, &env.category, &env.split.pn, version)
                    .as_deref()
                    == Some(slot.as_str())
            })
            .collect(),
        None => Vec::new(),
    };

    let prerm_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "prerm",
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
    )?;
    if prerm_status != 0 {
        return Ok(prerm_status);
    }

    // Real `infodirs`/`infodirs_inodes` (`vartree.py:2830-2846`): real
    // `INFOPATH`/`INFODIR`, collated from `/etc/env.d/*` the same way
    // `env_update::run_env_update` collates every other real
    // `COLON_SEPARATED` key -- see `env_update::info_dirs_inodes`'s own
    // doc comment for why this is a separate, narrower computation
    // rather than a refactor of that already-tested function.
    let infodirs_inodes = env_update::info_dirs_inodes(root);

    // Real `cfgfiledict`/`stale_confmem` (`vartree.py:2747`/`2931-2932`/
    // `3106-3109`): the real `_conf_mem_file` memory `ebuild_merge`
    // writes on merge, read once up front the same way real
    // `_unmerge_pkgfiles()` does, then pruned of every path this
    // unmerge actually removes and rewritten -- see `remove_contents`'s
    // own doc comment.
    let cfgfiledict = ebuild_merge::read_cfgfiledict(root);
    let mut stale_confmem: Vec<String> = Vec::new();

    // Real `_prune_plib_registry(unmerge=True, ...)` (`vartree.py:2493`),
    // called right before real `_unmerge_pkgfiles()` runs -- see
    // `ebuild_merge::preserve_libs_on_unmerge`'s own doc comment for the
    // full real grounding. `own_slot` defaults to `"0"` the same way
    // `parse_slot`'s own real fallback already does, matching real
    // `_pkg_str`'s own `settings["SLOT"]` fallback for a package with no
    // recorded slot at all.
    let preserved_paths = ebuild_merge::preserve_libs_on_unmerge(
        root,
        &env.category,
        &env.split.pn,
        &env.split.pf,
        own_slot.as_deref().unwrap_or("0"),
        &contents_text,
    )?;

    remove_contents(
        root,
        &env.category,
        &others_in_slot,
        &options.config_protect,
        &options.config_protect_mask,
        options.unmerge_orphans,
        &infodirs_inodes,
        &cfgfiledict,
        &mut stale_confmem,
        &preserved_paths,
        &contents_text,
    )?;
    if !stale_confmem.is_empty() {
        let mut updated = cfgfiledict;
        for filename in &stale_confmem {
            updated.remove(filename);
        }
        ebuild_merge::write_cfgfiledict(root, &updated)?;
    }

    let postrm_status = ebuild_phases::run_single_phase(
        ebuild_path,
        "postrm",
        root,
        portage_tmpdir,
        options.debug,
        &options.config_root,
        options.shell,
    )?;
    if postrm_status != 0 {
        return Ok(postrm_status);
    }

    std::fs::remove_dir_all(&vdb_dir).map_err(|e| format!("{}: {e}", vdb_dir.display()))?;
    if let Some(cat_dir) = vdb_dir.parent() {
        // Real `delete()`'s own best-effort `os.rmdir` of the now-maybe-
        // empty parent category directory -- ignored on failure (another
        // installed package in the same category is the common case).
        let _ = std::fs::remove_dir(cat_dir);
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-ebuild-unmerge-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn is_real_unmerge_command_covers_exactly_unmerge() {
        assert!(is_real_unmerge_command("unmerge"));
        assert!(!is_real_unmerge_command("merge"));
        assert!(!is_real_unmerge_command("qmerge"));
        assert!(!is_real_unmerge_command("install"));
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

        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            &contents,
        )
        .expect("remove_contents succeeds");

        assert!(!root.join("usr/share/x/hello.txt").exists());
        assert!(root
            .join("usr/share/x/link.txt")
            .symlink_metadata()
            .is_err());
        assert!(!root.join("usr/share/x").exists());
        assert!(!root.join("usr/share").exists());
        assert!(!root.join("usr").exists());
    }

    #[test]
    fn remove_contents_leaves_a_locally_modified_file_in_place() {
        // Real "!mtime" staleness check (see this module's own doc
        // comment): a file whose live mtime no longer matches what
        // CONTENTS recorded is treated as locally modified and left
        // alone -- the same real mechanism that also protects a
        // CONFIG_PROTECT'd file on removal (its own recorded mtime
        // reflects the ._cfg-diverted write, never the real file's own).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"user's own edits").unwrap();

        // A recorded mtime that can never match the file just written
        // above (created "now").
        let contents = "obj /etc.conf abc123 1\n";
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("remove_contents succeeds");

        assert!(
            root.join("etc.conf").is_file(),
            "a locally-modified file must survive unmerge"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_deletes_a_locally_modified_file() {
        // Real `FEATURES=unmerge-orphans` (see this module's own doc
        // comment): unlike the default (previous test), a locally-
        // modified `obj`/`sym` entry is deleted anyway -- the `!mtime`
        // staleness check is bypassed entirely.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("etc.conf"), b"user's own edits").unwrap();

        let contents = "obj /etc.conf abc123 1\n";
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            true,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("remove_contents succeeds");

        assert!(
            !root.join("etc.conf").exists(),
            "unmerge-orphans deletes a locally-modified file too"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_still_respects_config_protect() {
        // Real `FEATURES=unmerge-orphans` explicitly excludes a
        // CONFIG_PROTECT'd path (real `not self.isprotected(obj)`) --
        // it isn't a blanket override of CONFIG_PROTECT, just of the
        // ordinary `!mtime` check.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/foo.conf"), b"user's own edits").unwrap();

        let contents = "obj /etc/foo.conf abc123 1\n";
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            true,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("remove_contents succeeds");

        assert!(
            root.join("etc/foo.conf").is_file(),
            "unmerge-orphans must not delete a CONFIG_PROTECT'd path"
        );
    }

    #[test]
    fn remove_contents_with_unmerge_orphans_leaves_a_symlink_to_a_directory_alone() {
        // Real `FEATURES=unmerge-orphans` explicitly excludes a symlink
        // whose live target is itself a directory (real comment: "Don't
        // unlink symlinks to directories here since that can remove
        // /lib and /usr/lib symlinks") -- falls through to the ordinary
        // mtime-checked removal instead, which also leaves it alone
        // here (a deliberately stale recorded mtime).
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("real-lib")).unwrap();
        std::os::unix::fs::symlink(root.join("real-lib"), root.join("lib")).unwrap();

        let contents = "sym /lib -> whatever 1\n";
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            true,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
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
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
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
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("missing entries are not an error");
    }

    #[test]
    fn remove_contents_skips_a_path_another_same_slot_package_still_owns() {
        // Real `is_owned`/"replaced" skip (see this module's own doc
        // comment): a path is left alone entirely -- not even reaching
        // the mtime check -- whenever some `others_in_slot` entry's own
        // real vdb `CONTENTS` also claims it.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share")).unwrap();
        std::fs::write(root.join("usr/share/shared.txt"), b"shared").unwrap();
        std::fs::write(root.join("usr/share/only-mine.txt"), b"mine").unwrap();

        // Real, matching mtimes for both files -- so the "shared.txt"
        // skip below can only be explained by the `is_owned` check
        // itself, not incidentally by an `!mtime` mismatch.
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
        remove_contents(
            &root,
            "dev-libs",
            &["otherpkg-2.0".to_string()],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            &contents,
        )
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
        // Real `stale_confmem` (`vartree.py:2931-2932`): a removed,
        // not-`is_owned` path that `cfgfiledict` (the real
        // `_conf_mem_file` "already offered this MD5" memory) still
        // remembers is collected so the caller can drop it from that
        // persisted memory afterward.
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
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &cfgfiledict,
            &mut stale_confmem,
            &BTreeSet::new(),
            &contents,
        )
        .expect("remove_contents succeeds");

        assert_eq!(stale_confmem, vec!["/etc.conf".to_string()]);
    }

    #[test]
    fn remove_contents_does_not_mark_an_owned_path_stale_even_if_cfgfiledict_remembers_it() {
        // Real ordering (`vartree.py:2928-2932`): `stale_confmem` is an
        // `elif` off the same `is_owned` check as the "replaced" skip --
        // a path another same-slot instance still owns is never
        // considered stale, since the memory is still meaningful for
        // that surviving instance.
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
        remove_contents(
            &root,
            "dev-libs",
            &["otherpkg-2.0".to_string()],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &cfgfiledict,
            &mut stale_confmem,
            &BTreeSet::new(),
            &contents,
        )
        .expect("remove_contents succeeds");

        assert!(stale_confmem.is_empty());
    }

    #[test]
    fn remove_contents_leaves_an_orphaned_symlink_alone_while_its_target_is_still_needed() {
        // Real bug #326685 (see this module's own doc comment): a live
        // symlink-to-directory this package's own CONTENTS recorded,
        // whose exact path another same-slot instance now claims as a
        // literal `dir` entry, is never deleted directly (the ordinary
        // `is_owned` skip already protects it) -- but here its own
        // target directory is never part of *this* package's own `dir`
        // entries at all (owned/populated by something else entirely),
        // so `remove_dirs` never gets a chance to resolve it either: the
        // symlink and its target must both survive untouched.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("keep/target")).unwrap();
        std::fs::write(root.join("keep/target/other.txt"), b"other").unwrap();
        std::os::unix::fs::symlink(root.join("keep/target"), root.join("keep/link")).unwrap();

        let other_vdb = root.join("var/db/pkg/dev-libs/orphanpkg-2.0");
        std::fs::create_dir_all(&other_vdb).unwrap();
        std::fs::write(other_vdb.join("CONTENTS"), "dir /keep/link\n").unwrap();

        let contents = "dir /keep\nsym /keep/link -> whatever 100\n";
        remove_contents(
            &root,
            "dev-libs",
            &["orphanpkg-2.0".to_string()],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
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
    fn remove_contents_deletes_an_orphaned_symlink_once_its_target_directory_empties_and_revisits_the_freed_parent(
    ) {
        // Real bug #326685 + bug #640058 (see this module's own doc
        // comment): this time the symlink's own target directory *is*
        // one of this package's own `dir` entries -- once it's actually
        // removed (nothing else needs it as a real directory), the
        // now-truly-orphaned symlink is deleted too, and its own parent
        // directory (which could only fail to rmdir because the symlink
        // was still occupying it) gets a revisit and is removed as well.
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
        remove_contents(
            &root,
            "dev-libs",
            &["orphanpkg-2.0".to_string()],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
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
        // Real condition: cleanup only fires when the *entire* remaining
        // content is a subset of {"dir","dir.old"} -- any other file
        // present means the directory genuinely still has real content,
        // so nothing is removed at all (not even "dir" itself).
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
        // Real `_unmerge_dirs()`'s own second trigger half (`vartree.py`
        // ~3226): `inode_key in infodirs_inodes` fires independently of
        // the literal `basename == "info"` check -- a real `INFOPATH`/
        // `INFODIR` entry can name any directory at all.
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
        // Real `_unmerge_dirs()`'s own INFOPATH cleanup (see this
        // module's own doc comment): without it, `usr/share/info` would
        // never empty out at all -- `dir` isn't one of this package's
        // own CONTENTS entries (real `install-info` writes it outside
        // any package's own tracked content), so it would otherwise sit
        // there forever, blocking the directory's own removal.
        let tmp = tempdir();
        let root = tmp.join("root");
        std::fs::create_dir_all(root.join("usr/share/info")).unwrap();
        std::fs::write(root.join("usr/share/info/dir"), b"").unwrap();

        let contents = "dir /usr/share\ndir /usr/share/info\n";
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("remove_contents succeeds");

        assert!(
            !root.join("usr/share/info").exists(),
            "the info directory is fully removed once its own leftover index is cleaned up"
        );
        assert!(!root.join("usr/share").exists());
    }

    #[test]
    fn remove_contents_removes_a_real_env_d_sourced_infopath_directory_not_named_info() {
        // End-to-end proof that `env_update::info_dirs_inodes`'s own
        // real `/etc/env.d` collation and `remove_contents`'s own
        // `infodirs_inodes` threading actually connect: a real
        // `INFOPATH` entry can name any directory at all, not just one
        // literally called "info" (see `cleanup_info_dir_fires_on_an_
        // infodirs_inodes_match_even_when_not_named_info` above for the
        // narrower, `cleanup_info_dir`-only version of this same real
        // condition).
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
        remove_contents(
            &root,
            "dev-libs",
            &[],
            "/etc",
            "",
            false,
            &infodirs_inodes,
            &BTreeMap::new(),
            &mut Vec::new(),
            &BTreeSet::new(),
            contents,
        )
        .expect("remove_contents succeeds");

        assert!(
            !root.join("opt/pkg/docs").exists(),
            "a real env.d-sourced INFOPATH directory is cleaned up even though it isn't named info"
        );
    }

    /// `UnmergeOptions::default()`, no overrides at all: a locally-
    /// modified file is deleted anyway, matching real portage's own
    /// real out-of-the-box behavior (real `unmerge-orphans` is a
    /// default-on `FEATURES` token, see `UnmergeOptions::
    /// unmerge_orphans`'s own doc comment). Complements
    /// `remove_contents_with_unmerge_orphans_deletes_a_locally_
    /// modified_file` above, which proves the same real logic via an
    /// explicit `true` argument to `remove_contents` directly rather
    /// than relying on the real default through the full `run_merge`/
    /// `run_unmerge` chain.
    #[test]
    fn real_unmerge_deletes_a_locally_modified_file_by_real_default() {
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
        // The category dir itself is now empty too.
        assert!(!root.join("var/db/pkg/dev-libs").exists());
    }

    #[test]
    fn real_unmerge_drops_a_stale_conf_mem_entry_but_keeps_an_unrelated_one() {
        // End-to-end proof that `ebuild_merge::read_cfgfiledict`/
        // `write_cfgfiledict` and `remove_contents`'s own `stale_confmem`
        // collection actually connect through `run_unmerge` (real
        // `vartree.py:2747`/`2931-2932`/`3106-3109`).
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
        )
        .expect("run_merge succeeds");
        assert_eq!(merge_status, 0);

        // A real `_conf_mem_file`: one entry for a path this package
        // actually owns and is about to remove, one entry for a wholly
        // unrelated path nothing here touches.
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
    fn real_unmerge_of_an_in_place_upgraded_slot_mate_keeps_the_shared_file() {
        // Real, end-to-end `others_in_slot` proof: merge both
        // othersinslotpkg-1.0 and -2.0 (same SLOT, both writing the same
        // real shared path -- an in-place upgrade, real portage's own
        // "install new, then remove old" merge-list order), then unmerge
        // the *old* 1.0 vdb entry. The shared file must survive (2.0
        // still owns it); the 1.0-only file must not.
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
            )
            .expect("run_merge succeeds");
            assert_eq!(merge_status, 0);
        }
        assert!(root.join("usr/share/othersinslotpkg/shared.txt").is_file());
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v1.txt")
            .is_file());
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v2.txt")
            .is_file());
        let vdb_v1 = root.join("var/db/pkg/dev-libs/othersinslotpkg-1.0");
        let vdb_v2 = root.join("var/db/pkg/dev-libs/othersinslotpkg-2.0");
        assert!(vdb_v1.is_dir());
        assert!(vdb_v2.is_dir());

        let unmerge_status = run_unmerge(
            &ebuild_v1,
            &root,
            &portage_tmpdir,
            &UnmergeOptions::default(),
        )
        .expect("run_unmerge succeeds");
        assert_eq!(unmerge_status, 0);

        assert!(
            root.join("usr/share/othersinslotpkg/shared.txt").is_file(),
            "othersinslotpkg-2.0 still owns shared.txt -- unmerging 1.0 must not delete it"
        );
        assert!(
            !root
                .join("usr/share/othersinslotpkg/only-in-v1.txt")
                .exists(),
            "only-in-v1.txt has no other owner -- unmerging 1.0 must delete it normally"
        );
        assert!(root
            .join("usr/share/othersinslotpkg/only-in-v2.txt")
            .is_file());
        assert!(!vdb_v1.exists());
        assert!(vdb_v2.is_dir(), "the 2.0 vdb entry itself is untouched");

        // Now unmerge the remaining 2.0 entry too: with no other owner
        // left, the shared file finally goes.
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
