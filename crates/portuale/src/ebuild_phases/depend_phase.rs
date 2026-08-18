//! The depend phase and its metadata cache.

use crate::ebuild_phases::*;

/// Runs one ebuild's `depend` phase and returns the raw metadata keys it emits.
///
/// `bin/ebuild.sh depend` writes `KEY=value` lines to `${PORTAGE_PIPE_FD}` (wired
/// to a `${T}` temp file). Never sandboxed; `Bash` only, since Brush takes no raw fd.
// Portage: EbuildMetadataPhase, bin/ebuild.sh, _doebuild_spawn
/// A `run_depend_phase` failure carrying the exit code, so `regen.rs` can replay
/// Portage's retry decision (`returncode != 1` is retried).
pub(crate) struct DependError {
    pub(crate) code: i32,
    pub(crate) message: String,
}

pub(crate) fn run_depend_phase(
    env: &Environment,
    root: &Path,
    config_root: &Path,
    debug: bool,
) -> Result<std::collections::HashMap<String, String>, DependError> {
    let bin_dir = bin_dir()
        .map_err(|message| DependError { code: 1, message })?
        .to_path_buf();
    let helpers_dir = bin_dir.join("ebuild-helpers");
    // Pre-spawn failure (like Portage's `doebuild` int retval): code 1, never retried.
    create_directories(env).map_err(|e| DependError {
        code: 1,
        message: e,
    })?;

    // Per-process stand-in for Portage's private metadata pipe: a fixed name let
    // concurrent runs delete each other's metadata and lose the package.
    let meta_path = env
        .t()
        .join(format!(".depend-metadata.{}", std::process::id()));
    let _ = std::fs::remove_file(&meta_path);

    let mut vars = phase_env_vars(PhaseEnvVarsArgs {
        env,
        root,
        ebuild_phase_value: "depend",
        debug,
        bin_dir: &bin_dir,
        helpers_dir: &helpers_dir,
        config_root,
        extra_env: &[],
    });
    vars.push(("PORTAGE_PIPE_FD".to_string(), "9".to_string()));

    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-c")
        .arg(r#"exec 9>"$1"; shift; exec "$@""#)
        .arg("portuale-regen") // $0
        .arg(&meta_path) // $1
        .arg("bash")
        .arg(bin_dir.join("ebuild.sh"))
        .arg("depend");
    cmd.envs(vars);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    let out = cmd.output().map_err(|e| DependError {
        // Portage: EbuildMetadataPhase._async_start — failing before the spawn
        // surfaces as code 1 ("expected", never retried).
        code: 1,
        message: format!("spawning bash for the depend phase failed: {e}"),
    })?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(DependError {
            // The exit code passes through verbatim (no sandbox-log rewrite: never sandboxed).
            code,
            message: format!(
                "depend phase failed for {}/{} (exit {code}):\n{tail}",
                env.category, env.split.pf,
            ),
        });
    }

    let text = std::fs::read_to_string(&meta_path).map_err(|e| DependError {
        // Exit 0 but no metadata: Portage's `metadata_valid == False` arm (code 1).
        code: 1,
        message: format!("{}: {e}", meta_path.display()),
    })?;
    let _ = std::fs::remove_file(&meta_path);
    Ok(parse_aux_entry(&text))
}

/// Splits the `KEY=value` lines from the phase pipe (or a depcachedir entry)
/// into the map `read_md5_cache` also returns. Lines without `=` are skipped.
pub(crate) fn parse_aux_entry(text: &str) -> std::collections::HashMap<String, String> {
    let mut md = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            md.insert(k.to_string(), v.to_string());
        }
    }
    md
}

/// Portage's flat depcachedir layout (`cache/flat_hash.py`):
/// `<depcachedir>/<repo path without '/'>/<cat>/<pf>`.
pub(crate) fn depcache_entry_path(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> std::path::PathBuf {
    let location = repo_location.to_string_lossy();
    depcachedir()
        .join(location.trim_start_matches('/').trim_end_matches('/'))
        .join(category)
        .join(pf)
}

/// `$PORTAGE_DEPCACHEDIR` when set, else `/var/cache/edb/dep`; process env only.
/// Portage: config.py, settings.depcachedir
pub(crate) fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}
