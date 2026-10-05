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
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use portage_vdb::InstalledDb;

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
}

enum Obj {
    /// A volatile file (lock files); `nlink` counts its names.
    Vol {
        data: Vec<u8>,
        mode: u32,
        owner: Owner,
        mtime_ns: i128,
        nlink: u32,
    },
}

enum Fh {
    /// A handle of the read-only view.
    Base(u64),
    /// A directory listing of this layer.
    Dir(Arc<Vec<DirEnt>>),
    /// A volatile file.
    Vol(u64),
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

pub struct RwView {
    view: View,
    #[allow(dead_code)] // used by the publish steps (S2, S3)
    db: Arc<dyn InstalledDb>,
    #[allow(dead_code)] // staged entries live here (S2)
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
            None => self.owner,
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

    fn obj_attr(&self, id: u64, obj: &Obj) -> Attr {
        match obj {
            Obj::Vol {
                data,
                mode,
                mtime_ns,
                nlink,
                ..
            } => Attr {
                ino: self.obj_ino(id),
                kind: Kind::File,
                perm: (*mode & 0o7777) as u16,
                size: data.len() as u64,
                nlink: *nlink,
                mtime_ns: *mtime_ns,
            },
        }
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

    /// The category a directory inode stands for, if it is one (stored or
    /// volatile).
    fn cat_of(&self, ino: u64) -> Res<Option<String>> {
        Ok(match self.view.node_of(ino)? {
            Node::Cat(c) => Some(c),
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
                        return Ok(self.obj_attr(id, &st.objs[&id]));
                    }
                }
                self.view.lookup(parent, name)
            }
            Node::Obj(_) => Err(ViewError::NotDir),
            _ => self.view.lookup(parent, name),
        }
    }

    pub fn getattr(&self, ino: u64) -> Res<Attr> {
        match self.view.node_of(ino)? {
            Node::Obj(id) => {
                let st = lock(&self.st);
                let obj = st.objs.get(&id).ok_or(ViewError::NoEnt)?;
                Ok(self.obj_attr(id, obj))
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
        let mut listing: Vec<DirEnt> = match self.view.opendir(ino) {
            Ok(vfh) => {
                let l = self.view.readdir(vfh);
                self.view.release(vfh);
                l?.as_ref().clone()
            }
            Err(ViewError::NoEnt) => {
                // A volatile category: `.` and `..` only, children below.
                let a = self.getattr(ino)?;
                vec![
                    DirEnt {
                        ino: a.ino,
                        kind: Kind::Dir,
                        name: ".".into(),
                    },
                    DirEnt {
                        ino: ROOT_INO,
                        kind: Kind::Dir,
                        name: "..".into(),
                    },
                ]
            }
            Err(e) => return Err(e),
        };
        let mut st = lock(&self.st);
        match &node {
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
            }
            Node::Cat(cat) => {
                let parent = Parent::Cat(cat.clone());
                for ((p, name), id) in st.names.range((parent.clone(), String::new())..) {
                    if p != &parent {
                        break;
                    }
                    listing.push(DirEnt {
                        ino: self.obj_ino(*id),
                        kind: Kind::File,
                        name: name.clone(),
                    });
                }
            }
            _ => {}
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
                match st.objs.get_mut(&id).ok_or(ViewError::NoEnt)? {
                    Obj::Vol { data, mtime_ns, .. } => {
                        if flags & libc::O_TRUNC != 0 && open_wants_write(flags) {
                            data.clear();
                            *mtime_ns = now_ns();
                        }
                    }
                }
                Ok(Self::new_fh(&mut st, Fh::Vol(id)))
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
                None => Err(ViewError::Stale),
            },
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
        let Some(cat) = self.cat_of(parent)? else {
            return Err(ViewError::Perm);
        };
        if !is_volatile_name(name) {
            return Err(ViewError::Perm);
        }
        self.getattr(parent)?;
        let mut st = lock(&self.st);
        let key = (Parent::Cat(cat), name.to_string());
        if let Some(&id) = st.names.get(&key) {
            if flags & libc::O_EXCL != 0 {
                return Err(ViewError::Exists);
            }
            drop(st);
            let fh = self.open(self.obj_ino(id), flags)?;
            return Ok((self.getattr(self.obj_ino(id))?, fh));
        }
        let id = st.next_obj;
        st.next_obj += 1;
        let obj = Obj::Vol {
            data: Vec::new(),
            mode: mode & 0o7777,
            owner: self.owner,
            mtime_ns: now_ns(),
            nlink: 1,
        };
        let attr = self.obj_attr(id, &obj);
        st.objs.insert(id, obj);
        st.names.insert(key, id);
        let fh = Self::new_fh(&mut st, Fh::Vol(id));
        Ok((attr, fh))
    }

    /// `write(fh, offset, data)`: the bytes written.
    pub fn write(&self, fh: u64, off: u64, bytes: &[u8]) -> Res<u32> {
        let mut st = lock(&self.st);
        let id = match st.handles.get(&fh) {
            Some(Fh::Vol(id)) => *id,
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
        }
        Ok(bytes.len() as u32)
    }

    /// `mkdir(parent, name, mode)`: a category at the root.
    pub fn mkdir(&self, parent: u64, name: &str, _mode: u32) -> Res<Attr> {
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
            // Staged `-MERGING-<pf>` directories are S2.
            _ => Err(ViewError::Perm),
        }
    }

    /// `unlink(parent, name)`.
    pub fn unlink(&self, parent: u64, name: &str) -> Res<()> {
        let Some(cat) = self.cat_of(parent)? else {
            return Err(ViewError::Perm);
        };
        let mut st = lock(&self.st);
        let key = (Parent::Cat(cat), name.to_string());
        let Some(id) = st.names.remove(&key) else {
            drop(st);
            // A stored entry is a directory; anything else is not here.
            return match self.view.lookup(parent, name) {
                Ok(_) => Err(ViewError::IsDir),
                Err(e) => Err(e),
            };
        };
        let gone = match st.objs.get_mut(&id) {
            Some(Obj::Vol { nlink, .. }) => {
                *nlink -= 1;
                *nlink == 0
            }
            None => true,
        };
        if gone {
            st.objs.remove(&id);
        }
        Ok(())
    }

    /// `rmdir(parent, name)`: an empty category.
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
            // Entries (S3) and staged directories (S2).
            _ => Err(ViewError::Perm),
        }
    }

    /// `rename(parent, name, newparent, newname)`.
    pub fn rename(&self, parent: u64, name: &str, newparent: u64, newname: &str) -> Res<()> {
        let (Some(cat), Some(newcat)) = (self.cat_of(parent)?, self.cat_of(newparent)?) else {
            return Err(ViewError::Perm);
        };
        if !(is_volatile_name(name) && is_volatile_name(newname)) {
            // Staged publishes and live rewrites are S2 and S3.
            return Err(ViewError::Perm);
        }
        let mut st = lock(&self.st);
        let id = st
            .names
            .remove(&(Parent::Cat(cat), name.to_string()))
            .ok_or(ViewError::NoEnt)?;
        if let Some(old) = st
            .names
            .insert((Parent::Cat(newcat), newname.to_string()), id)
            && old != id
            && let Some(Obj::Vol { nlink, .. }) = st.objs.get_mut(&old)
        {
            *nlink -= 1;
            if *nlink == 0 {
                st.objs.remove(&old);
            }
        }
        Ok(())
    }

    /// `link(ino, newparent, newname)`: hardlinks of volatile files only
    /// (real `_lockfile_was_removed` links the lock file to a hardlock
    /// name and compares the inodes).
    pub fn link(&self, ino: u64, newparent: u64, newname: &str) -> Res<Attr> {
        let id = self.obj_id(ino).ok_or(ViewError::Perm)?;
        let cat = self.cat_of(newparent)?.ok_or(ViewError::Perm)?;
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
        }
        st.names.insert(key, id);
        Ok(self.obj_attr(id, &st.objs[&id]))
    }

    /// `setattr(ino, …)`.
    pub fn setattr(&self, ino: u64, s: &SetAttr) -> Res<Attr> {
        if let Some(id) = self.obj_id(ino) {
            {
                let mut st = lock(&self.st);
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
                }
            }
            return self.getattr(ino);
        }
        let attr = self.getattr(ino)?;
        let foreign = s.uid.is_some_and(|u| u != self.owner.uid)
            || s.gid.is_some_and(|g| g != self.owner.gid);
        if foreign || s.mode.is_some() || s.size.is_some() {
            return Err(ViewError::Perm);
        }
        // `utime` on a category or the root (real `_bump_mtime`): accepted,
        // no lasting effect; the view derives those mtimes.
        Ok(attr)
    }
}

// The tests replay scripts on a database backend; without one there is
// nothing to write to.
#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
#[path = "vdb_rw/ops.rs"]
mod ops;

#[cfg(all(test, any(feature = "vdb-sqlite", feature = "vdb-redb")))]
mod tests {
    use super::ops::{Env, apply_fs, apply_rw, assert_same, envs, parse};
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
}
