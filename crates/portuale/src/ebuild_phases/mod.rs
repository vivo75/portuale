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

use crate::fetch::{self, FetchOptions};

mod commands;
mod depend_phase;
mod environment;
mod fetch_sources;
mod misc_functions;
mod package_env;
mod phase_env;
mod post_install_meta;
mod run_phase;
mod runtime_paths;
mod sandbox;
mod scheduler_registry;
mod single_phase;
mod standalone_config;

#[cfg(test)]
mod tests;

pub use commands::*;
pub(crate) use depend_phase::*;
pub(crate) use environment::*;
pub(crate) use fetch_sources::*;
pub(crate) use misc_functions::*;
pub(crate) use package_env::*;
pub(crate) use phase_env::*;
pub(crate) use post_install_meta::*;
pub(crate) use run_phase::*;
pub(crate) use runtime_paths::*;
pub use sandbox::*;
pub(crate) use scheduler_registry::*;
pub(crate) use single_phase::*;
pub(crate) use standalone_config::*;

use brush_builtins::ShellBuilderExt as _;

use regex::Regex;

use std::os::unix::io::FromRawFd as _;

use std::path::{Path, PathBuf};

/// The ebuild's EAPI per PMS 7.3.1: the `EAPI=` assignment on the first
/// non-blank, non-comment line only; anything else (including no match) is `"0"`.
/// PMS's `\1` backreference has no Rust-`regex` equivalent (`_parse_eapi_ebuild_head`),
/// so the three quote shapes are explicit alternatives.
fn parse_eapi(ebuild_text: &str) -> String {
    // PMS's `\1` backreference isn't expressible without backrefs (guaranteed
    // linear-time matching), so three explicit alternatives instead.
    let eapi_re = Regex::new(
        r#"^[ \t]*EAPI=(?:"([A-Za-z0-9+_.-]*)"|'([A-Za-z0-9+_.-]*)'|([A-Za-z0-9+_.-]*))[ \t]*([ \t]#.*)?$"#,
    )
    .expect("static regex is valid");
    let comment_or_blank = Regex::new(r"^\s*(#.*)?$").expect("static regex is valid");

    for line in ebuild_text.lines() {
        if comment_or_blank.is_match(line) {
            continue;
        }
        return match eapi_re.captures(line) {
            Some(caps) => caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            None => "0".to_string(),
        };
    }
    "0".to_string()
}

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

fn split_package(ebuild_path: &Path, package_dir_name: &str) -> Result<PackageSplit, String> {
    let pf = ebuild_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("{}: not a valid file path", ebuild_path.display()))?
        .to_string();
    let pvr = pf
        .strip_prefix(package_dir_name)
        .and_then(|rest| rest.strip_prefix('-'))
        .ok_or_else(|| {
            format!(
                "{}: filename doesn't start with the parent directory's own name ({package_dir_name:?})",
                ebuild_path.display()
            )
        })?
        .to_string();
    let revision_re = Regex::new(r"^(.*)-(r\d+)$").expect("static regex is valid");
    let (pv, pr) = match revision_re.captures(&pvr) {
        Some(caps) => (caps[1].to_string(), caps[2].to_string()),
        None => (pvr.clone(), "r0".to_string()),
    };
    let pn = package_dir_name.to_string();
    let p = format!("{pn}-{pv}");
    Ok(PackageSplit {
        p,
        pn,
        pv,
        pr,
        pvr,
        pf,
    })
}

/// Portage's `actionmap_deps` (`doebuild.py`): the prerequisite chain for `mydo`,
/// in order. Only the phase subset; an unrecognized `mydo` returns just itself.
fn phase_prerequisites(mydo: &str) -> Vec<&'static str> {
    const CHAIN: &[&str] = &[
        "pretend",
        "setup",
        "unpack",
        "prepare",
        "configure",
        "compile",
        "test",
        "install",
    ];
    match CHAIN.iter().position(|&p| p == mydo) {
        Some(idx) => CHAIN[..=idx].to_vec(),
        None => Vec::new(),
    }
}

/// Whether `command` runs through `run_commands` (the chained phases);
/// `ebuild.rs` falls back to its dry-run stub for anything else.
pub fn is_phase_command(command: &str) -> bool {
    phase_prerequisites(command).last() == Some(&command)
}

/// Whether `command` is a standalone single-phase `ebuild` command with no
/// prerequisite chain: `config`/`info`/`prerm`/`postrm`. `preinst`/`postinst`
/// stay merge-internal (ordering-tied to the file copy); `pretend` is already
/// chained; `ebuild.rs` handles `help`. `run_single_phase` fits all four.
/// Portage: doebuild.py
pub fn is_standalone_phase_command(command: &str) -> bool {
    matches!(command, "config" | "info" | "prerm" | "postrm")
}
