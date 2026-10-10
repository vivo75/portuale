//! Configuration for standalone `ebuild` phases and unmerge.

use crate::ebuild_phases::*;

/// Whether the md5-cache `RESTRICT` value carries `mirror` (or the
/// deprecated `nomirror`), USE-reduced against the empty fetch-side USE set.
pub(crate) fn restrict_mirror_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["mirror", "nomirror"])
}

/// Whether `RESTRICT` carries `fetch`, reduced the same way; gates the
/// plain-URI and public-mirror candidates in `fetch_src_uri`.
pub(crate) fn restrict_fetch_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["fetch"])
}

/// Whether `RESTRICT` carries `primaryuri`, reduced the same way; the
/// file's own literal URIs move to the front of its candidate list.
pub(crate) fn restrict_primaryuri_from_restrict(restrict: &str) -> bool {
    flat_field_has_token(restrict, &["primaryuri"])
}

/// Whether any of `wanted` survives USE-reduction of a raw
/// `RESTRICT`/`PROPERTIES` value against the empty USE set.
pub(crate) fn flat_field_has_token(raw: &str, wanted: &[&str]) -> bool {
    if raw.trim().is_empty() {
        return false;
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(
        &tokens,
        &std::collections::HashSet::new(),
        portage_use_reduce::MatchMode::Normal,
    )
    .map(|flat| flat.iter().any(|t| wanted.contains(&t.as_str())))
    .unwrap_or(false)
}

/// The USE-reduced `RESTRICT`/`PROPERTIES` list as plain text, deduped
/// and sorted like Portage's `_flatten` (Portage: config.py, _flatten).
/// `use_set` is the reduction input: empty except for `depend`.
pub(crate) fn flat_field_on(raw: &str, use_set: &std::collections::HashSet<String>) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let tokens: Vec<String> = raw.split_whitespace().map(String::from).collect();
    portage_use_reduce::use_reduce_flat(&tokens, use_set, portage_use_reduce::MatchMode::Normal)
        .map(|flat| {
            let unique: std::collections::BTreeSet<String> = flat.into_iter().collect();
            unique.into_iter().collect::<Vec<_>>().join(" ")
        })
        .unwrap_or_default()
}

/// The standalone base env: the package's effective `USE` plus the
/// resolved config env, exported into every phase. Skipped when
/// `extra_env` already carries `USE` (merge builds) and for `depend`.
/// Standalone `package.env` halves flow here too; see notes.
///
/// `(USE display pairs, build flag pairs)` from one standalone config
/// load -- the alias keeps the loader closure below under clippy's
/// `type_complexity` lint.
pub(crate) type StandaloneBaseEnv = (Vec<(String, bool)>, Vec<(String, String)>);

/// The resolved config for the standalone `ebuild <file>` paths;
/// `None` outside any repo checkout.
pub(crate) fn resolve_standalone_config(
    pkg_dir: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let _ = repo_root_for(pkg_dir)?;
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
}

/// The `cat/pkg-ver:slot/sub` identity to match `package.env` against
/// for an ebuild with no resolved graph entry, from md5-cache `SLOT`.
pub(crate) fn standalone_cpv_slot(pkg_dir: &Path, category: &str, pf: &str) -> Option<String> {
    let repo_root = repo_root_for(pkg_dir)?;
    let slot_raw = portage_repo::repo_aux_metadata(&repo_root, category, pf)
        .ok()
        .and_then(|m| m.get("SLOT").cloned())
        .unwrap_or_default();
    let (slot, sub_slot) = match slot_raw.split_once('/') {
        Some((s, ss)) => (s.to_string(), ss.to_string()),
        None if slot_raw.is_empty() => ("0".to_string(), "0".to_string()),
        None => (slot_raw.clone(), slot_raw),
    };
    // `pf` already carries `name-version`; the category + slot complete
    // the match string.
    Some(format!("{category}/{pf}:{slot}/{sub_slot}"))
}

pub(crate) fn phase_standalone_base_env(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
    ebuild_phase_value: &str,
    extra_env: &[(String, String)],
) -> (String, Vec<(String, String)>) {
    if ebuild_phase_value == "depend" || extra_env.iter().any(|(k, _)| k == "USE") {
        return (String::new(), Vec::new());
    }
    let Some((display, flags)) = (|| -> Option<StandaloneBaseEnv> {
        let config = resolve_standalone_config(&env.pkg_dir, config_root, eroot)?;
        let repos = portage_repo::find_repos(config_root).ok()?;
        let display = portage_repo::candidate_use_flags_display(
            &repos,
            &config,
            &env.category,
            &env.split.pn,
            &env.split.pvr,
        );
        let flags = portage_profile::phase_environ(&config, None);
        // Per-package `package.env` build vars, atom-matched against
        // the ebuild's own md5-cache identity. Layered after the base
        // flags so they win; the `USE=` half already folded in above.
        let cpv_slot = standalone_cpv_slot(&env.pkg_dir, &env.category, &env.split.pf)?;
        let mut flags = flags;
        // The run-wide base folds with pkg and calling env in order;
        // scalars lose to the caller.
        let profile_only_variables = config
            .resolved_incremental("PROFILE_ONLY_VARIABLES")
            .unwrap_or_default();
        let base = flags.clone();
        flags.extend(match_package_env_vars(
            &config.package_env_vars,
            &cpv_slot,
            &profile_only_variables,
            &base,
            &portage_profile::config_env_all(),
        ));
        // Per-package `FEATURES` is incremental: fold the matched raw
        // value onto the resolved run-wide list instead of replacing it.
        // No append when nothing matched, so fixtures without a
        // `package.env` `FEATURES` entry keep byte-identical output.
        let pkg_features =
            match_package_env_incremental_raw(&config.package_env_vars, &cpv_slot, "FEATURES");
        if !pkg_features.is_empty() {
            let run_wide = config
                .resolved_incremental("FEATURES")
                .or_else(|| {
                    config
                        .other_vars
                        .get("FEATURES")
                        .map(|f| f.split_whitespace().map(String::from).collect())
                })
                .unwrap_or_default()
                .join(" ");
            let folded = fold_package_env_incremental(
                &run_wide,
                &pkg_features,
                &std::env::var("FEATURES").unwrap_or_default(),
            );
            flags.push(("FEATURES".to_string(), folded.clone()));
            flags.push(("PORTAGE_FEATURES".to_string(), folded));
        }
        Some((display, flags))
    })() else {
        return (String::new(), Vec::new());
    };
    let enabled: Vec<&str> = display
        .iter()
        .filter(|(_, on)| *on)
        .map(|(f, _)| f.as_str())
        .collect();
    (enabled.join(" "), flags)
}

/// The ebuild's USE-reduced `RESTRICT`/`PROPERTIES`; `("", "")`
/// outside any repo checkout. `use_set` is empty except for `depend`.
pub(crate) fn restrict_and_properties(
    env: &Environment,
    use_set: &std::collections::HashSet<String>,
) -> (String, String) {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return (String::new(), String::new());
    };
    let metadata = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf).ok();
    let get = |key: &str| {
        metadata
            .as_ref()
            .and_then(|m| m.get(key))
            .map(String::as_str)
            .unwrap_or("")
    };
    (
        flat_field_on(get("RESTRICT"), use_set),
        flat_field_on(get("PROPERTIES"), use_set),
    )
}

/// Resolves a per-package `PORTAGE_TMPDIR` match, re-deriving the tmpdir.
/// The calling environment wins when it carries the key; a matched
/// directory that does not exist fails like Portage's `_check_temp_dir`.
/// `run_tmpdir` stands whenever nothing matched. The caller maps the
/// `Err` to exit 1.
pub(crate) fn resolve_entry_portage_tmpdir(
    package_env_vars: &[(String, Vec<(String, String)>)],
    cpv_slot: &str,
    run_tmpdir: &Path,
    process_tmpdir: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(process) = process_tmpdir {
        return Ok(process.to_path_buf());
    }
    let matched = match_package_env_scalar_raw(package_env_vars, cpv_slot, "PORTAGE_TMPDIR");
    let Some(dir) = matched.as_deref().filter(|v| !v.is_empty()) else {
        return Ok(run_tmpdir.to_path_buf());
    };
    if !Path::new(dir).is_dir() {
        return Err(format!(
            "The directory specified in your PORTAGE_TMPDIR variable, '{dir}',\n\
             does not exist.  Please create this directory or correct your PORTAGE_TMPDIR setting."
        ));
    }
    Ok(PathBuf::from(dir))
}

/// Standalone half of the above, for an `ebuild <file>` run with no
/// resolved graph entry. Keeps `run_tmpdir` when nothing resolves.
pub(crate) fn resolve_standalone_portage_tmpdir(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    run_tmpdir: &Path,
    process_tmpdir: Option<&Path>,
) -> Result<PathBuf, String> {
    if process_tmpdir.is_some() {
        return Ok(run_tmpdir.to_path_buf());
    }
    let Some((config, cpv_slot)) = standalone_package_env_lookup(ebuild_path, config_root, eroot)
    else {
        return Ok(run_tmpdir.to_path_buf());
    };
    resolve_entry_portage_tmpdir(&config.package_env_vars, &cpv_slot, run_tmpdir, None)
}

/// Folds a matched per-package `FEATURES` onto `run_wide_features`.
/// `None` when nothing resolves or nothing matched.
pub(crate) fn resolve_standalone_resolved_features(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    run_wide_features: &str,
    calling_features: &str,
) -> Option<String> {
    let (config, cpv_slot) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    let pkg_raw =
        match_package_env_incremental_raw(&config.package_env_vars, &cpv_slot, "FEATURES");
    if pkg_raw.is_empty() {
        return None;
    }
    Some(fold_package_env_incremental(
        run_wide_features,
        &pkg_raw,
        calling_features,
    ))
}

/// The matched `(compress, flags, bzip2_command)` triple for a
/// standalone `ebuild <file> package`; `None` when nothing matched.
/// Per-key calling-env-wins precedence stays the caller's job.
pub(crate) fn resolve_standalone_binpkg_compress(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<(String, Option<String>, Option<String>)> {
    let (config, cpv_slot) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    let profile_only = config
        .resolved_incremental("PROFILE_ONLY_VARIABLES")
        .unwrap_or_default();
    let matched = |key: &str| {
        if !package_env_key_allowed(key, &profile_only) {
            return None;
        }
        match_package_env_scalar_raw(&config.package_env_vars, &cpv_slot, key)
    };
    let compress = matched("BINPKG_COMPRESS")?;
    let flags = matched(&format!(
        "BINPKG_COMPRESS_FLAGS_{}",
        compress.to_uppercase()
    ))
    .or_else(|| matched("BINPKG_COMPRESS_FLAGS"));
    let bzip2 = matched("PORTAGE_BZIP2_COMMAND");
    Some((compress, flags, bzip2))
}

/// The resolved chain value below the calling env for a global scalar.
/// `None` when the chain names no value. Deliberately not `package.env`-matched.
pub(crate) fn resolve_standalone_chain_scalar(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
    key: &str,
) -> Option<String> {
    let (config, _) = standalone_package_env_lookup(ebuild_path, config_root, eroot)?;
    config.other_vars.get(key).cloned()
}

/// `resolve_standalone_chain_scalar` for `BINPKG_FORMAT`.
pub(crate) fn resolve_standalone_binpkg_format(
    ebuild_path: &Path,
    config_root: &Path,
    eroot: &Path,
) -> Option<String> {
    resolve_standalone_chain_scalar(ebuild_path, config_root, eroot, "BINPKG_FORMAT")
}

/// The resolved global `Config` for the config-less `emerge -C` path.
/// `None` when no config resolves; the caller keeps its default.
pub(crate) fn resolve_unmerge_config(
    config_root: &Path,
    eroot: &Path,
) -> Option<portage_profile::Config> {
    let repos = portage_repo::find_repos(config_root).ok()?;
    let main_repo = repos.iter().find(|r| r.is_main)?;
    let overlay_repos: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let repo_aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    let repo_masters: std::collections::HashMap<String, Vec<PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot,
    })
    .ok()
}
