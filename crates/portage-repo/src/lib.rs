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
mod categories;
mod config_match;
mod dep_chain;
mod dep_validate;
mod depclean;
mod env_roots;
mod error;
mod fetch_restrict;
mod globals;
mod graph;
mod info_pkgs;
mod installed;
mod license;
mod masked_deps;
mod metadata_cache;
mod paths;
mod provenance;
mod reinstall;
mod repo_config;
mod resolve;
mod slot_operator;
mod updates;
mod use_display;
mod use_flags;
mod use_unsat;
mod user_patches;
mod visibility;
mod visibility_use_flip;

/// A [`CategoryGate`](crate::CategoryGate) over `repos` for unit tests: the allow-list is the
/// union of their ebuild-bearing category dirs. That matches what their
/// `profiles/categories` files list everywhere except deliberately
/// unlisted fixture categories, so gating changes nothing for existing
/// tests — and tests
/// that pin unlisted-category behaviour build their own gate from
/// [`Categories::load`](crate::Categories) instead. Backing is
/// leaked: one tiny allocation per call site, freed at process exit.
#[cfg(test)]
pub(crate) fn test_gate(repos: &[crate::RepoConfig]) -> crate::CategoryGate<'static> {
    crate::CategoryGate::new(
        Box::leak(Box::new(repos.to_vec())),
        Box::leak(Box::new(crate::Categories {
            set: test_categories(repos),
            warned: std::cell::RefCell::new(std::collections::HashSet::new()),
        })),
        Box::leak(Box::new(std::path::PathBuf::from("/"))),
    )
}

/// The allow-list a [`test_gate`] gates on, as an owned set: the union of
/// `repos`' ebuild-bearing category dirs.
#[cfg(test)]
pub(crate) fn test_categories(repos: &[crate::RepoConfig]) -> std::collections::BTreeSet<String> {
    fn has_ebuild(pkg_dir: &std::path::Path) -> bool {
        let Ok(entries) = portage_util::read_dir_entries(pkg_dir) else {
            return false;
        };
        entries.into_iter().any(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.ends_with(".ebuild"))
        })
    }
    let mut categories = std::collections::BTreeSet::new();
    for repo in repos {
        let Ok(cats) = portage_util::read_dir_entries(&repo.location) else {
            continue;
        };
        for cat in cats.into_iter().filter(|e| e.path().is_dir()) {
            let name = cat.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "eclass" | "profiles" | "metadata" | "licenses" | "scripts" | "distfiles"
                )
            {
                continue;
            }
            let Ok(pkgs) = portage_util::read_dir_entries(&cat.path()) else {
                continue;
            };
            if pkgs
                .into_iter()
                .filter(|e| e.path().is_dir())
                .any(|p| has_ebuild(&p.path()))
            {
                categories.insert(name);
            }
        }
    }
    categories
}
#[cfg(test)]
mod tests;

pub use binary_index::*;
pub use binpkg_use::*;
pub use candidates::*;
pub use categories::*;
pub(crate) use config_match::*;
pub(crate) use dep_chain::*;
pub(crate) use dep_validate::*;
pub use depclean::*;
pub use env_roots::*;
pub use error::*;
pub(crate) use fetch_restrict::*;
pub use globals::*;
pub use graph::*;
pub use info_pkgs::*;
pub use installed::*;
pub(crate) use license::*;
pub use masked_deps::*;
pub use metadata_cache::*;
pub use paths::*;
pub use provenance::*;
pub use reinstall::*;
pub use repo_config::*;
pub use resolve::*;
pub use slot_operator::*;
pub use updates::*;
pub use use_display::*;
pub use use_flags::*;
pub(crate) use use_unsat::*;
pub use user_patches::*;
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
