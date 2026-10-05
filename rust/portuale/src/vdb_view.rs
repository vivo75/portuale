// The read-only filesystem view of an installed-package database
// (feat#157 / backlog #305, plan S7.2 and S7.3; design `docs/vdb_to_db.md`
// §12).
//
// This is the pure half of `portuale vdb mount`. Nothing here knows about
// FUSE: the tree, the inode numbers, `lookup`/`getattr`/`readdir`/`open`/
// `read`, the one-generation-per-handle rule, the EROFS decisions, the
// mtimes and the generated `metadata` file are plain methods on [`View`], so
// the unit tests drive them without a mount (this host has no `/dev/fuse`).
// `vdb_fuse.rs` is the thin translation layer to the `fuser` callbacks.
//
// The tree is `/` (the VDB root) -> `CAT/` -> `PF/` -> files.
//
// Inodes. An interning table maps every path seen (`Node`) to a number
// (root = 1, then 2, 3, ... in order of first sight) and back. It lives as
// long as the mount, never reuses a number and survives generation changes,
// so a path keeps its inode across commits, even when the entry is deleted
// and published again.
//
// Attributes (S0 finding `_bump_mtime`, `vartree.py:578-583`: it touches the
// category directory and the VDB root, not the entry directory):
//   - entry directory: mode = the stored directory mode (permission bits),
//     mtime = the stored `dir_mtime_ns`;
//   - category directory: mtime = the latest `dir_mtime_ns` of its entries;
//   - root: mtime = the latest over all categories;
//   - other directories 0755; files: the stored mode (permission bits), size
//     and mtime.
// FUSE carries nanoseconds, so a shown mtime is exact.
//
// The `metadata` file follows the stamp state stored with the entry
// (`MetadataStamp`):
//   - Valid  -> the stored bytes with every `#dir_mtime=` line rewritten to
//     the entry directory mtime this view shows (real `_read_metadata_file`
//     accepts the file iff `#format=1` and `#dir_mtime=` equals the
//     directory's `st_mtime_ns`, `vartree.py:176-185`). The size is that of
//     the rewritten bytes.
//   - Stale  -> served exactly as stored; it does not match and stays stale
//     (S0.3: a converter never repairs a stamp).
//   - Absent -> there is no `metadata` file.
//
// Generations. `InstalledDb::generation` is the cache key. Every operation
// that starts from a path (`lookup`, `getattr`, `opendir`, `open`) works on
// the current generation, so a lookup after a commit sees the new one.
// What a handle returns afterwards is pinned:
//   - `opendir` materialises the whole listing and the handle serves that
//     list, whatever commits follow;
//   - `open` of a file of up to `pin_max` bytes (default 64 KiB: every small
//     field file, the `metadata` file) reads the bytes once and the handle
//     serves those bytes;
//   - a bigger file is read with `read_file_at` (never a whole-blob load).
//     The database has no snapshot to pin, so the handle remembers the
//     file's size and mtime; once the generation has moved on, each read
//     first checks that they still match and answers `ESTALE` if the file
//     changed or went away, rather than mixing two generations.
//     (Residue: a rewrite with the same size and mtime is not noticed.)

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use portage_vdb::{EntryKey, EntryStat, FileMeta, InstalledDb, MetadataStamp};

/// The root directory's inode (FUSE's fixed `FUSE_ROOT_ID`).
pub const ROOT_INO: u64 = 1;

/// Files up to this many bytes are read once at `open` and pinned.
pub const DEFAULT_PIN_MAX: usize = 64 * 1024;

const METADATA: &str = "metadata";
const DIR_MTIME_PREFIX: &[u8] = b"#dir_mtime=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
}

/// What `getattr` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub ino: u64,
    pub kind: Kind,
    /// Permission bits only (`0o7777` mask).
    pub perm: u16,
    pub size: u64,
    pub nlink: u32,
    /// Nanoseconds since the epoch.
    pub mtime_ns: i128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEnt {
    pub ino: u64,
    pub kind: Kind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewError {
    /// No such path (also: the path vanished in a later generation).
    NoEnt,
    NotDir,
    IsDir,
    /// A handle that was never issued or was already released.
    BadHandle,
    /// Every write operation.
    Rofs,
    /// A pinned file changed or went away under an open handle.
    Stale,
    /// The backend failed.
    Io(String),
}

impl ViewError {
    /// The errno the adapter replies with.
    pub fn errno(&self) -> i32 {
        match self {
            ViewError::NoEnt => libc::ENOENT,
            ViewError::NotDir => libc::ENOTDIR,
            ViewError::IsDir => libc::EISDIR,
            ViewError::BadHandle => libc::EBADF,
            ViewError::Rofs => libc::EROFS,
            ViewError::Stale => libc::ESTALE,
            ViewError::Io(_) => libc::EIO,
        }
    }
}

impl From<portage_vdb::Error> for ViewError {
    fn from(e: portage_vdb::Error) -> Self {
        ViewError::Io(e.to_string())
    }
}

type Res<T> = Result<T, ViewError>;

/// The operations that would modify the tree. Each one is `EROFS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOp {
    Create,
    Mknod,
    Mkdir,
    Unlink,
    Rmdir,
    Rename,
    Symlink,
    Link,
    Setattr,
    Write,
    Setxattr,
    Removexattr,
    Fallocate,
    CopyFileRange,
}

/// The one decision for every write-class operation: refuse with `EROFS`.
pub fn deny(_op: WriteOp) -> ViewError {
    ViewError::Rofs
}

/// `open(2)` flags that ask for write access (`O_WRONLY`, `O_RDWR`,
/// `O_TRUNC`, `O_APPEND`, `O_CREAT`).
pub fn open_wants_write(flags: i32) -> bool {
    flags & libc::O_ACCMODE != libc::O_RDONLY
        || flags & (libc::O_TRUNC | libc::O_APPEND | libc::O_CREAT) != 0
}

/// Nanoseconds since the epoch as a `SystemTime` (negative: before it).
pub fn system_time(ns: i128) -> SystemTime {
    let secs = ns.div_euclid(1_000_000_000);
    let nanos = ns.rem_euclid(1_000_000_000) as u32;
    if secs >= 0 {
        UNIX_EPOCH + Duration::new(secs as u64, nanos)
    } else {
        // `secs` is negative and `nanos` counts forward from it.
        UNIX_EPOCH - Duration::new(secs.unsigned_abs() as u64, 0) + Duration::new(0, nanos)
    }
}

/// `stored` with every `#dir_mtime=` line's value replaced by `shown_ns`
/// (the line keeps its own line ending, or lack of one).
pub fn rewrite_dir_mtime(stored: &[u8], shown_ns: i128) -> Vec<u8> {
    let mut out = Vec::with_capacity(stored.len() + 8);
    for line in stored.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(DIR_MTIME_PREFIX) {
            out.extend_from_slice(DIR_MTIME_PREFIX);
            out.extend_from_slice(shown_ns.to_string().as_bytes());
            if line.ends_with(b"\n") {
                out.push(b'\n');
            }
        } else {
            out.extend_from_slice(line);
        }
    }
    out
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------- inodes

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Node {
    Root,
    Cat(String),
    Entry(EntryKey),
    File(EntryKey, String),
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::File(..) => Kind::File,
            _ => Kind::Dir,
        }
    }
}

#[derive(Default)]
struct Inodes {
    by_node: HashMap<Node, u64>,
    /// `nodes[ino - 1]`.
    nodes: Vec<Node>,
}

impl Inodes {
    fn new() -> Self {
        let mut t = Inodes::default();
        let root = t.intern(&Node::Root);
        debug_assert_eq!(root, ROOT_INO);
        t
    }

    fn intern(&mut self, node: &Node) -> u64 {
        if let Some(&ino) = self.by_node.get(node) {
            return ino;
        }
        self.nodes.push(node.clone());
        let ino = self.nodes.len() as u64;
        self.by_node.insert(node.clone(), ino);
        ino
    }

    fn node(&self, ino: u64) -> Option<Node> {
        let i = usize::try_from(ino.checked_sub(1)?).ok()?;
        self.nodes.get(i).cloned()
    }
}

// ----------------------------------------------------------- generations

/// The pf names of one category, in listing order.
struct CatSnap {
    pfs: Vec<String>,
    set: HashSet<String>,
}

/// One entry's stat plus its lazily read `metadata` bytes.
struct EntrySnap {
    stat: EntryStat,
    /// `None`: not read yet; `Some(None)`: the file vanished.
    metadata: Mutex<Option<Option<Arc<Vec<u8>>>>>,
}

impl EntrySnap {
    /// The files this view shows: a `metadata` file only when the stamp
    /// state says there is one.
    fn shown_files(&self) -> impl Iterator<Item = &FileMeta> {
        let absent = self.stat.metadata_stamp == MetadataStamp::Absent;
        self.stat
            .files
            .iter()
            .filter(move |f| !(absent && f.name == METADATA))
    }

    fn file(&self, name: &str) -> Option<&FileMeta> {
        self.shown_files().find(|f| f.name == name)
    }
}

#[derive(Default)]
struct GenCache {
    cats: HashMap<String, Arc<CatSnap>>,
    entries: HashMap<EntryKey, Arc<EntrySnap>>,
    cat_mtime: HashMap<String, i128>,
    root_mtime: Option<i128>,
}

/// The view of one database generation. Names and stats load lazily and
/// are cached here; a newer generation replaces the whole value.
struct Gen {
    generation: u64,
    cats: Vec<String>,
    cat_set: HashSet<String>,
    cache: Mutex<GenCache>,
}

// --------------------------------------------------------------- handles

enum Pinned {
    /// The bytes, read once at `open` (small files, the `metadata` file).
    Bytes(Arc<Vec<u8>>),
    /// A big stored file: read with `read_file_at`, validated against this
    /// size and mtime once the generation has moved on.
    Stat { len: u64, mtime_ns: i128 },
}

enum Handle {
    Dir {
        attr: Attr,
        listing: Arc<Vec<DirEnt>>,
    },
    File {
        generation: u64,
        attr: Attr,
        key: EntryKey,
        name: String,
        pinned: Pinned,
    },
}

// ------------------------------------------------------------------ view

pub struct View {
    db: Arc<dyn InstalledDb>,
    inodes: Mutex<Inodes>,
    current: Mutex<Option<Arc<Gen>>>,
    handles: Mutex<HashMap<u64, Handle>>,
    next_fh: AtomicU64,
    pin_max: usize,
}

impl View {
    pub fn new(db: Arc<dyn InstalledDb>) -> Self {
        View::with_pin_max(db, DEFAULT_PIN_MAX)
    }

    pub fn with_pin_max(db: Arc<dyn InstalledDb>, pin_max: usize) -> Self {
        View {
            db,
            inodes: Mutex::new(Inodes::new()),
            current: Mutex::new(None),
            handles: Mutex::new(HashMap::new()),
            next_fh: AtomicU64::new(1),
            pin_max,
        }
    }

    fn ino(&self, node: &Node) -> u64 {
        lock(&self.inodes).intern(node)
    }

    fn node(&self, ino: u64) -> Res<Node> {
        lock(&self.inodes).node(ino).ok_or(ViewError::NoEnt)
    }

    /// Run `f` until the generation reads the same before and after (a
    /// commit in the middle would mix two states), at most four times.
    fn stable<T>(&self, mut f: impl FnMut() -> portage_vdb::Result<T>) -> Res<(u64, T)> {
        let mut last = None;
        for _ in 0..4 {
            let before = self.db.generation()?;
            let v = f()?;
            if self.db.generation()? == before {
                return Ok((before, v));
            }
            last = Some((before, v));
        }
        Ok(last.expect("the loop ran"))
    }

    /// The current generation, rebuilt when `generation()` moved.
    fn current(&self) -> Res<Arc<Gen>> {
        let g = self.db.generation()?;
        if let Some(c) = lock(&self.current).as_ref()
            && c.generation == g
        {
            return Ok(c.clone());
        }
        let (generation, cats) = self.stable(|| self.db.categories())?;
        let new = Arc::new(Gen {
            generation,
            cat_set: cats.iter().cloned().collect(),
            cats,
            cache: Mutex::new(GenCache::default()),
        });
        *lock(&self.current) = Some(new.clone());
        Ok(new)
    }

    fn cat_snap(&self, gn: &Gen, cat: &str) -> Res<Option<Arc<CatSnap>>> {
        if !gn.cat_set.contains(cat) {
            return Ok(None);
        }
        if let Some(c) = lock(&gn.cache).cats.get(cat) {
            return Ok(Some(c.clone()));
        }
        let (_, pfs) = self.stable(|| self.db.category_entries(cat))?;
        let snap = Arc::new(CatSnap {
            set: pfs.iter().cloned().collect(),
            pfs,
        });
        Ok(Some(
            lock(&gn.cache)
                .cats
                .entry(cat.to_string())
                .or_insert(snap)
                .clone(),
        ))
    }

    fn entry_snap(&self, gn: &Gen, key: &EntryKey) -> Res<Option<Arc<EntrySnap>>> {
        let Some(cat) = self.cat_snap(gn, &key.category)? else {
            return Ok(None);
        };
        if !cat.set.contains(&key.pf) {
            return Ok(None);
        }
        if let Some(e) = lock(&gn.cache).entries.get(key) {
            return Ok(Some(e.clone()));
        }
        let Some(stat) = self.db.entry_stat(key)? else {
            return Ok(None);
        };
        let snap = Arc::new(EntrySnap {
            stat,
            metadata: Mutex::new(None),
        });
        Ok(Some(
            lock(&gn.cache)
                .entries
                .entry(key.clone())
                .or_insert(snap)
                .clone(),
        ))
    }

    /// The `metadata` bytes as this view serves them (`None`: no such
    /// file). Read once per generation and entry.
    fn metadata_bytes(&self, key: &EntryKey, entry: &EntrySnap) -> Res<Option<Arc<Vec<u8>>>> {
        if let Some(done) = lock(&entry.metadata).as_ref() {
            return Ok(done.clone());
        }
        let bytes = match (entry.stat.metadata_stamp, self.db.read_file(key, METADATA)?) {
            (MetadataStamp::Absent, _) | (_, None) => None,
            (MetadataStamp::Valid, Some(b)) => {
                Some(Arc::new(rewrite_dir_mtime(&b, entry.stat.dir_mtime_ns)))
            }
            (MetadataStamp::Stale, Some(b)) => Some(Arc::new(b)),
        };
        *lock(&entry.metadata) = Some(bytes.clone());
        Ok(bytes)
    }

    fn cat_mtime(&self, gn: &Gen, cat: &str) -> Res<i128> {
        if let Some(m) = lock(&gn.cache).cat_mtime.get(cat) {
            return Ok(*m);
        }
        let mut latest = 0i128;
        if let Some(snap) = self.cat_snap(gn, cat)? {
            for pf in &snap.pfs {
                if let Some(e) = self.entry_snap(gn, &EntryKey::new(cat, pf.as_str()))? {
                    latest = latest.max(e.stat.dir_mtime_ns);
                }
            }
        }
        lock(&gn.cache).cat_mtime.insert(cat.to_string(), latest);
        Ok(latest)
    }

    fn root_mtime(&self, gn: &Gen) -> Res<i128> {
        if let Some(m) = lock(&gn.cache).root_mtime {
            return Ok(m);
        }
        let mut latest = 0i128;
        for cat in &gn.cats {
            latest = latest.max(self.cat_mtime(gn, cat)?);
        }
        lock(&gn.cache).root_mtime = Some(latest);
        Ok(latest)
    }

    /// The attributes of `node` in `gn`; `NoEnt` when it is not there.
    fn attr_of(&self, gn: &Gen, node: &Node) -> Res<Attr> {
        let ino = self.ino(node);
        let dir = |perm: u16, nlink: u32, mtime_ns: i128| Attr {
            ino,
            kind: Kind::Dir,
            perm,
            size: 0,
            nlink,
            mtime_ns,
        };
        match node {
            Node::Root => Ok(dir(0o755, 2 + gn.cats.len() as u32, self.root_mtime(gn)?)),
            Node::Cat(cat) => {
                let snap = self.cat_snap(gn, cat)?.ok_or(ViewError::NoEnt)?;
                Ok(dir(
                    0o755,
                    2 + snap.pfs.len() as u32,
                    self.cat_mtime(gn, cat)?,
                ))
            }
            Node::Entry(key) => {
                let e = self.entry_snap(gn, key)?.ok_or(ViewError::NoEnt)?;
                Ok(dir(
                    (e.stat.dir_mode & 0o7777) as u16,
                    2,
                    e.stat.dir_mtime_ns,
                ))
            }
            Node::File(key, name) => {
                let e = self.entry_snap(gn, key)?.ok_or(ViewError::NoEnt)?;
                let meta = e.file(name).ok_or(ViewError::NoEnt)?;
                let size = if name == METADATA && e.stat.metadata_stamp == MetadataStamp::Valid {
                    match self.metadata_bytes(key, &e)? {
                        Some(b) => b.len() as u64,
                        None => return Err(ViewError::NoEnt),
                    }
                } else {
                    meta.len
                };
                Ok(Attr {
                    ino,
                    kind: Kind::File,
                    perm: (meta.mode & 0o7777) as u16,
                    size,
                    nlink: 1,
                    mtime_ns: meta.mtime_ns,
                })
            }
        }
    }

    /// The listing of a directory node, `.` and `..` first.
    fn listing_of(&self, gn: &Gen, node: &Node) -> Res<Vec<DirEnt>> {
        let me = self.ino(node);
        let parent = match node {
            Node::Root => ROOT_INO,
            Node::Cat(_) => ROOT_INO,
            Node::Entry(k) => self.ino(&Node::Cat(k.category.clone())),
            Node::File(..) => return Err(ViewError::NotDir),
        };
        let mut out = vec![
            DirEnt {
                ino: me,
                kind: Kind::Dir,
                name: ".".into(),
            },
            DirEnt {
                ino: parent,
                kind: Kind::Dir,
                name: "..".into(),
            },
        ];
        let mut push = |child: Node, name: &str| {
            out.push(DirEnt {
                ino: self.ino(&child),
                kind: child.kind(),
                name: name.to_string(),
            });
        };
        match node {
            Node::Root => {
                for cat in &gn.cats {
                    push(Node::Cat(cat.clone()), cat);
                }
            }
            Node::Cat(cat) => {
                let snap = self.cat_snap(gn, cat)?.ok_or(ViewError::NoEnt)?;
                for pf in &snap.pfs {
                    push(Node::Entry(EntryKey::new(cat.as_str(), pf.as_str())), pf);
                }
            }
            Node::Entry(key) => {
                let e = self.entry_snap(gn, key)?.ok_or(ViewError::NoEnt)?;
                for f in e.shown_files() {
                    push(Node::File(key.clone(), f.name.clone()), &f.name);
                }
            }
            Node::File(..) => unreachable!("returned above"),
        }
        Ok(out)
    }

    // ------------------------------------------------------ operations

    /// `lookup(parent, name)`.
    pub fn lookup(&self, parent: u64, name: &str) -> Res<Attr> {
        let parent = self.node(parent)?;
        let gn = self.current()?;
        let child = match &parent {
            Node::Root => Node::Cat(name.to_string()),
            Node::Cat(cat) => Node::Entry(EntryKey::new(cat.as_str(), name)),
            Node::Entry(key) => Node::File(key.clone(), name.to_string()),
            Node::File(..) => return Err(ViewError::NotDir),
        };
        // Checked in the current generation; an unknown name is `NoEnt`
        // without interning an inode for it.
        match &child {
            Node::Cat(c) if !gn.cat_set.contains(c) => return Err(ViewError::NoEnt),
            Node::Entry(k) => {
                let cat = self.cat_snap(&gn, &k.category)?.ok_or(ViewError::NoEnt)?;
                if !cat.set.contains(&k.pf) {
                    return Err(ViewError::NoEnt);
                }
            }
            Node::File(k, n) => {
                let e = self.entry_snap(&gn, k)?.ok_or(ViewError::NoEnt)?;
                if e.file(n).is_none() {
                    return Err(ViewError::NoEnt);
                }
            }
            _ => {}
        }
        self.attr_of(&gn, &child)
    }

    /// `getattr(ino)` without a handle: the current generation.
    pub fn getattr(&self, ino: u64) -> Res<Attr> {
        let node = self.node(ino)?;
        let gn = self.current()?;
        self.attr_of(&gn, &node)
    }

    /// `getattr(ino, fh)`: the attributes the handle was opened with.
    pub fn getattr_fh(&self, fh: u64) -> Res<Attr> {
        match lock(&self.handles).get(&fh) {
            Some(Handle::Dir { attr, .. } | Handle::File { attr, .. }) => Ok(attr.clone()),
            None => Err(ViewError::BadHandle),
        }
    }

    fn new_fh(&self, h: Handle) -> u64 {
        let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
        lock(&self.handles).insert(fh, h);
        fh
    }

    /// `opendir(ino)`: pins the listing of the current generation.
    pub fn opendir(&self, ino: u64) -> Res<u64> {
        let node = self.node(ino)?;
        if node.kind() != Kind::Dir {
            return Err(ViewError::NotDir);
        }
        let gn = self.current()?;
        let attr = self.attr_of(&gn, &node)?;
        let listing = Arc::new(self.listing_of(&gn, &node)?);
        Ok(self.new_fh(Handle::Dir { attr, listing }))
    }

    /// The pinned listing of a directory handle; the adapter serves offset
    /// `n` as `listing[n..]` and numbers an entry's next offset `index + 1`.
    pub fn readdir(&self, fh: u64) -> Res<Arc<Vec<DirEnt>>> {
        match lock(&self.handles).get(&fh) {
            Some(Handle::Dir { listing, .. }) => Ok(listing.clone()),
            Some(Handle::File { .. }) => Err(ViewError::NotDir),
            None => Err(ViewError::BadHandle),
        }
    }

    /// `open(ino, flags)`. A write-class open is `EROFS` (after the
    /// existence and type checks, as a read-only filesystem answers).
    pub fn open(&self, ino: u64, write: bool) -> Res<u64> {
        let node = self.node(ino)?;
        let gn = self.current()?;
        let attr = self.attr_of(&gn, &node)?;
        let Node::File(key, name) = node else {
            return Err(ViewError::IsDir);
        };
        if write {
            return Err(ViewError::Rofs);
        }
        let entry = self.entry_snap(&gn, &key)?.ok_or(ViewError::NoEnt)?;
        let (pinned, attr) = if name == METADATA {
            let bytes = self.metadata_bytes(&key, &entry)?.ok_or(ViewError::NoEnt)?;
            let attr = Attr {
                size: bytes.len() as u64,
                ..attr
            };
            (Pinned::Bytes(bytes), attr)
        } else if attr.size <= self.pin_max as u64 {
            let bytes = self.db.read_file(&key, &name)?.ok_or(ViewError::NoEnt)?;
            let attr = Attr {
                size: bytes.len() as u64,
                ..attr
            };
            (Pinned::Bytes(Arc::new(bytes)), attr)
        } else {
            (
                Pinned::Stat {
                    len: attr.size,
                    mtime_ns: attr.mtime_ns,
                },
                attr,
            )
        };
        Ok(self.new_fh(Handle::File {
            generation: gn.generation,
            attr,
            key,
            name,
            pinned,
        }))
    }

    /// `read(fh, offset, size)`: up to `size` bytes from `offset`, short at
    /// the end of the file.
    pub fn read(&self, fh: u64, off: u64, size: usize) -> Res<Vec<u8>> {
        // Copy what is needed so no lock is held across a backend call.
        let (generation, key, name, pinned) = match lock(&self.handles).get(&fh) {
            Some(Handle::File {
                generation,
                key,
                name,
                pinned,
                ..
            }) => (
                *generation,
                key.clone(),
                name.clone(),
                match pinned {
                    Pinned::Bytes(b) => Pinned::Bytes(b.clone()),
                    Pinned::Stat { len, mtime_ns } => Pinned::Stat {
                        len: *len,
                        mtime_ns: *mtime_ns,
                    },
                },
            ),
            Some(Handle::Dir { .. }) => return Err(ViewError::IsDir),
            None => return Err(ViewError::BadHandle),
        };
        match pinned {
            Pinned::Bytes(b) => {
                let start = usize::try_from(off).unwrap_or(usize::MAX).min(b.len());
                let end = start.saturating_add(size).min(b.len());
                Ok(b[start..end].to_vec())
            }
            Pinned::Stat { len, mtime_ns } => {
                if self.db.generation()? != generation {
                    match self.db.file_meta(&key, &name)? {
                        Some(m) if m.len == len && m.mtime_ns == mtime_ns => {}
                        _ => return Err(ViewError::Stale),
                    }
                }
                self.db
                    .read_file_at(&key, &name, off, size)?
                    .ok_or(ViewError::Stale)
            }
        }
    }

    /// `release` / `releasedir`; an unknown handle is ignored.
    pub fn release(&self, fh: u64) {
        lock(&self.handles).remove(&fh);
    }

    /// The generation a file handle was opened on.
    #[cfg(test)]
    pub fn handle_generation(&self, fh: u64) -> Option<u64> {
        match lock(&self.handles).get(&fh) {
            Some(Handle::File { generation, .. }) => Some(*generation),
            _ => None,
        }
    }

    /// Whether a file handle serves pinned bytes (`false`: `read_file_at`).
    #[cfg(test)]
    pub fn handle_is_bytes(&self, fh: u64) -> bool {
        matches!(
            lock(&self.handles).get(&fh),
            Some(Handle::File {
                pinned: Pinned::Bytes(_),
                ..
            })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_vdb::{EntryFile, EntryImage, FilesDb};
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::MetadataExt as _;
    use std::path::{Path, PathBuf};

    struct Env {
        label: &'static str,
        /// The `files` ROOT every database was converted from; its tree is
        /// the oracle for the attributes.
        root: PathBuf,
        db: Arc<dyn InstalledDb>,
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("portuale-vdbview-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn cp_a(from: &Path, to: &Path) {
        let st = std::process::Command::new("cp")
            .arg("-a")
            .arg(from)
            .arg(to)
            .status()
            .unwrap();
        assert!(st.success(), "cp -a failed");
    }

    fn key(cat: &str, pf: &str) -> EntryKey {
        EntryKey::new(cat, pf)
    }

    /// Add a live entry (no stamp) with these files.
    fn add_entry(db: &dyn InstalledDb, k: &EntryKey, files: &[(&str, &[u8])]) {
        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(k).unwrap();
        for (name, data) in files {
            txn.put_entry_file(k, name, data).unwrap();
        }
        txn.finish_entry(k).unwrap();
        txn.commit().unwrap();
    }

    /// Add a live entry with a freshly stamped `metadata` file.
    fn add_stamped_entry(db: &dyn InstalledDb, k: &EntryKey) {
        let mut txn = db.begin_write().unwrap();
        txn.begin_entry(k).unwrap();
        for (name, data) in [
            ("SLOT", &b"0\n"[..]),
            ("EAPI", b"8\n"),
            ("USE", b"abi_x86_64 ssl\n"),
            ("RDEPEND", b"dev-libs/foo\n"),
        ] {
            txn.put_entry_file(k, name, data).unwrap();
        }
        txn.put_entry_file(k, "CONTENTS", b"obj /usr/bin/x 0 0\n")
            .unwrap();
        txn.seal_entry(k).unwrap();
        txn.finish_entry(k).unwrap();
        txn.commit().unwrap();
    }

    /// A files ROOT with the fixture var tree, one stamped entry and the
    /// fixture's stale-stamp entry, plus a sqlite and a redb copy of it.
    fn envs(tag: &str) -> Vec<Env> {
        let root = scratch(tag);
        cp_a(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/var"),
            &root.join("var"),
        );
        let files = FilesDb::new(&root);
        add_stamped_entry(&files, &key("app-misc", "stamped-1.0"));
        let mut out = vec![Env {
            label: "files",
            root: root.clone(),
            db: Arc::new(files),
        }];
        let convert = |kind: &str, file: &str| {
            let args: Vec<String> = [
                "convert",
                "--from",
                &format!("files:{}", root.display()),
                "--to",
                &format!("{kind}:{}", root.join(file).display()),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let (mut o, mut e) = (Vec::new(), Vec::new());
            let code = crate::vdb_cmd::run_args(&args, &mut o, &mut e);
            assert_eq!(code, 0, "{}", String::from_utf8_lossy(&e));
        };
        #[cfg(feature = "vdb-sqlite")]
        {
            convert("sqlite", "vdb.sqlite");
            out.push(Env {
                label: "sqlite",
                root: root.clone(),
                db: Arc::new(portage_vdb::SqliteDb::open(root.join("vdb.sqlite")).unwrap()),
            });
        }
        #[cfg(feature = "vdb-redb")]
        {
            convert("redb", "vdb.redb");
            out.push(Env {
                label: "redb",
                root: root.clone(),
                db: Arc::new(portage_vdb::RedbDb::open(root.join("vdb.redb")).unwrap()),
            });
        }
        let _ = &convert;
        out
    }

    /// The database backends (their `generation` counter moves on every
    /// commit; `files` only moves it with a directory mtime).
    fn db_envs(tag: &str) -> Vec<Env> {
        envs(tag)
            .into_iter()
            .filter(|e| e.label != "files")
            .collect()
    }

    fn vdb_dir(env: &Env) -> PathBuf {
        env.root.join("var/db/pkg")
    }

    fn mtime_ns(p: &Path) -> i128 {
        let m = fs::metadata(p).unwrap();
        m.mtime() as i128 * 1_000_000_000 + m.mtime_nsec() as i128
    }

    fn names(listing: &[DirEnt]) -> Vec<&str> {
        listing
            .iter()
            .map(|e| e.name.as_str())
            .filter(|n| *n != "." && *n != "..")
            .collect()
    }

    fn list(v: &View, ino: u64) -> Arc<Vec<DirEnt>> {
        let fh = v.opendir(ino).unwrap();
        let l = v.readdir(fh).unwrap();
        v.release(fh);
        l
    }

    fn read_all(v: &View, ino: u64) -> Vec<u8> {
        let fh = v.open(ino, false).unwrap();
        let out = v.read(fh, 0, 1 << 24).unwrap();
        v.release(fh);
        out
    }

    /// Real `_read_metadata_file`'s acceptance rule (`vartree.py:157-185`):
    /// `#format=1` present, `#dir_mtime=<int>` present, equal to the
    /// directory's `st_mtime_ns`.
    fn real_reader_accepts(bytes: &[u8], dir_mtime_ns: i128) -> bool {
        let text = std::str::from_utf8(bytes).unwrap();
        let (mut version, mut stamp) = (None, None);
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("#format=") {
                match v.trim().parse::<u32>() {
                    Ok(1) => version = Some(1),
                    _ => return false,
                }
            } else if let Some(v) = line.strip_prefix("#dir_mtime=") {
                match v.trim().parse::<i128>() {
                    Ok(n) => stamp = Some(n),
                    Err(_) => return false,
                }
            }
        }
        version.is_some() && stamp == Some(dir_mtime_ns)
    }

    #[test]
    fn tree_listing_and_attributes_match_the_files_tree() {
        for env in envs("tree") {
            let v = View::new(env.db.clone());
            let label = env.label;
            let root_attr = v.getattr(ROOT_INO).unwrap();
            assert_eq!(root_attr.kind, Kind::Dir, "{label}");
            assert_eq!(root_attr.perm, 0o755, "{label}");
            let want_cats: BTreeSet<String> = fs::read_dir(vdb_dir(&env))
                .unwrap()
                .map(|e| e.unwrap())
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().into_string().unwrap())
                .collect();
            let root_list = list(&v, ROOT_INO);
            assert_eq!(
                names(&root_list),
                env.db
                    .categories()
                    .unwrap()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                "{label}: backend order"
            );
            let got: BTreeSet<String> = names(&root_list).iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want_cats, "{label}: categories");

            let mut root_latest = 0i128;
            let mut entries = 0;
            for cat in &want_cats {
                let cat_attr = v.lookup(ROOT_INO, cat).unwrap();
                let cat_list = list(&v, cat_attr.ino);
                let want_pfs: BTreeSet<String> = fs::read_dir(vdb_dir(&env).join(cat))
                    .unwrap()
                    .map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect();
                let got: BTreeSet<String> =
                    names(&cat_list).iter().map(|s| s.to_string()).collect();
                assert_eq!(got, want_pfs, "{label}: {cat}");
                let mut cat_latest = 0i128;
                for pf in &want_pfs {
                    entries += 1;
                    let dir = vdb_dir(&env).join(cat).join(pf);
                    let e_attr = v.lookup(cat_attr.ino, pf).unwrap();
                    assert_eq!(e_attr.kind, Kind::Dir);
                    assert_eq!(e_attr.mtime_ns, mtime_ns(&dir), "{label}: {cat}/{pf} mtime");
                    assert_eq!(
                        u32::from(e_attr.perm),
                        fs::metadata(&dir).unwrap().mode() & 0o7777,
                        "{label}: {cat}/{pf} mode"
                    );
                    cat_latest = cat_latest.max(e_attr.mtime_ns);
                    let e_list = list(&v, e_attr.ino);
                    let want_files: BTreeSet<String> = fs::read_dir(&dir)
                        .unwrap()
                        .map(|e| e.unwrap().file_name().into_string().unwrap())
                        .collect();
                    let got: BTreeSet<String> =
                        names(&e_list).iter().map(|s| s.to_string()).collect();
                    assert_eq!(got, want_files, "{label}: {cat}/{pf} files");
                    for f in &want_files {
                        let path = dir.join(f);
                        let st = fs::metadata(&path).unwrap();
                        let a = v.lookup(e_attr.ino, f).unwrap();
                        assert_eq!(a.kind, Kind::File);
                        assert_eq!(
                            u32::from(a.perm),
                            st.mode() & 0o7777,
                            "{label}: {cat}/{pf}/{f} mode"
                        );
                        assert_eq!(a.mtime_ns, mtime_ns(&path), "{label}: {cat}/{pf}/{f}");
                        let ent = e_list.iter().find(|d| d.name == *f).unwrap();
                        assert_eq!(ent.ino, a.ino, "readdir and lookup agree on the inode");
                        if f == "metadata" {
                            continue; // checked in the metadata tests
                        }
                        assert_eq!(a.size, st.len(), "{label}: {cat}/{pf}/{f} size");
                        assert_eq!(
                            read_all(&v, a.ino),
                            fs::read(&path).unwrap(),
                            "{label}: {cat}/{pf}/{f} bytes"
                        );
                    }
                }
                assert_eq!(cat_attr.mtime_ns, cat_latest, "{label}: {cat} mtime");
                assert_eq!(cat_attr.nlink, 2 + want_pfs.len() as u32);
                root_latest = root_latest.max(cat_latest);
            }
            assert!(entries > 50, "the fixture is not trivial");
            assert_eq!(root_attr.mtime_ns, root_latest, "{label}: root mtime");
            assert!(root_latest > 0);
            // Not found, at every level.
            assert_eq!(v.lookup(ROOT_INO, "no-such-cat"), Err(ViewError::NoEnt));
            let c = v.lookup(ROOT_INO, "dev-libs").unwrap().ino;
            assert_eq!(v.lookup(c, "no-such-pf"), Err(ViewError::NoEnt));
            let e = v.lookup(c, "oldmovepkg-1.0").unwrap().ino;
            assert_eq!(v.lookup(e, "NOPE"), Err(ViewError::NoEnt));
        }
    }

    #[test]
    fn a_valid_stamp_is_rewritten_to_the_mtime_shown_and_the_real_reader_accepts_it() {
        for env in envs("stamp") {
            let label = env.label;
            let v = View::new(env.db.clone());
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap().ino;
            let e = v.lookup(cat, "stamped-1.0").unwrap();
            let m = v.lookup(e.ino, "metadata").unwrap();
            let bytes = read_all(&v, m.ino);
            assert_eq!(
                m.size,
                bytes.len() as u64,
                "{label}: size is the served size"
            );
            assert!(bytes.starts_with(b"#format=1\n"), "{label}");
            assert!(
                real_reader_accepts(&bytes, e.mtime_ns),
                "{label}: stamp must equal the shown entry mtime {}",
                e.mtime_ns
            );
            // The files tree itself holds the same fact, so a mount of the
            // files backend is a faithful pass-through.
            let on_disk = fs::read(vdb_dir(&env).join("app-misc/stamped-1.0/metadata")).unwrap();
            assert!(real_reader_accepts(&on_disk, e.mtime_ns), "{label}");
            // The view stays a snapshot of one consistent state: the same
            // mtime and the same bytes through `getattr` of an open handle.
            let fh = v.open(m.ino, false).unwrap();
            assert_eq!(v.getattr_fh(fh).unwrap().size, bytes.len() as u64);
            v.release(fh);
        }
    }

    #[test]
    fn a_stale_stamp_stays_stale_and_an_absent_one_has_no_file() {
        for env in envs("stale") {
            let label = env.label;
            let v = View::new(env.db.clone());
            let cat = v.lookup(ROOT_INO, "dev-libs").unwrap().ino;
            let e = v.lookup(cat, "stalesnapshot-1.0").unwrap();
            let m = v.lookup(e.ino, "metadata").unwrap();
            let stored =
                fs::read(vdb_dir(&env).join("dev-libs/stalesnapshot-1.0/metadata")).unwrap();
            assert!(
                !real_reader_accepts(&stored, e.mtime_ns),
                "{label}: the fixture stamp does not match"
            );
            let bytes = read_all(&v, m.ino);
            assert_eq!(bytes, stored, "{label}: served exactly as stored");
            assert_eq!(m.size, stored.len() as u64, "{label}");
            assert!(!real_reader_accepts(&bytes, e.mtime_ns), "{label}");
            // An entry without a metadata file shows none.
            let plain = v.lookup(cat, "oldmovepkg-1.0").unwrap();
            assert_eq!(v.lookup(plain.ino, "metadata"), Err(ViewError::NoEnt));
            assert!(
                !names(&list(&v, plain.ino)).contains(&"metadata"),
                "{label}"
            );
        }
    }

    #[cfg(any(feature = "vdb-sqlite", feature = "vdb-redb"))]
    #[test]
    fn a_stored_stamp_that_differs_from_the_shown_mtime_is_rewritten() {
        // A database may hold a Valid stamp whose bytes carry another
        // number than the directory mtime it stores (an import from a
        // tree whose directory mtime was touched afterwards). The view
        // shows the stored mtime and rewrites the stamp to it.
        for env in db_envs("rewrite") {
            let k = key("app-misc", "restamp-1");
            let shown = 1_700_000_000_123_456_789i128;
            let file = |name: &str, data: &[u8]| EntryFile {
                meta: portage_vdb::FileMeta {
                    name: name.into(),
                    len: data.len() as u64,
                    mode: 0o644,
                    mtime_ns: 5,
                },
                data: data.to_vec(),
            };
            let image = EntryImage {
                key: k.clone(),
                files: vec![
                    file("SLOT", b"0\n"),
                    file("metadata", b"#format=1\nSLOT=0\n#dir_mtime=1\n"),
                ],
                dir_mode: 0o755,
                dir_mtime_ns: shown,
                metadata_stamp: portage_vdb::MetadataStamp::Valid,
            };
            let mut txn = env.db.begin_write().unwrap();
            txn.insert_entry(&image).unwrap();
            txn.commit().unwrap();
            let v = View::new(env.db.clone());
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap().ino;
            let e = v.lookup(cat, "restamp-1").unwrap();
            assert_eq!(e.mtime_ns, shown);
            let m = v.lookup(e.ino, "metadata").unwrap();
            let bytes = read_all(&v, m.ino);
            assert_eq!(
                bytes, b"#format=1\nSLOT=0\n#dir_mtime=1700000000123456789\n",
                "{}",
                env.label
            );
            assert_eq!(m.size, bytes.len() as u64, "size follows the rewrite");
            assert!(real_reader_accepts(&bytes, e.mtime_ns));
        }
    }

    #[test]
    fn rewrite_and_time_helpers() {
        assert_eq!(
            rewrite_dir_mtime(b"#format=1\nA=b\n#dir_mtime=11\n", 2222),
            b"#format=1\nA=b\n#dir_mtime=2222\n"
        );
        // No trailing newline stays without one; other lines untouched; a
        // value that merely contains the prefix is not a stamp line.
        assert_eq!(
            rewrite_dir_mtime(b"X=#dir_mtime=1\n#dir_mtime=5", 7),
            b"X=#dir_mtime=1\n#dir_mtime=7"
        );
        assert_eq!(rewrite_dir_mtime(b"", 1), b"");
        assert_eq!(system_time(0), UNIX_EPOCH);
        assert_eq!(
            system_time(1_700_000_000_123_456_789),
            UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789)
        );
        assert_eq!(system_time(-1), UNIX_EPOCH - Duration::from_nanos(1));
        assert_eq!(
            system_time(-1_500_000_000),
            UNIX_EPOCH - Duration::from_millis(1500)
        );
    }

    #[test]
    fn offset_reads_cross_chunk_boundaries() {
        let pattern: Vec<u8> = (0..200_001u32)
            .map(|i| (i.wrapping_mul(131) / 7) as u8)
            .collect();
        for env in envs("offset") {
            let k = key("app-misc", "bigfile-1");
            add_entry(&*env.db, &k, &[("BIG", &pattern), ("SLOT", b"0\n")]);
            let v = View::with_pin_max(env.db.clone(), 1024);
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap().ino;
            let e = v.lookup(cat, "bigfile-1").unwrap().ino;
            let big = v.lookup(e, "BIG").unwrap();
            assert_eq!(big.size, pattern.len() as u64, "{}", env.label);
            let fh = v.open(big.ino, false).unwrap();
            assert!(!v.handle_is_bytes(fh), "a big file is not loaded whole");
            let n = pattern.len() as u64;
            for off in [
                0,
                1,
                4095,
                4096,
                65_535,
                65_536,
                65_537,
                131_071,
                131_072,
                199_999,
                n - 1,
                n,
                n + 10,
            ] {
                for len in [1usize, 2, 4096, 65_536, 70_000, 1 << 20] {
                    let got = v.read(fh, off, len).unwrap();
                    let start = (off as usize).min(pattern.len());
                    let end = (start + len).min(pattern.len());
                    assert_eq!(
                        got,
                        pattern[start..end],
                        "{} off {off} len {len}",
                        env.label
                    );
                }
            }
            v.release(fh);
            assert_eq!(v.read(fh, 0, 1), Err(ViewError::BadHandle));
            // A small file is pinned whole at open.
            let slot = v.lookup(e, "SLOT").unwrap();
            let fh = v.open(slot.ino, false).unwrap();
            assert!(v.handle_is_bytes(fh));
            assert_eq!(v.read(fh, 0, 100).unwrap(), b"0\n");
            assert_eq!(v.read(fh, 1, 100).unwrap(), b"\n");
            assert_eq!(v.read(fh, 2, 100).unwrap(), b"");
        }
    }

    #[test]
    fn a_handle_keeps_the_generation_it_opened() {
        for env in db_envs("pin") {
            let label = env.label;
            let v = View::with_pin_max(env.db.clone(), 64);
            let big_old = vec![b'o'; 5000];
            let big_new = vec![b'n'; 6000];
            let k = key("app-misc", "pin-1");
            add_entry(
                &*env.db,
                &k,
                &[("CONTENTS", b"old contents\n"), ("BIG", &big_old)],
            );
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap().ino;
            let e = v.lookup(cat, "pin-1").unwrap().ino;
            let contents = v.lookup(e, "CONTENTS").unwrap();
            let big = v.lookup(e, "BIG").unwrap();

            // Open a small file, a big file and a directory on generation G.
            let fh_small = v.open(contents.ino, false).unwrap();
            let fh_big = v.open(big.ino, false).unwrap();
            let fh_dir = v.opendir(cat).unwrap();
            let g0 = v.handle_generation(fh_small).unwrap();
            let listing0 = v.readdir(fh_dir).unwrap();
            assert!(v.handle_is_bytes(fh_small) && !v.handle_is_bytes(fh_big));

            // An unrelated commit: a new entry.
            add_entry(&*env.db, &key("app-misc", "pin-2"), &[("SLOT", b"0\n")]);
            assert_ne!(
                env.db.generation().unwrap(),
                g0,
                "{label}: the commit moved it"
            );
            assert_eq!(v.read(fh_small, 0, 100).unwrap(), b"old contents\n");
            assert_eq!(
                v.read(fh_big, 4000, 100).unwrap(),
                vec![b'o'; 100],
                "{label}: the big file did not change, the handle keeps reading it"
            );
            assert_eq!(
                *v.readdir(fh_dir).unwrap(),
                *listing0,
                "{label}: pinned listing"
            );
            assert!(!names(&listing0).contains(&"pin-2"));
            // A new lookup and a new opendir see the new generation.
            assert!(v.lookup(cat, "pin-2").is_ok(), "{label}");
            assert!(names(&list(&v, cat)).contains(&"pin-2"), "{label}");
            // The category mtime follows the new entry.
            let new_e = v.lookup(cat, "pin-2").unwrap();
            assert_eq!(
                v.getattr(cat).unwrap().mtime_ns,
                v.getattr(e).unwrap().mtime_ns.max(new_e.mtime_ns)
            );

            // Now rewrite both files.
            let mut txn = env.db.begin_write().unwrap();
            txn.replace_file(&k, "CONTENTS", b"new contents, longer\n")
                .unwrap();
            txn.replace_file(&k, "BIG", &big_new).unwrap();
            txn.commit().unwrap();
            // The old small handle still serves the old bytes and attrs.
            assert_eq!(
                v.read(fh_small, 0, 100).unwrap(),
                b"old contents\n",
                "{label}"
            );
            assert_eq!(v.getattr_fh(fh_small).unwrap().size, 13);
            // The old big handle cannot serve the old bytes any more and
            // says so instead of returning new ones.
            assert_eq!(v.read(fh_big, 0, 100), Err(ViewError::Stale), "{label}");
            // New lookups and opens see the new generation.
            let c2 = v.lookup(e, "CONTENTS").unwrap();
            assert_eq!(c2.size, 21, "{label}");
            assert_eq!(read_all(&v, c2.ino), b"new contents, longer\n");
            assert_eq!(c2.ino, contents.ino, "same inode across the generation");
            let b2 = v.lookup(e, "BIG").unwrap();
            assert_eq!(b2.size, 6000);
            let fh = v.open(b2.ino, false).unwrap();
            assert_eq!(v.read(fh, 5990, 100).unwrap(), vec![b'n'; 10]);

            // Deleting the entry: the old big handle is stale, the small
            // one still has its bytes, a lookup says ENOENT.
            let mut txn = env.db.begin_write().unwrap();
            txn.delete_entry(&k).unwrap();
            txn.commit().unwrap();
            assert_eq!(v.read(fh, 0, 1), Err(ViewError::Stale), "{label}");
            assert_eq!(v.read(fh_small, 0, 3).unwrap(), b"old");
            assert_eq!(v.lookup(cat, "pin-1"), Err(ViewError::NoEnt));
            assert_eq!(v.lookup(e, "CONTENTS"), Err(ViewError::NoEnt));
            assert_eq!(v.getattr(contents.ino), Err(ViewError::NoEnt));
        }
    }

    #[test]
    fn inodes_are_stable_across_generations_and_unique() {
        for env in db_envs("ino") {
            let label = env.label;
            let v = View::new(env.db.clone());
            let walk = |v: &View| -> Vec<(String, u64)> {
                let mut out = vec![("/".to_string(), ROOT_INO)];
                for c in list(v, ROOT_INO)
                    .iter()
                    .filter(|d| d.name != "." && d.name != "..")
                {
                    out.push((c.name.clone(), c.ino));
                    for e in list(v, c.ino)
                        .iter()
                        .filter(|d| d.name != "." && d.name != "..")
                    {
                        out.push((format!("{}/{}", c.name, e.name), e.ino));
                        for f in list(v, e.ino)
                            .iter()
                            .filter(|d| d.name != "." && d.name != "..")
                        {
                            out.push((format!("{}/{}/{}", c.name, e.name, f.name), f.ino));
                        }
                    }
                }
                out
            };
            let before = walk(&v);
            let unique: BTreeSet<u64> = before.iter().map(|(_, i)| *i).collect();
            assert_eq!(unique.len(), before.len(), "{label}: one inode per path");
            assert_eq!(before[0].1, 1);

            // A commit in another category and a rewrite elsewhere.
            add_entry(&*env.db, &key("zz-new", "fresh-1"), &[("SLOT", b"0\n")]);
            let after = walk(&v);
            for (path, ino) in &before {
                let now = after.iter().find(|(p, _)| p == path).map(|(_, i)| *i);
                assert_eq!(now, Some(*ino), "{label}: {path} keeps its inode");
            }
            let fresh = after.iter().find(|(p, _)| p == "zz-new/fresh-1").unwrap().1;
            assert!(
                !unique.contains(&fresh),
                "{label}: a new path gets a new inode"
            );
            // `.` and `..`.
            let cat = v.lookup(ROOT_INO, "zz-new").unwrap().ino;
            let e = v.lookup(cat, "fresh-1").unwrap().ino;
            let l = list(&v, e);
            assert_eq!((l[0].name.as_str(), l[0].ino), (".", e));
            assert_eq!((l[1].name.as_str(), l[1].ino), ("..", cat));
            assert_eq!(list(&v, ROOT_INO)[1].ino, ROOT_INO);
            assert_eq!(list(&v, cat)[1].ino, ROOT_INO);

            // Deleted and published again: same inode.
            let mut txn = env.db.begin_write().unwrap();
            txn.delete_entry(&key("zz-new", "fresh-1")).unwrap();
            txn.commit().unwrap();
            assert_eq!(v.getattr(e), Err(ViewError::NoEnt), "{label}");
            add_entry(&*env.db, &key("zz-new", "fresh-1"), &[("SLOT", b"1\n")]);
            assert_eq!(v.lookup(cat, "fresh-1").unwrap().ino, e, "{label}");
            // An unknown inode is ENOENT.
            assert_eq!(v.getattr(9_999_999), Err(ViewError::NoEnt));
            assert_eq!(v.getattr(0), Err(ViewError::NoEnt));
        }
    }

    #[test]
    fn every_write_is_erofs_and_errors_map_to_errno() {
        for op in [
            WriteOp::Create,
            WriteOp::Mknod,
            WriteOp::Mkdir,
            WriteOp::Unlink,
            WriteOp::Rmdir,
            WriteOp::Rename,
            WriteOp::Symlink,
            WriteOp::Link,
            WriteOp::Setattr,
            WriteOp::Write,
            WriteOp::Setxattr,
            WriteOp::Removexattr,
            WriteOp::Fallocate,
            WriteOp::CopyFileRange,
        ] {
            assert_eq!(deny(op).errno(), libc::EROFS, "{op:?}");
        }
        assert!(!open_wants_write(libc::O_RDONLY));
        assert!(!open_wants_write(libc::O_RDONLY | libc::O_NONBLOCK));
        for f in [
            libc::O_WRONLY,
            libc::O_RDWR,
            libc::O_RDONLY | libc::O_TRUNC,
            libc::O_RDONLY | libc::O_APPEND,
            libc::O_RDONLY | libc::O_CREAT,
        ] {
            assert!(open_wants_write(f), "{f:#x}");
        }
        for (e, n) in [
            (ViewError::NoEnt, libc::ENOENT),
            (ViewError::NotDir, libc::ENOTDIR),
            (ViewError::IsDir, libc::EISDIR),
            (ViewError::BadHandle, libc::EBADF),
            (ViewError::Rofs, libc::EROFS),
            (ViewError::Stale, libc::ESTALE),
            (ViewError::Io("x".into()), libc::EIO),
        ] {
            assert_eq!(e.errno(), n);
        }

        for env in envs("erofs") {
            let v = View::new(env.db.clone());
            let cat = v.lookup(ROOT_INO, "dev-libs").unwrap().ino;
            let e = v.lookup(cat, "oldmovepkg-1.0").unwrap().ino;
            let f = v.lookup(e, "SLOT").unwrap().ino;
            assert_eq!(v.open(f, true), Err(ViewError::Rofs), "{}", env.label);
            assert!(v.open(f, false).is_ok());
            // Type and existence errors keep their own errno.
            assert_eq!(v.open(e, false), Err(ViewError::IsDir));
            assert_eq!(v.open(e, true), Err(ViewError::IsDir));
            assert_eq!(v.opendir(f), Err(ViewError::NotDir));
            assert_eq!(v.lookup(f, "x"), Err(ViewError::NotDir));
            assert_eq!(v.read(12345, 0, 1), Err(ViewError::BadHandle));
            assert_eq!(v.readdir(12345), Err(ViewError::BadHandle));
            assert_eq!(v.getattr_fh(12345), Err(ViewError::BadHandle));
            let dfh = v.opendir(e).unwrap();
            assert_eq!(v.read(dfh, 0, 1), Err(ViewError::IsDir));
            let ffh = v.open(f, false).unwrap();
            assert_eq!(v.readdir(ffh), Err(ViewError::NotDir));
            v.release(dfh);
            v.release(dfh); // a second release is harmless
        }
    }
}
