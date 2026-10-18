//! The scalar layers in Portage's order: env.d, make.globals, the profile chain, make.conf.

use crate::resolve::*;

/// What [`load_scalar_layers`] hands back to its caller.
pub(crate) struct LoadScalarLayersOut {
    pub(crate) chain: Vec<PathBuf>,
    /// The appended user-profile node, if Portage appends one.
    pub(crate) user_node: Option<UserProfileNode>,
    pub(crate) level_scalar_deltas: Vec<HashMap<String, String>>,
    pub(crate) profile_scalars: HashMap<String, String>,
}

/// The values [`load_scalar_layers`] reads from its caller.
pub(crate) struct LoadScalarLayersArgs<'a> {
    pub(crate) eroot: &'a Path,
    pub(crate) scalars: &'a mut HashMap<String, String>,
    pub(crate) config: &'a mut Config,
    pub(crate) config_root: &'a Path,
    pub(crate) all_repos: &'a [(String, PathBuf)],
    pub(crate) repo_aliases: &'a [(String, PathBuf)],
    pub(crate) parent_colon_repos: &'a HashSet<String>,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// Arguments of [`fold_profile_level_use`].
struct FoldProfileLevelUseArgs<'a> {
    level: &'a Path,
    rules: &'a NodeAtomRules,
    scalars: &'a mut HashMap<String, String>,
    config: &'a mut Config,
    warnings: &'a mut Vec<ProfileWarning>,
    level_scalar_deltas: &'a mut Vec<HashMap<String, String>>,
}

/// One profile level's `defaults`-tier contribution: its `make.defaults`
/// `USE` plus its own `package.use`. Appends one [`ProfileUseLayer`]
/// and one scalar delta; `USE=` values also join `config.use_tokens`
/// through [`process_lines`], like every chain level.
fn fold_profile_level_use(args: FoldProfileLevelUseArgs<'_>) -> Result<(), Error> {
    let FoldProfileLevelUseArgs {
        level,
        rules,
        scalars,
        config,
        warnings,
        level_scalar_deltas,
    } = args;
    // One level's `make.defaults` USE plus its own package.use; the
    // length delta slices out just this level's USE contribution.
    let before = config.use_tokens.len();
    let scalars_before = scalars.clone();
    let make_defaults = level.join("make.defaults");
    if make_defaults.is_file() {
        // USE is excluded from cross-level substitution (Portage quirk).
        scalars.remove("USE");
        note_higher(&mut *config, "USE");
        let text = fs::read_to_string(&make_defaults).map_err(|e| Error::ReadFile {
            path: make_defaults.display().to_string(),
            source: e,
        })?;
        let snapshot = IncrementalSnapshot::take(config);
        process_lines(&text, &mut *scalars, &mut *config, AcceptRawLayer::Defaults);
        snapshot.restore(&mut *config);
    }
    let level_make_defaults_use = config.use_tokens[before..].to_vec();
    let level_package_use = parse_package_use_lines(
        &filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use"))?,
            rules,
            AtomLineKind::Dict { allow_use: false },
            WARN_RANK_USE,
            warnings,
        ),
        false,
    );
    config.profile_use_layers.push(ProfileUseLayer {
        make_defaults_use: level_make_defaults_use,
        package_use: level_package_use,
    });
    level_scalar_deltas.push(
        scalars
            .iter()
            .filter(|(k, v)| scalars_before.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    Ok(())
}

/// The scalar layers in Portage's order: env.d, make.globals, the profile chain, make.conf.
pub(crate) fn load_scalar_layers(
    args: LoadScalarLayersArgs<'_>,
) -> Result<LoadScalarLayersOut, Error> {
    let LoadScalarLayersArgs {
        eroot,
        scalars,
        config,
        config_root,
        all_repos,
        repo_aliases,
        parent_colon_repos,
        warnings,
    } = args;
    // Layers in Portage `regenerate()` order: env.d, globals, the profile
    // chain (`defaults`), make.conf (`conf`). Portage: config.py `regenerate`
    // `env.d` is `<eroot>/etc/profile.env` (compiled `env-update` output).
    // Values substitute eagerly here (matching Portage's on-access result
    // for backward references) and seed `scalars` for every later file.
    // Only the static blacklist half is popped now; the profile half
    // (`PROFILE_ONLY_VARIABLES`) waits for the chain. Portage: _get_env_d
    // `parse_envd_text` serves both the first load and `reload_env_d`.
    if let Ok(text) = fs::read_to_string(eroot.join("etc/profile.env")) {
        let parsed = parse_envd_text(&text);
        for (key, value) in &parsed.vars {
            if ENV_BLACKLIST.contains(&key.as_str()) {
                continue;
            }
            scalars.insert(key.clone(), value.clone());
        }
        for (key, tokens) in &parsed.incremental_layers {
            if ENV_BLACKLIST.contains(&key.as_str()) {
                continue;
            }
            config
                .incremental_sources
                .entry(key.clone())
                .or_default()
                .push(tokens.clone());
        }
        config.envd_sets_path = parsed.sets_path;
        config.envd_raw = text;
        config.envd_vars = parsed
            .vars
            .into_iter()
            .filter(|(k, _)| !ENV_BLACKLIST.contains(&k.as_str()))
            .collect();
        config.envd_incremental_layers = parsed
            .incremental_layers
            .into_iter()
            .filter(|(k, _)| !ENV_BLACKLIST.contains(&k.as_str()))
            .collect();
    }
    // Which `scalars` keys the env.d read owned, so a later layer assigning
    // the same key joins `envd_higher_keys` and `reload_env_d` keeps it.

    // `globals`: `cnf/make.globals`, always sourced as the base layer
    // (Portage: config.py, `portage.const.GLOBAL_CONFIG_PATH`).
    // Read `config_root`-relative: the live path under `/`, absent under tests.
    let globals_path = config_root.join("usr/share/portage/config/make.globals");
    if globals_path.is_file() {
        let text = fs::read_to_string(&globals_path).map_err(|e| Error::ReadFile {
            path: globals_path.display().to_string(),
            source: e,
        })?;
        scalars.remove("USE");
        note_higher(&mut *config, "USE");
        let snapshot = IncrementalSnapshot::take(config);
        process_lines(&text, &mut *scalars, &mut *config, AcceptRawLayer::Globals);
        snapshot.restore(&mut *config);
    } else {
        // No `make.globals` under this config root (hermetic test roots):
        // seed Portage's two unmerge defaults under the stack, before
        // `make.conf` expansion, so later layers fold as on a real system
        // (`-*`/`-unmerge-orphans` drop the token, a literal
        // `UNINSTALL_IGNORE=` replaces the paths, and
        // `UNINSTALL_IGNORE="${UNINSTALL_IGNORE} …"` appends through the
        // seeded scalar). When the real file is read this is skipped: its
        // own assignments already carry both values.
        // Portage: cnf/make.globals:77-84, FEATURES `unmerge-orphans`;
        // cnf/make.globals:93, UNINSTALL_IGNORE `/lib/modules/*`.
        note_incremental(&mut *config, "FEATURES", "unmerge-orphans");
        scalars.insert("UNINSTALL_IGNORE".to_string(), "/lib/modules/*".to_string());
    }

    let make_profile = config_root.join("etc/portage/make.profile");
    let chain: Vec<PathBuf> = if make_profile.exists() {
        resolve_profile_chain(&make_profile, all_repos, repo_aliases, parent_colon_repos)?
    } else {
        Vec::new()
    };
    // Per-level record of what this level's `make.defaults` set, for the
    // profile USE_EXPAND-defaults fold once USE_EXPAND itself is final.
    let mut level_scalar_deltas: Vec<HashMap<String, String>> = Vec::new();
    // The profile nodes' atom rules, checked up front: an unsupported node
    // `eapi` aborts the whole run here, like Portage's `_addProfile`.
    let repo_rules = repo_rules_for(all_repos, warnings);
    let mut node_rules: Vec<NodeAtomRules> = Vec::with_capacity(chain.len());
    for level in &chain {
        node_rules.push(rules_for_node(level, all_repos, &repo_rules)?);
    }
    for (level, rules) in chain.iter().zip(node_rules.iter()) {
        fold_profile_level_use(FoldProfileLevelUseArgs {
            level,
            rules,
            scalars: &mut *scalars,
            config: &mut *config,
            warnings,
            level_scalar_deltas: &mut level_scalar_deltas,
        })?;
    }
    // The user profile is Portage's last profile node: its `make.defaults`
    // and `package.use` form one more level, after the chain, with the
    // node's own EAPI gating the wildcard syntax.
    // Portage: LocationsManager.py:169-189, config.py:683-700
    let user_node = resolve_user_profile_node(config_root, &chain, warnings);
    if let Some(user) = &user_node {
        let rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::PackageUse);
        fold_profile_level_use(FoldProfileLevelUseArgs {
            level: &user.path,
            rules: &rules,
            scalars: &mut *scalars,
            config: &mut *config,
            warnings,
            level_scalar_deltas: &mut level_scalar_deltas,
        })?;
    }
    // `profile.bashrc` of every node, chain order, user profile last; the
    // file is taken whatever the node's `profile-formats` (`package.bashrc`
    // is the format-gated half and is not read, backlog #413).
    // Portage: config.py:850-853
    config.profile_bashrc_files = chain
        .iter()
        .chain(user_node.iter().map(|u| &u.path))
        .map(|node| node.join("profile.bashrc"))
        .filter(|file| file.is_file())
        .collect();
    // `scalars` as the profile chain left it, before make.conf/env: a
    // USE_EXPAND variable still at this value folded per-level, not again
    // in the global conf-tier loop (Portage skips the `defaults` re-fold).
    // Profile half of the env.d blacklist: keys the
    // profile's `PROFILE_ONLY_VARIABLES` names are dropped unless a higher
    // layer also assigned them.
    if !config.envd_vars.is_empty() {
        let skip = usize::from(
            config
                .envd_incremental_layers
                .contains_key("PROFILE_ONLY_VARIABLES"),
        );
        let mut only: Vec<String> = config
            .incremental_sources
            .get("PROFILE_ONLY_VARIABLES")
            .map(|src| src.iter().skip(skip).flatten().cloned().collect())
            .unwrap_or_default();
        only.sort();
        only.dedup();
        for key in &only {
            if config.envd_vars.remove(key).is_some() && !config.envd_higher_keys.remove(key) {
                scalars.remove(key);
            }
            if config.envd_incremental_layers.remove(key).is_some()
                && let Some(src) = config.incremental_sources.get_mut(key)
                && !src.is_empty()
            {
                src.remove(0);
            }
        }
        config.envd_profile_only = only;
    }
    let profile_scalars = scalars.clone();

    let make_conf = config_root.join("etc/portage/make.conf");
    if make_conf.is_file() {
        let mut visited_sources = HashSet::new();
        let snapshot = IncrementalSnapshot::take(config);
        process_make_conf_file(
            &make_conf,
            config_root,
            &mut *scalars,
            &mut *config,
            &mut visited_sources,
        )?;
        snapshot.restore(&mut *config);
    }
    Ok(LoadScalarLayersOut {
        chain,
        user_node,
        level_scalar_deltas,
        profile_scalars,
    })
}
