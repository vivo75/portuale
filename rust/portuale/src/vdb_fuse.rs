// The FUSE adapter of `portuale vdb mount` (feat#157 / backlog #305, plan
// S7.2): a thin translation from the `fuser` callbacks to the pure
// `vdb_view::View`, which holds every decision (tree, inodes, attributes,
// generation pinning, EROFS, the generated `metadata` file) and is unit
// tested without a mount. This file only converts types and errno values;
// it needs `/dev/fuse` and `fusermount3` to run and is exercised by the S7.4
// commands on a FUSE-capable host, not by `cargo test`.
//
// `fuser` is built without its `libfuse` feature: it speaks the kernel
// protocol itself and mounts through the `fusermount3` helper, so no C
// library is linked.

use std::ffi::OsStr;
use std::io::{self, Read as _};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fuser::{
    AccessFlags, BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem,
    FopenFlags, Generation, INodeNo, LockOwner, MountOption, OpenFlags, RenameFlags, ReplyAttr,
    ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyStatfs,
    ReplyWrite, Request, Session, SessionACL, TimeOrNow, WriteFlags,
};
use portage_vdb::InstalledDb;

use crate::vdb_view::{self, Attr, Kind, View, ViewError, WriteOp};

/// Attribute and entry validity handed to the kernel. Short: a commit by
/// another process is seen within this time.
const TTL: Duration = Duration::from_secs(1);

struct Fs {
    view: View,
    uid: u32,
    gid: u32,
}

impl Fs {
    fn attr(&self, a: &Attr) -> FileAttr {
        let t = vdb_view::system_time(a.mtime_ns);
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
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
    }
}

fn errno(e: &ViewError) -> Errno {
    Errno::from_i32(e.errno())
}

fn denied(op: WriteOp) -> Errno {
    errno(&vdb_view::deny(op))
}

impl Filesystem for Fs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(name) = name.to_str() else {
            return reply.error(Errno::ENOENT);
        };
        match self.view.lookup(u64::from(parent), name) {
            Ok(a) => reply.entry(&TTL, &self.attr(&a), Generation(0)),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, fh: Option<FileHandle>, reply: ReplyAttr) {
        let r = match fh {
            Some(fh) => self.view.getattr_fh(u64::from(fh)),
            None => self.view.getattr(u64::from(ino)),
        };
        match r {
            Ok(a) => reply.attr(&TTL, &self.attr(&a)),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        reply.error(denied(WriteOp::Setattr));
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
        reply.error(denied(WriteOp::Mknod));
    }

    fn mkdir(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        reply.error(denied(WriteOp::Mkdir));
    }

    fn unlink(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(denied(WriteOp::Unlink));
    }

    fn rmdir(&self, _req: &Request, _parent: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(denied(WriteOp::Rmdir));
    }

    fn symlink(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _link_name: &OsStr,
        _target: &Path,
        reply: ReplyEntry,
    ) {
        reply.error(denied(WriteOp::Symlink));
    }

    fn rename(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _newparent: INodeNo,
        _newname: &OsStr,
        _flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        reply.error(denied(WriteOp::Rename));
    }

    fn link(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _newparent: INodeNo,
        _newname: &OsStr,
        reply: ReplyEntry,
    ) {
        reply.error(denied(WriteOp::Link));
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        match self
            .view
            .open(u64::from(ino), vdb_view::open_wants_write(flags.0))
        {
            // The content of a generation never changes under a handle, so
            // the kernel may keep its page cache for it.
            Ok(fh) => reply.opened(FileHandle(fh), FopenFlags::empty()),
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
        match self.view.read(u64::from(fh), offset, size as usize) {
            Ok(b) => reply.data(&b),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn write(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _offset: u64,
        _data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        reply.error(denied(WriteOp::Write));
    }

    fn flush(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        reply.ok();
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
        self.view.release(u64::from(fh));
        reply.ok();
    }

    fn fsync(
        &self,
        _req: &Request,
        _ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        reply.ok();
    }

    fn opendir(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        match self.view.opendir(u64::from(ino)) {
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
        let listing = match self.view.readdir(u64::from(fh)) {
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
        self.view.release(u64::from(fh));
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
        reply.error(denied(WriteOp::Setxattr));
    }

    fn removexattr(&self, _req: &Request, _ino: INodeNo, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(denied(WriteOp::Removexattr));
    }

    fn access(&self, _req: &Request, ino: INodeNo, mask: AccessFlags, reply: ReplyEmpty) {
        // Read and execute-search are granted by the mode bits the kernel
        // checks itself; a write probe is the one thing this answers.
        if mask.contains(AccessFlags::W_OK) {
            return match self.view.getattr(u64::from(ino)) {
                Ok(_) => reply.error(denied(WriteOp::Write)),
                Err(e) => reply.error(errno(&e)),
            };
        }
        match self.view.getattr(u64::from(ino)) {
            Ok(_) => reply.ok(),
            Err(e) => reply.error(errno(&e)),
        }
    }

    fn create(
        &self,
        _req: &Request,
        _parent: INodeNo,
        _name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        reply.error(denied(WriteOp::Create));
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
        reply.error(denied(WriteOp::Fallocate));
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
        reply.error(denied(WriteOp::CopyFileRange));
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
pub fn serve(
    db: Arc<dyn InstalledDb>,
    mountpoint: &Path,
    fsname: &str,
    allow_other: bool,
    on_mounted: impl FnOnce(),
) -> io::Result<()> {
    let set = block_termination_signals();
    let mut cfg = Config::default();
    cfg.mount_options = vec![
        MountOption::RO,
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
    let fs = Fs {
        view: View::new(db),
        uid,
        gid,
    };
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
    session.run()
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
) -> Result<(), String> {
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
            Some(b'0') => Ok(()),
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
            let r = serve(db, mountpoint, fsname, allow_other, || {
                report(true, "");
                detach_from_terminal();
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

/// Point stdin/stdout/stderr at `/dev/null` and leave the working
/// directory, so the daemon holds neither the terminal nor a directory.
fn detach_from_terminal() {
    use std::os::fd::AsRawFd as _;
    if let Ok(null) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
    {
        for fd in 0..=2 {
            // SAFETY: duplicating an open descriptor onto a standard one.
            unsafe { libc::dup2(null.as_raw_fd(), fd) };
        }
    }
    let _ = std::env::set_current_dir("/");
}
