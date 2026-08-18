//! GPG signature verification.

use crate::binpkg::*;

/// GPG binpkg signature policy + verification via the system `gpg`
/// subprocess, which keeps the musl-static story (zero linked crypto)
/// intact.
///
/// `BINPKG_GPG_VERIFY_BASE_COMMAND` is a template: `[PORTAGE_CONFIG]`
/// becomes `--homedir <BINPKG_GPG_VERIFY_GPG_HOME>` and `[SIGNATURE]`
/// becomes `<detached-.sig-file> -` for a detached `.sig` member, or
/// `--output - -` for an inline clear-signed `Manifest`. The verified
/// bytes go on `gpg`'s stdin; for clear-sign its stdout is the cleartext
/// parsed as the Manifest afterwards.
///
/// Policy: `binpkg-request-signature` forces both flags on (it beats
/// `binpkg-ignore-signature`), `binpkg-ignore-signature` forces both off,
/// otherwise signatures are verified when present but not required.
// Portage: `gpkg.checksum_helper(VERIFY)`, `gpkg._verify_binpkg`,
// `gpkg.__init__`, `cnf/make.globals`.
#[derive(Clone, Debug)]
pub struct GpgVerify {
    /// A failed or missing cryptographic check is fatal.
    pub verify_signature: bool,
    /// Signature files are mandatory: a member without its `.sig` sidecar
    /// (other than the unsigned `gpkg-1` version marker) is rejected even
    /// if its digests match.
    pub request_signature: bool,
    /// `BINPKG_GPG_VERIFY_BASE_COMMAND`.
    pub base_command: String,
    /// `BINPKG_GPG_VERIFY_GPG_HOME`.
    pub gpg_home: String,
}

/// The default verify command from `cnf/make.globals:53`.
pub const DEFAULT_GPG_VERIFY_BASE_COMMAND: &str = "/usr/bin/gpg --verify --batch --no-tty --yes --no-auto-check-trustdb --status-fd 2 [PORTAGE_CONFIG] [SIGNATURE]";

/// The default verify keyring from `cnf/make.globals:56`.
pub const DEFAULT_GPG_VERIFY_GPG_HOME: &str = "/etc/portage/gnupg";

impl Default for GpgVerify {
    fn default() -> Self {
        let (verify_signature, request_signature) =
            gpg_policy_for_features(&std::env::var("FEATURES").unwrap_or_default());
        Self {
            verify_signature,
            request_signature,
            base_command: std::env::var("BINPKG_GPG_VERIFY_BASE_COMMAND")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string()),
            gpg_home: std::env::var("BINPKG_GPG_VERIFY_GPG_HOME")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_GPG_HOME.to_string()),
        }
    }
}

/// The `request_signature` / `verify_signature` derivation as a pure
/// function of the `FEATURES` string, so it can be unit-tested without
/// mutating the process environment. Returns `(verify_signature,
/// request_signature)`.
// Portage: `gpkg.__init__`.
pub fn gpg_policy_for_features(features: &str) -> (bool, bool) {
    let has = |tok: &str| features.split_whitespace().any(|t| t == tok);
    // A request beats an ignore.
    if has("binpkg-request-signature") {
        (true, true)
    } else if has("binpkg-ignore-signature") {
        (false, false)
    } else {
        (true, false)
    }
}
