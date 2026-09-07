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
mod resolver_trace;
mod solver_bridge;

mod binary_index;
mod binpkg_use;
mod candidates;
mod config_match;
mod dep_chain;
mod depclean;
mod env_roots;
mod error;
mod fetch_restrict;
mod globals;
mod info_pkgs;
mod installed;
mod license;
mod masked_deps;
mod metadata_cache;
mod provenance;
mod reinstall;
mod repo_config;
mod resolve;
mod slot_operator;
mod updates;
mod use_display;
mod use_flags;
mod use_unsat;
mod visibility;
mod visibility_use_flip;

#[cfg(test)]
mod tests;

pub use binary_index::*;
pub use binpkg_use::*;
pub use candidates::*;
pub(crate) use config_match::*;
pub(crate) use dep_chain::*;
pub use depclean::*;
pub use env_roots::*;
pub use error::*;
pub(crate) use fetch_restrict::*;
pub use globals::*;
pub use info_pkgs::*;
pub use installed::*;
pub(crate) use license::*;
pub use masked_deps::*;
pub use metadata_cache::*;
pub use provenance::*;
pub use reinstall::*;
pub use repo_config::*;
pub use resolve::*;
pub use slot_operator::*;
pub use updates::*;
pub use use_display::*;
pub use use_flags::*;
pub(crate) use use_unsat::*;
pub use visibility::*;
pub use visibility_use_flip::*;

pub use merge_order::{
    DepEdge, DepPriority, TreeDisplayOrderArgs, dep_edge_satisfied_by_installed, kept_alt_branches,
    resolved_dep_targets, tree_display_order,
};

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
