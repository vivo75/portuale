//! The scalar layers in Portage's order: env.d, make.globals, the profile chain, make.conf.

use crate::resolve::*;

/// What [`load_scalar_layers`] hands back to its caller.
pub(crate) struct LoadScalarLayersOut {
    pub(crate) chain: Vec<PathBuf>,
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
    } = args;
    // Real `regenerate()` stacks its dbs `env.d`, `globals`, `defaults`
    // (the profile chain), `conf` (`make.conf`), `env` -- in that order
    // (real config.py:531 + `mydbs = self.configlist[:-1]` in
    // `regenerate`). Portuale reads them here in the same order so the
    // `const.INCREMENTALS` fold in `resolved_incremental` matches.
    //
    // `env.d`: real `_get_env_d` -> `<eroot>/etc/profile.env` (the
    // compiled `env-update` output; `eroot`, not `config_root` -- see
    // `resolve_config`'s doc comment). Only its incremental entries
    // (`CONFIG_PROTECT` / `CONFIG_PROTECT_MASK`, written there by
    // packages' `/etc/env.d/*` fragments) feed `emerge --info`; the rest
    // is build-phase env portuale doesn't model.
    //
    // Eager (portuale resolves eagerly where real expands lazily on
    // access): values already known -- earlier lines of this file --
    // substitute, matching real's on-access result for backward
    // references; forward references to later files stay literal here
    // and pick up their values when those files are read (the eager
    // approximation portuale uses throughout). Either way the values
    // seed `scalars`, so every later file (globals, profile chain,
    // `make.conf`, `package.env` files) expands against them -- real's
    // `expand_map = env_d.copy()` seeding.
    //
    // `parse_envd_text` is the one function both the first load and
    // `Config::reload_env_d` read through. The static half of real
    // `_env_d_blacklist` (`_env_blacklist`, real `config.py:734-743`;
    // portuale's `ENV_BLACKLIST`) is popped here; the profile half
    // (`PROFILE_ONLY_VARIABLES`) is not known until the chain resolves,
    // so those keys are popped right after the chain loop below. env.d
    // is a real scalar db too (real `configdict["env.d"]`): its `LANG`
    // / `LEX` / … surface in `emerge --info`. Lowest priority --
    // `make.globals`, the profile chain and `make.conf` all override.
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
    // Which `scalars` keys the env.d read owned, for the higher-layer
    // recording at every insert site below (`process_lines`,
    // `process_make_conf_file`, `apply_env_layer`, the `USE_EXPAND`
    // env-value loop): a key the env.d map holds that a later layer
    // assigns joins `envd_higher_keys`, so `reload_env_d` leaves the
    // higher value standing.

    // `globals`: `cnf/make.globals`, always sourced -- the base layer
    // under the profile chain (real config.py:532). Real portage ships it
    // at `portage.const.GLOBAL_CONFIG_PATH` (`/usr/share/portage/config`);
    // portuale reads it `config_root`-relative, which resolves to that
    // same absolute path for a live `PORTAGE_CONFIGROOT=/` run and stays
    // absent (so contributes nothing, deterministically) under a test /
    // fixture `config_root`.
    let globals_path = config_root.join("usr/share/portage/config/make.globals");
    if globals_path.is_file() {
        let text = fs::read_to_string(&globals_path).map_err(|e| Error::ReadFile {
            path: globals_path.display().to_string(),
            source: e,
        })?;
        scalars.remove("USE");
        note_higher(&mut *config, "USE");
        let snapshot = IncrementalSnapshot::take(config);
        process_lines(&text, &mut *scalars, &mut *config);
        snapshot.restore(&mut *config);
    }

    let make_profile = config_root.join("etc/portage/make.profile");
    let chain: Vec<PathBuf> = if make_profile.exists() {
        resolve_profile_chain(&make_profile, all_repos, repo_aliases, parent_colon_repos)?
    } else {
        Vec::new()
    };
    // Per-level record of which scalars *this* level's `make.defaults` set
    // (and to what) -- needed for the real config.py "translate profile
    // USE_EXPAND defaults into per-level USE flags" fold (2849-2889) done
    // once `USE_EXPAND` itself is final, further below.
    let mut level_scalar_deltas: Vec<HashMap<String, String>> = Vec::new();
    for level in &chain {
        // `defaults`-tier walk, one profile at a time (real
        // `regenerate()` over `configdict["defaults"]`): this level's
        // `make.defaults` `USE=`, then this level's own `package.use`.
        // `use_tokens` accumulates every level's `USE=` flat too (the
        // fallback path + inspection) -- slice out just this level's by
        // length delta.
        let before = config.use_tokens.len();
        let scalars_before = scalars.clone();
        let make_defaults = level.join("make.defaults");
        if make_defaults.is_file() {
            // Real config.py quirk: USE is excluded from cross-level
            // substitution -- see the module doc comment.
            scalars.remove("USE");
            note_higher(&mut *config, "USE");
            let text = fs::read_to_string(&make_defaults).map_err(|e| Error::ReadFile {
                path: make_defaults.display().to_string(),
                source: e,
            })?;
            let snapshot = IncrementalSnapshot::take(config);
            process_lines(&text, &mut *scalars, &mut *config);
            snapshot.restore(&mut *config);
        }
        let level_make_defaults_use = config.use_tokens[before..].to_vec();
        let level_package_use =
            parse_package_use_lines(&read_config_lines(&level.join("package.use"))?, false);
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
    }
    // `scalars` as the profile chain left it, before `make.conf`/env: a
    // USE_EXPAND variable still at this value below has had its final say
    // from the profile (`defaults` tier) and is folded per-level here, not
    // again in the global `conf`-tier loop.
    // Profile half of real `_env_d_blacklist` (`profile_only_variables`,
    // `config.py:734-743`): the profile's own `PROFILE_ONLY_VARIABLES`
    // fold (env.d's own assignment, source 0, excluded) names keys env.d
    // must not supply. A key the globals/profile chain also assigned
    // keeps that value; one only env.d held is dropped from `scalars`.
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
        level_scalar_deltas,
        profile_scalars,
    })
}
