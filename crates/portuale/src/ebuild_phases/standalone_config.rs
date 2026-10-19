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

/// Tokens `bin/ebuild` drops from the resolved `FEATURES` for the
/// direct path: `fail-clean` always, and `merge-wait` (a scheduler-only
/// token; keeping it would let WORKDIR removal run early). The `emerge`
/// path keeps both (the Scheduler gates on `merge-wait`). `noauto` is
/// NOT dropped here: `bin/ebuild` drops it only for `merge`, together
/// with the phase-loop semantics S5/S6 own.
/// Direct-path-only helper (the standalone base env and
/// `run_real_commands`); the merge path never calls it.
// Portage: bin/ebuild:305-309
pub(crate) fn discard_direct_ebuild_features(features: &str) -> String {
    features
        .split_whitespace()
        .filter(|t| *t != "fail-clean" && *t != "merge-wait")
        .collect::<Vec<_>>()
        .join(" ")
}

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

/// The md5-cache `SLOT` for `env`'s package (`None` outside a repo or
/// without the entry): what standalone phases export, since no graph
/// entry threads one. Portage reads the same key via `aux_get`
/// (`doebuild_environment` → `setcpv`).
// Portage: doebuild.py, doebuild_environment
pub(crate) fn md5_slot(env: &Environment) -> Option<String> {
    let repo_root = repo_root_for(&env.pkg_dir)?;
    let slot = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
        .ok()
        .and_then(|m| m.get("SLOT").cloned())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some(slot)
}

/// The md5-cache declared `IUSE` tokens for `env`'s package (raw,
/// `+`/`-` default markers kept for `iuse_bare`); empty outside a repo
/// or without the entry.
pub(crate) fn md5_iuse(env: &Environment) -> Vec<String> {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Vec::new();
    };
    portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
        .ok()
        .and_then(|m| m.get("IUSE").cloned())
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// The installed same-slot versions this build replaces, space-joined
/// and sorted (`""` on a fresh install): the `REPLACING_VERSIONS`
/// default for pretend/setup/preinst/postinst. Portage joins a set
/// (nondeterministic order); the sorted order here is the deterministic
/// equivalent. Only the main slot compares (a missing `SLOT` reads
/// `"0"`, like everywhere else).
// Portage: doebuild.py, doebuild (`vardb.match(cpv_slot) + vardb.match("="+cpv)`);
//   _emerge/EbuildExecuter.py, Scheduler.py, vartree.py (merge paths)
pub(crate) fn phase_replacing_versions(
    root: &Path,
    category: &str,
    pn: &str,
    slot: &str,
) -> String {
    let main_slot = slot.split('/').next().unwrap_or(slot);
    let mut versions: Vec<String> = portage_repo::installed_versions(root, category, pn)
        .into_iter()
        .filter(|version| {
            let pf = format!("{pn}-{version}");
            let raw = portage_repo::vdb_entry_slot(root, category, &pf);
            let installed_main = raw.split('/').next().unwrap_or("");
            let installed_main = if installed_main.is_empty() {
                "0"
            } else {
                installed_main
            };
            installed_main == main_slot
        })
        .collect();
    versions.sort();
    versions.join(" ")
}

/// `IUSE_EFFECTIVE` plus the `___in_portage_iuse` body for a standalone
/// phase: the md5-cache declared `IUSE` folded over the resolved
/// config's implicit set (`None` outside a repo, without a config, or
/// below `iuse_effective`).
// Portage: config.py, setcpv
pub(crate) fn standalone_iuse_pair(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Option<(String, String)> {
    let eapi_attrs = portage_dep::eapi::eapi_attrs(Some(&env.eapi))
        .or_else(|_| portage_dep::eapi::eapi_attrs(None))
        .ok()?;
    let config = resolve_standalone_config(&env.pkg_dir, config_root, eroot)?;
    let iuse = md5_iuse(env);
    portage_profile::standalone_iuse_effective(&config, &iuse, eapi_attrs)
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

/// `PORTAGE_BASHRC_FILES` re-derived for one package when any profile node
/// carries `package.bashrc`; empty otherwise, so the run-wide value stands.
// Portage: config.py:2008-2022 (`_pbashrc`), doebuild.py (`PORTAGE_BASHRC_FILES`)
pub(crate) fn package_bashrc_env(
    config: &portage_profile::Config,
    cpv_slot: &str,
) -> Vec<(String, String)> {
    if config
        .profile_bashrc_nodes
        .iter()
        .all(|n| n.package_bashrc.is_empty())
    {
        return Vec::new();
    }
    let files: Vec<String> = config
        .bashrc_files_for(cpv_slot)
        .iter()
        .map(|f| f.display().to_string())
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    vec![("PORTAGE_BASHRC_FILES".to_string(), files.join("\n"))]
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
        let mut flags = portage_profile::phase_environ(&config, None);
        // `bin/ebuild` drops the scheduler-only tokens for the direct
        // path (this function serves that path only; the merge path
        // keeps them).
        for key in ["FEATURES", "PORTAGE_FEATURES"] {
            if let Some(slot) = flags.iter_mut().find(|(k, _)| k == key) {
                slot.1 = discard_direct_ebuild_features(&slot.1.clone());
            }
        }
        // The package's full effective `USE` (`PORTAGE_USE` shape: the
        // effective set narrowed to `IUSE ∪ IUSE_EFFECTIVE`), not just
        // the IUSE display list: a flag-less package still sees the
        // implicit expansion (e.g. `elibc_glibc`).
        // Portage: config.py, setcpv
        let effective = portage_repo::candidate_effective_use_flags(
            &repos,
            &config,
            &env.category,
            &env.split.pn,
            &env.split.pvr,
        );
        let iuse = md5_iuse(env);
        let use_flags = portage_profile::portage_use(&config, &iuse, &effective);
        // The display pairs carry the full `PORTAGE_USE`, all on: the
        // tail joins them into the exported `USE`.
        let display: Vec<(String, bool)> = use_flags.iter().map(|f| (f.clone(), true)).collect();
        // Portage's `setcpv` unprefixed filter over the narrowed USE:
        // `ARCH="amd64"` with no `amd64` in USE exports as `""`.
        let use_set: std::collections::HashSet<String> = use_flags.iter().cloned().collect();
        for var in &config.use_expand_unprefixed {
            if let Some(current) = flags.iter().find(|(k, _)| k == var).map(|(_, v)| v.clone()) {
                let filtered =
                    portage_profile::filter_unprefixed_use_expand_value(&current, &use_set);
                if let Some(slot) = flags.iter_mut().find(|(k, _)| k == var) {
                    slot.1 = filtered;
                }
            }
        }
        // Per-package `package.env` build vars, atom-matched against
        // the ebuild's own md5-cache identity. Layered after the base
        // flags so they win; the `USE=` half already folded in above.
        let cpv_slot = standalone_cpv_slot(&env.pkg_dir, &env.category, &env.split.pf)?;
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
            let folded = discard_direct_ebuild_features(&folded);
            flags.push(("FEATURES".to_string(), folded.clone()));
            flags.push(("PORTAGE_FEATURES".to_string(), folded));
        }
        flags.extend(package_bashrc_env(&config, &cpv_slot));
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

/// Whether the raw (not USE-reduced) `PROPERTIES` names `interactive`: such
/// a package keeps the terminal as stdin in every phase.
// Portage: AbstractEbuildProcess.py, _start (`"interactive" in properties.split()`)
pub(crate) fn properties_name_interactive(env: &Environment) -> bool {
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return false;
    };
    portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
        .ok()
        .and_then(|m| m.get("PROPERTIES").cloned())
        .is_some_and(|raw| raw_properties_name_interactive(&raw))
}

/// The raw-string half of [`properties_name_interactive`]: `foo? ( interactive )`
/// counts even with `foo` off, as Portage splits the unreduced string.
pub(crate) fn raw_properties_name_interactive(raw: &str) -> bool {
    raw.split_whitespace().any(|w| w == "interactive")
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

/// Portage's `_validate_deps` REQUIRED_USE refusal text for a violated
/// `required_use` under `enabled`/`iuse`: the reduced subset line plus,
/// when the reduction shrank anything, the human-readable complete
/// expression — then the trailing blank line, byte for byte as
/// `doebuild.py` writes it (reduced raw, complete humanized).
// Portage: doebuild.py, _validate_deps
pub(crate) fn required_use_message(
    required_use: &str,
    enabled: &std::collections::HashSet<String>,
    iuse: &std::collections::HashSet<String>,
) -> String {
    let normalized = required_use
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Operators stay raw here (`tounicode`); only the complete
    // expression below is humanized.
    let reduced = match portage_required_use::unsatisfied_reduced(required_use, enabled, iuse) {
        Ok(Some(r)) => r,
        _ => normalized.clone(),
    };
    let mut out = format!(
        "\n  The following REQUIRED_USE flag constraints are unsatisfied:\n    {reduced}\n"
    );
    if reduced != normalized {
        out.push_str(&format!(
            "\n  The above constraints are a subset of the following complete expression:\n    {}\n",
            portage_required_use::human_readable(&normalized)
        ));
    }
    out.push('\n');
    out
}

/// The direct-`ebuild` REQUIRED_USE gate (`_validate_deps`,
/// `doebuild.py:1941-1975`): `Some` with Portage's exact stderr text when
/// the md5-cache `REQUIRED_USE` is violated by the standalone effective
/// USE; `None` when it passes or does not apply (no cache entry, empty
/// `REQUIRED_USE`, an EAPI without `required_use`, unresolvable config,
/// or a degenerate empty effective set that could only false-positive).
/// `Err` only when the expression itself is unevaluatable (fails closed,
/// like Portage's metadata-error path).
///
/// The USE domain is the full effective set
/// (`candidate_effective_use_flags`) and the validity domain the
/// implicit-folded IUSE (`implicit_iuse_set`) — the same pair the
/// resolve pass checks the emerge path with, never a re-derivation. The
/// EAPI decision reads `attrs.required_use` (S1), never a threshold.
/// Called once per CLI invocation from `ebuild.rs` (like Portage's
/// per-`doebuild` validation), so merge/unmerge internals — already
/// gated by the resolver, or teardown that Portage never gates — keep
/// their behaviour.
/// Portage: doebuild.py, _validate_deps
pub(crate) fn required_use_refusal(
    env: &Environment,
    config_root: &Path,
    eroot: &Path,
) -> Result<Option<String>, String> {
    // Unparsable head EAPI falls back to the permissive set, like the
    // phase env itself does; the unsupported-EAPI refusal runs first
    // anyway, so only supported EAPIs ever reach here.
    let attrs = portage_dep::eapi::eapi_attrs(Some(&env.eapi))
        .or_else(|_| portage_dep::eapi::eapi_attrs(None))
        .expect("None always yields the permissive set");
    if !attrs.required_use {
        return Ok(None);
    }
    let Some(repo_root) = repo_root_for(&env.pkg_dir) else {
        return Ok(None);
    };
    let Ok(metadata) = portage_repo::repo_aux_metadata(&repo_root, &env.category, &env.split.pf)
    else {
        return Ok(None);
    };
    let required_use = metadata
        .get("REQUIRED_USE")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if required_use.is_empty() {
        return Ok(None);
    }
    let Some(config) = resolve_standalone_config(&env.pkg_dir, config_root, eroot) else {
        return Ok(None);
    };
    let Ok(repos) = portage_repo::find_repos(config_root) else {
        return Ok(None);
    };
    let enabled: std::collections::HashSet<String> = portage_repo::candidate_effective_use_flags(
        &repos,
        &config,
        &env.category,
        &env.split.pn,
        &env.split.pvr,
    )
    .into_iter()
    .collect();
    if enabled.is_empty() {
        return Ok(None);
    }
    let iuse_raw = metadata.get("IUSE").map(String::as_str).unwrap_or("");
    let iuse_set = portage_repo::implicit_iuse_set(iuse_raw, &config);
    match portage_required_use::check_required_use(&required_use, &enabled, &iuse_set) {
        Ok(true) => Ok(None),
        Ok(false) => Ok(Some(required_use_message(
            &required_use,
            &enabled,
            &iuse_set,
        ))),
        Err(e) => Err(format!(
            "{}: invalid REQUIRED_USE {required_use:?}: {e}",
            env.ebuild_abs.display()
        )),
    }
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
