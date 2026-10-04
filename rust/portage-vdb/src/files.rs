//! `FilesDb`: the historic `var/db/pkg` directory tree.
//!
//! S1.1 stub. Only the pure path methods work; every method that would do
//! I/O returns [`Error::Unsupported`] naming the plan step that moves
//! today's code here (read side S1.2/S1.3, merge writes S1.4, unmerge and
//! W4 S1.5, world S1.6). Nothing calls it yet.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryImage, EntryKey, Error, FileMeta,
    InstalledDb, PreservedLibs, Result, Snapshot, World, WorldSets, WriteTxn,
};

/// The VDB directory under a root, joined without canonicalising (today's
/// `root.join("var/db/pkg")`).
pub(crate) const VDB_PATH: &str = "var/db/pkg";

/// The `files` backend for one root, or for one bare VDB directory.
#[derive(Debug, Clone)]
pub struct FilesDb {
    /// `Some` for [`FilesDb::new`]: the root the D4 stores (`world`,
    /// `counter`, ...) live under. `None` for [`FilesDb::open_vdb_dir`].
    root: Option<PathBuf>,
    /// `<root>/var/db/pkg`, or the directory given to `open_vdb_dir`.
    vdb: PathBuf,
}

impl FilesDb {
    /// The `files` backend of `root`: entries under `<root>/var/db/pkg`,
    /// D4 stores under `<root>/var/lib/portage` and
    /// `<root>/var/cache/edb`. `root` is used as given (no
    /// canonicalisation, no I/O).
    pub fn new(root: &Path) -> Self {
        FilesDb {
            root: Some(root.to_path_buf()),
            vdb: root.join(VDB_PATH),
        }
    }

    /// A read-only view of a bare VDB directory with no root around it
    /// (N12: the remote `server:<path>` shadow and its `pull_dir` copy).
    /// Not registered anywhere. The D4 stores are absent: their reads
    /// return empty values and every write fails (from S1.2/S1.7 on).
    pub fn open_vdb_dir(vdb_dir: &Path) -> Self {
        FilesDb {
            root: None,
            vdb: vdb_dir.to_path_buf(),
        }
    }

    /// The root given to [`FilesDb::new`]; `None` for
    /// [`FilesDb::open_vdb_dir`].
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
}

fn todo_step<T>(what: &str, step: &str) -> Result<T> {
    Err(Error::Unsupported(format!(
        "FilesDb::{what} is not implemented yet (feat#157 {step})"
    )))
}

impl InstalledDb for FilesDb {
    fn kind(&self) -> BackendKind {
        BackendKind::Files
    }

    fn vdb_dir(&self) -> Option<PathBuf> {
        Some(self.vdb.clone())
    }

    fn generation(&self) -> Result<u64> {
        todo_step("generation", "S1.2")
    }

    fn category_generation(&self, _category: &str) -> Result<u64> {
        todo_step("category_generation", "S1.2")
    }

    fn entries(&self) -> Result<Vec<EntryKey>> {
        todo_step("entries", "S1.2")
    }

    fn category_entries(&self, _category: &str) -> Result<Vec<String>> {
        todo_step("category_entries", "S1.2")
    }

    fn has_entry(&self, _key: &EntryKey) -> Result<bool> {
        todo_step("has_entry", "S1.3")
    }

    fn aux_get(&self, _key: &EntryKey, _field: &str) -> Result<Option<String>> {
        todo_step("aux_get", "S1.2")
    }

    fn snapshot(&self) -> Result<Arc<Snapshot>> {
        todo_step("snapshot", "S1.2")
    }

    fn list_files(&self, _key: &EntryKey) -> Result<Option<Vec<FileMeta>>> {
        todo_step("list_files", "S2.2")
    }

    fn file_meta(&self, _key: &EntryKey, _name: &str) -> Result<Option<FileMeta>> {
        todo_step("file_meta", "S1.5")
    }

    fn read_file(&self, _key: &EntryKey, _name: &str) -> Result<Option<Vec<u8>>> {
        todo_step("read_file", "S1.2")
    }

    fn read_file_at(
        &self,
        _key: &EntryKey,
        _name: &str,
        _off: u64,
        _len: usize,
    ) -> Result<Option<Vec<u8>>> {
        todo_step("read_file_at", "S7.2")
    }

    fn read_pending_file(&self, _key: &EntryKey, _name: &str) -> Result<Option<Vec<u8>>> {
        todo_step("read_pending_file", "S1.4")
    }

    fn read_file_all(&self, _name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>> {
        todo_step("read_file_all", "S1.4")
    }

    fn entry_image(&self, _key: &EntryKey) -> Result<Option<EntryImage>> {
        todo_step("entry_image", "S2.2")
    }

    fn reverse_dependents(&self, _cp: &str, _classes: &[DepClass]) -> Result<Vec<DepRecord>> {
        todo_step("reverse_dependents", "S1.3")
    }

    fn owners(&self, _paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        todo_step("owners", "S1.4")
    }

    fn world(&self) -> Result<World> {
        todo_step("world", "S1.6")
    }

    fn world_sets(&self) -> Result<WorldSets> {
        todo_step("world_sets", "S1.6")
    }

    fn preserved_libs(&self) -> Result<PreservedLibs> {
        todo_step("preserved_libs", "S1.4")
    }

    fn config_memory(&self) -> Result<ConfigMemory> {
        todo_step("config_memory", "S1.4")
    }

    fn counter(&self) -> Result<Option<Counter>> {
        todo_step("counter", "S1.4")
    }

    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>> {
        todo_step("begin_write", "S1.4")
    }
}
