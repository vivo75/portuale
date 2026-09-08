//! `FilesDb`'s write side: the merge's `-MERGING-<pf>` entry, the
//! `counter` store, the preserved-libs registry and the config-protect
//! memory.
//!
//! The code `portuale::ebuild_merge` ran before, moved without changing a
//! syscall. Nothing is atomic and nothing is locked: [`FilesTxn`] applies
//! every call when it is made and [`WriteTxn::commit`] is only an ordering
//! point.
//!
//! The on-disk formats of the registry and the config memory live here
//! too, because the database backends store the parsed values and a
//! converter must read and write the `files` bytes: the registry is
//! Portage's `PreservedLibsRegistry.store()` JSON, the config memory its
//! `grabdict`/`writedict` `"path value\n"` lines.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::files::{FilesDb, is_absent, parse_metadata_text};
use crate::{
    ConfigMemory, Counter, EntryImage, EntryKey, Error, InstalledDb as _, METADATA_FILE_FIELDS,
    METADATA_FILE_FORMAT_VERSION, MetadataStamp, PreservedLibs, PreservedLibsEntry, Result, World,
    WorldSets, WriteTxn,
};

/// The counter store under a root.
// Portage: lib/portage/const.py, CACHE_PATH
const COUNTER_PATH: &str = "var/cache/edb/counter";
/// The preserved-libs registry under a root.
const PRESERVED_LIBS_PATH: &str = "var/lib/portage/preserved_libs_registry";
/// The config-protect memory under a root.
const CONFIG_MEMORY_PATH: &str = "var/lib/portage/config";
/// The world file under a root.
// Portage: lib/portage/const.py, WORLD_FILE
const WORLD_PATH: &str = "var/lib/portage/world";
/// The world-sets file under a root.
const WORLD_SETS_PATH: &str = "var/lib/portage/world_sets";

impl FilesDb {
    /// `<vdb>/<category>/-MERGING-<pf>`, no I/O.
    pub(crate) fn pending_dir(&self, key: &EntryKey) -> PathBuf {
        self.vdb_path().join(&key.category).join(format!(
            "{}{}",
            portage_util::MERGING_IDENTIFIER,
            key.pf
        ))
    }

    /// `<root>/<rel>` for a D4 store; [`Error::Unsupported`] on a bare one.
    fn store_path(&self, rel: &str) -> Result<PathBuf> {
        match self.root() {
            Some(root) => Ok(root.join(rel)),
            None => Err(Error::Unsupported(format!(
                "{rel}: a bare VDB directory ({}) has no root to hold it",
                self.vdb_path().display()
            ))),
        }
    }

    /// `<root>/<rel>` for a D4 store read; `None` on a bare VDB directory
    /// (the read then sees an empty store).
    fn store_path_opt(&self, rel: &str) -> Option<PathBuf> {
        self.root().map(|root| root.join(rel))
    }

    /// One `open` of `-MERGING-<pf>/<name>`; a missing entry or file is `Ok(None)`.
    pub(crate) fn read_pending(&self, key: &EntryKey, name: &str) -> Result<Option<Vec<u8>>> {
        let path = self.pending_dir(key).join(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if is_absent(&e) => Ok(None),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// The `counter` file as `next_counter` reads it: `None` when it is
    /// missing, unreadable or not an integer after trimming.
    pub(crate) fn read_counter(&self) -> Option<Counter> {
        let path = self.store_path_opt(COUNTER_PATH)?;
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .map(Counter)
    }

    /// One `read_to_string` of the world file; missing is empty, any other
    /// failure is [`Error::Io`]. Blank, `#` and `@` lines are dropped.
    pub(crate) fn read_world(&self) -> Result<World> {
        let Some(path) = self.store_path_opt(WORLD_PATH) else {
            return Ok(World::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(World::default()),
            Err(e) => return Err(Error::io(path, e)),
        };
        Ok(World {
            atoms: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
                .map(String::from)
                .collect(),
        })
    }

    /// As [`FilesDb::read_world`], keeping only `@` lines and stripping
    /// every leading `@`.
    pub(crate) fn read_world_sets(&self) -> Result<WorldSets> {
        let Some(path) = self.store_path_opt(WORLD_SETS_PATH) else {
            return Ok(WorldSets::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(WorldSets::default());
            }
            Err(e) => return Err(Error::io(path, e)),
        };
        Ok(WorldSets {
            sets: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && l.starts_with('@'))
                .map(|l| l.trim_start_matches('@').to_string())
                .collect(),
        })
    }

    /// The registry as read: missing/unreadable/non-UTF-8/unparsable is
    /// empty. Not pruned (that stays with the caller).
    pub(crate) fn read_preserved_libs(&self) -> PreservedLibs {
        let entries = self
            .store_path_opt(PRESERVED_LIBS_PATH)
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| parse_preserved_libs(&text))
            .unwrap_or_default();
        PreservedLibs {
            loaded: entries.clone(),
            entries,
        }
    }

    /// The config memory as read: one read; missing or unreadable is empty.
    pub(crate) fn read_config_memory(&self) -> ConfigMemory {
        let mut entries = BTreeMap::new();
        if let Some(text) = self
            .store_path_opt(CONFIG_MEMORY_PATH)
            .and_then(|path| fs::read_to_string(path).ok())
        {
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    entries.insert(key.to_string(), value.to_string());
                }
            }
        }
        ConfigMemory { entries }
    }
}

/// `files`' [`WriteTxn`]: every call goes straight to the filesystem;
/// [`WriteTxn::commit`] does nothing and dropping undoes nothing.
pub(crate) struct FilesTxn<'a> {
    pub(crate) db: &'a FilesDb,
    /// Set by the first world write of this transaction, so the next one
    /// skips its `create_dir_all`.
    portage_dir_made: bool,
}

impl<'a> FilesTxn<'a> {
    pub(crate) fn new(db: &'a FilesDb) -> Self {
        FilesTxn {
            db,
            portage_dir_made: false,
        }
    }

    /// The shared body of `set_world` / `set_world_sets`: `create_dir_all`
    /// once per transaction, then the lines joined by `\n` with a trailing
    /// `\n`, one plain `std::fs::write`.
    fn write_world_store(&mut self, rel: &str, lines: Vec<String>) -> Result<()> {
        let path = self.db.store_path(rel)?;
        if !self.portage_dir_made {
            create_parent(&path)?;
            self.portage_dir_made = true;
        }
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        fs::write(&path, body).map_err(|e| Error::io(path, e))
    }
}

fn mtime_ns_of(st: &fs::Metadata) -> i128 {
    st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128
}

/// Set the access and modification time of a file or directory to `ns`
/// (nanoseconds since the epoch) with `futimens` on a read-only
/// descriptor. Never touches the directory entry of `path` itself.
fn set_mtime_ns(path: &Path, ns: i128) -> Result<()> {
    let secs = ns.div_euclid(1_000_000_000);
    let nanos = ns.rem_euclid(1_000_000_000) as u32;
    let bad = || Error::Invalid(format!("{}: mtime {ns} out of range", path.display()));
    let whole = u64::try_from(secs.unsigned_abs()).map_err(|_| bad())?;
    let t = if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(whole, nanos))
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::new(whole, 0))
            .and_then(|t| t.checked_add(Duration::new(0, nanos)))
    }
    .ok_or_else(bad)?;
    let f = fs::File::open(path).map_err(|e| Error::io(path, e))?;
    f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t))
        .map_err(|e| Error::io(path, e))
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o7777))
        .map_err(|e| Error::io(path, e))
}

/// `body` with every `#dir_mtime=` line removed and a final newline.
fn without_stamp(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for line in body.split_inclusive(|&b| b == b'\n') {
        if !line.starts_with(b"#dir_mtime=") {
            out.extend_from_slice(line);
        }
    }
    if !out.is_empty() && out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    out
}

impl FilesTxn<'_> {
    /// Steps 1-5 of [`WriteTxn::insert_entry`] inside the pending dir.
    fn write_image(&self, pending: &Path, image: &EntryImage) -> Result<()> {
        let key = &image.key;
        let meta_path = pending.join("metadata");
        // 1. bytes.
        for f in &image.files {
            let path = pending.join(&f.meta.name);
            let body: Vec<u8>;
            let data: &[u8] = if f.meta.name == "metadata"
                && image.metadata_stamp == MetadataStamp::Valid
            {
                body = without_stamp(&f.data);
                let fmt = format!("#format={METADATA_FILE_FORMAT_VERSION}");
                if !body.split(|&b| b == b'\n').any(|l| l == fmt.as_bytes()) {
                    return Err(Error::Invalid(format!(
                        "{key}: Valid stamp but metadata has no #format={METADATA_FILE_FORMAT_VERSION}"
                    )));
                }
                &body
            } else {
                &f.data
            };
            fs::write(&path, data).map_err(|e| Error::io(path, e))?;
        }
        // 2. file mtimes and modes (metadata later).
        for f in image.files.iter().filter(|f| f.meta.name != "metadata") {
            let path = pending.join(&f.meta.name);
            set_mtime_ns(&path, f.meta.mtime_ns)?;
            set_mode(&path, f.meta.mode)?;
        }
        // 3. directory mtime, before any stamp.
        set_mtime_ns(pending, image.dir_mtime_ns)?;
        // 4. the stamp, last.
        if let Some(f) = image.files.iter().find(|f| f.meta.name == "metadata") {
            set_mode(&meta_path, 0o600)?;
            let st = fs::metadata(pending).map_err(|e| Error::io(pending, e))?;
            let cur = mtime_ns_of(&st);
            match image.metadata_stamp {
                MetadataStamp::Valid => {
                    use std::io::Write as _;
                    let mut h = fs::OpenOptions::new()
                        .append(true)
                        .open(&meta_path)
                        .map_err(|e| Error::io(&meta_path, e))?;
                    writeln!(h, "#dir_mtime={cur}").map_err(|e| Error::io(&meta_path, e))?;
                }
                MetadataStamp::Stale => {
                    let stored = std::str::from_utf8(&f.data)
                        .ok()
                        .and_then(parse_metadata_text)
                        .and_then(|(_, stamp)| stamp);
                    if stored == Some(cur) {
                        let text = String::from_utf8_lossy(&f.data);
                        let fixed: String = text
                            .split_inclusive('\n')
                            .map(|l| {
                                if l.starts_with("#dir_mtime=") {
                                    format!("#dir_mtime={}\n", cur - 1)
                                } else {
                                    l.to_string()
                                }
                            })
                            .collect();
                        fs::write(&meta_path, fixed).map_err(|e| Error::io(&meta_path, e))?;
                    }
                }
                MetadataStamp::Absent => {}
            }
            set_mtime_ns(&meta_path, f.meta.mtime_ns)?;
            set_mode(&meta_path, f.meta.mode)?;
        }
        // 5. directory mode.
        set_mode(pending, image.dir_mode)
    }
}

/// `std::fs::create_dir_all` of `path`'s parent, error on the parent.
/// Portage's `lockdir(<vdb>)`: a blocking exclusive `flock(2)` on
/// `<parent>/.<basename>.portage_lockfile`, unlinked again on release.
/// Because a waiter may hold an fd on the removed inode, the acquirer
/// re-checks that the path still names the file it locked and starts over
/// when it does not. Scoped here to one counter tick.
// Portage: lib/portage/locks.py, lockdir
struct VdbLock {
    _file: fs::File,
    path: PathBuf,
}

impl VdbLock {
    fn acquire(vdb: &Path) -> Result<Self> {
        use std::os::unix::io::AsRawFd as _;
        let parent = vdb.parent().unwrap_or_else(|| Path::new("."));
        let base = vdb
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let path = parent.join(format!(".{base}.portage_lockfile"));
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        loop {
            let file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| Error::io(&path, e))?;
            // SAFETY: `flock` takes only the fd, which `file` keeps valid.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(Error::io(path, std::io::Error::last_os_error()));
            }
            let (Ok(held), Ok(named)) = (file.metadata(), fs::metadata(&path)) else {
                continue; // unlinked by the previous holder: start over
            };
            if held.ino() == named.ino() && held.dev() == named.dev() {
                return Ok(Self { _file: file, path });
            }
        }
    }
}

impl Drop for VdbLock {
    /// Unlink while still holding the lock, then the fd closes and the
    /// lock is released.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Portage's `write_atomic`: follow symlinks, write a temporary file in
/// the same directory, copy the mode of the file it replaces, then
/// `rename` over the target.
// Portage: lib/portage/util/__init__.py, atomic_ofstream
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write as _;
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    create_parent(&target)?;
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let base = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let tmp = parent.join(format!("{base}.{}.{n}", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(Error::io(tmp, e)),
        }
    };
    let result = (|| {
        file.write_all(data)?;
        if let Ok(st) = fs::metadata(&target) {
            fs::set_permissions(&tmp, fs::Permissions::from_mode(st.mode() & 0o7777))?;
        }
        drop(file);
        fs::rename(&tmp, &target)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(Error::io(target, e));
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    Ok(())
}

impl WriteTxn for FilesTxn<'_> {
    /// Remove a stale `-MERGING-<pf>` a killed merge left, then create the
    /// pending dir (and the category dir).
    fn begin_entry(&mut self, key: &EntryKey) -> Result<()> {
        let tmp_dir = self.db.pending_dir(key);
        if tmp_dir.exists() {
            fs::remove_dir_all(&tmp_dir).map_err(|e| Error::io(&tmp_dir, e))?;
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| Error::io(&tmp_dir, e))?;
        Ok(())
    }

    /// `remove_dir_all` of the `-MERGING-<pf>` directory, then the category
    /// directory if it is now empty (best effort). The live `<pf>` is left.
    fn discard_pending(&mut self, key: &EntryKey) -> Result<()> {
        let dir = self.db.pending_dir(key);
        if !dir.is_dir() {
            return Err(Error::Invalid(format!("no pending entry for {key}")));
        }
        fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        if let Some(cat_dir) = dir.parent() {
            let _ = fs::remove_dir(cat_dir);
        }
        Ok(())
    }

    /// `std::fs::write` into the pending dir (truncate in place, mode from
    /// the umask).
    fn put_entry_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        let path = self.db.pending_dir(key).join(name);
        fs::write(&path, data).map_err(|e| Error::io(path, e))
    }

    /// `std::fs::copy(src, <pending>/<name>)` (mode copied, mtime not).
    /// The error names `src`.
    fn copy_entry_file(&mut self, key: &EntryKey, name: &str, src: &Path) -> Result<()> {
        fs::copy(src, self.db.pending_dir(key).join(name)).map_err(|e| Error::io(src, e))?;
        Ok(())
    }

    /// Portage's `counter_tick_core`: under the VDB lock, the next value
    /// is the larger of the `counter` file (`-1` when missing or
    /// unparsable) and every installed entry's `COUNTER`, plus one, written
    /// back atomically with no newline.
    // Portage: lib/portage/vartree.py, counter_tick_core
    fn next_counter(&mut self) -> Result<Counter> {
        let counter_path = self.db.store_path(COUNTER_PATH)?;
        let _lock = VdbLock::acquire(self.db.vdb_path())?;
        let mut max = self.db.read_counter().map_or(-1, |c| c.0);
        for key in self.db.entries()? {
            // Skip a missing entry, field, or a value that is not an integer.
            if let Some(Ok(c)) = self
                .db
                .aux_get_field(&key, "COUNTER")
                .map(|v| v.trim().parse::<i64>())
            {
                max = max.max(c);
            }
        }
        let next = max + 1;
        write_atomic(&counter_path, next.to_string().as_bytes())?;
        Ok(Counter(next))
    }

    /// Write the consolidated `metadata` file of the pending entry: every
    /// [`METADATA_FILE_FIELDS`] file present, whitespace-normalised and
    /// sorted under `#format=1`, then `#dir_mtime=<st_mtime_ns>` appended
    /// in a second open. No-op when no field file exists.
    fn seal_entry(&mut self, key: &EntryKey) -> Result<()> {
        let dbdir = self.db.pending_dir(key);
        let mut data: Vec<(String, String)> = Vec::new();
        for &field in METADATA_FILE_FIELDS {
            let path = dbdir.join(field);
            // A non-UTF-8 field is sealed as its lossy text, like an
            // unsealed read.
            let Ok(raw) = fs::read(&path) else {
                continue;
            };
            let raw = String::from_utf8_lossy(&raw);
            data.push((
                field.to_string(),
                raw.split_whitespace().collect::<Vec<_>>().join(" "),
            ));
        }
        if data.is_empty() {
            return Ok(());
        }
        data.sort();

        let metadata_path = dbdir.join("metadata");
        let mut body = format!("#format={METADATA_FILE_FORMAT_VERSION}\n");
        for (k, v) in &data {
            body.push_str(&format!("{k}={v}\n"));
        }
        fs::write(&metadata_path, &body).map_err(|e| Error::io(&metadata_path, e))?;

        // `#dir_mtime=` is appended after the body is on disk, so the
        // stamp reflects the body write and the append itself leaves it
        // untouched (no new dir entry).
        let st = fs::metadata(&dbdir).map_err(|e| Error::io(&dbdir, e))?;
        let dir_mtime_ns = st.mtime() as i128 * 1_000_000_000 + st.mtime_nsec() as i128;
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&metadata_path)
            .map_err(|e| Error::io(&metadata_path, e))?;
        use std::io::Write as _;
        writeln!(f, "#dir_mtime={dir_mtime_ns}").map_err(|e| Error::io(&metadata_path, e))?;
        Ok(())
    }

    /// Publish the pending entry: `remove_dir_all` a live same-`pf` entry
    /// when present, then `rename` the pending dir onto it. Same category
    /// dir, so the rename is atomic and keeps the `metadata` stamp. Rename
    /// errors name the live path.
    fn finish_entry(&mut self, key: &EntryKey) -> Result<()> {
        let tmp_dir = self.db.pending_dir(key);
        let final_dir = self.db.vdb_path().join(&key.category).join(&key.pf);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).map_err(|e| Error::io(&final_dir, e))?;
        }
        fs::rename(&tmp_dir, &final_dir).map_err(|e| Error::io(&final_dir, e))?;
        Ok(())
    }

    /// Whole-entry write for converters: directory mtimes before the
    /// stamp, stale stays stale, no `metadata` file is ever added. Order,
    /// in the pending `-MERGING-<pf>` dir: files with their bytes; file
    /// mtimes and modes; the directory mtime; the stamp last; the
    /// directory mode; then `finish_entry`. Any failure removes the
    /// pending directory.
    fn insert_entry(&mut self, image: &EntryImage) -> Result<()> {
        let key = &image.key;
        for f in &image.files {
            let n = &f.meta.name;
            if n.is_empty() || n == "." || n == ".." || n.contains(['/', '\0']) {
                return Err(Error::Invalid(format!("{key}: bad file name {n:?}")));
            }
        }
        let has_meta = image.files.iter().any(|f| f.meta.name == "metadata");
        match (image.metadata_stamp, has_meta) {
            (MetadataStamp::Absent, true) => {
                return Err(Error::Invalid(format!(
                    "{key}: stamp state Absent but the image holds a metadata file"
                )));
            }
            (MetadataStamp::Valid, false) => {
                return Err(Error::Invalid(format!(
                    "{key}: stamp state Valid but the image holds no metadata file"
                )));
            }
            _ => {}
        }
        self.begin_entry(key)?;
        let pending = self.db.pending_dir(key);
        let r = self.write_image(&pending, image);
        let r = r.and_then(|()| self.finish_entry(key));
        if r.is_err() {
            let _ = fs::remove_dir_all(&pending);
        }
        r
    }

    /// `remove_dir_all` of the live entry (its error is returned), then a
    /// best-effort `remove_dir` of the category directory.
    fn delete_entry(&mut self, key: &EntryKey) -> Result<()> {
        let dir = self.db.vdb_path().join(&key.category).join(&key.pf);
        fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        if let Some(cat_dir) = dir.parent() {
            let _ = fs::remove_dir(cat_dir);
        }
        Ok(())
    }

    /// A plain `std::fs::write` of `<entry>/<name>` (truncate in place, no
    /// temporary file, no rename), so the entry directory's mtime and the
    /// `metadata` stamp are untouched.
    fn replace_file(&mut self, key: &EntryKey, name: &str, data: &[u8]) -> Result<()> {
        let path = self
            .db
            .vdb_path()
            .join(&key.category)
            .join(&key.pf)
            .join(name);
        fs::write(&path, data).map_err(|e| Error::io(path, e))
    }

    /// The caller sorts and de-duplicates. `create_dir_all` of
    /// `var/lib/portage` (once per transaction), the atoms joined by
    /// newlines plus a trailing newline, a plain `std::fs::write`: no
    /// temporary file, no lock.
    fn set_world(&mut self, world: &World) -> Result<()> {
        self.write_world_store(WORLD_PATH, world.atoms.clone())
    }

    /// As [`WriteTxn::set_world`], each name written as `@<name>`.
    fn set_world_sets(&mut self, sets: &WorldSets) -> Result<()> {
        self.write_world_store(
            WORLD_SETS_PATH,
            sets.sets.iter().map(|n| format!("@{n}")).collect(),
        )
    }

    /// Writes nothing at all (not even the parent directory) when
    /// `entries == loaded`; otherwise the [`format_preserved_libs`] bytes
    /// with a plain `std::fs::write`.
    fn set_preserved_libs(&mut self, libs: &PreservedLibs) -> Result<()> {
        if libs.entries == libs.loaded {
            return Ok(());
        }
        let out = format_preserved_libs(&libs.entries);
        let path = self.db.store_path(PRESERVED_LIBS_PATH)?;
        create_parent(&path)?;
        fs::write(&path, out).map_err(|e| Error::io(path, e))
    }

    /// `create_dir_all` of the parent, then `"<path> <md5>\n"` per entry
    /// in key order, unconditionally.
    fn set_config_memory(&mut self, memory: &ConfigMemory) -> Result<()> {
        let path = self.db.store_path(CONFIG_MEMORY_PATH)?;
        create_parent(&path)?;
        let mut text = String::new();
        for (k, v) in &memory.entries {
            text.push_str(&format!("{k} {v}\n"));
        }
        fs::write(&path, text).map_err(|e| Error::io(path, e))
    }

    /// The historic `counter` file: the bare integer, no newline, one
    /// `std::fs::write` after `create_dir_all` of the parent, as
    /// `next_counter` writes it. A plain store: it may lower the counter.
    fn set_counter(&mut self, counter: Counter) -> Result<()> {
        let path = self.db.store_path(COUNTER_PATH)?;
        create_parent(&path)?;
        fs::write(&path, counter.0.to_string()).map_err(|e| Error::io(path, e))
    }

    /// Nothing to do: every call above was applied when it was made.
    fn commit(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

/// A minimal JSON string-literal reader for the registry: narrow by
/// design, not a general parser.
fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{C}'),
                'u' => {
                    let hex: String = (0..4).map(|_| chars.next()).collect::<Option<String>>()?;
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

fn skip_json_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

fn parse_json_string_array(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Option<Vec<String>> {
    skip_json_ws(chars);
    if chars.next()? != '[' {
        return None;
    }
    let mut out = Vec::new();
    skip_json_ws(chars);
    if chars.peek() == Some(&']') {
        chars.next();
        return Some(out);
    }
    loop {
        skip_json_ws(chars);
        out.push(parse_json_string(chars)?);
        skip_json_ws(chars);
        match chars.next()? {
            ',' => continue,
            ']' => return Some(out),
            _ => return None,
        }
    }
}

/// Parses exactly the shape Portage's `PreservedLibsRegistry.store()`
/// writes: `{"cp:slot": [cpv, counter, [paths...]], ...}`. `None` on any
/// deviation; readers treat that like a missing file.
pub fn parse_preserved_libs(text: &str) -> Option<BTreeMap<String, PreservedLibsEntry>> {
    let mut chars = text.chars().peekable();
    let mut entries = BTreeMap::new();
    skip_json_ws(&mut chars);
    if chars.next()? != '{' {
        return None;
    }
    skip_json_ws(&mut chars);
    if chars.peek() == Some(&'}') {
        chars.next();
        return Some(entries);
    }
    loop {
        skip_json_ws(&mut chars);
        let key = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ':' {
            return None;
        }
        skip_json_ws(&mut chars);
        if chars.next()? != '[' {
            return None;
        }
        skip_json_ws(&mut chars);
        let cpv = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        skip_json_ws(&mut chars);
        let counter = parse_json_string(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ',' {
            return None;
        }
        let paths = parse_json_string_array(&mut chars)?;
        skip_json_ws(&mut chars);
        if chars.next()? != ']' {
            return None;
        }
        entries.insert(
            key,
            PreservedLibsEntry {
                cpv,
                counter,
                paths,
            },
        );
        skip_json_ws(&mut chars);
        match chars.next()? {
            ',' => continue,
            '}' => return Some(entries),
            _ => return None,
        }
    }
}

fn json_quote(s: &str) -> String {
    // Portage's `json.dumps(..., ensure_ascii=False)` quoting.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Portage's `store()` JSON layout (sorted keys); `{}` when empty, no
/// trailing newline.
pub fn format_preserved_libs(entries: &BTreeMap<String, PreservedLibsEntry>) -> String {
    if entries.is_empty() {
        return String::from("{}");
    }
    let mut out = String::from("{\n");
    let n = entries.len();
    for (i, (key, entry)) in entries.iter().enumerate() {
        out.push_str(&format!("\t{}: [\n", json_quote(key)));
        out.push_str(&format!("\t\t{},\n", json_quote(&entry.cpv)));
        out.push_str(&format!("\t\t{},\n", json_quote(&entry.counter)));
        if entry.paths.is_empty() {
            out.push_str("\t\t[]\n");
        } else {
            out.push_str("\t\t[\n");
            for (j, p) in entry.paths.iter().enumerate() {
                out.push_str(&format!("\t\t\t{}", json_quote(p)));
                out.push_str(if j + 1 < entry.paths.len() {
                    ",\n"
                } else {
                    "\n"
                });
            }
            out.push_str("\t\t]\n");
        }
        out.push_str("\t]");
        out.push_str(if i + 1 < n { ",\n" } else { "\n" });
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstalledDb;

    fn put_live(root: &Path, cat: &str, pf: &str, files: &[(&str, &str)]) {
        let dir = root.join("var/db/pkg").join(cat).join(pf);
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            fs::write(dir.join(name), body).unwrap();
        }
    }
}
