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

impl GpgVerify {
    /// The `settings`-derived verify configuration, via the same "read the
    /// env var, fall back to `make.globals`' default" shortcut every other
    /// execution CLI boundary here takes. A per-binrepo
    /// `verify-signature` (`binrepos.conf`) is **not** consulted here;
    /// use [`GpgVerify::from_binrepo`] on the `--getbinpkg` path, which
    /// knows which repo the download came from; this fallback is for a
    /// local `$PKGDIR` file that has no binrepo at all.
    pub fn from_env() -> Self {
        Self::default()
    }

    /// The same policy from a **resolved** `FEATURES` list:
    /// `set_resolved_features` replaces `from_env`'s raw read on the
    /// `emerge` paths, so a `make.conf`
    /// `binpkg-request-signature`/`binpkg-ignore-signature` takes effect.
    pub fn from_features(features: &str) -> Self {
        let (verify_signature, request_signature) = gpg_policy_for_features(features);
        Self {
            verify_signature,
            request_signature,
            base_command: std::env::var("BINPKG_GPG_VERIFY_BASE_COMMAND")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_BASE_COMMAND.to_string()),
            gpg_home: std::env::var("BINPKG_GPG_VERIFY_GPG_HOME")
                .unwrap_or_else(|_| DEFAULT_GPG_VERIFY_GPG_HOME.to_string()),
        }
    }

    /// The per-binrepo policy with `verify-signature` **set**: an explicit
    /// `false` in a `binrepos.conf` section is the meaningful case (the
    /// default is already `true`), and carries "explicitly true" semantics
    /// when set. `FEATURES` then overrides in one direction if stronger,
    /// so a `--getbinpkg` download from a repo that opted out merges
    /// unsigned packages.
    // Portage: `gpkg.__init__`.
    pub fn from_binrepo(verify_signature: bool, features: &str) -> Self {
        let has = |tok: &str| features.split_whitespace().any(|t| t == tok);
        let (mut verify, mut request) = if verify_signature {
            (true, true)
        } else {
            (false, false)
        };
        if has("binpkg-request-signature") {
            verify = true;
            request = true;
        } else if has("binpkg-ignore-signature") {
            verify = false;
            request = false;
        }
        Self {
            verify_signature: verify,
            request_signature: request,
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

/// Fill the verify command's two placeholders: `[PORTAGE_CONFIG]` becomes
/// `--homedir <gpg_home>`, `[SIGNATURE]` becomes `signature_arg` (either
/// `<sig-file> -` for a detached `.sig`, or `--output - -` for a
/// clear-signed Manifest). Split on whitespace, not `shlex` +
/// `varexpand` (a path with spaces would not split cleanly).
// Portage: `BINPKG_GPG_VERIFY_BASE_COMMAND`.
pub(crate) fn gpg_verify_argv(template: &str, gpg_home: &str, signature_arg: &str) -> Vec<String> {
    template
        .replace("[PORTAGE_CONFIG]", &format!("--homedir {gpg_home} "))
        .replace("[SIGNATURE]", signature_arg)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Run one `gpg --verify` argv with `stdin_data` on its stdin. Exit-status
/// 0 is not enough: GnuPG returns OK even for an untrusted signer, so the
/// `--status-fd` lines must carry both `GOODSIG` and
/// `TRUST_ULTIMATE`/`TRUST_FULLY`, else `InvalidSignature`. Returns gpg's
/// stdout (the cleartext for a clear-signed Manifest; empty for a
/// detached `.sig`).
// Portage: `gpkg.checksum_helper.finish`, `gpkg._check_gpg_status`.
pub(crate) fn run_gpg_verify(argv: &[String], stdin_data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning {}: {e}", argv[0]))?;
    // A write failure here (EPIPE: gpg already exited on bad input) is not
    // the verdict; gpg's exit status + status lines below are.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_data);
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("waiting for {}: {e}", argv[0]))?;
    let status_lines = String::from_utf8_lossy(&output.stderr);
    let good_sig = status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] GOODSIG"));
    let trusted = status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] TRUST_ULTIMATE") || l.starts_with("[GNUPG:] TRUST_FULLY"));
    if output.status.success() && good_sig && trusted {
        return Ok(output.stdout);
    }
    // Single-cause summaries only; otherwise "(none available)", so a
    // malformed signature gets no confident-sounding diagnosis.
    // Portage: `gpkg.show_gpg_error`.
    let mut causes = 0;
    let mut summary = "(none available)";
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] NODATA"))
    {
        causes += 1;
        summary = "binpkg appears unsigned (missing any signature)";
    }
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] NO_PUBKEY"))
    {
        causes += 1;
        summary = "binpkg signed with at least one unknown key.";
    }
    if status_lines
        .lines()
        .any(|l| l.starts_with("[GNUPG:] TRUST_UNDEFINED"))
    {
        causes += 1;
        summary = "binpkg signed with a known key of undefined trust.";
    }
    if causes != 1 {
        summary = "(none available)";
    }
    Err(format!(
        "GnuPG verification failed: {summary}\n{}",
        status_lines.trim_end()
    ))
}

/// Per-file detached verification: the `.sig` sidecar goes to a temp file
/// named in `[SIGNATURE]`, the member bytes on stdin. `what` names the
/// member for the error.
// Portage: `gpkg._verify_binpkg`'s `f_signature` branch.
pub(crate) fn verify_detached_signature(
    member_bytes: &[u8],
    sig_bytes: &[u8],
    gpg: &GpgVerify,
    what: &str,
) -> Result<(), String> {
    let sig_dir = TempDir::new("portuale-sign");
    let sig_path = sig_dir.join("x.sig");
    fs::write(&sig_path, sig_bytes).map_err(|e| format!("{}: {e}", sig_path.display()))?;
    let argv = gpg_verify_argv(
        &gpg.base_command,
        &gpg.gpg_home,
        &format!("{} -", sig_path.display()),
    );
    run_gpg_verify(&argv, member_bytes)
        .map(|_| ())
        .map_err(|e| format!("{what}: {e}"))
}

/// Clear-signed-`Manifest` verification: `[SIGNATURE]` becomes `--output -
/// -`, the whole signed Manifest goes on stdin, and gpg's stdout (the
/// cleartext `DATA` lines) comes back for Manifest parsing.
// Portage: `gpkg._verify_binpkg`'s Manifest branch.
pub(crate) fn verify_clearsigned_manifest(
    signed_manifest: &[u8],
    gpg: &GpgVerify,
    gpkg_path: &Path,
) -> Result<Vec<u8>, String> {
    let argv = gpg_verify_argv(&gpg.base_command, &gpg.gpg_home, "--output - -");
    run_gpg_verify(&argv, signed_manifest)
        .map_err(|e| format!("{}: Manifest {e}", gpkg_path.display()))
}
