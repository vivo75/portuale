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

use crate::vdb_view::{self, Kind, ViewError};
use fuser::{
    AccessFlags, BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem,
    FopenFlags, Generation, INodeNo, InitFlags, KernelConfig, LockOwner, MountOption, OpenFlags,
    RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, ReplyStatfs, ReplyWrite, Request, Session, SessionACL, TimeOrNow, WriteFlags,
};
use portage_vdb::InstalledDb;

/// `--rw`: where staged entries live while Portage writes them, and
/// the ROOT whose stores (world, counter, …) are imported at unmount.
pub struct RwOpts {
    pub scratch: PathBuf,
    pub root: PathBuf,
}
