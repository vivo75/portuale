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

use crate::vdb_rw::{Owner, SetAttr};
use crate::vdb_view::{self, Kind, ViewError};

/// `--rw`: where staged entries live while Portage writes them, and
/// the ROOT whose stores (world, counter, …) are imported at unmount.
pub struct RwOpts {
    pub scratch: PathBuf,
    pub root: PathBuf,
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
