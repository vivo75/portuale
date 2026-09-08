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
mod client_binary;
mod config_place;
mod host_keys;
mod ledger;
mod merge_script;
mod plan;
mod preflight;
mod ssh;
mod stages;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use binpkg_flow::*;
pub(crate) use client_binary::*;
pub(crate) use config_place::*;
pub(crate) use host_keys::*;
pub(crate) use ledger::*;
pub(crate) use merge_script::*;
pub use plan::*;
pub(crate) use preflight::*;
pub use ssh::*;
pub(crate) use stages::*;

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

impl StrictHostKeyChecking {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "accept-new" => Some(Self::AcceptNew),
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            _ => None,
        }
    }

    fn as_ssh_value(self) -> &'static str {
        match self {
            Self::AcceptNew => "accept-new",
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
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

impl RemoteTransport {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "ssh" => Some(Self::Ssh),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
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

impl ConfigPlacement {
    /// Parse `server:<path>` / `client:<path>`; anything else is a usage
    /// error naming the option.
    pub fn parse(option: &str, value: &str) -> Result<Self, String> {
        match value.split_once(':') {
            Some(("server", path)) if !path.is_empty() => Ok(Self::Server(path.to_string())),
            Some(("client", path)) if !path.is_empty() => Ok(Self::Client(path.to_string())),
            _ => Err(format!(
                "mrg: {option} must be server:<path> or client:<path>, got {value:?}"
            )),
        }
    }
}

/// mrg -> pretend handoff for remote resolve runs: `mrg` sets it from the
/// validated CLI surface, `pretend::run`'s getbinpkg dispatch takes it.
/// Set immediately before the in-process `pretend::run` call, taken
/// (cleared) at the dispatch site, so nothing leaks across calls. Only
/// `mrg` remote-with-atoms ever sets it; the `emerge` applet never does.
static REMOTE_EXEC: std::sync::RwLock<Option<RemoteContext>> = std::sync::RwLock::new(None);

/// Publish a remote execution for the upcoming `pretend::run` call.
pub fn set_remote_exec(ctx: RemoteContext) {
    *REMOTE_EXEC.write().unwrap() = Some(ctx);
}

/// Take a published remote execution, if any (clears the slot).
pub fn take_remote_exec() -> Option<RemoteContext> {
    REMOTE_EXEC.write().unwrap().take()
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

/// `--remote-*` ids besides `remote_hostname`, in OPTIONS-table order --
/// any of these without `--remote-hostname` is a usage error.
const REMOTE_OPTION_IDS: &[&str] = &[
    "remote_user",
    "remote_port",
    "remote_key_file",
    "remote_timeout",
    "remote_ssh_args",
    "remote_strict_host_key_checking",
    "remote_max_clock_skew",
    "remote_root",
    "remote_workdir",
    "remote_transport",
    "remote_binpkg",
    "remote_config_protect",
    "remote_config_protect_mask",
    "remote_etc_portage",
    "remote_vdb",
    "remote_edb",
    "remote_ledger_dir",
    "remote_require_ledger_match",
    "remote_portuale_binary",
    "remote_portuale_dir",
];

fn get(matches: &ArgMatches, id: &str) -> Option<String> {
    matches.get_one::<String>(id).cloned()
}

/// Whether `id` was provided on the command line, type-agnostic -- the
/// presence check must see both `Value` string options and `Flag` bools.
fn provided(matches: &ArgMatches, id: &str) -> bool {
    matches.contains_id(id)
        && matches
            .value_source(id)
            .is_some_and(|src| src == clap::parser::ValueSource::CommandLine)
}

/// Validate the `--remote-*` surface: `Ok(None)` = local mode,
/// `Ok(Some(ctx))` = remote mode, `Err(message)` = usage error (exit 2).
pub fn check_remote(matches: &ArgMatches) -> Result<Option<RemoteContext>, String> {
    let hostname = get(matches, "remote_hostname");
    let Some(hostname) = hostname else {
        if let Some(offender) = REMOTE_OPTION_IDS.iter().find(|id| provided(matches, id)) {
            let long = format!("--{}", offender.replace('_', "-"));
            return Err(format!("mrg: {long} requires --remote-hostname"));
        }
        return Ok(None);
    };
    if hostname.is_empty() {
        return Err("mrg: --remote-hostname requires a non-empty value".to_string());
    }
    if hostname.contains('@') {
        return Err("mrg: put the login user in --remote-user, not --remote-hostname".to_string());
    }
    let port = match get(matches, "remote_port") {
        None => 22,
        Some(raw) => raw.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
            format!("mrg: --remote-port: {raw:?} is not a valid TCP port (1-65535)")
        })?,
    };
    let timeout_secs = match get(matches, "remote_timeout") {
        None => 10,
        Some(raw) => raw.parse::<u64>().ok().ok_or_else(|| {
            format!("mrg: --remote-timeout: {raw:?} is not a non-negative integer")
        })?,
    };
    let max_clock_skew_secs = match get(matches, "remote_max_clock_skew") {
        None => 900,
        Some(raw) => raw.parse::<u64>().ok().ok_or_else(|| {
            format!("mrg: --remote-max-clock-skew: {raw:?} is not a non-negative integer")
        })?,
    };
    // Clap's `choices` already restrict these; the fallbacks keep the
    // validation total if the table ever drifts.
    let strict_host_key_checking = match get(matches, "remote_strict_host_key_checking") {
        None => StrictHostKeyChecking::AcceptNew,
        Some(raw) => StrictHostKeyChecking::parse(&raw).ok_or_else(|| {
            format!("mrg: --remote-strict-host-key-checking: {raw:?} is not accept-new, yes or no")
        })?,
    };
    let transport = match get(matches, "remote_transport") {
        None => RemoteTransport::Ssh,
        Some(raw) => RemoteTransport::parse(&raw)
            .ok_or_else(|| format!("mrg: --remote-transport: {raw:?} is not ssh or local"))?,
    };
    let root = get(matches, "remote_root").unwrap_or_else(|| "/".to_string());
    let vdb = match get(matches, "remote_vdb") {
        None => ConfigPlacement::Client(format!("{}/var/db/pkg", root.trim_end_matches('/'))),
        Some(raw) => ConfigPlacement::parse("--remote-vdb", &raw)?,
    };
    // Informational: the client has no binhosts, so only `server:`
    // parses; the resolve reads its own cache.
    let edb = match get(matches, "remote_edb") {
        None => ConfigPlacement::Server(format!("{}/var/cache/edb", root.trim_end_matches('/'))),
        Some(raw) => match ConfigPlacement::parse("--remote-edb", &raw)? {
            placement @ ConfigPlacement::Server(_) => placement,
            ConfigPlacement::Client(_) => {
                return Err(
                    "mrg: --remote-edb must be server:<path> (the client has no binhosts)"
                        .to_string(),
                );
            }
        },
    };
    let (config_protect, config_protect_explicit) = match get(matches, "remote_config_protect") {
        Some(raw) => (raw, true),
        None => ("/etc".to_string(), false),
    };
    let (config_protect_mask, config_protect_mask_explicit) =
        match get(matches, "remote_config_protect_mask") {
            Some(raw) => (raw, true),
            None => ("/etc/env.d".to_string(), false),
        };
    let portuale_binary = get(matches, "remote_portuale_binary").filter(|b| !b.is_empty());
    let portuale_dir = match get(matches, "remote_portuale_dir").filter(|d| !d.is_empty()) {
        Some(raw) if !raw.starts_with('/') => {
            return Err(format!(
                "mrg: --remote-portuale-dir must be an absolute client path, got {raw:?}"
            ));
        }
        other => other,
    };
    Ok(Some(RemoteContext {
        hostname,
        user: get(matches, "remote_user").filter(|u| !u.is_empty()),
        port,
        key_file: get(matches, "remote_key_file").filter(|k| !k.is_empty()),
        timeout_secs,
        ssh_args: get(matches, "remote_ssh_args").filter(|a| !a.is_empty()),
        strict_host_key_checking,
        max_clock_skew_secs,
        root,
        workdir: get(matches, "remote_workdir")
            .unwrap_or_else(|| "/var/tmp/portage-remote".to_string()),
        transport,
        binpkg: get(matches, "remote_binpkg").filter(|b| !b.is_empty()),
        etc_portage: match get(matches, "remote_etc_portage") {
            None => ConfigPlacement::Client("/etc/portage".to_string()),
            Some(raw) => ConfigPlacement::parse("--remote-etc-portage", &raw)?,
        },
        vdb,
        edb,
        ledger_dir: get(matches, "remote_ledger_dir").filter(|d| !d.is_empty()),
        require_ledger_match: matches.get_flag("remote_require_ledger_match"),
        config_protect,
        config_protect_explicit,
        config_protect_mask,
        config_protect_mask_explicit,
        portuale_binary,
        portuale_dir,
        bin_plan: ClientBinPlan::Unresolved,
    }))
}
