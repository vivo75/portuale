//! Config and repo loading, EMERGE_DEFAULT_OPTS, scheduling policy.

use crate::pretend::*;

/// Which running root, if any, `BDEPEND`/`IDEPEND` (and, for a native build,
/// `DEPEND`) resolve against instead of the target `ROOT`: always the running
/// root `/` at EAPI 7+, observable only for a genuine cross-root build.
/// `--root-deps` forces the same on top (and folds these keys into `RDEPEND`).
/// Portage: depgraph.py, depend_root selection
pub(crate) fn resolve_root_deps_running_root(
    root_deps: bool,
    target_root: &Path,
    running_root: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    (root_deps || running_root != target_root).then_some(running_root)
}

/// `emerge -C` without `--pretend` removes each selected package, then
/// deselects it from the world file (`--ask`/countdown handled by the caller).
/// Portage: _emerge/unmerge.py, unmerge
///
/// `BINPKG_COMPRESS`/`BINPKG_COMPRESS_FLAGS[_<NAME>]`/`PORTAGE_BZIP2_COMMAND`/
/// `PKGDIR`/`BINPKG_FORMAT` via the resolved chain (calling env over files),
/// falling back to `make.globals` defaults. The gpkg pair reads files-only;
/// `PORTAGE_BZIP2_COMMAND` and `PKGDIR` keep the full chain for both formats.
/// Portage: _config/special_env_vars.py, environ_filter; bin/gpkg-helper.py
///
/// `resolved_features`/`config`: `None` keeps the process-env fallback (reached
/// only when a caller could resolve no chain -- with no files the env stands).
pub(crate) fn package_options_from_env(
    shell: ebuild_phases::ShellBackend,
    debug: bool,
    resolved_features: Option<&str>,
    config: Option<&portage_profile::Config>,
) -> ebuild_package::PackageOptions {
    let feature_on = |token: &str| match resolved_features {
        Some(f) => f.split_whitespace().any(|t| t == token),
        None => feature_enabled(token),
    };
    let d = ebuild_package::PackageOptions::default();
    // The two lookup chains: full env-over-files, and files-only for gpkg.
    let chain = |key: &str| match config {
        Some(c) => portage_profile::env_over_config_scalar(c, key),
        // Config-less arm: calling env only, else `make.globals`' default.
        None => portage_profile::config_env_var(key),
    };
    let files = |key: &str| match config {
        Some(c) => c.other_vars.get(key).cloned(),
        None => None,
    };
    let binpkg_format = ebuild_package::resolve_binpkg_format(chain);
    // `xpak` uses the full chain, `gpkg` the files-only one (both env-only when
    // config-less). Portage: doebuild.py; bin/gpkg-helper.py
    let (binpkg_compress, binpkg_compress_flags) = if binpkg_format == "gpkg" && config.is_some() {
        let compress = ebuild_package::resolve_binpkg_compress(files);
        let flags = ebuild_package::resolve_binpkg_compress_flags(files, &compress);
        (compress, flags)
    } else {
        let compress = ebuild_package::resolve_binpkg_compress(chain);
        let flags = ebuild_package::resolve_binpkg_compress_flags(chain, &compress);
        (compress, flags)
    };
    ebuild_package::PackageOptions {
        debug,
        pkgdir: ebuild_package::resolve_pkgdir(chain),
        distdir: std::env::var_os("DISTDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| d.distdir.clone()),
        shell,
        binpkg_compress,
        binpkg_compress_flags,
        portage_bzip2_command: ebuild_package::resolve_portage_bzip2_command(chain),
        binpkg_format,
        config_root: portage_repo::config_root_from_env(),
        // Default-on token: enabled unless explicitly negated (see
        // `PackageOptions::buildpkg_live`).
        buildpkg_live: match resolved_features {
            Some(f) => f.split_whitespace().any(|t| t == "buildpkg-live"),
            None => !feature_enabled("-buildpkg-live"),
        },
        // Deliberately defaults off here (Portage default is on; see
        // `PackageOptions::binpkg_multi_instance`). Explicit opt-in.
        binpkg_multi_instance: feature_on("binpkg-multi-instance"),
        binpkg_signing: feature_on("binpkg-signing"),
        binpkg_gpg_signing_base_command: std::env::var("BINPKG_GPG_SIGNING_BASE_COMMAND")
            .unwrap_or_default(),
        binpkg_gpg_signing_digest: std::env::var("BINPKG_GPG_SIGNING_DIGEST").unwrap_or_default(),
        binpkg_gpg_signing_gpg_home: std::env::var("BINPKG_GPG_SIGNING_GPG_HOME")
            .unwrap_or_default(),
        binpkg_gpg_signing_key: std::env::var("BINPKG_GPG_SIGNING_KEY").unwrap_or_default(),
        pkgindex_header: config.map(portage_profile::pkgindex_header),
    }
}

/// Minimal POSIX-shell word splitting for `PORTAGE_IONICE_COMMAND`:
/// whitespace-separated, with `'...'` / `"..."` quoting and backslash escaping.
pub(crate) fn shell_split(s: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_word {
                    words.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            '\'' => {
                has_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                has_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                                cur.push(n);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                has_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            _ => {
                has_word = true;
                cur.push(c);
            }
        }
    }
    if has_word {
        words.push(cur);
    }
    words
}

/// `EMERGE_DEFAULT_OPTS`, split and **prepended** to argv, so argv always wins.
/// Portage: _emerge/main.py, parse_opts second pass
pub(crate) fn args_with_emerge_defaults(
    config: &portage_profile::Config,
    args: &[String],
) -> Vec<String> {
    // `--ignore-default-opts` comes from the command line.
    if args.iter().any(|arg| arg == "--ignore-default-opts") {
        return args.to_vec();
    }
    let mut out = config
        .other_vars
        .get("EMERGE_DEFAULT_OPTS")
        .map(|value| shell_split(value))
        .unwrap_or_default();
    out.extend(args.iter().cloned());
    out
}

/// Applies `PORTAGE_NICENESS`, `PORTAGE_IONICE_COMMAND` and scheduling policy to
/// this process for later subprocesses to inherit. Called once, unconditionally.
/// Portage: _emerge/actions.py, apply_priorities
///
/// No forkserver equivalent: `-jN` parallelism is threads, and thread creation
/// inherits the creator's policy.
pub(crate) fn apply_portage_scheduling_policy() {
    let pid = std::process::id().to_string();
    apply_scheduling_policy();
    if let Ok(niceness) = std::env::var("PORTAGE_NICENESS") {
        let niceness = niceness.trim();
        if !niceness.is_empty() {
            match std::process::Command::new("renice")
                .args(["-n", niceness, &pid])
                .stdout(std::process::Stdio::null())
                .status()
            {
                Err(_) => eprintln!(
                    " * PORTAGE_NICENESS not applied because the renice command was not found"
                ),
                Ok(s) if !s.success() => {
                    eprintln!(
                        " * renice command returned {} for main pid {pid}",
                        s.code().unwrap_or(-1)
                    );
                }
                Ok(_) => {}
            }
        }
    }
    if let Ok(cmd) = std::env::var("PORTAGE_IONICE_COMMAND") {
        let parts: Vec<String> = shell_split(&cmd)
            .into_iter()
            .map(|p| p.replace("${PID}", &pid).replace("$PID", &pid))
            .collect();
        if let Some((prog, args)) = parts.split_first() {
            match std::process::Command::new(prog).args(args).status() {
                Err(_) => {} // the kernel probably doesn't support ionice
                Ok(s) if !s.success() => {
                    eprintln!(
                        " * PORTAGE_IONICE_COMMAND returned {} for main pid {pid}",
                        s.code().unwrap_or(-1)
                    );
                    eprintln!(
                        " * See the make.conf(5) man page for PORTAGE_IONICE_COMMAND usage \
                         instructions."
                    );
                }
                Ok(_) => {}
            }
        }
    }
}

/// Maps `PORTAGE_SCHEDULING_POLICY` to `SCHED_*` and applies it. Unknown policy,
/// out-of-range priority, or syscall failure prints and returns; unset is a no-op.
/// Portage: _emerge/actions.py, set_scheduling_policy
pub(crate) fn apply_scheduling_policy() {
    let Ok(policy_name) = std::env::var("PORTAGE_SCHEDULING_POLICY") else {
        return;
    };
    let policy_name = policy_name.trim();
    if policy_name.is_empty() {
        return;
    }
    // IDs from linux/sched.h.
    let policy: libc::c_int = match policy_name {
        "other" => 0,
        "fifo" => 1,
        "round-robin" => 2,
        "batch" => 3,
        "idle" => 5,
        "deadline" => 6,
        _ => {
            eprintln!(" * Invalid policy in PORTAGE_SCHEDULING_POLICY.");
            eprintln!(
                " * See the make.conf(5) man page for PORTAGE_SCHEDULING_POLICY usage instructions."
            );
            return;
        }
    };
    // SAFETY: both take a plain `int` policy and have no memory effects.
    let min_prio = unsafe { libc::sched_get_priority_min(policy) };
    let max_prio = unsafe { libc::sched_get_priority_max(policy) };
    let priority = match std::env::var("PORTAGE_SCHEDULING_PRIORITY") {
        Ok(p) if !p.trim().is_empty() => match p.trim().parse::<libc::c_int>() {
            Ok(n) if n >= min_prio && n <= max_prio => n,
            _ => {
                eprintln!(" * Invalid priority in PORTAGE_SCHEDULING_PRIORITY.");
                eprintln!(
                    " * See the make.conf(5) man page for PORTAGE_SCHEDULING_PRIORITY usage instructions."
                );
                return;
            }
        },
        _ => min_prio.max(0),
    };
    // Zero-init, then set only `sched_priority` (a plain struct literal does not
    // compile for musl, whose `sched_param` carries extra SCHED_DEADLINE fields).
    // SAFETY: all-zeroes is a valid `sched_param` value; `sched_priority`
    // is overwritten immediately, and `param` outlives the call below.
    let mut param: libc::sched_param = unsafe { std::mem::zeroed() };
    param.sched_priority = priority;
    let pid = std::process::id() as libc::pid_t;
    // SAFETY: `param` outlives the call; `pid` / `policy` are plain ints.
    let rc = unsafe { libc::sched_setscheduler(pid, policy, &param) };
    if rc != 0 {
        eprintln!(
            " * Unable to apply PORTAGE_SCHEDULING_POLICY to main pid {pid}: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// `INSTALL_MASK` folded with the `no{man,info,doc}` tokens; non-empty process env
/// wins, else the resolved config. Empty-blanking-the-config is an unhandled corner.
/// Portage: bin/misc-functions.sh, preinst_mask
///
/// Explicit `--buildpkg[=n]` wins, else resolved `FEATURES` decides; split out for
/// testability. Portage: create_depgraph_params.py
pub(crate) fn buildpkg_from_config(
    buildpkg_opt: Option<bool>,
    config: &portage_profile::Config,
) -> bool {
    buildpkg_opt.unwrap_or_else(|| config_features_list(config).iter().any(|t| t == "buildpkg"))
}

/// Resolved `FEATURES` if assigned anywhere: the folded stack, else a raw split of
/// `other_vars["FEATURES"]`. `None` means nothing configured anywhere.
pub(crate) fn config_features_opt(config: &portage_profile::Config) -> Option<Vec<String>> {
    config.resolved_incremental("FEATURES").or_else(|| {
        config
            .other_vars
            .get("FEATURES")
            .map(|f| f.split_whitespace().map(String::from).collect())
    })
}

pub(crate) fn config_features_list(config: &portage_profile::Config) -> Vec<String> {
    config_features_opt(config).unwrap_or_default()
}

/// Whether the `$PKGDIR` scan trusts the `Packages` index; `None` (nothing
/// configured) stays trusted. Same resolved chain as every other read.
/// Portage: bintree.py, populate
pub(crate) fn pkgdir_index_trusted(config: &portage_profile::Config) -> bool {
    config_features_opt(config)
        .is_none_or(|features| features.iter().any(|t| t == "pkgdir-index-trusted"))
}

/// Resolved merge-time `FEATURES`, space-joined; shared with the remote server.
pub(crate) fn config_features_string(config: &portage_profile::Config) -> String {
    config_features_list(config).join(" ")
}

/// Resolved merge-time `PORTAGE_BZIP2_COMMAND`; shared with the remote server.
pub(crate) fn config_bzip2_command(config: &portage_profile::Config) -> String {
    config
        .other_vars
        .get("PORTAGE_BZIP2_COMMAND")
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bzip2".to_string())
}

/// Resolved merge-time `INSTALL_MASK` + `no{man,info,doc}` fold; shared with the
/// remote server. Portage: bin/misc-functions.sh, preinst_mask
pub(crate) fn config_install_mask(config: &portage_profile::Config) -> (String, bool) {
    let features = config_features_list(config);
    let configured = std::env::var("INSTALL_MASK")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| config.other_vars.get("INSTALL_MASK").cloned())
        .unwrap_or_default();
    crate::install_mask::resolve(&configured, &features)
}

/// `repos.conf` repos + resolved config for one root (`run()`'s preamble; shared
/// with `mrg --remote-binpkg`). `Err` is bare text; the caller prefixes it.
pub(crate) fn load_repos_and_config(
    config_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<(Vec<portage_repo::RepoConfig>, portage_profile::Config), String> {
    let repos = portage_repo::find_repos(config_root).map_err(|e| e.to_string())?;
    let config = resolve_config_for_root(config_root, &repos, root)?;
    Ok((repos, config))
}

/// [`load_repos_and_config`] for an internal probe that runs before the CLI's
/// own load: the repos.conf section-mismatch line stays the CLI's to print.
pub(crate) fn load_config_quiet(
    config_root: &std::path::Path,
    root: &std::path::Path,
) -> Result<portage_profile::Config, String> {
    let repos = portage_repo::find_repos_quiet(config_root).map_err(|e| e.to_string())?;
    resolve_config_for_root(config_root, &repos, root)
}

/// Profile half over a loaded repo list: both roots resolve the same trees.
/// Portage: create_trees (running tree inherits `PORTAGE_REPOSITORIES`)
pub(crate) fn resolve_config_for_root(
    config_root: &std::path::Path,
    repos: &[portage_repo::RepoConfig],
    root: &std::path::Path,
) -> Result<portage_profile::Config, String> {
    let main_repo = repos
        .iter()
        .find(|r| r.is_main)
        .ok_or_else(|| "no main repo found in repos.conf".to_string())?;

    // Each overlay's (name, location): for `::name` scoping and cross-repo parents.
    let overlay_repos: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();

    // Each repo's resolved `masters` chain, keyed by name.
    let repo_masters: std::collections::HashMap<String, Vec<std::path::PathBuf>> = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();

    // Every repo's `aliases` with locations, for aliased profile parents.
    let repo_aliases: Vec<(String, std::path::PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();

    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root,
        main_repo_location: &main_repo.location,
        overlay_repos: &overlay_repos,
        repo_aliases: &repo_aliases,
        main_repo_name: &main_repo.name,
        repo_masters: &repo_masters,
        eroot: root,
    })
    .map_err(|e| e.to_string())?;
    Ok(config)
}

/// Running root's profile for per-node USE paint; `None` on single-root runs and
/// load failure (target paint is the fallback).
pub(crate) fn load_running_config(
    target_root: &std::path::Path,
    running_root: &std::path::Path,
    repos: &[portage_repo::RepoConfig],
) -> Option<portage_profile::Config> {
    if running_root == target_root {
        return None;
    }
    resolve_config_for_root(running_root, repos, running_root).ok()
}
