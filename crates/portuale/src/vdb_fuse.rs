// The FUSE adapter of `portuale vdb mount` (feat#157 / backlog #305, plan
// S7.2): a thin translation from the `fuser` callbacks to the pure
// `vdb_view::View`, which holds every decision (tree, inodes, attributes,
// generation pinning, EROFS, the generated `metadata` file) and is unit
// tested without a mount. With `--rw` (#317) the callbacks go to
// `vdb_rw::RwView` instead, the read-write layer over the same view. This file only converts types and errno values;
// it needs `/dev/fuse` and `fusermount3` to run and is exercised by the S7.4
// commands on a FUSE-capable host, not by `cargo test`.
//
// `fuser` is built without its `libfuse` feature: it speaks the kernel
// protocol itself and mounts through the `fusermount3` helper, so no C
// library is linked.

use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fuser::{
    AccessFlags, BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem,
    FopenFlags, Generation, INodeNo, InitFlags, KernelConfig, LockOwner, MountOption, OpenFlags,
    RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, ReplyStatfs, ReplyWrite, Request, Session, SessionACL, TimeOrNow, WriteFlags,
};
use portage_vdb::InstalledDb;

use crate::vdb_rw::{Owner, RwView, SetAttr};
use crate::vdb_view::{self, Attr, Kind, View, ViewError, WriteOp};

/// Attribute and entry validity handed to the kernel. Short: a commit by
/// another process is seen within this time.
const TTL: Duration = Duration::from_secs(1);

/// What a mount serves: the read-only view, or the read-write layer
/// (`--rw`, #317) over it.
enum Mode {
    Ro(View),
    Rw(Box<RwView>),
}

struct Fs {
    mode: Mode,
    uid: u32,
    gid: u32,
}

/// `--rw`: where staged entries live while real Portage writes them, and
/// the ROOT whose stores (world, counter, …) are imported at unmount.
pub struct RwOpts {
    pub scratch: PathBuf,
    pub root: PathBuf,
}

impl Fs {
    fn attr(&self, a: &Attr) -> FileAttr {
        let t = vdb_view::system_time(a.mtime_ns);
        let (uid, gid) = match &self.mode {
            Mode::Ro(_) => (self.uid, self.gid),
            Mode::Rw(rw) => {
                let o = rw.owner_of(a.ino);
                (o.uid, o.gid)
            }
        };
        FileAttr {
            ino: INodeNo(a.ino),
            size: a.size,
            blocks: a.size.div_ceil(512),
            atime: t,
            mtime: t,
            ctime: t,
            crtime: t,
            kind: match a.kind {
                Kind::Dir => FileType::Directory,
                Kind::File => FileType::RegularFile,
            },
            perm: a.perm,
            nlink: a.nlink,
            uid,
            gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }

    fn lookup_(&self, parent: u64, name: &str) -> Result<Attr, ViewError> {
        match &self.mode {
            Mode::Ro(v) => v.lookup(parent, name),
            Mode::Rw(rw) => rw.lookup(parent, name),
        }
    }

    fn getattr_(&self, ino: u64) -> Result<Attr, ViewError> {
        match &self.mode {
            Mode::Ro(v) => v.getattr(ino),
            Mode::Rw(rw) => rw.getattr(ino),
        }
    }

    fn release_(&self, fh: u64) {
        match &self.mode {
            Mode::Ro(v) => v.release(fh),
            Mode::Rw(rw) => {
                let _ = rw.release(fh);
            }
        }
    }

    /// The read-write layer, or `EROFS` for `op` on a read-only mount.
    fn rw(&self, op: WriteOp) -> Result<&RwView, Errno> {
        match &self.mode {
            Mode::Ro(_) => Err(denied(op)),
            Mode::Rw(rw) => Ok(rw),
        }
    }

    fn entry(&self, r: Result<Attr, ViewError>, reply: ReplyEntry) {
        match r {
            Ok(a) => reply.entry(&TTL, &self.attr(&a), Generation(0)),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn empty(r: Result<(), ViewError>, reply: ReplyEmpty) {
        match r {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(errno(&e)),
        }
    }
}

fn errno(e: &ViewError) -> Errno {
    Errno::from_i32(e.errno())
}

fn denied(op: WriteOp) -> Errno {
    errno(&vdb_view::deny(op))
}

fn ns_of(t: TimeOrNow) -> i128 {
    let st = match t {
        TimeOrNow::SpecificTime(st) => st,
        TimeOrNow::Now => std::time::SystemTime::now(),
    };
    match st.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

impl Filesystem for Fs {
    fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> io::Result<()> {
        if let Mode::Rw(_) = self.mode {
            // Pass O_TRUNC to `open` instead of a separate setattr(size=0)
            // first: real's in-place `> environment.bz2` (pkg_postinst)
            // is then one live rewrite, stored at `close`.
            let _ = config.add_capabilities(InitFlags::FUSE_ATOMIC_O_TRUNC);
        }
        Ok(())
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            return reply.error(Errno::ENOENT);
        };
        self.entry(self.lookup_(u64::from(parent), name), reply);
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, fh: Option<FileHandle>, reply: ReplyAttr) {
        let r = match (&self.mode, fh) {
            (Mode::Ro(v), Some(fh)) => v.getattr_fh(u64::from(fh)),
            (Mode::Rw(rw), Some(fh)) => rw.getattr_fh(u64::from(ino), u64::from(fh)),
            (_, None) => self.getattr_(u64::from(ino)),
        };
        match r {
            Ok(a) => reply.attr(&TTL, &self.attr(&a)),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let rw = match self.rw(WriteOp::Setattr) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let s = SetAttr {
            mode,
            uid,
            gid,
            size,
            mtime_ns: mtime.map(ns_of),
        };
        match rw.setattr(u64::from(ino), &s) {
            Ok(a) => reply.attr(&TTL, &self.attr(&a)),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn readlink(&self, _req: &Request, _ino: INodeNo, reply: ReplyData) {
        // There are no symbolic links in this tree.
        reply.error(Errno::EINVAL);
    }

    fn mknod(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        // Real Portage creates regular files with open(O_CREAT), which
        // arrives as `create`; a mknod is outside the mapping.
        match self.rw(WriteOp::Mknod) {
            Ok(_) => reply.error(Errno::EPERM),
            Err(e) => reply.error(e),
        }
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let rw = match self.rw(WriteOp::Mkdir) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let Some(name) = name.to_str() else {
            return reply.error(Errno::EPERM);
        };
        self.entry(rw.mkdir(u64::from(parent), name, mode), reply);
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let rw = match self.rw(WriteOp::Unlink) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let Some(name) = name.to_str() else {
            return reply.error(Errno::ENOENT);
        };
        Self::empty(rw.unlink(u64::from(parent), name), reply);
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let rw = match self.rw(WriteOp::Rmdir) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let Some(name) = name.to_str() else {
            return reply.error(Errno::ENOENT);
        };
        Self::empty(rw.rmdir(u64::from(parent), name), reply);
    }

    fn symlink(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _link_name: &OsStr,
        _target: &Path,
        reply: ReplyEntry,
    ) {
        match self.rw(WriteOp::Symlink) {
            Ok(_) => reply.error(Errno::EPERM),
            Err(e) => reply.error(e),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        let rw = match self.rw(WriteOp::Rename) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let (Some(name), Some(newname)) = (name.to_str(), newname.to_str()) else {
            return reply.error(Errno::EPERM);
        };
        if !flags.is_empty() {
            // RENAME_EXCHANGE / RENAME_NOREPLACE are not used by Portage.
            return reply.error(Errno::EINVAL);
        }
        Self::empty(
            rw.rename(u64::from(parent), name, u64::from(newparent), newname),
            reply,
        );
    }

    fn link(
        &self,
        _req: &Request,
        ino: INodeNo,
        newparent: INodeNo,
        newname: &OsStr,
        reply: ReplyEntry,
    ) {
        let rw = match self.rw(WriteOp::Link) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let Some(newname) = newname.to_str() else {
            return reply.error(Errno::EPERM);
        };
        self.entry(
            rw.link(u64::from(ino), u64::from(newparent), newname),
            reply,
        );
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let r = match &self.mode {
            Mode::Ro(v) => v.open(u64::from(ino), vdb_view::open_wants_write(flags.0)),
            Mode::Rw(rw) => rw.open(u64::from(ino), flags.0),
        };
        match r {
            // Read-only: the content of a generation never changes under a
            // handle, so the kernel may keep its page cache for it.
            // Read-write: written files change, so no cache is kept.
            Ok(fh) => reply.opened(
                FileHandle(fh),
                match self.mode {
                    Mode::Ro(_) => FopenFlags::empty(),
                    Mode::Rw(_) => FopenFlags::FOPEN_DIRECT_IO,
                },
            ),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let r = match &self.mode {
            Mode::Ro(v) => v.read(u64::from(fh), offset, size as usize),
            Mode::Rw(rw) => rw.read(u64::from(fh), offset, size as usize),
        };
        match r {
            Ok(b) => reply.data(&b),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let rw = match self.rw(WriteOp::Write) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        match rw.write(u64::from(fh), offset, data) {
            Ok(n) => reply.written(n),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        match &self.mode {
            Mode::Ro(_) => reply.ok(),
            // A written live file is stored here, so `close` sees an error.
            Mode::Rw(rw) => Self::empty(rw.flush(u64::from(fh)), reply),
        }
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        match &self.mode {
            Mode::Ro(v) => {
                v.release(u64::from(fh));
                reply.ok();
            }
            // A release can publish (a live-entry rewrite, S3).
            Mode::Rw(rw) => Self::empty(rw.release(u64::from(fh)), reply),
        }
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        // Durability comes with the publishing transaction.
        reply.ok();
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        let r = match &self.mode {
            Mode::Ro(v) => v.opendir(u64::from(ino)),
            Mode::Rw(rw) => rw.opendir(u64::from(ino)),
        };
        match r {
            Ok(fh) => reply.opened(FileHandle(fh), FopenFlags::empty()),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let r = match &self.mode {
            Mode::Ro(v) => v.readdir(u64::from(fh)),
            Mode::Rw(rw) => rw.readdir(u64::from(fh)),
        };
        let listing = match r {
            Ok(l) => l,
            Err(e) => return reply.error(errno(&e)),
        };
        for (i, ent) in listing.iter().enumerate().skip(offset as usize) {
            let kind = match ent.kind {
                Kind::Dir => FileType::Directory,
                Kind::File => FileType::RegularFile,
            };
            // The offset of an entry is the index of the one after it.
            if reply.add(INodeNo(ent.ino), (i + 1) as u64, kind, &ent.name) {
                break;
            }
        }
        reply.ok();
    }

    fn releasedir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        reply: ReplyEmpty,
    ) {
        self.release_(u64::from(fh));
        reply.ok();
    }

    fn fsyncdir(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        reply.statfs(0, 0, 0, 0, 0, 4096, 255, 0);
    }

    fn setxattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _name: &OsStr,
        _value: &[u8],
        _flags: i32,
        _position: u32,
        reply: ReplyEmpty,
    ) {
        match self.rw(WriteOp::Setxattr) {
            Ok(_) => reply.error(Errno::EPERM),
            Err(e) => reply.error(e),
        }
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        match self.rw(WriteOp::Removexattr) {
            Ok(_) => reply.error(Errno::EPERM),
            Err(e) => reply.error(e),
        }
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        // Read and execute-search are granted by the mode bits the kernel
        // checks itself; a write probe is the one thing this answers.
        let exists = self.getattr_(u64::from(ino));
        match (exists, &self.mode) {
            (Err(e), _) => reply.error(errno(&e)),
            (Ok(_), Mode::Ro(_)) if mask.contains(AccessFlags::W_OK) => {
                reply.error(denied(WriteOp::Write))
            }
            (Ok(_), _) => reply.ok(),
        }
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        let rw = match self.rw(WriteOp::Create) {
            Ok(rw) => rw,
            Err(e) => return reply.error(e),
        };
        let Some(name) = name.to_str() else {
            return reply.error(Errno::EPERM);
        };
        match rw.create(u64::from(parent), name, mode, flags) {
            Ok((a, fh)) => reply.created(
                &TTL,
                &self.attr(&a),
                Generation(0),
                FileHandle(fh),
                FopenFlags::FOPEN_DIRECT_IO,
            ),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn fallocate(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _length: u64,
        _mode: i32,
        reply: ReplyEmpty,
    ) {
        match self.rw(WriteOp::Fallocate) {
            Ok(_) => reply.error(Errno::from_i32(libc::EOPNOTSUPP)),
            Err(e) => reply.error(e),
        }
    }

    fn copy_file_range(
        &self,
        _req: &Request,
        _ino_in: INodeNo,
        _fh_in: FileHandle,
        _offset_in: u64,
        _ino_out: INodeNo,
        _fh_out: FileHandle,
        _offset_out: u64,
        _len: u64,
        _flags: fuser::CopyFileRangeFlags,
        reply: fuser::ReplyWrite,
    ) {
        // Real `shutil.copyfile` tries copy_file_range first (S0) and falls
        // back to plain writes on EOPNOTSUPP.
        match self.rw(WriteOp::CopyFileRange) {
            Ok(_) => reply.error(Errno::from_i32(libc::EOPNOTSUPP)),
            Err(e) => reply.error(e),
        }
    }
}

/// Block SIGINT, SIGTERM and SIGHUP in the calling thread (and so in every
/// thread it spawns afterwards) and return the set, for `sigwait`.
fn block_termination_signals() -> libc::sigset_t {
    // SAFETY: plain libc signal-set calls on a local, initialised set.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaddset(&mut set, sig);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

/// Mount `db` read-only at `mountpoint` and serve it until it is unmounted
/// (`fusermount3 -u`) or SIGINT/SIGTERM/SIGHUP arrives (then it unmounts
/// itself). `on_mounted` runs once the mount exists, before serving.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn serve(
    db: Arc<dyn InstalledDb>,
    mountpoint: &Path,
    fsname: &str,
    allow_other: bool,
    rw: Option<&RwOpts>,
    on_mounted: impl FnOnce(),
) -> io::Result<()> {
    let set = block_termination_signals();
    let mut cfg = Config::default();
    cfg.mount_options = vec![
        if rw.is_some() {
            MountOption::RW
        } else {
            MountOption::RO
        },
        MountOption::NoSuid,
        MountOption::NoDev,
        MountOption::NoAtime,
        MountOption::FSName(fsname.to_string()),
        MountOption::Subtype("portuale-vdb".to_string()),
    ];
    if allow_other {
        cfg.acl = SessionACL::All;
    }
    // SAFETY: geteuid/getegid never fail and touch no memory.
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let import = rw.map(|o| (o.root.clone(), db.clone()));
    let mode = match rw {
        None => Mode::Ro(View::new(db)),
        Some(o) => Mode::Rw(Box::new(RwView::new(
            db,
            o.scratch.clone(),
            Owner { uid, gid },
        )?)),
    };
    let fs = Fs { mode, uid, gid };
    let mut session = Session::new(fs, mountpoint, &cfg)?;
    on_mounted();
    let mut unmounter = session.unmount_callable();
    std::thread::Builder::new()
        .name("vdb-mount-signals".into())
        .spawn(move || {
            let mut sig = 0;
            // SAFETY: `set` is initialised; `sig` is a valid out pointer.
            if unsafe { libc::sigwait(&set, &mut sig) } == 0 {
                let _ = unmounter.unmount();
            }
        })?;
    session.run()?;
    // `--rw`: real emerge wrote world, the counter, … on disk under ROOT
    // while the entries went to the database; take them in now (#317 S4).
    if let Some((root, db)) = import {
        match crate::vdb_cmd::import_stores(&root, &*db) {
            Ok(rep) => eprintln!("portuale vdb mount: imported {rep} from {}", root.display()),
            Err(e) => {
                eprintln!(
                    "portuale vdb mount: importing the stores from {} failed: {e}; run \
                     `portuale vdb import-stores files:{} …` by hand",
                    root.display(),
                    root.display()
                );
                return Err(io::Error::other(e.to_string()));
            }
        }
    }
    Ok(())
}

/// Run `serve` in a background process: fork, `setsid`, report the mount
/// result to the parent over a pipe, then detach from the terminal. The
/// parent returns `Ok(())` once the mount exists, or the child's error.
///
/// `open` is called in the child (a redb or sqlite handle must not cross a
/// `fork`).
pub fn serve_background(
    open: impl FnOnce() -> Result<Arc<dyn InstalledDb>, String>,
    mountpoint: &Path,
    fsname: &str,
    allow_other: bool,
    rw: Option<&RwOpts>,
) -> Result<(), String> {
    // `--rw`: the daemon's last words (the store import at unmount) go to
    // a file next to the scratch directory, which the next mount of the
    // same database clears but this file survives (#320).
    let log = rw.map(|o| rw_log_path(&o.scratch));
    let mut fds = [0i32; 2];
    // SAFETY: `fds` is a valid two-element array.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(format!("pipe: {}", io::Error::last_os_error()));
    }
    let (rd, wr) = (fds[0], fds[1]);
    // SAFETY: the process is single-threaded here (no runtime was started
    // by the `vdb` applet), so the child may keep running Rust code.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(format!("fork: {}", io::Error::last_os_error()));
    }
    if pid > 0 {
        // Parent: wait for the child's verdict.
        // SAFETY: both ends are open descriptors this function owns.
        unsafe { libc::close(wr) };
        let mut f = {
            use std::os::fd::FromRawFd as _;
            // SAFETY: `rd` is an open descriptor owned by nobody else.
            unsafe { std::fs::File::from_raw_fd(rd) }
        };
        let mut buf = Vec::new();
        let _ = f.read_to_end(&mut buf);
        return match buf.first() {
            Some(b'0') => {
                if let Some(log) = &log {
                    eprintln!(
                        "portuale vdb mount: the unmount report (stores imported, or why not) \
                         goes to {}",
                        log.display()
                    );
                }
                Ok(())
            }
            Some(_) => Err(String::from_utf8_lossy(&buf[1..]).into_owned()),
            None => Err("the mount process exited before the mount was ready".into()),
        };
    }
    // Child.
    // SAFETY: closing the read end and starting a new session.
    unsafe {
        libc::close(rd);
        libc::setsid();
    }
    let reported = std::cell::Cell::new(false);
    let report = |ok: bool, msg: &str| {
        // One verdict only: `wr` is closed after it and its number reused.
        if reported.replace(true) {
            return;
        }
        let mut data = vec![if ok { b'0' } else { b'1' }];
        data.extend_from_slice(msg.as_bytes());
        // SAFETY: `wr` is open; `data` is a valid buffer of its length.
        unsafe {
            libc::write(wr, data.as_ptr().cast(), data.len());
            libc::close(wr);
        }
    };
    let code = match open() {
        Err(e) => {
            report(false, &e);
            2
        }
        Ok(db) => {
            let r = serve(db, mountpoint, fsname, allow_other, rw, || {
                report(true, "");
                detach_from_terminal(log.as_deref());
            });
            match r {
                Ok(()) => 0,
                Err(e) => {
                    // Before `on_mounted` ran the parent is still waiting.
                    report(false, &e.to_string());
                    2
                }
            }
        }
    };
    // SAFETY: leave without running the parent's atexit handlers or
    // flushing its buffers a second time.
    unsafe { libc::_exit(code) }
}

/// The unmount-report file of a `--rw` mount: the scratch directory's
/// name plus `.log`, beside it (the scratch directory itself is emptied
/// by the next mount).
fn rw_log_path(scratch: &Path) -> PathBuf {
    let mut name = scratch.as_os_str().to_os_string();
    name.push(".log");
    PathBuf::from(name)
}

/// Point stdin at `/dev/null` and stdout/stderr at `log` (truncated; or
/// `/dev/null` when there is none or it cannot be opened), and leave the
/// working directory, so the daemon holds neither the terminal nor a
/// directory.
fn detach_from_terminal(log: Option<&Path>) {
    use std::os::fd::AsRawFd as _;
    let open_null = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
    };
    if let Ok(null) = open_null() {
        // SAFETY: duplicating an open descriptor onto a standard one.
        unsafe { libc::dup2(null.as_raw_fd(), 0) };
        let out = log
            .and_then(|p| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(p)
                    .ok()
            })
            .unwrap_or(null);
        for fd in 1..=2 {
            // SAFETY: as above.
            unsafe { libc::dup2(out.as_raw_fd(), fd) };
        }
    }
    let _ = std::env::set_current_dir("/");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unmount_report_file_sits_beside_the_scratch_directory() {
        assert_eq!(
            rw_log_path(Path::new("/var/tmp/portuale-vdb-rw-00ab")),
            PathBuf::from("/var/tmp/portuale-vdb-rw-00ab.log")
        );
    }

    /// #320: a detached daemon's stderr goes to the log file, so the line
    /// printed after `session.run()` returns is not lost. Run in a child
    /// (the redirect replaces the process's own descriptors).
    #[test]
    fn a_detached_daemon_writes_its_stderr_to_the_log_file() {
        if let Some(log) = std::env::var_os("PORTUALE_DETACH_CHILD") {
            detach_from_terminal(Some(Path::new(&log)));
            eprintln!("portuale vdb mount: imported nothing");
            return;
        }
        let dir = portage_util::TempDir::new("portuale-detach-test").keep();
        let log = dir.join("mount.log");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--test-threads=1",
                "--exact",
                "vdb_fuse::tests::a_detached_daemon_writes_its_stderr_to_the_log_file",
                "--nocapture",
            ])
            .env("PORTUALE_DETACH_CHILD", &log)
            .status()
            .unwrap();
        assert!(status.success());
        // (The test harness's own result line follows it in the file.)
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .starts_with("portuale vdb mount: imported nothing\n")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
