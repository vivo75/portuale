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

pub struct RwView {
    view: View,
    db: Arc<dyn InstalledDb>,
    scratch: PathBuf,
    owner: Owner,
    st: Mutex<RwState>,
}

impl RwView {
    fn new_fh(st: &mut RwState, h: Fh) -> u64 {
        let fh = st.next_fh;
        st.next_fh += 1;
        st.handles.insert(fh, h);
        fh
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
