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

mod depclean;
mod info_pkgs;
mod installed;
mod masked_deps;
mod resolve;
pub use depclean::*;
pub use info_pkgs::*;
pub use installed::*;
pub use masked_deps::*;
pub use resolve::*;

use portage_versions::vercmp;

use std::cell::RefCell;

use std::cmp::Ordering;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use std::fs;

use std::path::{Path, PathBuf};

use std::rc::Rc;

use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use std::sync::{OnceLock, RwLock};
