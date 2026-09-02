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
mod merge_script;
mod plan;
mod preflight;
mod ssh;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use binpkg_flow::*;
pub(crate) use ledger::*;
pub(crate) use merge_script::*;
pub use plan::*;
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

/// Where a config tree lives (`--remote-etc-portage server:<path>` /
/// `client:<path>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPlacement {
    /// Read directly off the server filesystem.
    Server(String),
    /// Pulled once over the multiplexed connection at plan start.
    Client(String),
}

/// Validated remote target: Ansible's `play_context` split -- connection
/// parameters in one struct, validated once, threaded through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteContext {
    /// Client hostname / IP (never `user@host` -- see `check_remote`).
    pub hostname: String,
    /// SSH login user; `None` lets ssh pick (its own default).
    pub user: Option<String>,
    /// SSH port.
    pub port: u16,
    /// Private key file; `None` defers to ssh-agent/defaults.
    pub key_file: Option<String>,
    /// Seconds to wait establishing the connection.
    pub timeout_secs: u64,
    /// Extra argv for every ssh invocation (whitespace-split, v1 cut:
    /// no quoting -- pass one flat string of simple tokens).
    pub ssh_args: Option<String>,
    /// Host-key policy.
    pub strict_host_key_checking: StrictHostKeyChecking,
    /// Abort when `|server - client|` clock differs by more seconds;
    /// `0` disables.
    pub max_clock_skew_secs: u64,
    /// Target `${ROOT}` on the client.
    pub root: String,
    /// Per-unit work area on the client.
    pub workdir: String,
    /// How driver scripts + files reach the client.
    pub transport: RemoteTransport,
    /// One explicit binpkg file to bundle, stream and unpack (bypasses
    /// resolution; slices 2-4 trials, later an escape hatch).
    pub binpkg: Option<String>,
    /// Where `/etc/portage` resolves from (default
    /// `client:/etc/portage`).
    pub etc_portage: ConfigPlacement,
    /// Where the client installed-db lives (default
    /// `client:<root>/var/db/pkg`). `server:` = stateless client: files
    /// merge with no vdb entry, no old hooks, and fail-closed collisions.
    pub vdb: ConfigPlacement,
    /// Binhost-index cache side, informational (default
    /// `server:<root>/var/cache/edb`): the client has no binhosts, so
    /// only `server:` parses -- the resolve reads its own cache.
    pub edb: ConfigPlacement,
    /// Server ledger directory override (`None` =
    /// `<placed-PKGDIR>/remote-ledger`).
    pub ledger_dir: Option<String>,
    /// Strict-mode ledger provenance enforcement
    /// (`--remote-require-ledger-match`): abort before merging anything
    /// unless the client's and server's newest ledger line agree.
    pub require_ledger_match: bool,
    /// Binary the server ships to the client (`--remote-portuale-binary`):
    /// `None` = this process's own binary (`current_exe()`).
    pub portuale_binary: Option<String>,
    /// Client directory holding the portuale binary
    /// (`--remote-portuale-dir`): `None` = the default pair
    /// (`/opt/bin`, then `/usr/local/bin`). Must be absolute when given.
    pub portuale_dir: Option<String>,
    /// Resolved client binary placement (post-preflight):
    /// `Unresolved` until the preflight facts decide it. Local transport
    /// without the override never resolves past `Unresolved` -- it keeps
    /// `current_exe()` and installs nothing.
    pub bin_plan: ClientBinPlan,
    /// Space-separated CONFIG_PROTECT list for the client merge (default
    /// `/etc`; the resolve path derives it from the placed
    /// config unless explicitly flagged).
    pub config_protect: String,
    /// Whether `--remote-config-protect` was passed explicitly.
    pub config_protect_explicit: bool,
    /// Space-separated CONFIG_PROTECT_MASK list (Portage default
    /// `/etc/env.d`).
    pub config_protect_mask: String,
    /// Whether `--remote-config-protect-mask` was passed explicitly.
    pub config_protect_mask_explicit: bool,
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
