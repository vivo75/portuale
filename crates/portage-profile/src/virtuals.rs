//! Deprecated `virtuals` files (backlog #513).
//!
//! `PROVIDE` virtuals were removed in portage-2.3.25, so Portage 3.0.83
//! reads every `virtuals` file only to warn about it. Two warnings exist:
//! a Python `UserWarning` for each non-empty profile `virtuals` file (the
//! profile chain plus `/etc/portage/profile`), printed at config load, and
//! the `/etc/portage/virtuals` block printed by `validate_ebuild_environment`
//! for the actions that validate the environment.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::atom_lines::{ProfileWarning, WARN_RANK_VIRTUALS_PROFILE};

/// The `/etc/portage/virtuals` block, exactly as Portage writes it to
/// stderr (a leading blank line, a trailing blank line). Printed by the CLI
/// for the actions Portage validates (see [`Config::user_virtuals_deprecated`]).
// Portage: config.py, `validate` (`/etc/portage/virtuals is deprecated`)
pub const USER_VIRTUALS_NOTICE: &str = "\n!!! /etc/portage/virtuals is deprecated and ignored.\n\
!!! Support for PROVIDE virtuals was removed in\n\
!!! portage-2.3.25; use GLEP 37 virtual packages\n\
!!! instead. Please remove this file.\n\n";

/// Whether Portage's `grabdict` returns a non-empty dict for `path`: some
/// line, not starting with `#`, has a key plus at least one value once a
/// `#` token ends the line. A missing file is empty; any other read error
/// is reported, like Portage's `grablines` (an `EACCES` there raises
/// `PermissionDenied` and aborts the config load — backlog #549).
// Portage: util/__init__.py, `grabdict` (`len(myline) < 2` lines are skipped)
pub(crate) fn grabdict_nonempty(path: &Path) -> Result<bool, std::io::Error> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    Ok(text.lines().any(|line| {
        if line.starts_with('#') {
            return false;
        }
        line.split_whitespace()
            .take_while(|token| !token.starts_with('#'))
            .count()
            >= 2
    }))
}

/// The config-load warning for each profile directory whose `virtuals` file
/// is non-empty, in profile-chain order. The text is the Python
/// `warnings.warn` rendering Portage prints (`file:line: UserWarning: msg`
/// and the source line), with the interpreter's install prefix left out:
/// the location is `portage/package/ebuild/_config/VirtualsManager.py:54`.
// Portage: _config/VirtualsManager.py, `_read_dirVirtuals` (line 54)
pub(crate) fn profile_virtuals_warnings(
    profiles: &[PathBuf],
) -> Result<Vec<ProfileWarning>, Error> {
    let mut warnings = Vec::new();
    for dir in profiles {
        let file = dir.join("virtuals");
        let nonempty = match grabdict_nonempty(&file) {
            Ok(nonempty) => nonempty,
            // Portage's `grablines` turns an `EACCES` read into
            // `PermissionDenied`, which aborts the run (`bin/emerge`
            // prints `Permission denied: '<file>'` and exits 13).
            // Portage: util/__init__.py, `grablines`; bin/emerge:63-65
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(Error::VirtualsPermissionDenied {
                    path: file.display().to_string(),
                });
            }
            Err(e) => {
                return Err(Error::ReadFile {
                    path: file.display().to_string(),
                    source: e,
                });
            }
        };
        if nonempty {
            warnings.push(ProfileWarning {
                rank: WARN_RANK_VIRTUALS_PROFILE,
                text: format!(
                    "portage/package/ebuild/_config/VirtualsManager.py:54: UserWarning: {} is deprecated, support for PROVIDE virtuals was removed in portage-2.3.25, use GLEP 37 virtual packages instead\n  warnings.warn(",
                    file.display()
                ),
            });
        }
    }
    Ok(warnings)
}
