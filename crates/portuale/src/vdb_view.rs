// The read-only filesystem view of an installed-package database.
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
// Attributes: entry directory mode and mtime are the stored ones; a
// category shows the latest of its entries, the root the latest category;
// other directories are 0755; files show the stored mode, size and mtime.
// FUSE carries nanoseconds, so a shown mtime is exact.
//
// The `metadata` file follows the stamp state stored with the entry
// (`MetadataStamp`):
//   - Valid  -> the stored bytes with every `#dir_mtime=` line rewritten to
//     the entry directory mtime this view shows. The size is that of
//     the rewritten bytes.
//   - Stale  -> served exactly as stored; it does not match and stays stale.
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
//     The check runs on every read (an in-place rewrite on `files` does
//     not move the generation). Residue, by the nature of a stat: a
//     rewrite with the same size *and* the same mtime is not noticed.

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
    /// A write the read-write layer (`vdb_rw`) does not map.
    Perm,
    /// `rmdir` of a directory that still has children.
    NotEmpty,
    /// The name exists already (`mkdir`, `O_EXCL`, `link`).
    Exists,
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
            ViewError::Perm => libc::EPERM,
            ViewError::NotEmpty => libc::ENOTEMPTY,
            ViewError::Exists => libc::EEXIST,
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
pub(crate) enum Node {
    Root,
    Cat(String),
    Entry(EntryKey),
    File(EntryKey, String),
    /// An object of the read-write layer (`vdb_rw`): a volatile
    /// file, a staged directory or file. This view never serves it.
    Obj(u64),
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::File(..) | Node::Obj(_) => Kind::File,
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

    /// Make `ino` stand for `node` (a rename that keeps the inode).
    fn rebind(&mut self, ino: u64, node: Node) {
        let Some(i) = ino.checked_sub(1).and_then(|i| usize::try_from(i).ok()) else {
            return;
        };
        if let Some(old) = self.nodes.get(i).cloned() {
            if self.by_node.get(&old) == Some(&ino) {
                self.by_node.remove(&old);
            }
            self.nodes[i] = node.clone();
            self.by_node.insert(node, ino);
        }
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
        /// The generation the handle opened on; only the tests ask.
        #[cfg_attr(not(test), allow(dead_code))]
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

    /// The inode of `node`, interned on first use (stable, never reused).
    pub(crate) fn ino_of(&self, node: &Node) -> u64 {
        self.ino(node)
    }

    /// The node an inode stands for.
    pub(crate) fn node_of(&self, ino: u64) -> Res<Node> {
        self.node(ino)
    }

    /// Make `ino` stand for `node` from now on (the read-write layer's
    /// publish keeps the staged directory's inode, as a rename does).
    pub(crate) fn rebind(&self, ino: u64, node: Node) {
        lock(&self.inodes).rebind(ino, node);
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
            Node::Obj(_) => Err(ViewError::NoEnt),
            Node::File(key, name) => {
                let e = self.entry_snap(gn, key)?.ok_or(ViewError::NoEnt)?;
                let snap_meta = e.file(name).ok_or(ViewError::NoEnt)?;
                // `files`: an in-place `replace_file` moves neither the
                // directory mtimes nor the generation, so the cached
                // stat of an ordinary field file is re-read (one stat).
                // `metadata` is served from its own rewritten bytes.
                let fresh;
                let meta = if name == METADATA {
                    snap_meta
                } else {
                    match self.db.file_meta(key, name)? {
                        Some(m) => {
                            fresh = m;
                            &fresh
                        }
                        None => return Err(ViewError::NoEnt),
                    }
                };
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
            Node::File(..) | Node::Obj(_) => return Err(ViewError::NotDir),
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
            Node::File(..) | Node::Obj(_) => unreachable!("returned above"),
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
            Node::File(..) | Node::Obj(_) => return Err(ViewError::NotDir),
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
}
