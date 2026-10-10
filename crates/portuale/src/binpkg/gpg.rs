//! GPG signature verification.

use crate::binpkg::*;

/// Real GPG binpkg signature policy + verification (real
/// `lib/portage/gpkg.py`'s `checksum_helper(VERIFY)` and
/// `gpkg._verify_binpkg`'s own GPG layer), via the system `gpg`
/// subprocess -- the same "shell out to the real tool" stance `tar` /
/// the compressors / `wget` already take, which keeps the musl-static
/// story (zero linked crypto) intact.
///
/// Real `BINPKG_GPG_VERIFY_BASE_COMMAND` (`cnf/make.globals:53`) is a
/// template: `[PORTAGE_CONFIG]` -> `--homedir <BINPKG_GPG_VERIFY_GPG_HOME>`
/// and `[SIGNATURE]` -> `<detached-.sig-file> -` for a detached `.sig`
/// member, or `--output - -` for an inline clear-signed `Manifest`. The
/// verified bytes go on `gpg`'s stdin; for clear-sign its stdout is the
/// cleartext (what real parses as the Manifest afterwards).
///
/// Policy mirrors real `gpkg.__init__` (`gpkg.py:792-819`) with no
/// per-binrepo override (that override is display/parse-only in
/// portuale -- see `GpgVerify::from_env`): `binpkg-request-signature`
/// forces both flags on (it beats `binpkg-ignore-signature`, real's own
/// `if`/`elif` order), `binpkg-ignore-signature` forces both off,
/// otherwise signatures are verified when present but not required.
#[derive(Clone, Debug)]
pub struct GpgVerify {
    /// Real `gpkg.verify_signature`: a failed or missing cryptographic
    /// check is fatal.
    pub verify_signature: bool,
    /// Real `gpkg.request_signature`: signature files are mandatory --
    /// a member without its `.sig` sidecar (other than the unsigned
    /// `gpkg-1` version marker) is rejected even if its digests match.
    pub request_signature: bool,
    /// Real `BINPKG_GPG_VERIFY_BASE_COMMAND`.
    pub base_command: String,
    /// Real `BINPKG_GPG_VERIFY_GPG_HOME`.
    pub gpg_home: String,
}

/// Real `cnf/make.globals:53`'s own default verify command.
pub const DEFAULT_GPG_VERIFY_BASE_COMMAND: &str = "/usr/bin/gpg --verify --batch --no-tty --yes --no-auto-check-trustdb --status-fd 2 [PORTAGE_CONFIG] [SIGNATURE]";

/// Real `cnf/make.globals:56`'s own default verify keyring.
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
    /// Real portage's own `settings`-derived verify configuration, via
    /// the same "read the env var, fall back to `make.globals`'s own
    /// default" shortcut every other real-execution CLI boundary in this
    /// portuale already takes: `FEATURES` (`binpkg-request-signature` /
    /// `binpkg-ignore-signature`), `BINPKG_GPG_VERIFY_BASE_COMMAND`,
    /// `BINPKG_GPG_VERIFY_GPG_HOME`. A per-binrepo
    /// `verify-signature` (`binrepos.conf`) is **not** consulted here --
    /// use [`GpgVerify::from_binrepo`] on the `--getbinpkg` path, which
    /// knows which repo the download came from; this fallback is for a
    /// local `$PKGDIR` file that has no binrepo at all.
    pub fn from_env() -> Self {
        Self::default()
    }

    /// The same policy from a **resolved** `FEATURES` list (real
    /// `settings.features`, #37 S3): `set_resolved_features` replaces
    /// `from_env`'s raw read on the `emerge` paths, so a `make.conf`
    /// `binpkg-request-signature`/`binpkg-ignore-signature` takes
    /// effect.
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

    /// Real `gpkg.__init__`'s own per-binrepo policy (`gpkg.py:792-819`)
    /// with `verify-signature` **set**: real's shipped
    /// `cnf/binrepos.conf` `[DEFAULT] verify-signature = true` means an
    /// explicit `false` in a `binrepos.conf` section is the meaningful
    /// case, and portuale's [`portage_profile::BinRepo::verify_signature`]
    /// is already `true` by default, so a `true` here carries real's
    /// "explicitly true" semantics: signature verification and the
    /// sidecar requirement both on. `FEATURES` then overrides in one
    /// direction if stronger (`binpkg-request-signature` forces both on,
    /// `binpkg-ignore-signature` forces both off), exactly real's own
    /// `if`/`elif` tail -- so a `--getbinpkg` download from a repo that
    /// opted out merges unsigned packages the way real does (backlog
    /// #43).
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

/// Real `gpkg.__init__`'s own `request_signature` / `verify_signature`
/// derivation (`gpkg.py:798-819`), as a pure function of the `FEATURES`
/// string so it can be unit-tested without mutating the process
/// environment. Returns `(verify_signature, request_signature)`.
pub fn gpg_policy_for_features(features: &str) -> (bool, bool) {
    let has = |tok: &str| features.split_whitespace().any(|t| t == tok);
    // Real's own `if`/`elif` order: a request beats an ignore.
    if has("binpkg-request-signature") {
        (true, true)
    } else if has("binpkg-ignore-signature") {
        (false, false)
    } else {
        (true, false)
    }
}

/// Fill real `BINPKG_GPG_VERIFY_BASE_COMMAND`'s own two placeholders
/// (`gpkg.py:500-525`): `[PORTAGE_CONFIG]` -> `--homedir <gpg_home>`,
/// `[SIGNATURE]` -> `signature_arg` (either `<sig-file> -` for a
/// detached `.sig`, or `--output - -` for a clear-signed Manifest).
/// Split on whitespace -- a deliberate narrowing of real's
/// `shlex.split` + `varexpand` (real's own default template and every
/// test command split cleanly; a path with spaces would not).
pub(crate) fn gpg_verify_argv(template: &str, gpg_home: &str, signature_arg: &str) -> Vec<String> {
    template
        .replace("[PORTAGE_CONFIG]", &format!("--homedir {gpg_home} "))
        .replace("[SIGNATURE]", signature_arg)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Run one `gpg --verify` argv with `stdin_data` on its stdin, mirroring
/// real `checksum_helper.finish` + `_check_gpg_status`
/// (`gpkg.py:624-659` + `:590-612`): exit-status 0 is not enough --
/// GnuPG returns OK even for an untrusted signer, so the `--status-fd`
/// lines must carry both `GOODSIG` and `TRUST_ULTIMATE`/`TRUST_FULLY`,
/// else `InvalidSignature`. Returns gpg's stdout (the cleartext for a
/// clear-signed Manifest; empty for a detached `.sig`).
pub(crate) fn run_gpg_verify(argv: &[String], stdin_data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawning {}: {e}", argv[0]))?;
    // A write failure here (EPIPE -- gpg already exited on bad input)
    // carries no signal of its own; gpg's exit status + status lines
    // below are the verdict, so it is deliberately ignored.
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
    // Real `show_gpg_error`'s own single-cause summaries
    // (`gpkg.py:563-579`): only when exactly one cause matches, else
    // "(none available)" -- a malformed signature must not get a
    // confident-sounding diagnosis.
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

/// Real per-file detached verification (`_verify_binpkg`'s own
/// `f_signature` branch, `gpkg.py:1764-1788`): the `.sig` sidecar goes
/// to a temp file named in `[SIGNATURE]`, the member bytes on stdin.
/// `what` names the member for the error (real reports per-file).
pub(crate) fn verify_detached_signature(
    member_bytes: &[u8],
    sig_bytes: &[u8],
    gpg: &GpgVerify,
    what: &str,
) -> Result<(), String> {
    let sig_dir = TempDir::new("portuale-sign").keep();
    let sig_path = sig_dir.join("x.sig");
    fs::write(&sig_path, sig_bytes).map_err(|e| format!("{}: {e}", sig_path.display()))?;
    let argv = gpg_verify_argv(
        &gpg.base_command,
        &gpg.gpg_home,
        &format!("{} -", sig_path.display()),
    );
    let result = run_gpg_verify(&argv, member_bytes).map(|_| ());
    let _ = fs::remove_file(&sig_path);
    result.map_err(|e| format!("{what}: {e}"))
}

/// Real clear-signed-`Manifest` verification (`_verify_binpkg`'s own
/// Manifest branch, `gpkg.py:1714-1731`): `[SIGNATURE]` -> `--output -
/// -`, the whole signed Manifest on stdin, gpg's stdout (the
/// cleartext `DATA` lines) back for Manifest parsing.
pub(crate) fn verify_clearsigned_manifest(
    signed_manifest: &[u8],
    gpg: &GpgVerify,
    gpkg_path: &Path,
) -> Result<Vec<u8>, String> {
    let argv = gpg_verify_argv(&gpg.base_command, &gpg.gpg_home, "--output - -");
    run_gpg_verify(&argv, signed_manifest)
        .map_err(|e| format!("{}: Manifest {e}", gpkg_path.display()))
}
