// Profile-chain + make.conf + package.mask/.unmask/.accept_keywords
// resolution for USE/ACCEPT_KEYWORDS/visibility. Replaces the base
// `emerge --pretend` slice's hardcoded `ACCEPT_KEYWORDS="amd64"`/`USE=""`
// with the inherited-chain + `make.conf` + per-package-override mechanism.
//
// Scope cuts a caller must know (details in LLM/code-notes/portage-profile/lib.md):
// - Cross-repo `parent` references (`reponame:path`, `:path`) resolve, gated
//   on the node's repo declaring `profile-formats = portage-2`; an absent key
//   means "no `portage-2`" (no EAPI-conditional default).
// - ARCH-based KEYWORDS validation is out; `_*` IUSE-aware expansion lives in
//   `portage-repo::effective_use_flags`; `USE_EXPAND` (+`_UNPREFIXED`,
//   `_IMPLICIT`, `_HIDDEN`, `IUSE_IMPLICIT`, `USE_EXPAND_VALUES_<v>`) is read.
// - Full `USE_ORDER` chain modeled (`env.d` … `env`); see `Config` fields.
// - No line continuation / multi-line quoted values; `#` is a comment only at
//   line start. Non-USE/ACCEPT_KEYWORDS vars are last-wins scalars.
// - `package.mask`/`.unmask`: main repo + every profile level + the user
//   profile as Portage's last profile node (`/etc/portage/profile`, only
//   when a `make.profile` resolved; the node's `eapi` file or `None`,
//   no wildcards) + user files (wildcards on), plus every overlay's
//   repo-level entries (`::repo`-scoped, implicit `masters` = main repo
//   only). No eclass inheritance, no explicit `masters =`.
// - `package.accept_keywords`: profile-chain + user-profile node (no
//   wildcards, no build-ids, like the chain) + user-level, purely
//   additive; a bare atom gains the implicit `~arch` default at every
//   level. (`package.keywords`, chain or user, is not read.)
// - `package.use`: repo-level, per-profile-level (the user profile forms
//   the last level, its `make.defaults` USE included, wildcards only
//   without a node `eapi`), and user-level sources at their own
//   `USE_ORDER` positions; repo `make.defaults` USE leads the `repo`
//   layer; only user-level lines get the `USE_EXPAND`-prefix shorthand.
// - `packages` (`@system`), `@profile` (`profile-set` levels, always
//   including the user node), and `package.provided` files are read;
//   `@system` also stacks the user profile's `packages` (node EAPI,
//   no wildcards).
// - `use.mask`/`use.force` (+`.stable.` variants) and `package.use.mask`/
//   `.force` (+`.stable.` variants) are read but never folded into `use_flags`;
//   `portage-repo` applies them last (force-add, then force-remove).
//   Stability for the `.stable.` variants comes from `portage-repo::is_stable`.
// - `license_groups` (+`ACCEPT_LICENSE`/`package.license`), `ACCEPT_PROPERTIES`
//   (`*` default), `ACCEPT_RESTRICT`, `PKGDIR` (`/var/cache/binpkgs` default),
//   `arch.list`, and `binrepos.conf`/`PORTAGE_BINHOST` are read.
// - `${VAR}` substitution persists across profile levels for every variable
//   except USE, which resets before each level's `make.defaults`.
// Portage: ebuild/config.py, MaskManager.py, KeywordsManager, UseManager,
//   LicenseManager, _sets/profiles.py, _config/LocationsManager.py

use std::collections::{HashMap, HashSet};

use std::fs;

use std::path::{Path, PathBuf};

mod phase_environ;

mod atom_lines;
mod binrepo;
mod config;
mod env_layer;
mod envd;
mod fingerprints;
mod incrementals;
mod make_conf;
mod package_files;
mod pkgindex_header;
mod profile_chain;
mod resolve;

#[cfg(test)]
mod tests;

pub(crate) use atom_lines::*;
pub use binrepo::*;
pub use config::*;
pub use env_layer::*;
pub(crate) use envd::*;
pub use fingerprints::*;
pub use incrementals::*;
pub(crate) use make_conf::*;
pub(crate) use package_files::*;
pub use pkgindex_header::*;
pub(crate) use profile_chain::*;
pub use resolve::*;

pub use phase_environ::{
    ENV_BLACKLIST, ENVIRON_FILTER, PORTUALE_COMPUTED, PhaseUse, apply_unprefixed_use_expand_filter,
    filter_unprefixed_use_expand_value, in_portage_iuse_body, phase_environ, phase_environ_pkg,
    portage_use, standalone_iuse_effective,
};

/// Error resolving profile/config files. Distinct variants mirror the
/// Portage message shapes so `Display` reproduces them byte-for-byte.
#[derive(Debug)]
pub enum Error {
    /// `reading {path}: {source}`
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    /// `resolving profile {path}: {source}`
    ResolveProfile {
        path: String,
        source: std::io::Error,
    },
    /// `parent {parent:?} not found: {parents_file} (not inside any known repo)`
    ParentNotFoundOutsideRepos {
        parent: String,
        parents_file: String,
    },
    /// `parent {parent:?} not found: {parents_file} (no repo named {repo_name:?})`
    ParentNotFoundUnknownRepo {
        parent: String,
        parents_file: String,
        repo_name: String,
    },
    /// A profile node's `eapi` file holds an unsupported EAPI: Portage's
    /// `ParseError`, which empties the whole profile list and aborts the
    /// run. `Display` already carries the `!!!` framing, so the CLI prints
    /// it verbatim (no `emerge: ` prefix).
    UnsupportedProfileEapi { eapi: String, path: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ReadFile { path, source } => write!(f, "reading {path}: {source}"),
            Error::ResolveProfile { path, source } => {
                write!(f, "resolving profile {path}: {source}")
            }
            Error::ParentNotFoundOutsideRepos {
                parent,
                parents_file,
            } => {
                write!(
                    f,
                    "parent {parent:?} not found: {parents_file} (not inside any known repo)"
                )
            }
            Error::ParentNotFoundUnknownRepo {
                parent,
                parents_file,
                repo_name,
            } => write!(
                f,
                "parent {parent:?} not found: {parents_file} (no repo named {repo_name:?})"
            ),
            Error::UnsupportedProfileEapi { eapi, path } => {
                write!(
                    f,
                    "!!! ParseError: Profile contains unsupported EAPI '{eapi}': '{path}'\n\
                     !!! Your current profile is invalid. If you have just changed your profile\n\
                     !!! configuration, you should revert back to the previous configuration.\n\
                     !!! Allowed actions are limited to --help, --info, --search, --sync, and\n\
                     !!! --version."
                )
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}
