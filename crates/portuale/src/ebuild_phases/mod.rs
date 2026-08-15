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

mod sandbox;
pub use sandbox::*;

use brush_builtins::ShellBuilderExt as _;

use regex::Regex;

use std::os::unix::io::FromRawFd as _;

use std::path::{Path, PathBuf};
