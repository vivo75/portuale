// Repo/metadata/vdb access for the single-atom `emerge --pretend` portuale
// slice (see LLM/agent-context.md's depgraph/config-resolution follow-up
// work, and LLM/what-this-proves.md for the full scope writeup).
//
// Overlays: every `[reponame]` section in `repos.conf` with a `location`
// (not just `[DEFAULT] main-repo`) is now a candidate source -- see
// `find_repos`. Candidates for a given category/package are gathered from
// ALL configured repos (mirroring real `portdbapi.cp_list`, which does
// the same: an overlay isn't "consulted only if the main repo has
// nothing," every repo's ebuilds are real candidates), sorted ascending
// by `(priority, name)` exactly like real portage's own `prepos_order`,
// so a tie between two repos providing the identical version is broken
// in favor of the higher-priority one (see `resolve_pretend`'s final
// `max_by`). A repo's `priority` is its explicit `repos.conf` value if
// present, else `-1000` for the main repo (real portage's own default --
// see `lib/portage/repository/config.py`) or `0` for anything else.
//
// KNOWN, DOCUMENTED SCOPE CUTS for v1 (all confirmed with the user before
// implementing):
//   - No per-repo `package.mask`/`.unmask`/`profiles/`, no `masters`
//     (eclass inheritance across repos), no `::repo`-constrained atoms
//     (out of `portage-dep`'s v1 grammar already) -- overlays only widen
//     *which ebuilds are candidates*, nothing about how they're
//     evaluated once found.
//   - Ebuild metadata comes from `metadata/md5-cache/<cat>/<pf>` (plain
//     `KEY=value` text -- confirmed against a real vendored tree), never
//     from executing the ebuild in bash. This is deliberate: it lets
//     `--pretend` work without the bash dependency that real phase
//     execution will eventually require (see LLM/agent-context.md's "Deferred:
//     ebuild phase execution").
//   - No virtuals, no backtracking.
//
// USE/ACCEPT_KEYWORDS/package.mask/.unmask/.accept_keywords/.use are no
// longer hardcoded: they're computed by `portage_profile::resolve_config`
// (real profile chain + make.conf + package.*, with its own documented
// scope cuts -- see that crate's doc comment) and threaded through
// `resolve_pretend`/`resolve_pretend_graph` as a `&portage_profile::Config`.
// `package.use` in particular is applied per package, not globally: see
// `effective_use_flags` and its use in `resolve_pretend_graph` below --
// this is the one place `Config`'s fields aren't just handed to
// `use_reduce_flat`/`is_visible` unchanged, since matching a `package.use`
// entry against a specific candidate (to support slotted/versioned
// entries, not just bare atoms) needs that candidate's resolved SLOT,
// which only exists at this repo-aware layer.
//
// Dependency recursion (see `resolve_pretend_graph` below) walks DEPEND
// and RDEPEND only, with its own documented scope cuts -- see that
// function's doc comment.
//
// Config/target roots are read from the real `PORTAGE_CONFIGROOT` and
// `ROOT` environment variables (portage's own mechanism for relocating
// `/etc/portage` and the install target -- see lib/portage/const.py),
// defaulting to `/` when unset. This is not a portuale-only convention: it's
// what lets tests point at a fixture tree without needing anything
// portuale-specific.

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

// feat#157 S1.2: the `metadata` format (the 23-field set, the format
// version, `_in_metadata_file`) lives in `portage-vdb`, next to the code
// that reads it; one copy only.
pub use portage_vdb::{METADATA_FILE_FIELDS, METADATA_FILE_FORMAT_VERSION, in_metadata_file};
