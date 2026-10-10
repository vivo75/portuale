//! The depend phase and its metadata cache.

use crate::ebuild_phases::*;

/// Run one ebuild's `depend` phase (real `EbuildMetadataPhase` /
/// `doebuild(mydo="depend")`) and return the raw metadata keys it emits.
///
/// Real `bin/ebuild.sh`'s `depend` branch (`ebuild.sh:781-804`) writes
/// `KEY=value` lines (`DEPEND RDEPEND SLOT SRC_URI RESTRICT HOMEPAGE
/// LICENSE DESCRIPTION KEYWORDS INHERITED IUSE REQUIRED_USE PDEPEND
/// BDEPEND EAPI PROPERTIES DEFINED_PHASES IDEPEND INHERIT`) to
/// `${PORTAGE_PIPE_FD}` -- real portage's `_metadata_fd` -- so incidental
/// stdout/stderr can't corrupt the metadata. Portuale wires that fd to
/// a `${T}` temp file via a tiny `exec 9>` shell wrapper (no `unsafe`,
/// no extra crate), then parses it back. `depend` is never sandboxed
/// (real `_doebuild_spawn`'s `SANDBOXED_SRC_PHASES` excludes it), so this
/// is a plain `bash bin/ebuild.sh depend` -- no `sandbox`/`unshare`
/// wrapper. Only the `Bash` backend is used (the metadata pipe is a raw
/// fd the in-process `Brush` interpreter can't be handed).
/// An exit-code-carrying `run_depend_phase` failure. Real
/// `EbuildMetadataPhase` reports every outcome as a `returncode`
/// (`os.EX_OK` / 1 for an expected failure like invalid metadata /
/// anything else for an unexpected one), and `MetadataRegen._task_exit`
/// keys its `cp_retry` decision off exactly that (`returncode != 1` is
/// retried). Threading the code through -- instead of a bare message --
/// is what lets `regen.rs` replay that decision.
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
    // Builddir setup happens before any phase spawns -- real's
    // `doebuild`-before-spawn int retval, code 1, never retried.
    create_directories(env).map_err(|e| DependError {
        code: 1,
        message: e,
    })?;

    // Real `EbuildMetadataPhase` reads the metadata from a private pipe,
    // so concurrent `emerge` processes never share it. The builddir is
    // shared (`${PORTAGE_TMPDIR}/portage/<cat>/<pf>`), so the file that
    // stands in for the pipe is per-process: a fixed name let two
    // concurrent cache-miss resolutions delete each other's metadata
    // and report the package as having no ebuilds.
    let meta_path = env
        .t()
        .join(format!(".depend-metadata.{}", std::process::id()));
    let _ = std::fs::remove_file(&meta_path);

    let mut vars = phase_env_vars(
        env,
        root,
        "depend",
        debug,
        &bin_dir,
        &helpers_dir,
        config_root,
        &[],
    );
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
        // Real `EbuildMetadataPhase._async_start`: `doebuild` failing
        // before it even spawns surfaces as an int retval (in
        // practice 1) -- an "expected" failure, never retried by
        // `metadata_regen_retry`'s `cp_retry` (which only re-runs a
        // cp whose phase died with an *unexpected* returncode).
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
            // The phase's own exit code passes through verbatim, exactly
            // like real `EbuildMetadataPhase`'s `returncode` (real only
            // rewrites it to 2 on a sandbox-log hit -- and the `depend`
            // phase is never sandboxed, real `_doebuild_spawn`'s
            // `SANDBOXED_SRC_PHASES` excludes it -- so no rewrite here).
            code,
            message: format!(
                "depend phase failed for {}/{} (exit {code}):\n{tail}",
                env.category, env.split.pf,
            ),
        });
    }

    let text = std::fs::read_to_string(&meta_path).map_err(|e| DependError {
        // The phase exited 0 but left no parseable metadata behind --
        // real's `metadata_valid == False` arm, which sets returncode 1
        // (an "expected" failure, never retried).
        code: 1,
        message: format!("{}: {e}", meta_path.display()),
    })?;
    let _ = std::fs::remove_file(&meta_path);
    Ok(parse_aux_entry(&text))
}

/// C2's registered cache-miss provider (see
/// `portage_repo::register_aux_metadata_provider`): regenerate one
/// ebuild's aux metadata by running its real `depend` phase -- real
/// `porttree.py`'s ebuild fallback when `metadata/md5-cache` has no
/// entry (`_pull_valid_cache` miss -> `doebuild(mydo="depend")`, C0's
/// oracle). The result is the same `HashMap` shape `read_md5_cache`
/// returns, so the resolver cannot tell the difference.
///
/// C3 adds the **depcachedir rung** around that phase run, exactly where
/// real `_pull_valid_cache` has it (`porttree.py:603-651`): before
/// running the phase, a depcachedir entry whose `_md5_` still matches
/// the ebuild is used as-is; after a successful run, the result is
/// written to the depcachedir (real `EbuildMetadataPhase` ->
/// `portdb._write_cache`, `porttree.py:578-596`) in real's flat layout
/// (`<depcachedir>/<repo path without the leading '/'>/<cat>/<pf>`,
/// `cache/flat_hash.py:20-22`). When that write cannot happen -- an
/// unwritable depcachedir, real's `secpass < 1` read-only branch -- the
/// result still lives for the process in `repo_aux_metadata`'s own memo
/// (C2). The write is best-effort; render/write live in `regen.rs`
/// because `--regen` writes the identical `flat_hash` format into the
/// repo's `metadata/md5-cache`, and the shared validator lives in
/// `portage_repo::md5_dict` (#46 S2).
///
/// Layering: this lives in `portuale` (the binary that owns real phase
/// execution) and is registered once from `main`; `portage-repo` only
/// holds the `fn`-pointer slot. `root`/`config_root`/`PORTAGE_TMPDIR`
/// come from the process environment, the same CLI-boundary defaults
/// `root_from_env`/`config_root_from_env`/`portage_tmpdir_from_env`
/// already establish. `depend` is never sandboxed, so this is a plain
/// `bash bin/ebuild.sh depend` (see `run_depend_phase`).
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

    // Real `_pull_valid_cache`: the depcachedir auxdb is consulted after
    // the repo's pregen `metadata/md5-cache` (C1's `read_md5_cache`) and
    // before `EbuildMetadataPhase` runs. Same validator as the repo
    // cache (real `template.validate_entry`), but the writable
    // depcachedir is `mtime_md5_database`: its `_eclasses_` carries the
    // eclass dir (`name\tpath\tmd5`, `store_eclass_paths = True`, S0
    // cell (a)).
    let depcache = depcache_entry_path(repo_location, category, pf);
    if let Ok(text) = std::fs::read_to_string(&depcache)
        && portage_repo::md5_dict::entry_is_valid(&text, &ebuild, repo_location, &masters, true)
    {
        return Ok(parse_aux_entry(&text));
    }

    // Real `EbuildMetadataPhase._async_start` (`:61-75`): the ebuild
    // head's EAPI is parsed before anything runs, and an unsupported one
    // skips the phase entirely -- the package is then masked by EAPI
    // visibility (S0 cell (e): `masked by: EAPI 9999`). Portuale returns
    // a failure instead of real's synthesized `{"EAPI": ...}` map (a
    // documented narrowing): the cp is dropped from candidates rather
    // than rendered as an EAPI mask.
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

    // Real `_write_cache`: `metadata["_md5_"] = ebuild_hash.md5` then
    // `cache[cpv] = metadata` -- the md5-dict/`flat_hash` writer.
    // Best-effort: a read-only depcachedir keeps the result in memory
    // (C2's memo) exactly like real's volatile `_ro_auxdb` branch.
    if let Ok(body) = crate::regen::render_entry(&md, &ebuild, repo_location, &masters, true)
        && let Some(dir) = depcache.parent()
    {
        let _ = crate::regen::write_entry(dir, pf, &body);
    }
    Ok(md)
}

/// Split the `KEY=value` lines the `depend` phase pipe (and a
/// depcachedir entry) carries into the map `read_md5_cache` also
/// returns. Lines without `=` are skipped (real's
/// `_parse_data` raises `CacheCorruption` for those from the cache
/// reader; the phase pipe is written by `bin/ebuild.sh` and never emits
/// one).
pub(crate) fn parse_aux_entry(text: &str) -> std::collections::HashMap<String, String> {
    let mut md = std::collections::HashMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            md.insert(k.to_string(), v.to_string());
        }
    }
    md
}

/// Real `flat_hash.FsBased.__init__` (`cache/flat_hash.py:20-22`) with
/// the `md5_database`/`md5-dict` label: `os.path.join(depcachedir,
/// repo_path.lstrip('/').rstrip('/'))/<category>/<pf>` -- a flat tree
/// keyed by the repo's absolute path minus the leading separator (C0's
/// oracle: `/var/cache/edb/dep/var/db/repos/porttest/porttest/docs-1.0`).
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

/// Real `settings.depcachedir` (`config.py:1114-1128`): the
/// `PORTAGE_DEPCACHEDIR` setting when present, else
/// `portage.const.DEPCACHE_PATH` = `/var/cache/edb/dep`. Not relative to
/// `ROOT` (real rebases it under `eroot` only in the unprivileged
/// non-`/` target-root fallback, which portuale's root runs never take).
/// Narrowing: real resolves the setting through the full config stack
/// (so a `make.conf` assignment wins over the environment); portuale
/// reads the process environment only, the same CLI-boundary default
/// the phase env's own whitelist (`ENVIRON_WHITELIST`) already carries.
pub(crate) fn depcachedir() -> std::path::PathBuf {
    std::env::var_os("PORTAGE_DEPCACHEDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/var/cache/edb/dep"))
}
