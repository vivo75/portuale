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
}
