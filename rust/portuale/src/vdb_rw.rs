// The read-write layer of `portuale vdb mount --rw` (backlog #317): what
// real Portage writes into `var/db/pkg` while it merges and unmerges,
// mapped onto database transactions. Design:
// `docs/superpowers/specs/2026-10-05-rw-fuse-vdb-design.md`; plan:
// `docs/02.317-rw-fuse.opus.md`; the real operation sequences:
// `docs/evidence/317-s0-capture.md`.
//
// `RwView` wraps the read-only `vdb_view::View` and answers first from
// its own objects, then from the view:
//
// - volatile names (memory only, never stored): slot lock files
//   (`<cat>/.<pn>:<slot>.portage_lockfile`), their hardlink checks
//   (`..*.portage_lockfile.hardlock-*`), and categories created by
//   `mkdir` that hold no entry yet (S1);
// - staged `<cat>/-MERGING-<pf>/` directories on scratch disk, published
//   with one `insert_entry` transaction at the rename (S2);
// - rewrites and removals inside live entries (S3).
//
// Every object gets its inode from the view's one table (`Node::Obj`), so
// inodes stay unique and a rename keeps its inode. Like `vdb_view`, this
// file holds every decision and is unit tested without a mount;
// `vdb_fuse.rs` only translates types.

use std::collections::{BTreeMap, HashMap};
use std::os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use portage_vdb::{Counter, EntryFile, EntryImage, EntryKey, FileMeta, InstalledDb, MetadataStamp};

use crate::vdb_view::{Attr, DirEnt, Kind, Node, ROOT_INO, View, ViewError, open_wants_write};

type Res<T> = Result<T, ViewError>;

/// Handles of this layer are numbered from here, above the view's own.
const FH_BASE: u64 = 1 << 48;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ns() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i128)
}

/// The names real Portage's lock code creates next to entries
/// (`locks.py` `lockfile` with `wantnewlockfile`, `hardlock_name`,
/// `_lockfile_was_removed`): kept in memory only.
pub(crate) fn is_volatile_name(name: &str) -> bool {
    name.starts_with('.')
        && (name.ends_with(".portage_lockfile")
            || name.contains(".portage_lockfile.hardlock-")
            || name.ends_with("-inode-test"))
}

/// What `setattr` asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetAttr {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub size: Option<u64>,
    pub mtime_ns: Option<i128>,
}

/// Owner attributes the adapter reports: the volatile files' own, or the
/// mounting user's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
}

/// The directory a name lives in, for this layer's own names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Parent {
    Cat(String),
    /// A staged `-MERGING-<pf>` directory (its object id).
    Staged(u64),
}

/// The prefix real Portage gives an entry while it is being merged
/// (`const.py` `MERGING_IDENTIFIER`).
const MERGING: &str = "-MERGING-";

enum Obj {
    /// A volatile file (lock files); `nlink` counts its names.
    Vol {
        data: Vec<u8>,
        mode: u32,
        owner: Owner,
        mtime_ns: i128,
        nlink: u32,
    },
    /// A staged `<cat>/-MERGING-<pf>` directory, backed by `path` in the
    /// scratch area. Its mtime follows real filesystem rules: a create,
    /// unlink or rename inside it bumps it, a `utime` sets it.
    StagedDir {
        cat: String,
        path: PathBuf,
        mode: u32,
        mtime_ns: i128,
    },
    /// A file inside a staged directory: the scratch file holds the bytes,
    /// mode and mtime.
    StagedFile { path: PathBuf },
}

enum Fh {
    /// A handle of the read-only view.
    Base(u64),
    /// A directory listing of this layer.
    Dir(Arc<Vec<DirEnt>>),
    /// A volatile file.
    Vol(u64),
    /// An open staged file.
    Staged(std::fs::File),
}

struct RwState {
    next_obj: u64,
    objs: HashMap<u64, Obj>,
    /// This layer's names: `(parent, name) -> object id`. Hardlinks are two
    /// names with one id.
    names: BTreeMap<(Parent, String), u64>,
    /// Categories that exist only because of `mkdir` (no entry yet),
    /// with their mtime.
    vol_cats: BTreeMap<String, i128>,
    handles: HashMap<u64, Fh>,
    next_fh: u64,
}

impl RwState {
    fn new_obj(&mut self, obj: Obj) -> u64 {
        let id = self.next_obj;
        self.next_obj += 1;
        self.objs.insert(id, obj);
        id
    }

    /// The children of `parent`, in name order.
    fn children(&self, parent: &Parent) -> Vec<(String, u64)> {
        self.names
            .range((parent.clone(), String::new())..)
            .take_while(|((p, _), _)| p == parent)
            .map(|((_, n), id)| (n.clone(), *id))
            .collect()
    }

    /// A create, unlink or rename inside staged directory `dir`.
    fn bump(&mut self, dir: u64) {
        if let Some(Obj::StagedDir { mtime_ns, .. }) = self.objs.get_mut(&dir) {
            *mtime_ns = now_ns();
        }
    }

    /// Drop one name of object `id`; the object goes with its last name.
    fn drop_name(&mut self, id: u64) {
        let gone = match self.objs.get_mut(&id) {
            Some(Obj::Vol { nlink, .. }) => {
                *nlink -= 1;
                *nlink == 0
            }
            Some(_) => true,
            None => false,
        };
        if gone {
            self.objs.remove(&id);
        }
    }
}

fn io_err(e: std::io::Error) -> ViewError {
    match e.kind() {
        std::io::ErrorKind::NotFound => ViewError::NoEnt,
        std::io::ErrorKind::AlreadyExists => ViewError::Exists,
        _ => ViewError::Io(e.to_string()),
    }
}

fn mtime_of(md: &std::fs::Metadata) -> i128 {
    md.mtime() as i128 * 1_000_000_000 + md.mtime_nsec() as i128
}

/// The `metadata` stamp state of a staged entry whose directory shows
/// `dir_mtime_ns`: real `_read_metadata_file` (`vartree.py:157-185`) only
/// trusts a file whose `#dir_mtime=` line equals the directory's
/// `st_mtime_ns`.
pub(crate) fn stamp_state(metadata: Option<&[u8]>, dir_mtime_ns: i128) -> MetadataStamp {
    let Some(bytes) = metadata else {
        return MetadataStamp::Absent;
    };
    let stamp = bytes
        .split(|&b| b == b'\n')
        .find_map(|l| l.strip_prefix(DIR_MTIME_PREFIX))
        .and_then(|v| std::str::from_utf8(v).ok())
        .and_then(|v| v.trim().parse::<i128>().ok());
    if stamp == Some(dir_mtime_ns) {
        MetadataStamp::Valid
    } else {
        MetadataStamp::Stale
    }
}

const DIR_MTIME_PREFIX: &[u8] = b"#dir_mtime=";

pub struct RwView {
    view: View,
    db: Arc<dyn InstalledDb>,
    scratch: PathBuf,
    owner: Owner,
    st: Mutex<RwState>,
}

impl RwView {
    /// A read-write layer over `db`. `scratch` is emptied (a previous
    /// daemon's staged entries are discarded, like real Portage removes a
    /// stale `-MERGING-` directory) and created. `owner` is what `getattr`
    /// reports for everything but volatile files.
    pub fn new(db: Arc<dyn InstalledDb>, scratch: PathBuf, owner: Owner) -> std::io::Result<Self> {
        match std::fs::remove_dir_all(&scratch) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        std::fs::create_dir_all(&scratch)?;
        Ok(RwView {
            view: View::new(db.clone()),
            db,
            scratch,
            owner,
            st: Mutex::new(RwState {
                next_obj: 1,
                objs: HashMap::new(),
                names: BTreeMap::new(),
                vol_cats: BTreeMap::new(),
                handles: HashMap::new(),
                next_fh: FH_BASE,
            }),
        })
    }

    /// The owner `getattr` reports for `ino`.
    pub fn owner_of(&self, ino: u64) -> Owner {
        let st = lock(&self.st);
        match self.obj_id(ino).and_then(|id| st.objs.get(&id)) {
            Some(Obj::Vol { owner, .. }) => *owner,
            _ => self.owner,
        }
    }

    fn obj_id(&self, ino: u64) -> Option<u64> {
        match self.view.node_of(ino) {
            Ok(Node::Obj(id)) => Some(id),
            _ => None,
        }
    }

    fn obj_ino(&self, id: u64) -> u64 {
        self.view.ino_of(&Node::Obj(id))
    }

    fn new_fh(st: &mut RwState, h: Fh) -> u64 {
        let fh = st.next_fh;
        st.next_fh += 1;
        st.handles.insert(fh, h);
        fh
    }

    fn obj_attr(&self, id: u64, obj: &Obj) -> Res<Attr> {
        let ino = self.obj_ino(id);
        Ok(match obj {
            Obj::Vol {
                data,
                mode,
                mtime_ns,
                nlink,
                ..
            } => Attr {
                ino,
                kind: Kind::File,
                perm: (*mode & 0o7777) as u16,
                size: data.len() as u64,
                nlink: *nlink,
                mtime_ns: *mtime_ns,
            },
            Obj::StagedDir { mode, mtime_ns, .. } => Attr {
                ino,
                kind: Kind::Dir,
                perm: (*mode & 0o7777) as u16,
                size: 0,
                nlink: 2,
                mtime_ns: *mtime_ns,
            },
            Obj::StagedFile { path } => {
                let md = std::fs::metadata(path).map_err(io_err)?;
                Attr {
                    ino,
                    kind: Kind::File,
                    perm: (md.mode() & 0o7777) as u16,
                    size: md.len(),
                    nlink: 1,
                    mtime_ns: mtime_of(&md),
                }
            }
        })
    }

    fn vol_cat_attr(&self, cat: &str, mtime_ns: i128) -> Attr {
        Attr {
            ino: self.view.ino_of(&Node::Cat(cat.to_string())),
            kind: Kind::Dir,
            perm: 0o755,
            size: 0,
            nlink: 2,
            mtime_ns,
        }
    }

    /// Whether the view (the database) has category `cat`.
    fn base_has_cat(&self, cat: &str) -> Res<bool> {
        match self.view.lookup(ROOT_INO, cat) {
            Ok(_) => Ok(true),
            Err(ViewError::NoEnt) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The directory `ino` stands for, as a parent of this layer's names:
    /// a category (stored or volatile) or a staged directory.
    fn parent_of(&self, ino: u64) -> Res<Option<Parent>> {
        Ok(match self.view.node_of(ino)? {
            Node::Cat(c) => Some(Parent::Cat(c)),
            Node::Obj(id) => match lock(&self.st).objs.get(&id) {
                Some(Obj::StagedDir { .. }) => Some(Parent::Staged(id)),
                Some(_) => return Err(ViewError::NotDir),
                None => return Err(ViewError::NoEnt),
            },
            _ => None,
        })
    }

    // ------------------------------------------------------- read side

    pub fn lookup(&self, parent: u64, name: &str) -> Res<Attr> {
        match self.view.node_of(parent)? {
            Node::Root => {
                if let Some(&m) = lock(&self.st).vol_cats.get(name)
                    && !self.base_has_cat(name)?
                {
                    return Ok(self.vol_cat_attr(name, m));
                }
                self.view.lookup(parent, name)
            }
            Node::Cat(cat) => {
                {
                    let st = lock(&self.st);
                    if let Some(&id) = st.names.get(&(Parent::Cat(cat.clone()), name.to_string())) {
                        return self.obj_attr(id, &st.objs[&id]);
                    }
                }
                self.view.lookup(parent, name)
            }
            Node::Obj(dir) => {
                let st = lock(&self.st);
                match st.objs.get(&dir) {
                    Some(Obj::StagedDir { .. }) => {}
                    Some(_) => return Err(ViewError::NotDir),
                    None => return Err(ViewError::NoEnt),
                }
                let id = *st
                    .names
                    .get(&(Parent::Staged(dir), name.to_string()))
                    .ok_or(ViewError::NoEnt)?;
                self.obj_attr(id, &st.objs[&id])
            }
            _ => self.view.lookup(parent, name),
        }
    }

    pub fn getattr(&self, ino: u64) -> Res<Attr> {
        match self.view.node_of(ino)? {
            Node::Obj(id) => {
                let st = lock(&self.st);
                let obj = st.objs.get(&id).ok_or(ViewError::NoEnt)?;
                self.obj_attr(id, obj)
            }
            Node::Cat(cat) => match self.view.getattr(ino) {
                Err(ViewError::NoEnt) => {
                    let m = *lock(&self.st).vol_cats.get(&cat).ok_or(ViewError::NoEnt)?;
                    Ok(self.vol_cat_attr(&cat, m))
                }
                r => r,
            },
            _ => self.view.getattr(ino),
        }
    }

    /// `getattr` with a handle: this layer's handles report the object's
    /// current attributes.
    pub fn getattr_fh(&self, ino: u64, fh: u64) -> Res<Attr> {
        let base = match lock(&self.st).handles.get(&fh) {
            Some(Fh::Base(b)) => Some(*b),
            Some(_) => None,
            None => return Err(ViewError::BadHandle),
        };
        match base {
            Some(b) => self.view.getattr_fh(b),
            None => self.getattr(ino),
        }
    }

    pub fn opendir(&self, ino: u64) -> Res<u64> {
        let node = self.view.node_of(ino)?;
        let me = self.getattr(ino)?;
        if me.kind != Kind::Dir {
            return Err(ViewError::NotDir);
        }
        let mut listing: Vec<DirEnt> = match &node {
            Node::Obj(_) => Vec::new(),
            _ => match self.view.opendir(ino) {
                Ok(vfh) => {
                    let l = self.view.readdir(vfh);
                    self.view.release(vfh);
                    l?.as_ref().clone()
                }
                Err(ViewError::NoEnt) => Vec::new(),
                Err(e) => return Err(e),
            },
        };
        if listing.is_empty() {
            let parent = match &node {
                Node::Obj(id) => match lock(&self.st).objs.get(id) {
                    Some(Obj::StagedDir { cat, .. }) => self.view.ino_of(&Node::Cat(cat.clone())),
                    _ => ROOT_INO,
                },
                _ => ROOT_INO,
            };
            listing = vec![
                DirEnt {
                    ino: me.ino,
                    kind: Kind::Dir,
                    name: ".".into(),
                },
                DirEnt {
                    ino: parent,
                    kind: Kind::Dir,
                    name: "..".into(),
                },
            ];
        }
        let mut st = lock(&self.st);
        let parent = match &node {
            Node::Root => {
                for cat in st.vol_cats.keys() {
                    if !listing.iter().any(|d| &d.name == cat) {
                        listing.push(DirEnt {
                            ino: self.view.ino_of(&Node::Cat(cat.clone())),
                            kind: Kind::Dir,
                            name: cat.clone(),
                        });
                    }
                }
                None
            }
            Node::Cat(cat) => Some(Parent::Cat(cat.clone())),
            Node::Obj(id) => Some(Parent::Staged(*id)),
            _ => None,
        };
        if let Some(parent) = parent {
            for (name, id) in st.children(&parent) {
                let kind = match st.objs.get(&id) {
                    Some(Obj::StagedDir { .. }) => Kind::Dir,
                    _ => Kind::File,
                };
                listing.push(DirEnt {
                    ino: self.obj_ino(id),
                    kind,
                    name,
                });
            }
        }
        Ok(Self::new_fh(&mut st, Fh::Dir(Arc::new(listing))))
    }

    pub fn readdir(&self, fh: u64) -> Res<Arc<Vec<DirEnt>>> {
        match lock(&self.st).handles.get(&fh) {
            Some(Fh::Dir(l)) => Ok(l.clone()),
            Some(_) => Err(ViewError::NotDir),
            None => Err(ViewError::BadHandle),
        }
    }

    /// `open(ino, flags)`.
    pub fn open(&self, ino: u64, flags: i32) -> Res<u64> {
        match self.view.node_of(ino)? {
            Node::Obj(id) => {
                let mut st = lock(&self.st);
                let h = match st.objs.get_mut(&id).ok_or(ViewError::NoEnt)? {
                    Obj::Vol { data, mtime_ns, .. } => {
                        if flags & libc::O_TRUNC != 0 && open_wants_write(flags) {
                            data.clear();
                            *mtime_ns = now_ns();
                        }
                        Fh::Vol(id)
                    }
                    Obj::StagedFile { path } => {
                        let f = std::fs::OpenOptions::new()
                            .read(true)
                            .write(open_wants_write(flags))
                            .truncate(flags & libc::O_TRUNC != 0 && open_wants_write(flags))
                            .open(&*path)
                            .map_err(io_err)?;
                        Fh::Staged(f)
                    }
                    Obj::StagedDir { .. } => return Err(ViewError::IsDir),
                };
                Ok(Self::new_fh(&mut st, h))
            }
            _ => {
                if open_wants_write(flags) {
                    // Live-entry rewrites are S3.
                    self.view.getattr(ino)?;
                    return Err(ViewError::Perm);
                }
                let vfh = self.view.open(ino, false)?;
                Ok(Self::new_fh(&mut lock(&self.st), Fh::Base(vfh)))
            }
        }
    }

    pub fn read(&self, fh: u64, off: u64, size: usize) -> Res<Vec<u8>> {
        let st = lock(&self.st);
        match st.handles.get(&fh) {
            Some(Fh::Base(v)) => {
                let v = *v;
                drop(st);
                self.view.read(v, off, size)
            }
            Some(Fh::Vol(id)) => match st.objs.get(id) {
                Some(Obj::Vol { data, .. }) => {
                    let start = usize::try_from(off).unwrap_or(usize::MAX).min(data.len());
                    let end = start.saturating_add(size).min(data.len());
                    Ok(data[start..end].to_vec())
                }
                _ => Err(ViewError::Stale),
            },
            Some(Fh::Staged(f)) => {
                let mut buf = vec![0u8; size];
                let n = f.read_at(&mut buf, off).map_err(io_err)?;
                buf.truncate(n);
                Ok(buf)
            }
            Some(Fh::Dir(_)) => Err(ViewError::IsDir),
            None => Err(ViewError::BadHandle),
        }
    }

    /// `release` / `releasedir`.
    pub fn release(&self, fh: u64) -> Res<()> {
        let h = lock(&self.st).handles.remove(&fh);
        if let Some(Fh::Base(v)) = h {
            self.view.release(v);
        }
        Ok(())
    }

    // ------------------------------------------------------ write side

    /// `create(parent, name, mode, flags)`: a new file, opened.
    pub fn create(&self, parent: u64, name: &str, mode: u32, flags: i32) -> Res<(Attr, u64)> {
        let parent_key = self.parent_of(parent)?.ok_or(ViewError::Perm)?;
        self.getattr(parent)?;
        let mut st = lock(&self.st);
        let key = (parent_key.clone(), name.to_string());
        if let Some(&id) = st.names.get(&key) {
            if flags & libc::O_EXCL != 0 {
                return Err(ViewError::Exists);
            }
            drop(st);
            let ino = self.obj_ino(id);
            let fh = self.open(ino, flags)?;
            return Ok((self.getattr(ino)?, fh));
        }
        let (obj, h) = match &parent_key {
            Parent::Cat(_) if is_volatile_name(name) => (
                Obj::Vol {
                    data: Vec::new(),
                    mode: mode & 0o7777,
                    owner: self.owner,
                    mtime_ns: now_ns(),
                    nlink: 1,
                },
                None,
            ),
            Parent::Cat(_) => return Err(ViewError::Perm),
            Parent::Staged(dir) => {
                let Some(Obj::StagedDir { path, .. }) = st.objs.get(dir) else {
                    return Err(ViewError::NoEnt);
                };
                let path = path.join(name);
                let f = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .mode(mode & 0o7777)
                    .open(&path)
                    .map_err(io_err)?;
                (Obj::StagedFile { path }, Some(f))
            }
        };
        let id = st.new_obj(obj);
        st.names.insert(key, id);
        if let Parent::Staged(dir) = parent_key {
            st.bump(dir);
        }
        let attr = self.obj_attr(id, &st.objs[&id])?;
        let fh = Self::new_fh(
            &mut st,
            match h {
                Some(f) => Fh::Staged(f),
                None => Fh::Vol(id),
            },
        );
        Ok((attr, fh))
    }

    /// `write(fh, offset, data)`: the bytes written.
    pub fn write(&self, fh: u64, off: u64, bytes: &[u8]) -> Res<u32> {
        let mut st = lock(&self.st);
        let id = match st.handles.get(&fh) {
            Some(Fh::Vol(id)) => *id,
            Some(Fh::Staged(f)) => {
                f.write_all_at(bytes, off).map_err(io_err)?;
                return Ok(bytes.len() as u32);
            }
            Some(_) => return Err(ViewError::Perm),
            None => return Err(ViewError::BadHandle),
        };
        match st.objs.get_mut(&id).ok_or(ViewError::Stale)? {
            Obj::Vol { data, mtime_ns, .. } => {
                let off = usize::try_from(off).map_err(|_| ViewError::Perm)?;
                if data.len() < off + bytes.len() {
                    data.resize(off + bytes.len(), 0);
                }
                data[off..off + bytes.len()].copy_from_slice(bytes);
                *mtime_ns = now_ns();
            }
            _ => return Err(ViewError::Stale),
        }
        Ok(bytes.len() as u32)
    }

    /// `mkdir(parent, name, mode)`: a category at the root, or a staged
    /// `-MERGING-<pf>` directory in a category.
    pub fn mkdir(&self, parent: u64, name: &str, mode: u32) -> Res<Attr> {
        match self.view.node_of(parent)? {
            Node::Root => {
                if self.lookup(ROOT_INO, name).is_ok() {
                    return Err(ViewError::Exists);
                }
                if name.starts_with('.') || name.contains('/') {
                    return Err(ViewError::Perm);
                }
                let m = now_ns();
                lock(&self.st).vol_cats.insert(name.to_string(), m);
                Ok(self.vol_cat_attr(name, m))
            }
            Node::Cat(cat) => {
                let pf = name.strip_prefix(MERGING).unwrap_or("");
                if pf.is_empty() || pf.contains('/') {
                    return Err(ViewError::Perm);
                }
                self.getattr(parent)?;
                if self.lookup(parent, name).is_ok() {
                    return Err(ViewError::Exists);
                }
                let mut st = lock(&self.st);
                let id = st.next_obj;
                let path = self.scratch.join(id.to_string());
                std::fs::create_dir(&path).map_err(io_err)?;
                let id = st.new_obj(Obj::StagedDir {
                    cat: cat.clone(),
                    path,
                    mode: mode & 0o7777,
                    mtime_ns: now_ns(),
                });
                st.names.insert((Parent::Cat(cat), name.to_string()), id);
                self.obj_attr(id, &st.objs[&id])
            }
            _ => Err(ViewError::Perm),
        }
    }

    /// `unlink(parent, name)`.
    pub fn unlink(&self, parent: u64, name: &str) -> Res<()> {
        let Some(parent_key) = self.parent_of(parent)? else {
            return Err(ViewError::Perm);
        };
        let mut st = lock(&self.st);
        let key = (parent_key.clone(), name.to_string());
        let Some(&id) = st.names.get(&key) else {
            drop(st);
            if let Parent::Staged(_) = parent_key {
                return Err(ViewError::NoEnt);
            }
            // A stored entry is a directory; anything else is not here.
            return match self.view.lookup(parent, name) {
                Ok(_) => Err(ViewError::IsDir),
                Err(e) => Err(e),
            };
        };
        match st.objs.get(&id) {
            Some(Obj::StagedDir { .. }) => return Err(ViewError::IsDir),
            Some(Obj::StagedFile { path }) => {
                std::fs::remove_file(path).map_err(io_err)?;
            }
            _ => {}
        }
        st.names.remove(&key);
        st.drop_name(id);
        if let Parent::Staged(dir) = parent_key {
            st.bump(dir);
        }
        Ok(())
    }

    /// `rmdir(parent, name)`: an empty category, or an emptied staged
    /// directory (real removes a stale `-MERGING-<pf>` before it starts).
    pub fn rmdir(&self, parent: u64, name: &str) -> Res<()> {
        match self.view.node_of(parent)? {
            Node::Root => {
                let attr = self.lookup(ROOT_INO, name)?;
                let fh = self.opendir(attr.ino)?;
                let listing = self.readdir(fh);
                self.release(fh)?;
                if listing?.len() > 2 {
                    return Err(ViewError::NotEmpty);
                }
                lock(&self.st).vol_cats.remove(name);
                Ok(())
            }
            Node::Cat(cat) => {
                let mut st = lock(&self.st);
                let key = (Parent::Cat(cat), name.to_string());
                match st.names.get(&key).copied() {
                    Some(id) => {
                        let Some(Obj::StagedDir { path, .. }) = st.objs.get(&id) else {
                            return Err(ViewError::NotDir);
                        };
                        if !st.children(&Parent::Staged(id)).is_empty() {
                            return Err(ViewError::NotEmpty);
                        }
                        let _ = std::fs::remove_dir(path);
                        st.names.remove(&key);
                        st.objs.remove(&id);
                        Ok(())
                    }
                    // Live entries are S3.
                    None => {
                        drop(st);
                        self.view.lookup(parent, name)?;
                        Err(ViewError::Perm)
                    }
                }
            }
            _ => Err(ViewError::Perm),
        }
    }

    /// `rename(parent, name, newparent, newname)`.
    pub fn rename(&self, parent: u64, name: &str, newparent: u64, newname: &str) -> Res<()> {
        let (Some(from), Some(to)) = (self.parent_of(parent)?, self.parent_of(newparent)?) else {
            return Err(ViewError::Perm);
        };
        match (&from, &to) {
            // Publish: `<cat>/-MERGING-<pf>` -> `<cat>/<pf>`.
            (Parent::Cat(c1), Parent::Cat(c2))
                if c1 == c2
                    && name.strip_prefix(MERGING) == Some(newname)
                    && !newname.is_empty() =>
            {
                self.publish(c1, name, newname)
            }
            (Parent::Cat(_), Parent::Cat(_))
                if is_volatile_name(name) && is_volatile_name(newname) =>
            {
                self.move_name(from, name, to, newname)
            }
            // `write_atomic` inside a staged directory.
            (Parent::Staged(a), Parent::Staged(b)) if a == b => {
                self.move_name(from, name, to, newname)
            }
            // Live-entry rewrites are S3.
            _ => Err(ViewError::Perm),
        }
    }

    /// Rename one of this layer's names (volatile files, staged files),
    /// replacing whatever `newname` named.
    fn move_name(&self, from: Parent, name: &str, to: Parent, newname: &str) -> Res<()> {
        let mut st = lock(&self.st);
        let id = *st
            .names
            .get(&(from.clone(), name.to_string()))
            .ok_or(ViewError::NoEnt)?;
        let new_path = match (&to, st.objs.get(&id)) {
            (Parent::Staged(dir), Some(Obj::StagedFile { path })) => {
                let Some(Obj::StagedDir { path: dpath, .. }) = st.objs.get(dir) else {
                    return Err(ViewError::NoEnt);
                };
                let np = dpath.join(newname);
                std::fs::rename(path, &np).map_err(io_err)?;
                Some(np)
            }
            (Parent::Staged(_), _) => return Err(ViewError::Perm),
            _ => None,
        };
        st.names.remove(&(from.clone(), name.to_string()));
        if let Some(old) = st.names.insert((to.clone(), newname.to_string()), id)
            && old != id
        {
            st.drop_name(old);
        }
        if let (Some(np), Some(Obj::StagedFile { path })) = (new_path, st.objs.get_mut(&id)) {
            *path = np;
        }
        if let Parent::Staged(dir) = to {
            st.bump(dir);
        }
        Ok(())
    }

    /// The image real Portage built in staged directory `dir`.
    fn staged_image(&self, st: &RwState, dir: u64, key: EntryKey) -> Res<EntryImage> {
        let Some(Obj::StagedDir { mode, mtime_ns, .. }) = st.objs.get(&dir) else {
            return Err(ViewError::NoEnt);
        };
        let mut files = Vec::new();
        for (name, id) in st.children(&Parent::Staged(dir)) {
            let Some(Obj::StagedFile { path }) = st.objs.get(&id) else {
                return Err(ViewError::Perm);
            };
            let md = std::fs::metadata(path).map_err(io_err)?;
            let data = std::fs::read(path).map_err(io_err)?;
            files.push(EntryFile {
                meta: FileMeta {
                    name,
                    len: data.len() as u64,
                    mode: md.mode(),
                    mtime_ns: mtime_of(&md),
                },
                data,
            });
        }
        let metadata = files
            .iter()
            .find(|f| f.meta.name == "metadata")
            .map(|f| f.data.as_slice());
        let metadata_stamp = stamp_state(metadata, *mtime_ns);
        Ok(EntryImage {
            key,
            files,
            dir_mode: *mode & 0o7777,
            dir_mtime_ns: *mtime_ns,
            metadata_stamp,
        })
    }

    /// `rename(<cat>/-MERGING-<pf>, <cat>/<pf>)`: store the staged entry in
    /// one transaction (replacing a live `<pf>`), raise the counter to its
    /// `COUNTER`, then drop the scratch copy. The directory keeps its inode
    /// under the new name. A failed transaction is `EIO` and leaves the
    /// staged entry as it was.
    fn publish(&self, cat: &str, staged: &str, pf: &str) -> Res<()> {
        let mut st = lock(&self.st);
        let dir = *st
            .names
            .get(&(Parent::Cat(cat.to_string()), staged.to_string()))
            .ok_or(ViewError::NoEnt)?;
        let key = EntryKey::new(cat, pf);
        // A rename onto a non-empty directory fails on a real filesystem;
        // real `dblink.treewalk` removes the live entry first.
        let cat_ino = self.view.ino_of(&Node::Cat(cat.to_string()));
        if self.view.lookup(cat_ino, pf).is_ok() {
            return Err(ViewError::NotEmpty);
        }
        let image = self.staged_image(&st, dir, key.clone())?;
        let counter = image
            .files
            .iter()
            .find(|f| f.meta.name == "COUNTER")
            .and_then(|f| {
                std::str::from_utf8(&f.data)
                    .ok()?
                    .trim()
                    .parse::<i64>()
                    .ok()
            });
        let commit = || -> portage_vdb::Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.insert_entry(&image)?;
            if let Some(c) = counter
                && self.db.counter()?.is_none_or(|cur| cur.0 < c)
            {
                txn.set_counter(Counter(c))?;
            }
            txn.commit()
        };
        commit().map_err(|e| ViewError::Io(e.to_string()))?;
        // A directory rename keeps every child's inode: the kernel still
        // holds dentries for the files real Portage just wrote, and
        // `pkg_postinst` reopens `environment.bz2` through them.
        let mut rebinds = Vec::new();
        for (name, id) in st.children(&Parent::Staged(dir)) {
            st.names.remove(&(Parent::Staged(dir), name.clone()));
            st.objs.remove(&id);
            rebinds.push((self.obj_ino(id), Node::File(key.clone(), name)));
        }
        st.names
            .remove(&(Parent::Cat(cat.to_string()), staged.to_string()));
        if let Some(Obj::StagedDir { path, .. }) = st.objs.remove(&dir) {
            let _ = std::fs::remove_dir_all(path);
        }
        // The entry is now a stored category member; a volatile category
        // record for it is no longer needed.
        st.vol_cats.remove(cat);
        drop(st);
        self.view.rebind(self.obj_ino(dir), Node::Entry(key));
        for (ino, node) in rebinds {
            self.view.rebind(ino, node);
        }
        Ok(())
    }

    /// `link(ino, newparent, newname)`: hardlinks of volatile files only
    /// (real `_lockfile_was_removed` links the lock file to a hardlock
    /// name and compares the inodes).
    pub fn link(&self, ino: u64, newparent: u64, newname: &str) -> Res<Attr> {
        let id = self.obj_id(ino).ok_or(ViewError::Perm)?;
        let Some(Parent::Cat(cat)) = self.parent_of(newparent)? else {
            return Err(ViewError::Perm);
        };
        if !is_volatile_name(newname) {
            return Err(ViewError::Perm);
        }
        let mut st = lock(&self.st);
        let key = (Parent::Cat(cat), newname.to_string());
        if st.names.contains_key(&key) {
            return Err(ViewError::Exists);
        }
        match st.objs.get_mut(&id).ok_or(ViewError::NoEnt)? {
            Obj::Vol { nlink, .. } => *nlink += 1,
            _ => return Err(ViewError::Perm),
        }
        st.names.insert(key, id);
        self.obj_attr(id, &st.objs[&id])
    }

    /// `setattr(ino, …)`.
    pub fn setattr(&self, ino: u64, s: &SetAttr) -> Res<Attr> {
        if let Some(id) = self.obj_id(ino) {
            {
                let mut st = lock(&self.st);
                let owner = self.owner;
                match st.objs.get_mut(&id).ok_or(ViewError::NoEnt)? {
                    Obj::Vol {
                        data,
                        mode,
                        owner,
                        mtime_ns,
                        ..
                    } => {
                        if let Some(m) = s.mode {
                            *mode = m & 0o7777;
                        }
                        if let Some(u) = s.uid {
                            owner.uid = u;
                        }
                        if let Some(g) = s.gid {
                            owner.gid = g;
                        }
                        if let Some(n) = s.size {
                            data.resize(usize::try_from(n).map_err(|_| ViewError::Perm)?, 0);
                        }
                        if let Some(t) = s.mtime_ns {
                            *mtime_ns = t;
                        }
                    }
                    Obj::StagedDir { mode, mtime_ns, .. } => {
                        Self::check_owner(owner, s)?;
                        if s.size.is_some() {
                            return Err(ViewError::IsDir);
                        }
                        if let Some(m) = s.mode {
                            *mode = m & 0o7777;
                        }
                        if let Some(t) = s.mtime_ns {
                            *mtime_ns = t;
                        }
                    }
                    Obj::StagedFile { path } => {
                        Self::check_owner(owner, s)?;
                        if let Some(m) = s.mode {
                            std::fs::set_permissions(
                                &*path,
                                std::fs::Permissions::from_mode(m & 0o7777),
                            )
                            .map_err(io_err)?;
                        }
                        let f = std::fs::OpenOptions::new()
                            .write(true)
                            .open(&*path)
                            .map_err(io_err)?;
                        if let Some(n) = s.size {
                            f.set_len(n).map_err(io_err)?;
                        }
                        if let Some(t) = s.mtime_ns {
                            f.set_modified(crate::vdb_view::system_time(t))
                                .map_err(io_err)?;
                        }
                    }
                }
            }
            return self.getattr(ino);
        }
        let attr = self.getattr(ino)?;
        Self::check_owner(self.owner, s)?;
        if s.mode.is_some() || s.size.is_some() {
            return Err(ViewError::Perm);
        }
        // `utime` on a category or the root (real `_bump_mtime`): accepted,
        // no lasting effect; the view derives those mtimes.
        Ok(attr)
    }

    /// The database stores no owner: a `chown` to the owner `getattr`
    /// reports is a no-op, any other is `EPERM`.
    fn check_owner(owner: Owner, s: &SetAttr) -> Res<()> {
        if s.uid.is_some_and(|u| u != owner.uid) || s.gid.is_some_and(|g| g != owner.gid) {
            return Err(ViewError::Perm);
        }
        Ok(())
    }
}

// The tests replay scripts on a database backend; without one there is
// nothing to write to.
#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
#[path = "vdb_rw/ops.rs"]
mod ops;

#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
mod tests {
    use super::ops::{Env, apply_fs, apply_rw, assert_same, envs, parse, resolve};
    use super::*;

    fn rw(env: &Env) -> RwView {
        RwView::new(
            env.db.clone(),
            env.scratch.clone(),
            Owner { uid: 0, gid: 0 },
        )
        .unwrap()
    }

    #[test]
    fn a_lock_cycle_leaves_the_database_untouched() {
        for env in envs("lockcycle") {
            let g0 = env.db.generation().unwrap();
            let v = rw(&env);
            let ops = parse(include_str!("vdb_rw/testdata/lock-cycle.ops"));
            apply_fs(&env.files_vdb, &ops);
            apply_rw(&v, &ops);
            assert_eq!(env.db.generation().unwrap(), g0, "{}", env.label);
            assert_same(&env.files_root, &*env.db);
            assert_eq!(
                v.lookup(ROOT_INO, "app-misc"),
                Err(ViewError::NoEnt),
                "{}: rmdir removed the empty category",
                env.label
            );
        }
    }

    #[test]
    fn a_hardlink_shares_the_inode_and_unlink_keeps_the_other_name() {
        for env in envs("hardlink") {
            let v = rw(&env);
            apply_rw(
                &v,
                &parse(
                    "mkdir app-misc 755\n\
                     create app-misc/.a:0.portage_lockfile 660\n\
                     link app-misc/.a:0.portage_lockfile app-misc/..a:0.portage_lockfile.hardlock-h-1\n\
                     sameino app-misc/.a:0.portage_lockfile app-misc/..a:0.portage_lockfile.hardlock-h-1\n\
                     unlink app-misc/.a:0.portage_lockfile\n",
                ),
            );
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap();
            let left = v
                .lookup(cat.ino, "..a:0.portage_lockfile.hardlock-h-1")
                .unwrap();
            assert_eq!(left.nlink, 1, "{}", env.label);
            assert_eq!(
                v.lookup(cat.ino, ".a:0.portage_lockfile"),
                Err(ViewError::NoEnt)
            );
        }
    }

    #[test]
    fn a_lock_file_takes_chown_and_keeps_the_owner_in_memory() {
        for env in envs("chown") {
            let v = rw(&env);
            apply_rw(
                &v,
                &parse("mkdir app-misc 755\ncreate app-misc/.a:0.portage_lockfile 660\n"),
            );
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap();
            let f = v.lookup(cat.ino, ".a:0.portage_lockfile").unwrap();
            let s = SetAttr {
                gid: Some(250),
                ..SetAttr::default()
            };
            v.setattr(f.ino, &s).unwrap();
            assert_eq!(v.owner_of(f.ino), Owner { uid: 0, gid: 250 });
            assert_eq!(v.owner_of(cat.ino), Owner { uid: 0, gid: 0 });
        }
    }

    #[test]
    fn names_this_layer_does_not_map_are_eperm() {
        for env in envs("eperm") {
            let v = rw(&env);
            let (_, fh) = v
                .create(ROOT_INO, ".x.portage_lockfile", 0o644, libc::O_CREAT)
                .map_or((None, 0), |(a, fh)| (Some(a), fh));
            assert_eq!(fh, 0, "{}: nothing is created at the root", env.label);
            v.mkdir(ROOT_INO, "app-misc", 0o755).unwrap();
            let cat = v.lookup(ROOT_INO, "app-misc").unwrap();
            assert_eq!(
                v.create(cat.ino, "stray", 0o644, libc::O_CREAT).err(),
                Some(ViewError::Perm)
            );
            assert_eq!(
                v.mkdir(ROOT_INO, "app-misc", 0o755).err(),
                Some(ViewError::Exists)
            );
            assert_eq!(
                v.mkdir(cat.ino, "nested", 0o755).err(),
                Some(ViewError::Perm),
                "a staged -MERGING- dir is S2; any other name stays EPERM"
            );
        }
    }

    #[test]
    fn a_stored_category_cannot_be_removed_and_its_entries_stay_listed() {
        for env in envs("storedcat") {
            let v = rw(&env);
            let cat = env.seeded.category.clone();
            assert_eq!(v.rmdir(ROOT_INO, &cat).err(), Some(ViewError::NotEmpty));
            let c = v.lookup(ROOT_INO, &cat).unwrap();
            let (lf, fh) = v
                .create(c.ino, ".p:0.portage_lockfile", 0o660, libc::O_CREAT)
                .unwrap();
            v.release(fh).unwrap();
            let dfh = v.opendir(c.ino).unwrap();
            let names: Vec<String> = v
                .readdir(dfh)
                .unwrap()
                .iter()
                .map(|d| d.name.clone())
                .collect();
            v.release(dfh).unwrap();
            assert!(names.contains(&env.seeded.pf), "{names:?}");
            assert!(names.contains(&".p:0.portage_lockfile".to_string()));
            assert_eq!(v.getattr(lf.ino).unwrap().kind, Kind::File);
        }
    }

    fn script(name: &str) -> Vec<super::ops::Op> {
        let text = match name {
            "merge-new" => include_str!("vdb_rw/testdata/merge-new.ops"),
            _ => unreachable!("{name}"),
        };
        parse(text)
    }

    #[test]
    fn a_new_merge_through_the_view_equals_the_same_merge_on_files() {
        for env in envs("mergenew") {
            let ops = script("merge-new");
            apply_fs(&env.files_vdb, &ops);
            let v = rw(&env);
            let g0 = env.db.generation().unwrap();
            let (staging, publish) = ops.split_at(ops.len() - 1);
            apply_rw(&v, staging);
            assert_eq!(
                env.db.generation().unwrap(),
                g0,
                "{}: nothing is stored before the rename",
                env.label
            );
            apply_rw(&v, publish);
            assert_same(&env.files_root, &*env.db);
            let k = EntryKey::new("app-misc", "foo-1.0");
            assert_eq!(
                env.db.entry_stat(&k).unwrap().unwrap().metadata_stamp,
                MetadataStamp::Valid,
                "{}: the stamp real appended matches the staged dir mtime",
                env.label
            );
            assert!(
                std::fs::read_dir(&env.scratch).unwrap().next().is_none(),
                "{}: the scratch copy is gone",
                env.label
            );
        }
    }

    #[test]
    fn the_published_entry_keeps_the_staged_inode_and_reads_back() {
        for env in envs("inode") {
            let ops = script("merge-new");
            let v = rw(&env);
            let (staging, publish) = ops.split_at(ops.len() - 1);
            apply_rw(&v, staging);
            let staged = resolve(&v, "app-misc/-MERGING-foo-1.0").unwrap();
            let staged_slot = resolve(&v, "app-misc/-MERGING-foo-1.0/SLOT").unwrap();
            apply_rw(&v, publish);
            // The kernel keeps the inodes it learned before the rename.
            assert_eq!(v.getattr(staged_slot).unwrap().size, 2);
            assert_eq!(resolve(&v, "app-misc/foo-1.0").unwrap(), staged);
            assert_eq!(
                resolve(&v, "app-misc/-MERGING-foo-1.0"),
                Err(ViewError::NoEnt)
            );
            let f = resolve(&v, "app-misc/foo-1.0/SLOT").unwrap();
            assert_eq!(f, staged_slot, "the file keeps its inode");
            let fh = v.open(f, libc::O_RDONLY).unwrap();
            assert_eq!(v.read(fh, 0, 64).unwrap(), b"0\n");
            v.release(fh).unwrap();
        }
    }

    #[test]
    fn stamp_state_follows_the_real_rule() {
        assert_eq!(stamp_state(None, 5), MetadataStamp::Absent);
        assert_eq!(
            stamp_state(Some(b"#format=1\nSLOT=0\n#dir_mtime=5\n"), 5),
            MetadataStamp::Valid
        );
        assert_eq!(
            stamp_state(Some(b"#format=1\n#dir_mtime=4\n"), 5),
            MetadataStamp::Stale
        );
        assert_eq!(
            stamp_state(Some(b"#format=1\nSLOT=0\n"), 5),
            MetadataStamp::Stale
        );
    }

    #[test]
    fn publishing_raises_the_counter_to_the_entry_counter() {
        for env in envs("counter") {
            let v = rw(&env);
            apply_rw(&v, &script("merge-new"));
            assert_eq!(
                env.db.counter().unwrap(),
                Some(portage_vdb::Counter(41)),
                "{}",
                env.label
            );
        }
    }

    #[test]
    fn a_rename_onto_a_live_entry_is_enotempty() {
        for env in envs("onto") {
            let v = rw(&env);
            apply_rw(
                &v,
                &parse(
                    "mkdir dev-libs/-MERGING-seed-1 755\n\
                     create dev-libs/-MERGING-seed-1/SLOT 644\n\
                     write dev-libs/-MERGING-seed-1/SLOT 1\\n\n",
                ),
            );
            let cat = resolve(&v, "dev-libs").unwrap();
            assert_eq!(
                v.rename(cat, "-MERGING-seed-1", cat, "seed-1"),
                Err(ViewError::NotEmpty)
            );
            assert!(resolve(&v, "dev-libs/-MERGING-seed-1/SLOT").is_ok());
        }
    }

    #[test]
    fn a_stale_merging_dir_is_removable_and_a_new_view_clears_scratch() {
        for env in envs("stale") {
            let v = rw(&env);
            apply_rw(
                &v,
                &parse(
                    "mkdir dev-libs/-MERGING-x-1 755\n\
                     create dev-libs/-MERGING-x-1/SLOT 644\n",
                ),
            );
            let cat = resolve(&v, "dev-libs").unwrap();
            assert_eq!(
                v.rmdir(cat, "-MERGING-x-1"),
                Err(ViewError::NotEmpty),
                "rmdir before the rmtree emptied it"
            );
            apply_rw(
                &v,
                &parse("unlink dev-libs/-MERGING-x-1/SLOT\nrmdir dev-libs/-MERGING-x-1\n"),
            );
            assert_eq!(resolve(&v, "dev-libs/-MERGING-x-1"), Err(ViewError::NoEnt));
            apply_rw(
                &v,
                &parse("mkdir dev-libs/-MERGING-y-1 755\ncreate dev-libs/-MERGING-y-1/SLOT 644\n"),
            );
            drop(v);
            let v2 = rw(&env);
            assert!(std::fs::read_dir(&env.scratch).unwrap().next().is_none());
            assert_eq!(resolve(&v2, "dev-libs/-MERGING-y-1"), Err(ViewError::NoEnt));
        }
    }

    #[cfg(feature = "vdb-sqlite")]
    #[test]
    fn a_failed_publish_is_eio_and_keeps_the_staged_entry() {
        for env in envs("eio").into_iter().filter(|e| e.label == "sqlite") {
            let ro: Arc<dyn InstalledDb> =
                Arc::new(portage_vdb::SqliteDb::open_readonly(&env.db_path).unwrap());
            let v = RwView::new(ro, env.scratch.clone(), Owner { uid: 0, gid: 0 }).unwrap();
            let ops = script("merge-new");
            let (staging, _) = ops.split_at(ops.len() - 1);
            apply_rw(&v, staging);
            let cat = resolve(&v, "app-misc").unwrap();
            let r = v.rename(cat, "-MERGING-foo-1.0", cat, "foo-1.0");
            assert!(matches!(r, Err(ViewError::Io(_))), "{r:?}");
            assert_eq!(ViewError::Io(String::new()).errno(), libc::EIO);
            assert!(resolve(&v, "app-misc/-MERGING-foo-1.0/CONTENTS").is_ok());
        }
    }

    #[test]
    fn names_inside_a_staged_dir_are_free_but_nested_dirs_are_eperm() {
        for env in envs("nested") {
            let v = rw(&env);
            apply_rw(&v, &parse("mkdir dev-libs/-MERGING-z-1 755\n"));
            let d = resolve(&v, "dev-libs/-MERGING-z-1").unwrap();
            assert_eq!(v.mkdir(d, "sub", 0o755).err(), Some(ViewError::Perm));
            let cat = resolve(&v, "dev-libs").unwrap();
            assert_eq!(
                v.mkdir(cat, "not-merging", 0o755).err(),
                Some(ViewError::Perm)
            );
        }
    }
}
