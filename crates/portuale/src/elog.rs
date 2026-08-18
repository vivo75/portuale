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

/// `utc_stamp`'s formatting for an arbitrary `SystemTime`, reused for
/// `.logid`-mtime-based build-log naming instead of "now".
pub(crate) fn utc_stamp_at(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant's algorithm) -- no chrono dep.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}
