//! `FilesDb`: the historic `var/db/pkg` directory tree.
//!
//! Per-entry and `aux` reads keep the same `openat`/`statx` sequence and
//! the same in-process memo as before. The merge's writes are in
//! `files_write.rs`. Every method not moved yet returns
//! [`Error::Unsupported`] naming the plan step that moves it.

use crate::types::DIR_MODE_MASK;
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

/// The VDB directory under a root, joined without canonicalising.
pub(crate) const VDB_PATH: &str = "var/db/pkg";

/// The `files` backend for one root, or for one bare VDB directory.
#[derive(Debug, Clone)]
pub struct FilesDb {
    /// The root the D4 stores live under; `None` for a bare VDB directory.
    root: Option<PathBuf>,
    /// `<root>/var/db/pkg`, or the directory given to `open_vdb_dir`.
    vdb: PathBuf,
}

impl FilesDb {
    /// The `files` backend of `root`: entries under `<root>/var/db/pkg`,
    /// D4 stores under `<root>/var/lib/portage` and `<root>/var/cache/edb`.
    pub fn new(root: &Path) -> Self {
        FilesDb {
            root: Some(root.to_path_buf()),
            vdb: root.join(VDB_PATH),
        }
    }

    /// A read-only view of a bare VDB directory with no root around it.
    /// Not registered anywhere. The D4 stores are absent: their reads
    /// return empty values and every write fails.
    pub fn open_vdb_dir(vdb_dir: &Path) -> Self {
        FilesDb {
            root: None,
            vdb: vdb_dir.to_path_buf(),
        }
    }

    /// The `files` backend a command-line `files:PATH` names: `PATH` is a
    /// root or the VDB directory itself. `None` when `PATH` is no directory.
    pub fn from_cli_path(path: &Path) -> Option<Self> {
        if path.ends_with(VDB_PATH) {
            // `<root>/var/db/pkg` is three levels below the root.
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

    /// The root given to [`FilesDb::new`]; `None` for a bare VDB directory.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// `<root>/var/db/pkg` (or the bare VDB directory), no I/O.
    pub(crate) fn vdb_path(&self) -> &Path {
        &self.vdb
    }
}

/// `st_mtime` of `p` in nanos, 0 when missing or unreadable: the
/// invalidation signal for category-scoped caches (every vdb mutation
/// replaces or removes a package dir).
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

/// Parse and validate a consolidated `metadata` snapshot: `#format=`
/// must match and `#dir_mtime=` must equal the dir mtime, else `None`.
/// A truncated write lacks the stamp (written last) and is rejected.
// Portage: lib/portage/vartree.py, _read_metadata_file
fn read_metadata_file(path: &Path, dir_mtime_ns: i128) -> Option<HashMap<String, String>> {
    let raw = fs::read_to_string(path).ok()?;
    let (result, dir_mtime) = parse_metadata_text(&raw)?;
    if dir_mtime != Some(dir_mtime_ns) {
        return None;
    }
    Some(result)
}

/// The text half of [`read_metadata_file`], shared with the SQLite
/// backend: the parsed fields and the `#dir_mtime=` value when present.
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

/// The per-key fallback read: the raw file whitespace-normalised, with an
/// absent file as `""`. Invalid UTF-8 decodes lossy, matching Portage's
/// `errors="replace"`.
// Portage: lib/portage/vartree.py, _aux_get
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

/// Whether `value` is a `SLOT` Portage's `aux_get` keeps instead of
/// translating to `"0"`: `slot(/slot)?` with `slot = [\w][\w+.-]*`. Empty
/// is *not* invalid: a missing/empty `SLOT` stays `""`.
// Portage: lib/portage/versions.py, _valid_slot
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

/// Portage's invalid-`SLOT` → `"0"` translation, for present (non-empty)
/// values only. The memoised value is the translated one (`"0"` is valid).
// Portage: lib/portage/vartree.py, aux_get
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

    /// A cheap fingerprint of the VDB structure: the vdb dir's mtime plus
    /// every category dir's mtime, xor-folded with a count. ~30 `stat`s vs
    /// the ~2000 `SLOT` reads a full scan does.
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

    /// Portage's `_aux_get` for one in-set key: `stat` the package dir
    /// once, try the consolidated `metadata` snapshot (a validated one is
    /// *complete*, so a missing field is `""` with no `open()`), otherwise
    /// read the individual file. `None` when the entry does not exist.
    /// The invalid-`SLOT` -> `"0"` translation applies, via
    /// [`translate_aux_slot`]; the `EAPI == "" -> "0"` one does not.
    // Portage: lib/portage/vartree.py, _aux_get
    pub(crate) fn aux_get_field(&self, key: &EntryKey, field: &str) -> Option<String> {
        // One `statx` for both the existence test and the `st_mtime_ns`
        // validity check below.
        let dir = self.entry_dir(key);
        let st = fs::metadata(&dir).ok().filter(|st| st.is_dir())?;
        let dir_mtime_ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;

        // Per-instance metadata keyed on the dir's `st_mtime_ns`: a snapshot
        // fills all 23 fields at once, the fallback resolves lazily per
        // key. An in-place field rewrite leaves the dir mtime alone, so a
        // value read once is served until the dir changes. Thread-local,
        // so the lookup stays lock-free.
        type CacheKey = (PathBuf, String, String);
        type AuxCache = HashMap<CacheKey, (i128, Rc<HashMap<String, String>>)>;
        thread_local! {
            static AUX_CACHE: RefCell<AuxCache> = RefCell::new(HashMap::new());
        }
        let cache_key = (self.vdb.clone(), key.category.clone(), key.pf.clone());

        // The `stat` above is the validity signal, paid on every call; the
        // memo removes the snapshot read and the per-key `open`.
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
            // This key is not resolved yet; drop the shared handle before
            // mutating so `Rc::make_mut` can reuse it.
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
                    // A validated snapshot is complete: pre-fill every
                    // member so an absent one is `""` with no `open()`.
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

/// The claims of one entry's `CONTENTS` text: a parsable line (see
/// [`crate::contents`]) claims its path; one leading `/` is ignored on
/// both sides. Each claim pairs the first of `paths` it matches with
/// `key`.
pub(crate) fn claim_paths(
    text: &str,
    key: &EntryKey,
    wanted: &HashMap<&[u8], &[u8]>,
    out: &mut Vec<(Vec<u8>, EntryKey)>,
) {
    for line in text.lines() {
        let Some(entry) = crate::contents::parse_line(line) else {
            continue;
        };
        let path = entry.path();
        let path = path.strip_prefix('/').unwrap_or(path);
        if let Some(p) = wanted.get(path.as_bytes()) {
            out.push((p.to_vec(), key.clone()));
        }
    }
}

/// For `owners` on every backend: each distinct normal form (one leading
/// `/` stripped) of `paths`, with the first input path that has it.
pub(crate) fn owner_wanted<'a>(paths: &[&'a [u8]]) -> HashMap<&'a [u8], &'a [u8]> {
    let mut m: HashMap<&[u8], &[u8]> = HashMap::new();
    for p in paths {
        m.entry(p.strip_prefix(b"/").unwrap_or(p)).or_insert(p);
    }
    m
}

/// The spellings a `CONTENTS` line can have for the normal form `n`:
/// `/n`, and `n` itself when it does not start with `/`.
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
                // An in-progress `-MERGING-<pf>` entry is never installed.
                if portage_util::is_merging_vdb_entry(&dirname) {
                    continue;
                }
                out.push(EntryKey::new(category.clone(), dirname));
            }
        }
        Ok(out)
    }

    /// The category directories of the VDB, in listing order; empty when
    /// the VDB cannot be listed.
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
            // `d_type` fast path; the `path().is_dir()` fallback covers
            // symlinked entries and `DT_UNKNOWN` filesystems.
            e.file_type().is_ok_and(|t| t.is_dir()) || e.path().is_dir()
        }) {
            let name = e.file_name().to_string_lossy().to_string();
            // An in-progress `-MERGING-<pf>` entry is never installed.
            if portage_util::is_merging_vdb_entry(&name) {
                continue;
            }
            out.push(name);
        }
        Ok(out)
    }

    fn has_entry(&self, key: &EntryKey) -> Result<bool> {
        // One `statx`.
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
        // One `open`, no separate existence `stat`.
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

    /// One named file of every live entry, in listing order. A file that
    /// cannot be read is `None`, whatever the reason; an unreadable VDB
    /// or category directory ends or skips the listing.
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

    /// A whole live entry for a converter: every file with its bytes, mode
    /// and mtime, the directory's mode and mtime, and the `metadata` stamp
    /// state. `None` when the entry does not exist.
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
            dir_mode: dst.mode() & DIR_MODE_MASK,
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
            dir_mode: dst.mode() & DIR_MODE_MASK,
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

    /// File owners: each entry's `CONTENTS` is read (a missing,
    /// unreadable or non-UTF-8 file owns nothing) and every claim is
    /// paired with the first of `paths` it matches (see [`claim_paths`]).
    /// The package-move fallback and the `<package>-<version>` split stay
    /// above this crate.
    fn owners(&self, paths: &[&[u8]]) -> Result<Vec<(Vec<u8>, EntryKey)>> {
        let mut out = Vec::new();
        let wanted = owner_wanted(paths);
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
                let _ = self.has_entry(&key);
                let Some(text) = self
                    .read_file(&key, "CONTENTS")
                    .ok()
                    .flatten()
                    .and_then(|b| String::from_utf8(b).ok())
                else {
                    continue;
                };
                claim_paths(&text, &key, &wanted, &mut out);
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

    // Only deterministic oracles pin the hash: absence (0), presence
    // (non-zero, non-one), and sensitivity to structural change.

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
