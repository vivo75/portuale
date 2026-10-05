//! `FilesDb`: the historic `var/db/pkg` directory tree.
//!
//! The read side of per-entry and `aux` reads (S1.2) is today's
//! `portage-repo` code, moved here unchanged: the same `openat`/`statx`
//! sequence and the same in-process memo (thread-local, validated by the
//! entry directory's `st_mtime_ns`). The merge's write side (S1.4: the
//! pending entry, the counter, the preserved-libs registry and the config
//! memory) is in `files_write.rs`. Every method that is not moved yet
//! returns [`Error::Unsupported`] naming the plan step that moves it
//! (`reverse_dependents` stays above this crate on `files`, see the module
//! doc item 19; S1.5 moved `owners`, `read_file_all`, `delete_entry`
//! `replace_file`; S1.6 moved `world` / `world_sets`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::files_write::FilesTxn;
use crate::{
    BackendKind, ConfigMemory, Counter, DepClass, DepRecord, EntryFile, EntryImage, EntryKey,
    EntryStat, Error, FileMeta, InstalledDb, METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION,
    MetadataStamp, PreservedLibs, Result, Snapshot, World, WorldSets, WriteTxn, in_metadata_file,
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

    /// The `files` backend a command-line `files:PATH` names: `PATH` is a
    /// root, or the VDB directory itself (a path ending in the VDB
    /// location means the root three levels up, so the D4 stores come
    /// along). Any other existing directory is a bare VDB directory
    /// ([`FilesDb::open_vdb_dir`]: `root()` is `None`, no D4 stores).
    /// `None` when `PATH` is no directory.
    pub fn from_cli_path(path: &Path) -> Option<Self> {
        if path.ends_with(VDB_PATH) {
            let root = path.ancestors().nth(3)?;
            let root = if root.as_os_str().is_empty() {
                Path::new(".")
            } else {
                root
            };
            return Some(FilesDb::new(root));
        }
        if path.join(VDB_PATH).is_dir() {
            Some(FilesDb::new(path))
        } else {
            path.is_dir().then(|| FilesDb::open_vdb_dir(path))
        }
    }

    /// The root given to [`FilesDb::new`]; `None` for
    /// [`FilesDb::open_vdb_dir`].
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// `<root>/var/db/pkg` (or the bare VDB directory), no I/O.
    pub(crate) fn vdb_path(&self) -> &Path {
        &self.vdb
    }
}

/// `st_mtime` of `p` in nanos, 0 when the path is missing or unreadable.
/// The invalidation signal for category-scoped caches (moved from
/// `portage-repo::dir_mtime_nanos`): every portuale vdb mutation replaces
/// or removes a package dir (`publish_vdb_tmp` `remove_dir_all` +
/// `rename`; unmerge removes the dir), so the *category* dir mtime
/// catches all of them. An external in-place rewrite of a file inside a
/// running process is not caught -- the same in-process-cache property
/// real's `vardb` dbapi has; no portuale writer does it.
fn dir_mtime_nanos(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64)
}

/// A read error that means "no such entry or file".
pub(crate) fn is_absent(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn file_meta_of(name: String, st: &fs::Metadata) -> FileMeta {
    FileMeta {
        name,
        len: st.len(),
        mode: st.mode(),
        mtime_ns: st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128,
    }
}

/// Real `_read_metadata_file(path, dir_st)` (`vartree.py:115-187`): parse
/// and validate a consolidated `metadata` snapshot. `None` unless
/// `#format=` parses to [`METADATA_FILE_FORMAT_VERSION`] **and**
/// `#dir_mtime=` parses and equals the package directory's `st_mtime_ns`.
/// A `#format=` this version does not know is rejected immediately (real
/// abandons the file on the version line, before parsing the rest); other
/// `#` lines are ignored; a line without `=` is skipped;
/// `k, v = line.split("=", 1)` with the last duplicate winning. Real
/// writes `#dir_mtime=` last, so a file left truncated by an interrupted
/// write lacks it and is rejected rather than read as a short snapshot.
fn read_metadata_file(path: &Path, dir_mtime_ns: i128) -> Option<HashMap<String, String>> {
    let raw = fs::read_to_string(path).ok()?;
    let (result, dir_mtime) = parse_metadata_text(&raw)?;
    if dir_mtime != Some(dir_mtime_ns) {
        return None;
    }
    Some(result)
}

/// The text half of [`read_metadata_file`], shared with the SQLite
/// backend (which stores the stamp state per entry and so does not compare
/// the directory mtime): the parsed fields and the `#dir_mtime=` value
/// when present. `None` unless `#format=` is present and equals
/// [`METADATA_FILE_FORMAT_VERSION`] (or a `#format=`/`#dir_mtime=` value
/// does not parse).
pub(crate) fn parse_metadata_text(raw: &str) -> Option<(HashMap<String, String>, Option<i128>)> {
    let mut result: HashMap<String, String> = HashMap::new();
    let mut version: Option<u32> = None;
    let mut dir_mtime: Option<i128> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(v) = rest.strip_prefix("format=") {
                version = Some(v.parse::<u32>().ok()?);
                if version != Some(METADATA_FILE_FORMAT_VERSION) {
                    return None;
                }
            } else if let Some(v) = rest.strip_prefix("dir_mtime=") {
                dir_mtime = Some(v.parse::<i128>().ok()?);
            }
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            result.insert(k.to_string(), v.to_string());
        }
    }
    version?;
    Some((result, dir_mtime))
}

/// The per-key fallback read: the raw file normalised exactly like real
/// `_aux_get` (`" ".join(myd.split())`, `vartree.py:1044-1046`), with an
/// absent file as `""`. Real applies the same normalisation on this path,
/// so the bytes are the same whether the snapshot validated or not.
/// Invalid UTF-8 decodes lossy (`U+FFFD`), matching real's
/// `encoding="utf-8", errors="replace"` (`vartree.py:1035-1039`) instead
/// of collapsing to `""` (backlog #125, audit O15).
fn read_vdb_file(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| normalise_aux_bytes(&bytes))
        .unwrap_or_default()
}

/// The bytes of one field file as `aux_get` serves them: lossy UTF-8,
/// whitespace-joined (shared with the SQLite backend).
pub(crate) fn normalise_aux_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `value` is a `SLOT` real `aux_get` keeps instead of
/// translating to `"0"`: `slot(/slot)?` with `slot = [\w][\w+.-]*`
/// (ASCII — `versions.py:38`, `re.ASCII`), the `/sub` half iff the
/// EAPI has `slot_operator` (`versions.py:76-90`). Portuale does no
/// EAPI parametrization inside the EAPI 5+ floor (`agent-context.md`:
/// every live EAPI has `slot_operator`, `eapi.py:319`), so this is
/// always the operator shape; an entry whose own `EAPI` is empty (for
/// which real would use the single-slot shape after its `EAPI ""→"0"`
/// step) is out of scope — that translation is #115's documented cut,
/// and no such entry has a portuale-visible consumer anyway (Phase 12
/// S0.3). Empty is *not* invalid here: a missing/empty `SLOT` stays
/// `""` at this layer and keeps #126's O5 caller contracts.
fn is_valid_aux_slot(value: &str) -> bool {
    fn is_slot(s: &str) -> bool {
        let mut chars = s.bytes();
        match chars.next() {
            Some(b) if b.is_ascii_alphanumeric() || b == b'_' => {}
            _ => return false,
        }
        chars.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'-'))
    }
    match value.split_once('/') {
        Some((main, sub)) => is_slot(main) && is_slot(sub),
        None => is_slot(value),
    }
}

/// Real `aux_get`'s invalid-`SLOT` → `"0"` translation, applied at the
/// one seam every vdb `SLOT` read flows through. Real translates in
/// the outer `aux_get` (`vartree.py:967-972`); here
/// [`FilesDb::aux_get_field`] serves both roles, so it lives here, for the
/// `SLOT` key only, over present (non-empty) values. The memoised value
/// is the translated one (observably identical: the translation is
/// idempotent, `"0"` is valid).
pub(crate) fn translate_aux_slot(key: &str, value: String) -> String {
    if key == "SLOT" && !value.is_empty() && !is_valid_aux_slot(&value) {
        return "0".to_string();
    }
    value
}

impl FilesDb {
    /// `<vdb>/<category>/<pf>`, no I/O.
    fn entry_dir(&self, key: &EntryKey) -> PathBuf {
        self.vdb.join(&key.category).join(&key.pf)
    }

    /// A cheap fingerprint of the VDB directory's structure (moved from
    /// `portage-repo::vdb_fingerprint`): the vdb dir's own mtime plus
    /// every category dir's mtime, xor-folded with a count. Any package
    /// merge/unmerge changes the mtime of `var/db/pkg` (new category) or
    /// of a category dir (new/removed package dir), so a matching
    /// fingerprint means the installed set is unchanged. ~30 `stat`s vs
    /// the ~2000 `SLOT` file reads a full scan does.
    fn fingerprint(&self) -> u64 {
        let vdb = self.vdb.as_path();
        let Ok(cats) = portage_util::read_dir_entries(vdb) else {
            return 0;
        };
        let mut acc = dir_mtime_nanos(vdb);
        let mut count: u64 = 0;
        for cat in cats.into_iter().filter(|e| e.path().is_dir()) {
            acc ^= dir_mtime_nanos(&cat.path()).rotate_left((count % 61) as u32 + 1);
            count += 1;
        }
        acc ^ count.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Real `_aux_get(cpv, wants, st)` (`vartree.py:975-1053`) for one
    /// in-set key (moved from `portage-repo::vdb_aux_get`): `stat` the
    /// package dir once, try the consolidated `metadata` snapshot, and
    /// serve an in-set key from it with **no** `open()`; otherwise read
    /// the individual file (the pre-#109 path). `None` when the entry
    /// directory does not exist (one `statx`, no more).
    ///
    /// The snapshot rule is real's own (`vartree.py:1010-1017`): a
    /// validated snapshot is *complete*, so a field missing from it had no
    /// individual file either and is served as `""` instead of paying an
    /// `open()` that would just fail. Safe by construction for added
    /// fields -- adding a per-field file bumps the package dir's mtime,
    /// which invalidates the snapshot -- and the residual in-place-rewrite
    /// hole is real's own documented one (why it calls `_bump_mtime` on
    /// both sides of `aux_update`).
    ///
    /// Real's `aux_get` `EAPI == "" -> "0"` translation is not here:
    /// callers default individually and that parity question is a filed
    /// residue. The invalid-`SLOT` -> `"0"` half *is* here
    /// ([`translate_aux_slot`], #115 S1).
    fn aux_get_field(&self, key: &EntryKey, field: &str) -> Option<String> {
        // #112: one `statx` for both the existence test and the
        // `st_mtime_ns` validity check below.
        let dir = self.entry_dir(key);
        let st = fs::metadata(&dir).ok().filter(|st| st.is_dir())?;
        let dir_mtime_ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;

        // Real `aux_get`'s `_aux_cache["packages"]` (`vartree.py:909-973`):
        // per-instance metadata keyed on the package dir's `st_mtime_ns`,
        // so a second key on the same instance costs one `stat` instead of
        // a snapshot read (and a per-key `open()` on the fallback path).
        // `cache_these = _aux_cache_keys ∪ wants` is why a validated
        // snapshot fills **all** 23 fields at once; the fallback path
        // resolves lazily per key and records each resolved value,
        // including `""`.
        //
        // The in-process staleness property is real's own: an in-place
        // rewrite of a field file leaves the dir mtime alone, so a value
        // read once is served until the dir changes. Real calls
        // `_bump_mtime` on both sides of `aux_update` for exactly that
        // reason; the same property is why portuale's merge path (which
        // replaces the whole entry dir) is safe. Thread-local so the
        // lookup stays lock-free, like `EUF_CACHE`.
        type CacheKey = (PathBuf, String, String);
        type AuxCache = HashMap<CacheKey, (i128, Rc<HashMap<String, String>>)>;
        thread_local! {
            static AUX_CACHE: RefCell<AuxCache> = RefCell::new(HashMap::new());
        }
        let cache_key = (self.vdb.clone(), key.category.clone(), key.pf.clone());

        // The `stat` above is the validity signal, paid on every call
        // (real stats before consulting `_aux_cache` too); the memo
        // removes the snapshot read and the per-key `open`.
        let cached = AUX_CACHE.with(|c| {
            c.borrow()
                .get(&cache_key)
                .filter(|(mtime, _)| *mtime == dir_mtime_ns)
                .map(|(_, map)| Rc::clone(map))
        });
        if let Some(map) = cached {
            if let Some(v) = map.get(field) {
                return Some(translate_aux_slot(field, v.clone()));
            }
            // Fallback path: this key has not been resolved yet. Drop the
            // shared handle before mutating so `Rc::make_mut` can reuse it.
            drop(map);
            let value = translate_aux_slot(field, read_vdb_file(&dir.join(field)));
            AUX_CACHE.with(|c| {
                if let Some((_, map)) = c.borrow_mut().get_mut(&cache_key) {
                    Rc::make_mut(map).insert(field.to_string(), value.clone());
                }
            });
            return Some(value);
        }

        // Miss or dir-mtime change: read and validate the snapshot once.
        let mut map: HashMap<String, String> =
            match read_metadata_file(&dir.join("metadata"), dir_mtime_ns) {
                Some(snapshot) => {
                    // A validated snapshot is complete for the 23-set:
                    // pre-fill every member so an absent one is `""` with
                    // no `open()`.
                    let mut m = snapshot;
                    for &f in METADATA_FILE_FIELDS {
                        m.entry(f.to_string()).or_default();
                    }
                    m
                }
                None => HashMap::new(),
            };
        let value = map
            .get(field)
            .cloned()
            .unwrap_or_else(|| read_vdb_file(&dir.join(field)));
        let value = translate_aux_slot(field, value);
        map.insert(field.to_string(), value.clone());
        AUX_CACHE.with(|c| {
            c.borrow_mut()
                .insert(cache_key, (dir_mtime_ns, Rc::new(map)));
        });
        Some(value)
    }
}

/// The claims of one entry's `CONTENTS` text (the per-line rule of
/// [`FilesDb::owners`], shared with the SQLite backend): a line whose
/// first two words are a kind (`obj`, `sym`, `dir`, `dev`, `fif`, `bin`)
/// and a path claims that path; one leading `/` is ignored on both sides.
/// Each claim pairs the first of `paths` it matches with `key`.
pub(crate) fn claim_paths(
    text: &str,
    key: &EntryKey,
    paths: &[&[u8]],
    out: &mut Vec<(Vec<u8>, EntryKey)>,
) {
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let path = match (words.next(), words.next()) {
            (Some("obj" | "sym" | "dir" | "dev" | "fif" | "bin"), Some(path)) => {
                path.strip_prefix('/').unwrap_or(path)
            }
            _ => continue,
        };
        let claimed = paths
            .iter()
            .find(|p| p.strip_prefix(b"/").unwrap_or(p) == path.as_bytes());
        if let Some(p) = claimed {
            out.push((p.to_vec(), key.clone()));
        }
    }
}

/// For the indexed `owners` of the database backends (S8.2): each distinct
/// normal form (one leading `/` stripped, as [`claim_paths`] compares) of
/// `paths`, with the first input path that has it, which is the one
/// `claim_paths` reports.
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
pub(crate) fn owner_wanted<'a>(paths: &[&'a [u8]]) -> HashMap<&'a [u8], &'a [u8]> {
    let mut m: HashMap<&[u8], &[u8]> = HashMap::new();
    for p in paths {
        m.entry(p.strip_prefix(b"/").unwrap_or(p)).or_insert(p);
    }
    m
}

/// The spellings a `CONTENTS` line can have for the normal form `n`, as
/// stored in the `owner` index (the path as written): `/n`, and `n` itself
/// when it does not start with `/` (a line whose path has none).
#[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
pub(crate) fn owner_spellings(n: &[u8]) -> Vec<Vec<u8>> {
    let mut v = vec![[b"/".as_slice(), n].concat()];
    if !n.starts_with(b"/") {
        v.push(n.to_vec());
    }
    v
}

pub(crate) fn todo_step<T>(what: &str, step: &str) -> Result<T> {
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
        Ok(self.fingerprint())
    }

    fn category_generation(&self, category: &str) -> Result<u64> {
        Ok(dir_mtime_nanos(&self.vdb.join(category)))
    }

    fn entries(&self) -> Result<Vec<EntryKey>> {
        let mut out = Vec::new();
        let Ok(cats) = portage_util::read_dir_entries(&self.vdb) else {
            return Ok(out);
        };
        for cat in cats.into_iter().filter(|e| e.path().is_dir()) {
            let category = cat.file_name().to_string_lossy().to_string();
            let Ok(pkgs) = portage_util::read_dir_entries(&cat.path()) else {
                continue;
            };
            for pkg in pkgs.into_iter().filter(|e| e.path().is_dir()) {
                let dirname = pkg.file_name().to_string_lossy().to_string();
                // Real `vardbapi._excluded_dirs`: an in-progress
                // `-MERGING-<pf>` entry is never an installed package.
                if portage_util::is_merging_vdb_entry(&dirname) {
                    continue;
                }
                out.push(EntryKey::new(category.clone(), dirname));
            }
        }
        Ok(out)
    }

    /// The category directories of the VDB (each one tested with
    /// `is_dir`), in listing order; empty when the VDB cannot be listed.
    fn categories(&self) -> Result<Vec<String>> {
        let Ok(cats) = portage_util::read_dir_entries(&self.vdb) else {
            return Ok(Vec::new());
        };
        Ok(cats
            .into_iter()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect())
    }

    fn pending_entries(&self) -> Result<Vec<EntryKey>> {
        let mut out = Vec::new();
        let Ok(cats) = portage_util::read_dir_entries(&self.vdb) else {
            return Ok(out);
        };
        for cat in cats.into_iter().filter(|e| e.path().is_dir()) {
            let category = cat.file_name().to_string_lossy().to_string();
            let Ok(pkgs) = portage_util::read_dir_entries(&cat.path()) else {
                continue;
            };
            for pkg in pkgs.into_iter().filter(|e| e.path().is_dir()) {
                let dirname = pkg.file_name().to_string_lossy().to_string();
                if let Some(pf) = dirname.strip_prefix(portage_util::MERGING_IDENTIFIER) {
                    out.push(EntryKey::new(category.clone(), pf));
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn category_entries(&self, category: &str) -> Result<Vec<String>> {
        let Ok(entries) = portage_util::read_dir_entries(&self.vdb.join(category)) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for e in entries.into_iter().filter(|e| {
            // `d_type` fast path: a real directory needs no `statx`. The
            // `path().is_dir()` fallback keeps exact semantics for
            // symlinked entries and `DT_UNKNOWN` filesystems.
            e.file_type().is_ok_and(|t| t.is_dir()) || e.path().is_dir()
        }) {
            let name = e.file_name().to_string_lossy().to_string();
            // Real `vardbapi._excluded_dirs`: an in-progress
            // `-MERGING-<pf>` entry is never an installed version.
            if portage_util::is_merging_vdb_entry(&name) {
                continue;
            }
            out.push(name);
        }
        Ok(out)
    }

    fn has_entry(&self, key: &EntryKey) -> Result<bool> {
        // One `statx`, following symlinks like the `is_dir()` it replaces.
        Ok(fs::metadata(self.entry_dir(key)).is_ok_and(|st| st.is_dir()))
    }

    fn aux_get(&self, key: &EntryKey, field: &str) -> Result<Option<String>> {
        if !in_metadata_file(field) {
            return Err(Error::Invalid(format!(
                "aux_get key {field:?} is not one of the 23 metadata fields (use read_file)"
            )));
        }
        Ok(self.aux_get_field(key, field))
    }

    fn snapshot(&self) -> Result<Arc<Snapshot>> {
        todo_step("snapshot", "S3.2")
    }

    fn list_files(&self, key: &EntryKey) -> Result<Option<Vec<FileMeta>>> {
        let dir = self.entry_dir(key);
        if !fs::metadata(&dir).is_ok_and(|st| st.is_dir()) {
            return Ok(None);
        }
        let entries =
            portage_util::read_dir_entries(&dir).map_err(|e| Error::io(dir.clone(), e))?;
        let mut out = Vec::new();
        for e in entries {
            let path = e.path();
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                return Err(Error::Invalid(format!(
                    "{}: entry file name is not UTF-8",
                    path.display()
                )));
            };
            let st = fs::metadata(&path).map_err(|e| Error::io(path.clone(), e))?;
            if st.is_file() {
                out.push(file_meta_of(name, &st));
            }
        }
        Ok(Some(out))
    }

    fn file_meta(&self, key: &EntryKey, name: &str) -> Result<Option<FileMeta>> {
        let path = self.entry_dir(key).join(name);
        match fs::metadata(&path) {
            Ok(st) => Ok(Some(file_meta_of(name.to_string(), &st))),
            Err(e) if is_absent(&e) => Ok(None),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    fn read_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        // One `open`, no separate existence `stat` (today's raw read).
        // A missing entry directory and a missing file are both `None`.
        let path = self.entry_dir(key).join(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if is_absent(&e) => Ok(None),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    fn read_file_at(
        &self,
        key: &EntryKey,
        name: &str,
        off: u64,
        len: usize,
    ) -> Result<Option<Vec<u8>>> {
        use std::os::unix::fs::FileExt as _;
        let path = self.entry_dir(key).join(name);
        let file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if is_absent(&e) => return Ok(None),
            Err(e) => return Err(Error::io(path, e)),
        };
        let st = file.metadata().map_err(|e| Error::io(path.clone(), e))?;
        if !st.is_file() {
            return Ok(None);
        }
        // Never allocate more than the file can still give.
        let want = (len as u64).min(st.len().saturating_sub(off)) as usize;
        let mut buf = vec![0u8; want];
        let mut done = 0;
        while done < want {
            match file.read_at(&mut buf[done..], off + done as u64) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::io(path, e)),
            }
        }
        buf.truncate(done);
        Ok(Some(buf))
    }

    fn read_pending_file(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        self.read_pending(key, name)
    }

    /// Moved from `needed_elf::read_all_needed_entries`: the same walk and
    /// the same order of `open`s. Every category directory is listed and
    /// tested first; then, category by category, the entry directories
    /// are listed and tested, and the file of each non-`-MERGING-` entry
    /// is read. A file that cannot be read is `None`, whatever the reason
    /// (today's `read_to_string(..).unwrap_or_default()`); an unreadable
    /// VDB or category directory ends or skips the listing.
    fn read_file_all(&self, name: &str) -> Result<Vec<(EntryKey, Option<Vec<u8>>)>> {
        let mut out = Vec::new();
        let Ok(categories) = portage_util::read_dir_entries(&self.vdb) else {
            return Ok(out);
        };
        let category_names: Vec<String> = categories
            .into_iter()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        for category in category_names {
            let category_path = self.vdb.join(&category);
            let Ok(packages) = portage_util::read_dir_entries(&category_path) else {
                continue;
            };
            let pf_names: Vec<String> = packages
                .into_iter()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            for pf in pf_names {
                if portage_util::is_merging_vdb_entry(&pf) {
                    continue;
                }
                let data = fs::read(category_path.join(&pf).join(name)).ok();
                out.push((EntryKey::new(category.clone(), pf), data));
            }
        }
        Ok(out)
    }

    /// Every regular file of the live entry (sorted by name, like
    /// `list_files`; a sub-directory is not part of an entry and is
    /// skipped), exact bytes, `st_mode` and `st_mtime_ns`; the directory's
    /// `st_mode` and `st_mtime_ns`; and the `metadata` stamp state by the
    /// real reader rule ([`parse_metadata_text`] + equality with the
    /// directory's `st_mtime_ns`). The directory is `stat`ed first; reads
    /// do not change its mtime.
    fn entry_image(&self, key: &EntryKey) -> Result<Option<EntryImage>> {
        let dir = self.entry_dir(key);
        let dst = match fs::metadata(&dir) {
            Ok(st) if st.is_dir() => st,
            Ok(_) => return Ok(None),
            Err(e) if is_absent(&e) => return Ok(None),
            Err(e) => return Err(Error::io(dir, e)),
        };
        let dir_mtime_ns = dst.mtime() as i128 * 1_000_000_000 + dst.mtime_nsec() as i128;
        let entries =
            portage_util::read_dir_entries(&dir).map_err(|e| Error::io(dir.clone(), e))?;
        let mut files = Vec::new();
        for e in entries {
            let path = e.path();
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                return Err(Error::Invalid(format!(
                    "{}: entry file name is not UTF-8",
                    path.display()
                )));
            };
            let st = fs::metadata(&path).map_err(|e| Error::io(path.clone(), e))?;
            if !st.is_file() {
                continue;
            }
            let data = fs::read(&path).map_err(|e| Error::io(path.clone(), e))?;
            files.push(EntryFile {
                meta: file_meta_of(name, &st),
                data,
            });
        }
        files.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
        let metadata_stamp = match files.iter().find(|f| f.meta.name == "metadata") {
            None => MetadataStamp::Absent,
            Some(f) => {
                let valid = std::str::from_utf8(&f.data)
                    .ok()
                    .and_then(parse_metadata_text)
                    .is_some_and(|(_, stamp)| stamp == Some(dir_mtime_ns));
                if valid {
                    MetadataStamp::Valid
                } else {
                    MetadataStamp::Stale
                }
            }
        };
        Ok(Some(EntryImage {
            key: key.clone(),
            files,
            dir_mode: dst.mode(),
            dir_mtime_ns,
            metadata_stamp,
        }))
    }

    fn entry_stat(&self, key: &EntryKey) -> Result<Option<EntryStat>> {
        let dir = self.entry_dir(key);
        let dst = match fs::metadata(&dir) {
            Ok(st) if st.is_dir() => st,
            Ok(_) => return Ok(None),
            Err(e) if is_absent(&e) => return Ok(None),
            Err(e) => return Err(Error::io(dir, e)),
        };
        let dir_mtime_ns = dst.mtime() as i128 * 1_000_000_000 + dst.mtime_nsec() as i128;
        let Some(mut files) = self.list_files(key)? else {
            return Ok(None);
        };
        files.sort_by(|a, b| a.name.cmp(&b.name));
        // Only the small `metadata` file is read, to judge its stamp.
        let metadata_stamp = if files.iter().any(|f| f.name == "metadata") {
            let path = dir.join("metadata");
            let valid = fs::read(&path)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .and_then(|t| parse_metadata_text(&t))
                .is_some_and(|(_, stamp)| stamp == Some(dir_mtime_ns));
            if valid {
                MetadataStamp::Valid
            } else {
                MetadataStamp::Stale
            }
        } else {
            MetadataStamp::Absent
        };
        Ok(Some(EntryStat {
            files,
            dir_mode: dst.mode(),
            dir_mtime_ns,
            metadata_stamp,
        }))
    }

    fn reverse_dependents(&self, _cp: &str, _classes: &[DepClass]) -> Result<Vec<DepRecord>> {
        todo_step(
            "reverse_dependents",
            "S2: on files the scan stays in portage-repo over aux_get, S1.3",
        )
    }

    /// Moved from `ebuild_merge::find_owners` (its directory walk plus the
    /// per-entry `installed_contents_files`): list the VDB, each category
    /// directory (tested with `is_dir`) and each entry directory (tested
    /// with `is_dir`), skip `-MERGING-` entries, `stat` the entry and read
    /// its `CONTENTS` (a missing, unreadable or non-UTF-8 file owns
    /// nothing). A `CONTENTS` line whose first two words are a kind
    /// (`obj`, `sym`, `dir`, `dev`, `fif`, `bin`) and a path claims that
    /// path; one leading `/` is ignored on both sides of the comparison.
    /// Each claim pairs the first of `paths` it matches with the entry.
    ///
    /// Not done here, because both live above this crate: the
    /// package-move fallback and the `<package>-<version>` split that
    /// `find_owners` applies to the entry name (it skips a name that does
    /// not split). `find_owners` applies the split to the result.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        let mut out = Vec::new();
        let Ok(categories) = portage_util::read_dir_entries(&self.vdb) else {
            return Ok(out);
        };
        for category_entry in categories {
            let category_path = category_entry.path();
            if !category_path.is_dir() {
                continue;
            }
            let category = category_entry.file_name().to_string_lossy().to_string();
            let Ok(packages) = portage_util::read_dir_entries(&category_path) else {
                continue;
            };
            for pkg_entry in packages {
                if !pkg_entry.path().is_dir() {
                    continue;
                }
                let pf = pkg_entry.file_name().to_string_lossy().to_string();
                if portage_util::is_merging_vdb_entry(&pf) {
                    continue;
                }
                let key = EntryKey::new(category.clone(), pf);
                // Today's `resolve_vdb_entry` tests the entry once before
                // the read; keep the `statx`.
                let _ = self.has_entry(&key);
                let Some(text) = self
                    .read_file(&key, "CONTENTS")
                    .ok()
                    .flatten()
                    .and_then(|b| String::from_utf8(b).ok())
                else {
                    continue;
                };
                claim_paths(&text, &key, paths, &mut out);
            }
        }
        Ok(out)
    }

    fn world(&self) -> Result<World> {
        self.read_world()
    }

    fn world_sets(&self) -> Result<WorldSets> {
        self.read_world_sets()
    }

    fn preserved_libs(&self) -> Result<PreservedLibs> {
        Ok(self.read_preserved_libs())
    }

    fn config_memory(&self) -> Result<ConfigMemory> {
        Ok(self.read_config_memory())
    }

    fn counter(&self) -> Result<Option<Counter>> {
        Ok(self.read_counter())
    }

    /// No I/O and no lock: the `files` transaction applies each call when it is made
    /// (crate doc item 20).
    fn begin_write(&self) -> Result<Box<dyn WriteTxn + '_>> {
        Ok(Box::new(FilesTxn::new(self)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch root, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("portage-vdb-s12-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    // ---- `generation` (moved with `vdb_fingerprint` from portage-repo:
    // the `all_installed_packages` cache key: the vdb dir's own mtime
    // plus every category dir's mtime, xor-folded with the category
    // count; a missing vdb is exactly 0). Only deterministic oracles pin
    // the hash: absence (0), presence (non-zero, non-one), and
    // sensitivity to structural change. ----

    /// A missing vdb fingerprints to exactly 0.
    #[test]
    fn generation_of_a_missing_vdb_is_zero() {
        let root = scratch("fp-missing");
        assert_eq!(FilesDb::new(&root).generation().unwrap(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    /// An existing (even empty) vdb never fingerprints to 0 or 1.
    #[test]
    fn generation_of_an_existing_vdb_is_neither_zero_nor_one() {
        let root = scratch("fp-empty");
        fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let fp = FilesDb::new(&root).generation().unwrap();
        assert_ne!(fp, 0);
        assert_ne!(fp, 1);
        let _ = fs::remove_dir_all(&root);
    }

    /// Adding a category changes the fingerprint.
    #[test]
    fn generation_changes_when_a_category_appears() {
        let root = scratch("fp-sensitivity");
        fs::create_dir_all(root.join("var/db/pkg")).unwrap();
        let db = FilesDb::new(&root);
        let before = db.generation().unwrap();
        fs::create_dir_all(root.join("var/db/pkg/dev-libs")).unwrap();
        assert_ne!(before, db.generation().unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn category_generation_is_zero_when_missing_and_nonzero_when_present() {
        let root = scratch("catgen");
        fs::create_dir_all(root.join("var/db/pkg/dev-libs")).unwrap();
        let db = FilesDb::new(&root);
        assert_eq!(db.category_generation("sys-apps").unwrap(), 0);
        assert_ne!(db.category_generation("dev-libs").unwrap(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn listing_skips_merging_entries_and_plain_files() {
        let root = scratch("listing");
        let cat = root.join("var/db/pkg/dev-libs");
        fs::create_dir_all(cat.join("a-1.0")).unwrap();
        fs::create_dir_all(cat.join("-MERGING-b-2.0")).unwrap();
        fs::write(cat.join("stray"), b"").unwrap();
        let db = FilesDb::new(&root);
        assert_eq!(db.category_entries("dev-libs").unwrap(), vec!["a-1.0"]);
        assert!(db.category_entries("nope").unwrap().is_empty());
        assert_eq!(
            db.entries().unwrap(),
            vec![EntryKey::new("dev-libs", "a-1.0")]
        );
        assert!(db.has_entry(&EntryKey::new("dev-libs", "a-1.0")).unwrap());
        assert!(!db.has_entry(&EntryKey::new("dev-libs", "stray")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn aux_get_distinguishes_missing_entry_empty_field_and_foreign_key() {
        let root = scratch("aux");
        let dir = root.join("var/db/pkg/dev-libs/a-1.0");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RDEPEND"), b"a\n  b\n").unwrap();
        fs::write(dir.join("SLOT"), b"!bad\n").unwrap();
        fs::write(dir.join("CONTENTS"), b"obj /x\n").unwrap();
        let db = FilesDb::new(&root);
        let k = EntryKey::new("dev-libs", "a-1.0");
        assert_eq!(db.aux_get(&k, "RDEPEND").unwrap().as_deref(), Some("a b"));
        assert_eq!(db.aux_get(&k, "SLOT").unwrap().as_deref(), Some("0"));
        assert_eq!(db.aux_get(&k, "DEPEND").unwrap().as_deref(), Some(""));
        assert_eq!(
            db.aux_get(&EntryKey::new("dev-libs", "zz-1"), "SLOT")
                .unwrap(),
            None
        );
        assert!(matches!(db.aux_get(&k, "CONTENTS"), Err(Error::Invalid(_))));
        assert_eq!(
            db.read_file(&k, "CONTENTS").unwrap().as_deref(),
            Some(&b"obj /x\n"[..])
        );
        assert_eq!(db.read_file(&k, "nope").unwrap(), None);
        assert_eq!(
            db.read_file(&EntryKey::new("dev-libs", "zz-1"), "SLOT")
                .unwrap(),
            None
        );
        let m = db.file_meta(&k, "CONTENTS").unwrap().unwrap();
        assert_eq!((m.name.as_str(), m.len), ("CONTENTS", 7));
        assert_eq!(db.file_meta(&k, "nope").unwrap(), None);
        let names: Vec<String> = db
            .list_files(&k)
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["CONTENTS", "RDEPEND", "SLOT"]);
        assert!(
            db.list_files(&EntryKey::new("dev-libs", "zz-1"))
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(&root);
    }
}
