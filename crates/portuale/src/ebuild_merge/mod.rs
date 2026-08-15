//! Real merge/filesystem mutation: after the `install` phase chain,
//! run `pkg_preinst`, copy `${D}`'s files/dirs/symlinks into `${ROOT}`,
//! write a vdb entry, then run `pkg_postinst`.
//! Portage: lib/portage/dbapi/vartree.py, dblink.merge, treewalk, mergeme
//!
//! CONFIG_PROTECT (for `obj` and `sym`), `collision-protect`,
//! `protect-owned`, preserve-libs handling, blocker exclusion and
//! `config-protect-if-modified` are implemented; see the submodules.
//! `pkg_preinst`/`pkg_postinst` run via `run_single_phase`, not the
//! `actionmap_deps` chain, like `treewalk()` itself.
//!
//! Known gaps: orphaned-preserved-lib `scanelf`, privilege-dropping,
//! and parts of the `SLOT`/`repository` fallback chains. Merge order is
//! sorted by filename for determinism rather than `os.listdir()` order;
//! `CONTENTS` line order has no semantic meaning.

use crate::ebuild_phases;
mod options;
mod protect;
mod tree;
pub use options::*;
pub(crate) use protect::*;
pub(crate) use tree::*;

use md5::{Digest, Md5};

use mrg_director::PackagesDb as _;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use std::os::unix::fs::{FileTypeExt, MetadataExt};

use std::path::{Path, PathBuf};
