// The read-write layer of `portuale vdb mount --rw`: what Portage writes
// into `var/db/pkg` while it merges and unmerges, mapped onto database
// transactions.
//
// `RwView` wraps the read-only `vdb_view::View` and answers first from
// its own objects, then from the view:
//
// - volatile names (memory only, never stored): slot lock files and
//   categories created by `mkdir` that hold no entry yet;
// - staged `<cat>/-MERGING-<pf>/` directories on scratch disk, published
//   with one `insert_entry` transaction at the rename;
// - rewrites and removals inside live entries.
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

/// The names Portage's lock code creates next to entries: kept in memory
/// only.
// Portage: locks.py, lockfile
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
    /// A live entry: new names Portage creates in it (`write_atomic` temps,
    /// `_umask_test` files) until they are renamed onto a field.
    Live(EntryKey),
}

/// The prefix Portage gives an entry while it is being merged.
// Portage: const.py, MERGING_IDENTIFIER
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
    /// scratch area. Its mtime follows filesystem rules: a create,
    /// unlink or rename inside it bumps it, a `utime` sets it.
    StagedDir {
        cat: String,
        path: PathBuf,
        mode: u32,
        mtime_ns: i128,
    },
    /// A file inside a staged directory, or a new name inside a live
    /// entry: the scratch file holds the bytes, mode and mtime.
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
    /// A stored file of a live entry opened for writing: the new bytes
    /// build up in a scratch copy and are stored with `replace_file` at
    /// `flush` (the `close` Portage sees).
    Live {
        key: EntryKey,
        name: String,
        file: std::fs::File,
        path: PathBuf,
        dirty: bool,
    },
}

struct RwState {
    next_obj: u64,
    objs: HashMap<u64, Obj>,
    /// This layer's names: `(parent, name) -> object id`. Hardlinks are two
    /// names with one id.
    names: BTreeMap<(Parent, String), u64>,
    /// Categories that exist only because of `mkdir` (no entry yet), or
    /// because an unmerge emptied them and Portage has not removed them yet,
    /// with their mtime.
    vol_cats: BTreeMap<String, i128>,
    /// Stored files Portage has unlinked from a live entry (an `rmtree` in
    /// progress): hidden from the view until `rmdir` deletes the entry.
    hidden: BTreeMap<EntryKey, std::collections::BTreeSet<String>>,
    handles: HashMap<u64, Fh>,
    next_fh: u64,
    /// Names for scratch files that are not in a staged directory.
    next_scratch: u64,
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
/// `dir_mtime_ns`: a stamp is trusted only when it equals the directory's
/// `st_mtime_ns`.
// Portage: vartree.py, _read_metadata_file
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

/// The category a category inode stands for.
fn cat_name(view: &View, ino: u64) -> Res<String> {
    match view.node_of(ino)? {
        Node::Cat(c) => Ok(c),
        _ => Err(ViewError::NotDir),
    }
}

pub struct RwView {
    view: View,
    db: Arc<dyn InstalledDb>,
    scratch: PathBuf,
    owner: Owner,
    st: Mutex<RwState>,
}

impl RwView {
    /// A read-write layer over `db`. `scratch` is emptied (a previous
    /// daemon's staged entries are discarded, like Portage removes a
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
                hidden: BTreeMap::new(),
                handles: HashMap::new(),
                next_fh: FH_BASE,
                next_scratch: 1,
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
            Node::Entry(k) => Some(Parent::Live(k)),
            _ => None,
        })
    }

    /// A scratch path for a file outside any staged directory.
    fn scratch_file(&self, st: &mut RwState) -> PathBuf {
        let n = st.next_scratch;
        st.next_scratch += 1;
        self.scratch.join(format!("live-{n}"))
    }

    /// Whether `key` is being removed (Portage's `rmtree` has unlinked some
    /// of its files).
    fn removing(st: &RwState, key: &EntryKey) -> bool {
        st.hidden.get(key).is_some_and(|h| !h.is_empty())
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
            Node::Entry(key) => {
                {
                    let st = lock(&self.st);
                    if let Some(&id) = st.names.get(&(Parent::Live(key.clone()), name.to_string()))
                    {
                        return self.obj_attr(id, &st.objs[&id]);
                    }
                    if st.hidden.get(&key).is_some_and(|h| h.contains(name)) {
                        return Err(ViewError::NoEnt);
                    }
                }
                self.view.lookup(parent, name)
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
            Node::File(key, name) => {
                if lock(&self.st)
                    .hidden
                    .get(&key)
                    .is_some_and(|h| h.contains(&name))
                {
                    return Err(ViewError::NoEnt);
                }
                self.view.getattr(ino)
            }
            _ => self.view.getattr(ino),
        }
    }

    pub fn readdir(&self, fh: u64) -> Res<Arc<Vec<DirEnt>>> {
        match lock(&self.st).handles.get(&fh) {
            Some(Fh::Dir(l)) => Ok(l.clone()),
            Some(_) => Err(ViewError::NotDir),
            None => Err(ViewError::BadHandle),
        }
    }

    /// `flush` (each `close`): store a written live file with one
    /// `replace_file` transaction. A failure is `EIO`, which `close`
    /// returns to the caller; the scratch copy stays for a later flush.
    pub fn flush(&self, fh: u64) -> Res<()> {
        let mut st = lock(&self.st);
        let Some(Fh::Live {
            key,
            name,
            path,
            dirty,
            ..
        }) = st.handles.get_mut(&fh)
        else {
            return Ok(());
        };
        if !*dirty {
            return Ok(());
        }
        let data = std::fs::read(&*path).map_err(io_err)?;
        let (key, name) = (key.clone(), name.clone());
        let commit = || -> portage_vdb::Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.replace_file(&key, &name, &data)?;
            txn.commit()
        };
        commit().map_err(|e| ViewError::Io(e.to_string()))?;
        if let Some(Fh::Live { dirty, .. }) = st.handles.get_mut(&fh) {
            *dirty = false;
        }
        Ok(())
    }

    /// `write(fh, offset, data)`: the bytes written.
    pub fn write(&self, fh: u64, off: u64, bytes: &[u8]) -> Res<u32> {
        let mut st = lock(&self.st);
        let id = match st.handles.get_mut(&fh) {
            Some(Fh::Vol(id)) => *id,
            Some(Fh::Staged(f)) => {
                f.write_all_at(bytes, off).map_err(io_err)?;
                return Ok(bytes.len() as u32);
            }
            Some(Fh::Live { file, dirty, .. }) => {
                file.write_all_at(bytes, off).map_err(io_err)?;
                *dirty = true;
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
            if let Parent::Live(entry) = &parent_key {
                // A stored file: hidden now, deleted with the entry at
                // `rmdir` (Portage's `shutil.rmtree`).
                self.lookup(parent, name)?;
                lock(&self.st)
                    .hidden
                    .entry(entry.clone())
                    .or_default()
                    .insert(name.to_string());
                return Ok(());
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
            // `write_atomic` inside a live entry (W4 `removeFromContents`,
            // `aux_update`): the temp's bytes replace the stored file.
            (Parent::Live(a), Parent::Live(b)) if a == b => {
                let is_temp = lock(&self.st)
                    .names
                    .contains_key(&(Parent::Live(a.clone()), name.to_string()));
                if is_temp {
                    self.replace_from_temp(a, name, newname)
                } else {
                    self.rename_stored_file(a, name, newname)
                }
            }
            // Portage: vartree.py, vardbapi.move_ent (the entry directory
            // moves, possibly into another category).
            (Parent::Cat(c1), Parent::Cat(c2))
                if !name.starts_with(MERGING)
                    && !newname.starts_with(MERGING)
                    && !is_volatile_name(name)
                    && !is_volatile_name(newname)
                    && !newname.is_empty() =>
            {
                self.move_entry(c1, name, c2, newname)
            }
            // Other renames are unmapped.
            _ => Err(ViewError::Perm),
        }
    }

    /// `move_ent`'s `_movefile(origpath, newpath)`: one transaction that
    /// deletes the live entry and inserts its image under the new key
    /// (counters stay as they are). The category directory the move
    /// leaves behind is Portage's to keep (or remove); like on disk, a
    /// destination that exists is `ENOTEMPTY`. Inodes follow the rename.
    fn move_entry(&self, cat: &str, pf: &str, new_cat: &str, new_pf: &str) -> Res<()> {
        let old = EntryKey::new(cat, pf);
        let new = EntryKey::new(new_cat, new_pf);
        let mut st = lock(&self.st);
        let cat_ino = self.view.ino_of(&Node::Cat(cat.to_string()));
        self.view.lookup(cat_ino, pf)?;
        let new_cat_ino = self.view.ino_of(&Node::Cat(new_cat.to_string()));
        if self.view.lookup(new_cat_ino, new_pf).is_ok() {
            return Err(ViewError::NotEmpty);
        }
        // Temps in the entry, or files Portage already unlinked (a removal
        // in progress), have no meaning across a move.
        if Self::removing(&st, &old)
            || st
                .names
                .keys()
                .any(|(p, _)| *p == Parent::Live(old.clone()))
        {
            return Err(ViewError::Perm);
        }
        let mut image = self.db.entry_image(&old)?.ok_or(ViewError::NoEnt)?;
        let names: Vec<String> = image.files.iter().map(|f| f.meta.name.clone()).collect();
        image.key = new.clone();
        let commit = || -> portage_vdb::Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.delete_entry(&old)?;
            txn.insert_entry(&image)?;
            txn.commit()
        };
        commit().map_err(|e| ViewError::Io(e.to_string()))?;
        st.vol_cats.remove(new_cat);
        drop(st);
        // Every inode the kernel may hold follows the entry.
        self.view.rebind(
            self.view.ino_of(&Node::Entry(old.clone())),
            Node::Entry(new.clone()),
        );
        for name in names {
            let ino = self.view.ino_of(&Node::File(old.clone(), name.clone()));
            self.view.rebind(ino, Node::File(new.clone(), name));
        }
        Ok(())
    }

    /// `os.rename(<old_pf>.ebuild, <new_pf>.ebuild)` inside a live entry
    /// (`move_ent`): a stored file takes a new name (the whole entry is
    /// rewritten in one transaction, stamp turned stale as any change of
    /// the directory does).
    fn rename_stored_file(&self, key: &EntryKey, name: &str, newname: &str) -> Res<()> {
        let st = lock(&self.st);
        if Self::removing(&st, key) {
            return Err(ViewError::NoEnt);
        }
        let mut image = self.db.entry_image(key)?.ok_or(ViewError::NoEnt)?;
        let Some(pos) = image.files.iter().position(|f| f.meta.name == name) else {
            return Err(ViewError::NoEnt);
        };
        image.files.retain(|f| f.meta.name != newname);
        let pos = image
            .files
            .iter()
            .position(|f| f.meta.name == name)
            .unwrap_or(pos);
        image.files[pos].meta.name = newname.to_string();
        image.files.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
        image.dir_mtime_ns = now_ns();
        if image.metadata_stamp == MetadataStamp::Valid {
            image.metadata_stamp = MetadataStamp::Stale;
        }
        let commit = || -> portage_vdb::Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.insert_entry(&image)?;
            txn.commit()
        };
        commit().map_err(|e| ViewError::Io(e.to_string()))?;
        drop(st);
        let ino = self.view.ino_of(&Node::File(key.clone(), name.to_string()));
        self.view
            .rebind(ino, Node::File(key.clone(), newname.to_string()));
        Ok(())
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

    /// The image Portage built in staged directory `dir`.
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
        // A rename onto a non-empty directory fails on a filesystem;
        // Portage's `dblink.treewalk` removes the live entry first.
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
        // holds dentries for the files Portage just wrote, and
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

    /// `rename(<entry>/<temp>, <entry>/<field>)` inside a live entry.
    fn replace_from_temp(&self, key: &EntryKey, temp: &str, field: &str) -> Res<()> {
        let mut st = lock(&self.st);
        let tkey = (Parent::Live(key.clone()), temp.to_string());
        let id = *st.names.get(&tkey).ok_or(ViewError::NoEnt)?;
        let Some(Obj::StagedFile { path }) = st.objs.get(&id) else {
            return Err(ViewError::Perm);
        };
        if Self::removing(&st, key) {
            return Err(ViewError::NoEnt);
        }
        // A rename carries the temp's bytes, mode and mtime, and moves the
        // directory mtime, so a valid `metadata` stamp turns stale, as on
        // disk. The whole entry is rewritten in one transaction.
        let md = std::fs::metadata(path).map_err(io_err)?;
        let data = std::fs::read(path).map_err(io_err)?;
        let path = path.clone();
        let mut image = self.db.entry_image(key)?.ok_or(ViewError::NoEnt)?;
        let file = EntryFile {
            meta: FileMeta {
                name: field.to_string(),
                len: data.len() as u64,
                mode: md.mode(),
                mtime_ns: mtime_of(&md),
            },
            data,
        };
        match image.files.iter_mut().find(|f| f.meta.name == field) {
            Some(f) => *f = file,
            None => {
                image.files.push(file);
                image.files.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
            }
        }
        image.dir_mtime_ns = now_ns();
        if image.metadata_stamp == MetadataStamp::Valid {
            image.metadata_stamp = MetadataStamp::Stale;
        }
        let commit = || -> portage_vdb::Result<()> {
            let mut txn = self.db.begin_write()?;
            txn.insert_entry(&image)?;
            txn.commit()
        };
        commit().map_err(|e| ViewError::Io(e.to_string()))?;
        st.names.remove(&tkey);
        st.objs.remove(&id);
        drop(st);
        let _ = std::fs::remove_file(path);
        // The renamed file keeps its inode.
        self.view
            .rebind(self.obj_ino(id), Node::File(key.clone(), field.to_string()));
        Ok(())
    }

    /// `link(ino, newparent, newname)`: hardlinks of volatile files only
    /// (Portage links the lock file to a hardlock name and compares the
    /// inodes).
    // Portage: locks.py, _lockfile_was_removed
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
        // `utime` on a category or the root: accepted, no lasting effect;
        // the view derives those mtimes.
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
    use super::ops::Env;
    use super::*;

    #[test]
    fn stamp_state_follows_the_portage_rule() {
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
}
