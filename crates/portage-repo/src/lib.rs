//! Repo/metadata/vdb access for the `emerge --pretend` port.
//!
//! Candidates for a category/package come from every configured repo (main plus
//! overlays), ordered by `(priority, name)`. Ebuild metadata comes from
//! `metadata/md5-cache`, never by executing the ebuild. USE/keywords/mask state
//! is computed by `portage_profile` and threaded through as `&Config`;
//! `package.use` is applied per package (see `effective_use_flags`).
//! Dependency recursion walks DEPEND and RDEPEND only; config/target roots come
//! from `PORTAGE_CONFIGROOT` and `ROOT`, defaulting to `/`.
// Portage: lib/portage/const.py, lib/portage/repository/config.py, portdbapi.cp_list

pub mod md5_dict;
mod merge_order;

mod binary_index;
mod binpkg_use;
mod candidates;
mod depclean;
mod env_roots;
mod error;
mod globals;
mod info_pkgs;
mod installed;
mod license;
mod masked_deps;
mod metadata_cache;
mod reinstall;
mod repo_config;
mod resolve;
mod updates;

pub use binary_index::*;
pub use binpkg_use::*;
pub use candidates::*;
pub use depclean::*;
pub use env_roots::*;
pub use error::*;
pub use globals::*;
pub use info_pkgs::*;
pub use installed::*;
pub(crate) use license::*;
pub use masked_deps::*;
pub use metadata_cache::*;
pub use reinstall::*;
pub use repo_config::*;
pub use resolve::*;
pub use updates::*;

use portage_versions::vercmp;

use std::cell::RefCell;

use std::cmp::Ordering;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use std::fs;

use std::path::{Path, PathBuf};

use std::rc::Rc;

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use std::sync::{OnceLock, RwLock};

pub use portage_vdb::{METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION, in_metadata_file};
