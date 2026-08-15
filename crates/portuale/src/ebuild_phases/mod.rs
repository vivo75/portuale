//! Real ebuild phase execution: drives the unmodified `bin/ebuild.sh` and the
//! phase functions it sources, with Rust computing only the environment
//! (`doebuild_environment`: category/version splitting, directory layout) and
//! the per-command phase sequencing (`actionmap_deps`). One fresh shell per
//! phase, like Portage's fresh process per phase (a second phase can't re-export
//! the `readonly` vars a shared shell would keep).
//!
//! Scope: the chained phases `pretend`..`install` actually run, plus standalone
//! `config`/`info`/`prerm`/`postrm`; `merge`/`qmerge`/`unmerge`/`package` live in
//! their own modules; every other `ebuild` command is still a dry-run stub.
//! Build isolation (`sandbox`/`usersandbox`, `network-`/`ipc-`/`mount-`/`pid-sandbox`)
//! applies to the six `src_*` phases and forces the `Bash` backend there.
//! Backend: a genuine `bash` subprocess by default, embedded brush via `--shell brush`.
// Portage: doebuild.py, bin/ebuild.sh

mod commands;
mod depend_phase;
mod environment;
mod sandbox;
mod scheduler_registry;

pub use commands::*;
pub(crate) use depend_phase::*;
pub(crate) use environment::*;
pub use sandbox::*;
pub(crate) use scheduler_registry::*;

use brush_builtins::ShellBuilderExt as _;

use regex::Regex;

use std::os::unix::io::FromRawFd as _;

use std::path::{Path, PathBuf};

/// Portage's `_pkgsplit`-derived `P`/`PN`/`PV`/`PR`/`PVR`, except `PN` is the
/// ebuild's parent directory name (which Portage itself asserts holds).
/// Portage: doebuild.py
pub(crate) struct PackageSplit {
    pub(crate) p: String,
    pub(crate) pn: String,
    /// Portage's `PV`: version without the `-r<digits>` suffix.
    pub(crate) pv: String,
    /// Portage's `PR`: `"r0"` when no explicit suffix was present.
    pub(crate) pr: String,
    /// Portage's `PVR`: `PV` alone for `r0`, else `PV-PR`.
    pub(crate) pvr: String,
    pub(crate) pf: String,
}
