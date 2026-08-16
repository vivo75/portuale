//! `portuale portageq`: native `has_version` / `best_version` over the
//! installed-package database. Every other portageq command is refused by
//! name.
//!
//! Matching reuses the resolver's own installed-package matcher
//! (`portage_repo::best_installed_match`); there is no second matcher here.
//! Backend: `PORTUALE_VDB_BACKEND` + `PORTUALE_VDB_PATH` (exported by `mrg`
//! to its children) select a database, `PORTUALE_VDB_ROOT` names the ROOT it
//! belongs to. On redb, held read-write by a running `mrg`, lookups go to the
//! parent over the socket in `PORTUALE_VDB_IPC`.
//!
//! Behaviour contract: `LLM/evidence/305-s6-portageq.md` ("Rules the native
//! implementation must follow", rules 1-10). Where the captures table and
//! the rules disagree, the rules win.
// Portage: bin/portageq; dbapi/dep_expand.py, dep_expand; eapi.py, _get_eapi_attrs

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portage_vdb::BackendKind;

/// The categories that have `pn` installed under `root`. Also what the
/// parent pipe answers to a `categories` request.
pub(crate) fn installed_categories_of(root: &Path, pn: &str) -> Vec<String> {
    portage_vdb::for_root(root)
        .categories()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !portage_repo::installed_candidates(root, c, pn).is_empty())
        .collect()
}
