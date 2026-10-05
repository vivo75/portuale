// The FUSE adapter of `portuale vdb mount`: a thin translation from the
// `fuser` callbacks to the pure `vdb_view::View` (read-only) or
// `vdb_rw::RwView` (`--rw`). This file only converts types and errno values;
// it needs `/dev/fuse` and `fusermount3` to run.
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
/// (`--rw`) over it.
enum Mode {
    Ro(View),
    Rw(Box<RwView>),
}

/// `--rw`: where staged entries live while Portage writes them, and
/// the ROOT whose stores (world, counter, …) are imported at unmount.
pub struct RwOpts {
    pub scratch: PathBuf,
    pub root: PathBuf,
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

/// Arguments of [`serve`].
pub struct ServeArgs<'a> {
    pub db: Arc<dyn InstalledDb>,
    pub mountpoint: &'a Path,
    pub fsname: &'a str,
    pub allow_other: bool,
    pub rw: Option<&'a RwOpts>,
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
