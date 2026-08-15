//! Elog message collection and dispatch (Portage's `lib/portage/elog/`).
//!
//! After a merge, per-phase `${T}/logging/` files are filtered by class
//! and handed to the modules in `PORTAGE_ELOG_SYSTEM` (`echo`, `save`,
//! `save_summary`, `syslog`, `custom`, `mail`, `mail_summary`).
//! Settings come from the env only; the logdir is root-relative.

use crate::color::Colorizer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Phase order for message printing, regardless of file mtime.
/// Portage: const.EBUILD_PHASES.
const EBUILD_PHASES: &[&str] = &[
    "pretend",
    "setup",
    "unpack",
    "prepare",
    "configure",
    "compile",
    "test",
    "install",
    "package",
    "instprep",
    "preinst",
    "postinst",
    "prerm",
    "postrm",
    "nofetch",
    "config",
    "info",
    "other",
];
