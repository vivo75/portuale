//! The environment variables and PATH a phase runs with.

use crate::ebuild_phases::*;

/// The env block every phase and every `misc-functions.sh` `__dyn_*` command needs,
/// as raw pairs. `Brush` formats them into `export` source (`phase_setup_script`);
/// `Bash` passes them as subprocess env (no quoting step, no expansion risk).
/// Shared by `run_one_phase`/`run_misc_functions`; `extra_env` appends verbatim.
/// `PORTAGE_ECLASS_LOCATIONS`: shell-single-quoted path tokens (as Portage's
/// `eclass_locations_string` builds them) for `inherit()` to search — own repo
/// first, then masters in declared order. Resolution failure degrades to the
/// containing repo alone; outside a repo the value is empty (nothing to look in).
/// Portage: config.py, eclass_cache.py, bin/ebuild.sh
pub(crate) fn eclass_locations_value(pkg_dir: &Path, config_root: &Path) -> String {
    let Some(repo_root) = repo_root_for(pkg_dir) else {
        return String::new();
    };

    let masters: Vec<PathBuf> = (|| -> Option<Vec<PathBuf>> {
        let repos = portage_repo::find_repos(config_root).ok()?;
        let repo = repos.iter().find(|r| r.location == repo_root)?;
        Some(repo.masters.clone())
    })()
    .unwrap_or_default();

    let mut locations = masters;
    if !locations.contains(&repo_root) {
        locations.push(repo_root);
    }
    locations.reverse();

    locations
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PORTUALE_BIN`: absolute path of the running binary (under test, the `portuale`
/// binary next to `deps/`, never the libtest harness).
pub(crate) fn portuale_bin_value() -> String {
    #[cfg(test)]
    {
        crate::self_exe::built_portuale_bin().display().to_string()
    }
    #[cfg(not(test))]
    {
        crate::self_exe::self_exe().display().to_string()
    }
}

/// `PORTAGE_PYTHON`: the `portuale-python` shim, or `/usr/bin/python` under the
/// oracle handle.
pub(crate) fn portage_python_value(bin_dir: &Path) -> String {
    if std::env::var("PORTUALE_PYTHON_HELPERS").as_deref() == Ok("real") {
        "/usr/bin/python".to_string()
    } else {
        bin_dir.join("portuale-python").display().to_string()
    }
}
pub(crate) struct PhaseEnvVarsArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) extra_env: &'a [(String, String)],
}

pub(crate) fn phase_env_vars(args: PhaseEnvVarsArgs<'_>) -> Vec<(String, String)> {
    let PhaseEnvVarsArgs {
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    } = args;
    // No EPREFIX support: `ED` collapses to `D`'s value, no separate computation.
    let d = eapi_path_var(&env.eapi, &format!("{}/", env.d().display()));
    let root_value = eapi_path_var(&env.eapi, &format!("{}/", root.display()));
    let resolved_features = features_string(extra_env);
    let path = phase_path(helpers_dir, extra_env, &resolved_features);
    // Config-derived base for standalone runs (USE + whole config env); `("", [])`
    // for merge builds (their `extra_env` carries everything) and the `depend` phase.
    let standalone_base_env =
        phase_standalone_base_env(env, config_root, root, ebuild_phase_value, extra_env);
    // Drops non-whitelisted config vars once `${T}/environment` exists, so an
    // earlier phase's `unset` sticks; see `filter_calling_env`.
    let calling_env_filtered = filter_calling_env(ebuild_phase_value, &env.t(), extra_env);
    // Computed values seed after the config base, so they win; `extra_env` wins last.
    let mut vars: Vec<(String, String)> = if calling_env_filtered {
        standalone_base_env
            .1
            .into_iter()
            .filter(|(k, _)| environ_whitelisted(k))
            .collect()
    } else {
        standalone_base_env.1
    };
    vars.extend(vec![
        ("EAPI".to_string(), env.eapi.clone()),
        ("PN".to_string(), env.split.pn.clone()),
        ("PV".to_string(), env.split.pv.clone()),
        ("PR".to_string(), env.split.pr.clone()),
        ("PVR".to_string(), env.split.pvr.clone()),
        ("P".to_string(), env.split.p.clone()),
        ("PF".to_string(), env.split.pf.clone()),
        ("CATEGORY".to_string(), env.category.clone()),
        ("EBUILD".to_string(), env.ebuild_abs.display().to_string()),
        ("ROOT".to_string(), root_value.clone()),
        ("EROOT".to_string(), root_value),
        (
            "PORTAGE_BUILDDIR".to_string(),
            env.portage_builddir.display().to_string(),
        ),
        ("WORKDIR".to_string(), env.workdir().display().to_string()),
        ("S".to_string(), env.s().display().to_string()),
        ("D".to_string(), d.clone()),
        ("ED".to_string(), d),
        ("T".to_string(), env.t().display().to_string()),
        ("HOME".to_string(), env.home().display().to_string()),
        ("FILESDIR".to_string(), env.filesdir().display().to_string()),
        (
            "PORTAGE_BIN_PATH".to_string(),
            bin_dir.display().to_string(),
        ),
        // Absolute so the shim never depends on `PATH`.
        ("PORTUALE_BIN".to_string(), portuale_bin_value()),
        (
            "PORTAGE_ECLASS_LOCATIONS".to_string(),
            eclass_locations_value(&env.pkg_dir, config_root),
        ),
        ("PORTAGE_PYTHON".to_string(), portage_python_value(bin_dir)),
        // Portage's colormap, for `elog`/`einfo` colours in bash.
        (
            "PORTAGE_COLORMAP".to_string(),
            crate::color::phase_colormap_export(),
        ),
        ("PATH".to_string(), path),
        (
            "PORTAGE_TMPDIR".to_string(),
            env.portage_tmpdir().display().to_string(),
        ),
        // Always set; `phase-functions.sh` deletes staleness, `sandbox` appends violations.
        (
            "SANDBOX_LOG".to_string(),
            env.t().join("sandbox.log").display().to_string(),
        ),
        // `"0"` only when this phase is actually `sandbox`-wrapped.
        (
            "SANDBOX_DISABLED".to_string(),
            if fs_sandbox_for_phase(ebuild_phase_value, &resolved_features) {
                "0"
            } else {
                "1"
            }
            .to_string(),
        ),
        // Package effective `USE`: merge builds override via `extra_env`, standalone
        // runs use the config-derived base (`("", [])` for `depend` and threaded `USE`).
        ("USE".to_string(), standalone_base_env.0.clone()),
        ("EPREFIX".to_string(), String::new()),
        ("EMERGE_FROM".to_string(), "ebuild".to_string()),
        ("PORTAGE_QUIET".to_string(), "1".to_string()),
        (
            "PORTAGE_DEBUG".to_string(),
            if debug { "1" } else { "0" }.to_string(),
        ),
        ("EBUILD_PHASE".to_string(), ebuild_phase_value.to_string()),
    ]);

    // Resolved `FEATURES` when already in `vars`; else the process value. A raw
    // literal must not clobber the standalone folded base.
    if !vars.iter().any(|(k, _)| k == "FEATURES") {
        vars.push(("FEATURES".to_string(), phase_features_value()));
    }

    // `PORTAGE_RESTRICT`/`PORTAGE_PROPERTIES` for every phase, from the USE-reduced
    // fields; bash consumes them directly (never the raw values). `depend` reduces
    // on the config `USE` set, every other phase on the empty set.
    let restrict_use_set = if ebuild_phase_value == "depend" {
        depend_use_set(env, config_root, root)
    } else {
        std::collections::HashSet::new()
    };
    let (restrict, properties) = restrict_and_properties(env, &restrict_use_set);
    vars.push(("PORTAGE_RESTRICT".to_string(), restrict));
    vars.push(("PORTAGE_PROPERTIES".to_string(), properties));

    // `INHERITED` in every phase, so re-sourced ebuilds skip the illegal-inherit
    // QA notice. See `Environment::inherited`.
    if let Some(inherited) = &env.inherited {
        vars.push(("INHERITED".to_string(), inherited.clone()));
    }

    // Always set (`<checkout>/lib`, else `/`): upstream helpers `cd` into it
    // unconditionally, and `__dyn_clean` needs a valid cwd. The `empty`-dir
    // safe-cwd branch still wins (pre-created by `create_directories`).
    vars.push((
        "PORTAGE_PYM_PATH".to_string(),
        pym_path_value(&portage_checkout()),
    ));

    // The phase spawns with a cleared env, so the VDB selection and helpers mode
    // ride along explicitly (else `has_version` reads the files tree on other
    // backends). `phase-functions.sh` keeps `PORTUALE_*` out of the saved env.
    for k in [
        "PORTUALE_VDB_BACKEND",
        "PORTUALE_VDB_PATH",
        "PORTUALE_VDB_ROOT",
        "PORTUALE_PYTHON_HELPERS",
        crate::vdb_ipc::IPC_VAR,
    ] {
        if let Some(v) = std::env::var_os(k).filter(|v| !v.is_empty()) {
            vars.push((k.to_string(), v.to_string_lossy().into_owned()));
        }
    }
    // No exported choice means the files tree (the default backend).
    if !vars.iter().any(|(k, _)| k == "PORTUALE_VDB_BACKEND") {
        vars.push(("PORTUALE_VDB_BACKEND".to_string(), "files".to_string()));
    }

    // `PATH` was consumed as the helper-dirs base above; a verbatim pair would drop them.
    vars.extend(
        extra_env
            .iter()
            .filter(|(k, _)| {
                k != "PATH"
                    && (!calling_env_filtered
                        || environ_whitelisted(k)
                        || gpkg_helper_setting(ebuild_phase_value, k))
            })
            .cloned(),
    );

    // The actual dir moves to `PORTAGE_ACTUAL_DISTDIR`; `DISTDIR` becomes the
    // per-package fake. No `DISTDIR` at all leaves both untouched.
    // Portage: config.py, config.environ
    if let Some(actual) = vars
        .iter()
        .rev()
        .find(|(k, _)| k == "DISTDIR")
        .map(|(_, v)| v.clone())
    {
        vars.retain(|(k, _)| k != "DISTDIR" && k != "PORTAGE_ACTUAL_DISTDIR");
        vars.push(("PORTAGE_ACTUAL_DISTDIR".to_string(), actual));
        vars.push((
            "DISTDIR".to_string(),
            env.fake_distdir().display().to_string(),
        ));
    }

    // A set `LC_ALL` fans out to every `LC_*` and is itself dropped; the phase
    // never sees `LC_ALL`. The `extra_env` value wins over the process env.
    // Portage: EbuildPhase.py, _start; portage/util/locale.py, split_LC_ALL
    vars.retain(|(k, _)| k != "LC_ALL");
    // No IPC daemon: drop the whitelisted var or `has_version` takes the `ebuild-ipc` branch.
    vars.retain(|(k, _)| k != "PORTAGE_IPC_DAEMON");
    let lc_all = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "LC_ALL")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("LC_ALL").ok())
        .unwrap_or_default();
    if !lc_all.is_empty() {
        for category in [
            "LC_COLLATE",
            "LC_CTYPE",
            "LC_MONETARY",
            "LC_MESSAGES",
            "LC_NUMERIC",
            "LC_TIME",
            "LC_ADDRESS",
            "LC_IDENTIFICATION",
            "LC_MEASUREMENT",
            "LC_NAME",
            "LC_PAPER",
            "LC_TELEPHONE",
        ] {
            vars.push((category.to_string(), lc_all.clone()));
        }
    }

    vars
}

/// Trailing `/` on `D`/`ED`/`ROOT`/`EROOT` only for EAPI 6 and below; later EAPIs
/// strip it (so `ROOT=/` exports `""`). `value` arrives with its slash.
/// Portage: config.py, eapi.py
pub(crate) fn eapi_path_var(eapi: &str, value: &str) -> String {
    let trailing = matches!(eapi, "0" | "1" | "2" | "3" | "4" | "5" | "6");
    let base = value.trim_end_matches('/');
    if trailing {
        format!("{base}/")
    } else {
        base.to_string()
    }
}

/// Helper dir first, then the base `PATH` entries; the `xattr` dir goes in front
/// under `FEATURES=xattr`. The base is the threaded config `PATH` when present
/// (only when `env.d` sets it), else the process env.
/// Portage: doebuild.py, _doebuild_path
pub(crate) fn phase_path(
    helpers_dir: &Path,
    extra_env: &[(String, String)],
    features: &str,
) -> String {
    let base = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let helpers = helpers_dir.display().to_string();
    let xattr = helpers_dir.join("xattr").display().to_string();
    let mut parts = Vec::new();
    if feature_token_present(features, "xattr") {
        parts.push(xattr.as_str());
    }
    parts.push(helpers.as_str());
    let mut seen: std::collections::HashSet<&str> = parts.iter().copied().collect();
    for p in base.split(':') {
        if seen.insert(p) {
            parts.push(p);
        }
    }
    parts.join(":")
}

/// Arguments of [`phase_setup_script`].
pub(crate) struct PhaseSetupScriptArgs<'a> {
    pub(crate) env: &'a Environment,
    pub(crate) root: &'a Path,
    pub(crate) ebuild_phase_value: &'a str,
    pub(crate) debug: bool,
    pub(crate) bin_dir: &'a Path,
    pub(crate) helpers_dir: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) extra_env: &'a [(String, String)],
}

/// `Brush`-only: `phase_env_vars` as single-quote-escaped `export` text (config
/// values can be arbitrary text). `Bash` passes the pairs directly as env vars,
/// needing no quoting step.
pub(crate) fn phase_setup_script(args: PhaseSetupScriptArgs<'_>) -> String {
    let PhaseSetupScriptArgs {
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    } = args;
    let vars = phase_env_vars(PhaseEnvVarsArgs {
        env,
        root,
        ebuild_phase_value,
        debug,
        bin_dir,
        helpers_dir,
        config_root,
        extra_env,
    });
    let mut script = String::new();
    // Brush inherits the process env: drop what the whitelist wouldn't keep first.
    let set_here: std::collections::HashSet<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
    for (name, _) in std::env::vars() {
        if !environ_whitelisted(&name)
            && !set_here.contains(name.as_str())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !name.is_empty()
            && !name.as_bytes()[0].is_ascii_digit()
        {
            script.push_str(&format!("unset {name}\n"));
        }
    }
    for (name, value) in vars {
        script.push_str(&format!("export {name}={}\n", shell_single_quote(&value)));
    }
    script
}

/// A POSIX single-quoted word: everything inside is literal; an embedded `'` is
/// the `'\''` dance. Used for every brush `export` value.
pub(crate) fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}
