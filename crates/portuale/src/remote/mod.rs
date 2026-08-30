//! Remote binary-package merge over SSH (`mrg --remote-*`).
//!
//! Plan: `docs/remote-merge.md`.
//!
//! Shape, mirroring Ansible's `ansible.builtin.ssh` (a wrapper around the
//! system `ssh` binary, multiplexed, rc 255 = transport error): the server
//! shells out to OpenSSH, never links an SSH library, so the musl-static
//! story and the near-zero-dependency discipline both survive. The client
//! needs only bash ≥ 5.3 and POSIX `/bin` -- generated bash goes over
//! `ssh … bash -s` (stdin), machine-readable `KEY=VALUE` lines come back
//! on stdout, human log on stderr. No Python anywhere near the client.
//!
//! Binary-format scope: gpkg (`.gpkg.tar`) only. xpak (`.tbz2`) is out of
//! scope for `mrg` -- it is the old format and `mrg` does not target full
//! binary-format compatibility (the shared server-side `binpkg` reader may
//! open `.tbz2` incidentally, but that is never a tested or guaranteed
//! `mrg` path; `emerge`/`ebuild` keep their own local xpak support).

use clap::ArgMatches;

mod binpkg_flow;
mod ledger;
mod preflight;
mod ssh;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use binpkg_flow::*;
pub(crate) use ledger::*;
pub(crate) use preflight::*;
pub use ssh::*;

use portage_util::TempDir;

use std::collections::HashMap;

use std::path::Path;

use std::process::ExitCode;

// --- Option validation -----------------------------------------------------

/// Host-key policy (`--remote-strict-host-key-checking`). Default is
/// trust-on-first-use: a plain `yes` would fail on every new machine,
/// which is the common provisioning case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictHostKeyChecking {
    /// `accept-new`: add unknown keys (fingerprint printed), abort on
    /// changed keys.
    AcceptNew,
    /// `yes`: reject unknown keys outright.
    Yes,
    /// `no`: no checking (throwaway labs only).
    No,
}

/// How driver scripts and files reach the client (`--remote-transport`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTransport {
    /// Real SSH client (default).
    Ssh,
    /// Execute the *same generated driver* against local paths (no ssh):
    /// offline debugging and SSH-free driver tests. The hostname stays a
    /// label; key/port/user/timeout/host-key options are inert.
    Local,
}

/// Resolved client binary placement: the absolute client path every
/// generated script exports as `PORTUALE_BIN`, however it got there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientBinPlan {
    /// No preflight ran yet (fresh CLI parse, or local transport without
    /// `--remote-portuale-dir`, which skips the gates by design).
    Unresolved,
    /// The running binary itself (local transport without the override):
    /// no preflight checks, no install.
    Local(String),
    /// A client path whose full SHA-256 already matches: use it, install
    /// nothing.
    Use(String),
    /// No candidate matches: install there (as `portuale-<hash>`).
    Install(String),
}

impl ClientBinPlan {
    /// The absolute path the plan settled on, if any.
    pub fn abs(&self) -> Option<&str> {
        match self {
            Self::Unresolved => None,
            Self::Local(path) | Self::Use(path) | Self::Install(path) => Some(path),
        }
    }
}
