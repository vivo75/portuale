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
        drop_ebuild_phase: false,
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
    Ok(portage_repo::parse_aux_entry(&text))
}

/// Cache-miss provider: regenerates one ebuild's aux metadata by running its
/// `depend` phase, in the same `HashMap` shape `read_md5_cache` returns.
///
/// Wraps the phase run with the depcachedir rung: a fresh entry is reused
/// as-is, a successful run is written back (best-effort; a read-only dir keeps
/// the result in the in-memory memo).
///
/// Lives in `portuale` (registered once from `main`); `portage-repo` holds only
/// the fn-pointer slot.
// Portage: porttree.py, _pull_valid_cache, EbuildMetadataPhase, flat_hash.py
pub(crate) fn depend_phase_metadata(
    repo_location: &Path,
    category: &str,
    pf: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let Some((package, _version)) = portage_repo::split_pf(pf) else {
        return Err(format!("{pf}: not a package-version name"));
    };
    let ebuild = repo_location
        .join(category)
        .join(&package)
        .join(format!("{pf}.ebuild"));
    if !ebuild.is_file() {
        return Err(format!("{}: no ebuild", ebuild.display()));
    }
    let config_root = portage_repo::config_root_from_env();
    let root = portage_repo::root_from_env();
    let masters = portage_repo::md5_dict::repo_masters_for_location(repo_location);

    // The depcachedir auxdb sits between the repo pregen cache and the phase run;
    // same validator, but its `_eclasses_` carries eclass dir paths.
    if let Ok(md) = portage_repo::depcache_aux_metadata(repo_location, category, pf) {
        return Ok(md);
    }
    let depcache = portage_repo::depcache_entry_path(repo_location, category, pf);

    // Portage parses the EAPI head first and skips unsupported ones; portuale
    // returns a failure instead, so the cp drops out rather than masking by EAPI.
    let head_eapi = parse_eapi(&std::fs::read_to_string(&ebuild).unwrap_or_default());
    let head_eapi = if head_eapi.is_empty() {
        "0".to_string()
    } else {
        head_eapi
    };
    if !portage_repo::md5_dict::eapi_is_supported(&head_eapi) {
        return Err(format!(
            "{}: EAPI {head_eapi} is unsupported",
            ebuild.display()
        ));
    }

    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
    let env = compute_environment(&ebuild, &portage_tmpdir)?;
    let md = run_depend_phase(&env, &root, &config_root, false).map_err(|e| e.message)?;

    // Portage: _write_cache — stamps the ebuild md5, then writes the entry.
    if let Ok(body) = crate::regen::render_entry(&md, &ebuild, repo_location, &masters, true)
        && let Some(dir) = depcache.parent()
    {
        let _ = crate::regen::write_entry(dir, pf, &body);
    }
    Ok(md)
}
