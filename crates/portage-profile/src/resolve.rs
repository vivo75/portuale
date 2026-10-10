//! `resolve_config`: building the full `Config` from disk and the environment.

use crate::*;

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn resolve_config(
    config_root: &Path,
    main_repo_location: &Path,
    overlay_repos: &[(String, PathBuf)],
    repo_aliases: &[(String, PathBuf)],
    main_repo_name: &str,
    repo_masters: &HashMap<String, Vec<PathBuf>>,
    eroot: &Path,
) -> Result<Config, Error> {
    let mut config = Config {
        envd_eroot: eroot.to_path_buf(),
        ..Config::default()
    };
    let mut scalars: HashMap<String, String> = HashMap::new();

    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();

    // Real `layout.conf` `profile-formats = portage-2` gate on a profile
    // `parent` line's own `reponame:path`/`:path` cross-repo syntax
    // (real `_config/LocationsManager.py:47`/`259`): the set of repo
    // names that declare it. Read here directly (a tiny dedicated
    // layout.conf reader -- see `repo_profile_formats` -- rather than
    // threading a param through every `resolve_config` call site) so
    // `masters` resolution in `portage_repo::find_repos` and this gate
    // stay independently minimal.
    let parent_colon_repos: HashSet<String> = all_repos
        .iter()
        .filter(|(_, loc)| repo_profile_formats(loc).iter().any(|f| f == "portage-2"))
        .map(|(name, _)| name.clone())
        .collect();

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
        note_higher(&mut config, "USE");
        let snapshot = IncrementalSnapshot::take(&config);
        process_lines(&text, &mut scalars, &mut config);
        snapshot.restore(&mut config);
    }

    let make_profile = config_root.join("etc/portage/make.profile");
    let chain: Vec<PathBuf> = if make_profile.exists() {
        resolve_profile_chain(&make_profile, &all_repos, repo_aliases, &parent_colon_repos)?
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
            note_higher(&mut config, "USE");
            let text = fs::read_to_string(&make_defaults).map_err(|e| Error::ReadFile {
                path: make_defaults.display().to_string(),
                source: e,
            })?;
            let snapshot = IncrementalSnapshot::take(&config);
            process_lines(&text, &mut scalars, &mut config);
            snapshot.restore(&mut config);
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
        let snapshot = IncrementalSnapshot::take(&config);
        process_make_conf_file(
            &make_conf,
            config_root,
            &mut scalars,
            &mut config,
            &mut visited_sources,
        )?;
        snapshot.restore(&mut config);
    }

    // Real `config.regenerate()`'s `env` `USE_ORDER` layer -- the process
    // environment overrides / stacks on the profile chain + `make.conf`.
    // See `apply_env_layer` / `ENV_*_VARS`.
    apply_env_layer(&mut scalars, &mut config);

    // USE_EXPAND (PMS 7.3.4; real config.py's own regenerate(), "Do the
    // USE calculation last because it depends on USE_EXPAND"): now that
    // every profile level's own make.defaults plus make.conf have been
    // read, `config.use_expand` holds the final, incrementally-stacked
    // set of USE_EXPAND variable NAMES (e.g. "VIDEO_CARDS"). Each named
    // variable's own current VALUE -- read from `scalars`, the same
    // last-level-wins mechanism every other non-USE/ACCEPT_KEYWORDS
    // variable already uses (see the module doc comment; a deliberate
    // simplification of real portage's own genuinely-incremental
    // per-USE_EXPAND-variable behavior -- extending the pre-existing
    // "no incremental merge outside USE/ACCEPT_KEYWORDS" cut to these
    // variables too, not a new one) -- is expanded into
    // lowercase-prefixed pseudo-USE-flags via the exact same
    // apply_incremental token semantics (`-flag`/`flag`/`+flag`/`-*`)
    // USE itself already uses, folded directly into `use_flags`.
    // `USE_EXPAND_UNPREFIXED` (real `config.py`'s own companion
    // mechanism -- no prefix at all, applied in the loop right below
    // this one) IS now read too. `USE_EXPAND_IMPLICIT` + `IUSE_IMPLICIT`
    // + `USE_EXPAND_VALUES_<v>` are now read as well, feeding the real
    // EAPI 5+ `IUSE_EFFECTIVE` computation just after the unprefixed loop
    // -- **the earlier "`USE_EXPAND_HIDDEN`/`USE_EXPAND_IMPLICIT` are
    // display-only" note here was wrong for `USE_EXPAND_IMPLICIT`**: it
    // drives `pkg.iuse.is_valid_flag` (so a `foo[elibc_glibc]` USE-dep
    // matches a `foo` that never lists `elibc_glibc`), not just
    // `emerge --info` display. `USE_EXPAND_HIDDEN` genuinely *is*
    // display-only for EAPI 5+ (see `Config::iuse_effective`); it is read
    // now (`Config::use_expand_hidden`) and honoured only by
    // `emerge --pretend -v`'s own `USE_EXPAND` grouping in `portage_repo`.
    // IUSE-aware `_*`
    // wildcard expansion (`linguas_*`) IS now done, but in
    // `portage_repo::effective_use_flags` (it needs a specific
    // candidate's own IUSE). `package.use`'s own USE_EXPAND-prefix
    // shorthand (`VIDEO_CARDS: nvidia` lines) is read too (see the
    // `package.use` bullet in this comment).
    //
    // `env` `USE_ORDER` layer for the USE_EXPAND / USE_EXPAND_UNPREFIXED
    // variable *values* (`VIDEO_CARDS=nouveau emerge …`): their names
    // aren't known until `USE_EXPAND` itself is finalized above, so the
    // env override is folded into `scalars` here, right before both
    // expansion loops read them. Real portage treats each as its own
    // incremental; portuale's last-wins `scalars` model just lets the
    // env value replace the profile/`make.conf` one.
    let env_expand_names: Vec<String> = config
        .use_expand
        .iter()
        .chain(config.use_expand_unprefixed.iter())
        .cloned()
        .collect();
    for var in env_expand_names {
        if let Some(value) = config_env_var(&var) {
            note_higher(&mut config, &var);
            scalars.insert(var, value);
        }
    }

    // Real config.py (2849-2889): "In order to best accommodate the
    // long-standing practice of setting default USE_EXPAND variables in
    // the profile's make.defaults, we translate these variables into
    // their equivalent USE flags" -- **per profile level**, with the
    // folded flags *prepended* before that level's own `USE=` line, so a
    // sub-profile (or that same level's `package.use`) can incrementally
    // cancel them. Real then skips re-folding USE_EXPAND for the
    // `defaults` configdict (2964); portuale mirrors that with the
    // `profile_scalars` guard in the two global loops just below.
    //
    // Without this, `LUA_SINGLE_TARGET="lua5-1"` from `profiles/base/
    // make.defaults` folded once (last-wins) into the high-priority
    // `conf` tier would be re-applied *after* `base/package.use`'s
    // `app-editors/neovim lua_single_target_luajit
    // -lua_single_target_lua5-1`, leaving both single-target flags on and
    // tripping neovim's `^^ ( ... )` REQUIRED_USE.
    {
        let expand_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand.iter().cloned().collect();
            v.sort();
            v
        };
        let unprefixed_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand_unprefixed.iter().cloned().collect();
            v.sort();
            v
        };
        let prefix_tok = |var: &str, tok: &str| -> String {
            let prefix = var.to_lowercase();
            if let Some(rest) = tok.strip_prefix('-') {
                format!("-{prefix}_{rest}")
            } else if let Some(rest) = tok.strip_prefix('+') {
                format!("{prefix}_{rest}")
            } else {
                format!("{prefix}_{tok}")
            }
        };
        // Real `config.py` (2853-2866) folds *every* profile level's own
        // `make.defaults` value for each USE_EXPAND var into that level's
        // USE contribution -- the flags then stack incrementally through
        // `USE_ORDER` (a later `arch/amd64` `VIDEO_CARDS="amdgpu fbdev …"`
        // does NOT wipe an earlier `default/linux` `VIDEO_CARDS="dummy
        // fbdev"`; the union stands). The only thing that suppresses the
        // per-level fold is `make.conf`/env overriding the var: real then
        // hits `is_not_incremental` in the `conf`/`env` configdict pass
        // (2961-2978), which *clears* every `<var>_*` flag and re-adds
        // that tier's value wholesale -- handled by the global loops
        // below. So the guard is per-var ("did conf/env touch it"), never
        // per-level ("is this level's value the one that survived").
        let conf_overrode = |var: &str| profile_scalars.get(var) != scalars.get(var);
        for (i, delta) in level_scalar_deltas.iter().enumerate() {
            let mut expand_use: Vec<String> = Vec::new();
            for var in &unprefixed_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(String::from));
                }
            }
            for var in &expand_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(|t| prefix_tok(var, t)));
                }
            }
            if expand_use.is_empty() {
                continue;
            }
            let joined = expand_use.join(" ");
            apply_use_incremental(&joined, &mut config.use_flags);
            config.profile_use_layers[i]
                .make_defaults_use
                .insert(0, joined);
        }
    }

    let use_expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    for var in use_expand_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Folded per-level above -- the profile chain had the final say
        // and `make.conf`/env did not touch it (real skips the `defaults`
        // configdict in its own USE_EXPAND re-fold, 2964).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        let prefix = var.to_lowercase();
        let prefixed: String = value
            .split_whitespace()
            .map(|tok| {
                // Real config.py's own early-expand loop: "-flag" keeps
                // its own "-" outside the new prefix ("-video_cards_x",
                // still recognized as a removal by apply_incremental
                // below); a leading "+" is stripped first, matching
                // ordinary USE token handling, so it doesn't get
                // literally baked into the prefixed flag name.
                if let Some(rest) = tok.strip_prefix('-') {
                    format!("-{prefix}_{rest}")
                } else if let Some(rest) = tok.strip_prefix('+') {
                    format!("{prefix}_{rest}")
                } else {
                    format!("{prefix}_{tok}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        apply_use_incremental(&prefixed, &mut config.use_flags);
        config.conf_use_tokens.push(prefixed);
    }

    // USE_EXPAND_UNPREFIXED: real config.py's own companion to
    // USE_EXPAND -- the exact same mechanism (variable NAMES
    // incrementally stacked into `config.use_expand_unprefixed` via
    // "USE_EXPAND_UNPREFIXED" during make.defaults/make.conf processing
    // above, each named variable's own current VALUE read from
    // `scalars`), except the value is folded into `use_flags` via
    // `apply_incremental` *directly*, with no `lowercase(name)_` prefix
    // at all -- real Gentoo's own profile sets
    // `USE_EXPAND_UNPREFIXED="ARCH"`, so `ARCH="amd64"` contributes the
    // bare `amd64` flag, not `arch_amd64` (this is literally how
    // `amd64`/`x86`/`arm64` etc. exist as real USE flags at all).
    let use_expand_unprefixed_vars: Vec<String> =
        config.use_expand_unprefixed.iter().cloned().collect();
    for var in use_expand_unprefixed_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Profile-only values are folded per-level above (real 2863-2866).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        apply_use_incremental(value, &mut config.use_flags);
        config.conf_use_tokens.push(value.clone());
    }

    // Real EAPI 5+ `IUSE_EFFECTIVE` (`config.py::_calc_iuse_effective`) --
    // computed now that `use_expand`/`use_expand_unprefixed`/
    // `use_expand_implicit`/`iuse_implicit` are all fully stacked and
    // every `USE_EXPAND_VALUES_<v>` scalar has been read. See
    // `Config::iuse_effective`'s own doc comment for what this grants a
    // candidate's `is_valid_flag` domain on top of its declared `IUSE`.
    // Deliberately a plain scalar read of each `USE_EXPAND_VALUES_<v>`
    // (portuale's established "no incremental merge outside USE/
    // ACCEPT_KEYWORDS" cut -- `USE_EXPAND_VALUES_*` isn't even in real
    // portage's own INCREMENTALS list anyway).
    config.iuse_effective = config.iuse_implicit.clone();
    for var in &config.use_expand_unprefixed {
        if !config.use_expand_implicit.contains(var) {
            continue;
        }
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(String::from));
        }
    }
    for var in &config.use_expand_implicit {
        if !config.use_expand.contains(var) {
            continue;
        }
        let prefix = var.to_lowercase();
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(|v| format!("{prefix}_{v}")));
        }
    }

    // use.mask/use.force: every profile level's own file (in chain
    // order), stacked with the same -atom removal semantics
    // package.mask uses (see stack_mask_lines) -- confirmed by reading
    // UseManager.getUseMask/getUseForce's own pkg=None (global) case,
    // which returns stack_lists(self._usemask_list/self._useforce_list,
    // incremental=True) directly, never consulting a repo-level or
    // per-package source at all (those only exist on the *per-package*
    // path, out of scope for portuale's flat/global USE model, same
    // as package.use's own repo/profile/user-only sourcing already is).
    // NOT folded into `use_flags` here -- real config.py's own
    // `regenerate()` applies `self.useforce`/`self.usemask` (which
    // `setcpv()` sets to the *per-package* `getUseForce(pkg)`/
    // `getUseMask(pkg)` -- global use.force/use.mask combined with the
    // atom-scoped package.use.force/.mask portuale already applies
    // per-candidate) as the literal *last* step of its own incremental
    // USE walk (`lib/portage/package/ebuild/config.py`, ~line 3024:
    // `myflags.update(self.useforce); ...;
    // myflags.difference_update(self.usemask)`), strictly *after* the
    // `pkg` (`package.use`) tier -- not folded in early alongside
    // `defaults`/`conf` the way portuale previously (incorrectly) did,
    // which let a `package.use` entry override a global force/mask
    // decision real portage never lets it override. See
    // `portage-repo`'s own `effective_use_flags` doc comment for where
    // `use_force`/`use_mask` actually get applied now -- alongside the
    // atom-scoped `package_use_force`/`package_use_mask` it already
    // positions correctly, force-add first then force-remove, so a flag
    // in both ends up masked, not forced, exactly like real portage.
    let mut usemask_sources: Vec<Vec<String>> = Vec::new();
    let mut useforce_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        usemask_sources.push(read_config_lines(&level.join("use.mask"))?);
        useforce_sources.push(read_config_lines(&level.join("use.force"))?);
    }
    config.use_force = stack_mask_lines(&useforce_sources).into_iter().collect();
    config.use_mask = stack_mask_lines(&usemask_sources).into_iter().collect();

    // PORTAGE_ARCHLIST: same chain, same stacking semantics as
    // use.mask/use.force just above -- see `archlist`'s own doc comment.
    let mut archlist_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        archlist_sources.push(read_config_lines(&level.join("arch.list"))?);
    }
    config.archlist = stack_mask_lines(&archlist_sources).into_iter().collect();

    // Real append_repo scopes EVERY repo's own repo-level package.mask/
    // .unmask, including the main repo's own -- not just an overlay's,
    // confirmed by reading MaskManager.py's own repo_pkgmasklines/
    // repo_pkgunmasklines loop (`for repo in repositories.
    // repos_with_profiles()`, unconditional). Portuale previously left
    // the main repo's own entries unscoped, a genuine, documented gap
    // (see this function's own earlier doc comment on the overlay
    // scoping work): an identically-named package.mask atom from main
    // would incorrectly also mask a same-named-but-different overlay
    // package that no mask file ever actually mentions. Note this is
    // NOT the same as the profile-chain's own package.mask/.unmask below
    // (`for level in &chain`) or the user-level one further down --
    // real MaskManager.py's own profile_pkgmasklines/user_pkgmasklines
    // never get append_repo'd at all, only the repo-level ones do, so
    // those two stay exactly as unscoped as they already were.
    let main_repo_mask_lines =
        read_config_lines(&main_repo_location.join("profiles/package.mask"))?;
    let main_repo_unmask_lines =
        read_config_lines(&main_repo_location.join("profiles/package.unmask"))?;
    let mut mask_sources: Vec<Vec<String>> =
        vec![scope_repo_mask_lines(&main_repo_mask_lines, main_repo_name)];
    let mut unmask_sources: Vec<Vec<String>> = vec![scope_repo_mask_lines(
        &main_repo_unmask_lines,
        main_repo_name,
    )];
    for (repo_name, repo_location) in overlay_repos {
        // Real masters (`repo_masters`, resolved by the caller from real
        // `repos.conf`'s own `masters = name1 name2 ...` key --
        // `portage_repo::find_repos`'s own `RepoConfig::masters` doc
        // comment has the full real grounding, `config.py:1229-1260`):
        // an overlay's own package.mask is stacked *on top of* every one
        // of its declared masters' own package.mask, in declared order,
        // before the combined result gets the usual "::reponame"
        // scoping. No entry for `repo_name` in `repo_masters` at all
        // (every pre-existing test call site, which never populates this
        // map) falls back to the same "main repo alone" default this
        // portuale always used before `masters =` parsing existed, so
        // nothing already-tested changes behavior. Simplified from real
        // `MaskManager.py`'s own per-master `stack_lists` (which stacks
        // each master separately against the repo's own lines, then
        // concatenates every one of those per-master results, so a
        // "-atom" removal line's own unmatched-master warning can be
        // attributed correctly) to one flat `stack_mask_lines` call over
        // every master's lines followed by the repo's own -- produces
        // the identical final masked-atom set for the common case (no
        // fixture in portuale exercises the exotic case where two
        // masters' own "-atom" removals would need to be told apart).
        let masters = repo_masters
            .get(repo_name)
            .cloned()
            .unwrap_or_else(|| vec![main_repo_location.to_path_buf()]);
        let mut mastered_lines_stack: Vec<Vec<String>> = Vec::with_capacity(masters.len() + 1);
        for master_location in &masters {
            mastered_lines_stack.push(read_config_lines(
                &master_location.join("profiles/package.mask"),
            )?);
        }
        mastered_lines_stack.push(read_config_lines(
            &repo_location.join("profiles/package.mask"),
        )?);
        let mastered_mask_lines = stack_mask_lines(&mastered_lines_stack);
        mask_sources.push(scope_repo_mask_lines(&mastered_mask_lines, repo_name));
        // package.unmask deliberately does NOT get the same masters
        // treatment -- confirmed by reading MaskManager.py's own two
        // loops side by side: the package.mask loop stacks each
        // master's own lines in first, but the package.unmask loop
        // only ever stacks a repo's own lines against itself
        // (`stack_lists([repo_lines], incremental=1, ...)`, no masters
        // iteration at all), a real asymmetry in real portage itself,
        // not a simplification on portuale's part.
        unmask_sources.push(scope_repo_mask_lines(
            &read_config_lines(&repo_location.join("profiles/package.unmask"))?,
            repo_name,
        ));
    }
    for level in &chain {
        mask_sources.push(read_config_lines(&level.join("package.mask"))?);
        unmask_sources.push(read_config_lines(&level.join("package.unmask"))?);
    }
    mask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.mask"),
    )?);
    unmask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.unmask"),
    )?);

    config.package_mask = stack_mask_lines(&mask_sources);
    config.package_unmask = stack_mask_lines(&unmask_sources);

    // package.accept_keywords: profile-chain (in chain order), then
    // user-level -- real KeywordsManager.getPKeywords iterates its own
    // per-profile-level dicts first, then the user-level one, extending
    // the same accumulating "extra accepted keywords" list each time (no
    // "-atom" removal exists for this file at all, unlike package.mask,
    // so a flat concatenation-then-parse is equivalent to parsing each
    // source separately and concatenating the results). No repo-level
    // source exists for this file in real portage at all (unlike
    // package.mask's repo-level profiles/package.mask) -- confirmed by
    // reading KeywordsManager.__init__, which never reads a
    // repo-location path for either package.accept_keywords or its
    // package.keywords alias.
    let mut accept_keywords_lines: Vec<String> = Vec::new();
    for level in &chain {
        accept_keywords_lines.extend(read_config_lines(&level.join("package.accept_keywords"))?);
    }
    accept_keywords_lines.extend(read_config_lines(
        &config_root.join("etc/portage/package.accept_keywords"),
    )?);
    config.package_accept_keywords = parse_package_accept_keywords_lines(&accept_keywords_lines);
    // A bare atom (empty token list, preserved by the parser above) gets
    // real `accept_keywords_defaults`'s own implicit meaning: "~" plus
    // every *plain* (non-`~`/`-`-prefixed) token in the *final* global
    // ACCEPT_KEYWORDS -- computed once here, against `config.
    // accept_keywords` as already fully resolved by this point (every
    // profile-level/make.conf ACCEPT_KEYWORDS contribution above already
    // folded in), exactly matching what real portage computes it from at
    // both of its own two call sites (`KeywordsManager.__init__`'s own
    // `global_accept_keywords` parameter, `getPKeywords`'s own `pgroups`
    // -- both already-resolved global ACCEPT_KEYWORDS by the time either
    // runs). Sorted only for deterministic test assertions; downstream
    // consumption (`specificity_ordered_flags`) folds these into a
    // `HashSet`, so fold order was never semantically significant.
    let mut accept_keywords_defaults: Vec<String> = config
        .accept_keywords
        .iter()
        .filter(|k| !k.starts_with('~') && !k.starts_with('-'))
        .map(|k| format!("~{k}"))
        .collect();
    accept_keywords_defaults.sort();
    for (_, tokens) in &mut config.package_accept_keywords {
        if tokens.is_empty() {
            *tokens = accept_keywords_defaults.clone();
        }
    }

    // package.use: three real sources, each into its own field at its own
    // real USE_ORDER position (the "Config depth" slice, SCOPE_BACKLOG
    // Part 2.C -- see the module doc comment's `package.use` bullet and
    // `portage-repo::effective_use_flags` for the per-package walk):
    //   - repo-level (`<repo>/profiles/package.use`, every configured
    //     repo -- real UseManager._parse_repository_files_to_dict_of_dicts
    //     iterates repositories.repos_with_profiles()) -> package_use_repo,
    //     real configdict["repo"]. Overlay lines are `::repo`-scoped via
    //     scope_repo_package_use_lines (the same "package.mask, widened"
    //     precedent); the main repo's stay unscoped, since real portage's
    //     own masters-chain lookup (`repos = masters + [pkg.repo]`) always
    //     includes it (every overlay here implicitly masters main, no
    //     explicit `masters =` support yet).
    //   - every profile level's own package.use (chain order) ->
    //     package_use (real configdict["defaults"], _pkgprofileuse).
    //   - user-level `/etc/portage/package.use` -> package_use_user (real
    //     configdict["pkg"], _pusedict / getPUSE).
    // Each source is parsed independently (rather than one concatenated
    // pass) so only the user-level one gets the USE_EXPAND-prefix
    // shorthand's real user-only `extended_syntax` -- see
    // parse_package_use_lines. Still purely additive within each source
    // (no `-atom` removal for this file). The `defaults`-tier profile
    // `package.use` is interleaved per profile level (see
    // `Config::profile_use_layers`, built in the `make.defaults` chain
    // loop above); the flat `config.package_use` built here is only the
    // fallback for a hand-built `Config`.
    let mut repo_use_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_use_lines = read_config_lines(&repo_location.join("profiles/package.use"))?;
        repo_use_lines.extend(scope_repo_package_use_lines(&overlay_use_lines, repo_name));
    }
    let mut profile_use_lines: Vec<String> = Vec::new();
    for level in &chain {
        profile_use_lines.extend(read_config_lines(&level.join("package.use"))?);
    }
    let user_use_lines = read_config_lines(&config_root.join("etc/portage/package.use"))?;
    config.package_use_repo = parse_package_use_lines(&repo_use_lines, false);

    // Repo `make.defaults` USE -- real `config.py`'s `_repo_make_defaults`,
    // the global (not package-scoped) half of `configdict["repo"]`. Read
    // each repo's `profiles/make.defaults`, keeping every `USE=` value in
    // file order with `${VAR}` expanded against the scalars accumulated
    // so far. The main repo's entry (empty name) applies to every
    // package; an overlay's (real name) only to a candidate from that
    // overlay -- see `Config::repo_make_defaults_use`.
    config.repo_make_defaults_use = std::iter::once((
        String::new(),
        read_repo_make_defaults_use(&main_repo_location.join("profiles/make.defaults"), &scalars),
    ))
    .chain(overlay_repos.iter().map(|(name, location)| {
        (
            name.clone(),
            read_repo_make_defaults_use(&location.join("profiles/make.defaults"), &scalars),
        )
    }))
    .filter(|(_, tokens)| !tokens.is_empty())
    .collect();

    // `configdict["features"]["USE"]` -- real `config.py` appends `test`
    // to this tier when `FEATURES` names it. Portuale's `FEATURES` is a
    // last-wins scalar (make.conf / profile `make.defaults` / the `env`
    // layer, whichever set it last), a documented simplification.
    let features_has_test = scalars
        .get("FEATURES")
        .into_iter()
        .flat_map(|f| f.split_whitespace())
        .any(|t| t == "test");
    config.features_use = if features_has_test {
        vec!["test".to_string()]
    } else {
        Vec::new()
    };

    // `configdict["env.d"]["USE"]` -- the lowest `USE_ORDER` tier, from
    // `<eroot>/etc/profile.env` (real `_get_env_d`). Practically
    // always empty (`/etc/env.d/*` doesn't set `USE`).
    config.envd_use_tokens = read_envd_use_tokens(eroot);

    config.package_use = parse_package_use_lines(&profile_use_lines, false);
    config.package_use_user = parse_package_use_lines(&user_use_lines, true);

    // Real `UseManager.extract_global_USE_changes` (`config.py:832`): the
    // `*/*` package atom's own `*/*` entry in the **user** `package.use`
    // (`configdict["pkg"]` only -- real's `_pusedict` is built from user
    // files alone, never profile/repo `package.use`) is *popped out* of
    // the per-package dict and appended to `configdict["conf"]["USE"]`,
    // so it stacks on the global USE exactly like `make.conf` `USE=`
    // would. USE_EXPAND shorthand (`GRUB_PLATFORMS: efi-64 pc`) is
    // already prefix-expanded by `parse_package_use_lines(.., true)`.
    // Folded here -- after the USE_EXPAND / USE_EXPAND_UNPREFIXED loops
    // above -- so a `*/* -lua_single_target_lua5-1` really removes the
    // value the profile's own `LUA_SINGLE_TARGET=` scalar just folded in.
    // Popped from `package_use_user` too (real deletes the `*/*` key), so
    // it is not *also* re-applied per-candidate in `effective_use_flags`.
    let global_user_use: String = {
        let mut toks: Vec<String> = Vec::new();
        config.package_use_user.retain(|(atom, tokens)| {
            if atom == "*/*" {
                toks.extend(tokens.iter().cloned());
                false
            } else {
                true
            }
        });
        toks.join(" ")
    };
    if !global_user_use.is_empty() {
        apply_use_incremental(&global_user_use, &mut config.use_flags);
        config.conf_use_tokens.push(global_user_use);
    }

    // package.env (real config.py:894 `grabdict_package` +
    // `_grab_pkg_env`): `/etc/portage/package.env` maps an atom to one or
    // more env-file names under `/etc/portage/env/`; each file is a
    // `make.conf`-style `KEY=value` snippet (with `source`) layered onto
    // a matching package's config. Same `<atom> <token...>` line grammar
    // as `package.accept_keywords` (real `grabdict` for both). The `USE=`
    // half of each file lands in `package_env_use` (the `pkg` `USE_ORDER`
    // layer, before user `package.use`); the non-`USE` scalar half lands
    // in `package_env_vars` (a per-package build-phase env override).
    let package_env_lines = read_config_lines(&config_root.join("etc/portage/package.env"))?;
    config.package_env = parse_package_accept_keywords_lines(&package_env_lines)
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .collect();
    let env_dir = config_root.join("etc/portage/env");
    // Each `package.env` entry's files share one expand-map copy (see
    // `expand_package_env_files`); the two halves split it afterwards.
    let package_env_expanded: Vec<(String, Vec<(String, String)>)> = config
        .package_env
        .iter()
        .map(|(atom, files)| {
            (
                atom.clone(),
                expand_package_env_files(&env_dir, files, config_root, &scalars),
            )
        })
        .collect();
    config.package_env_use = package_env_expanded
        .iter()
        .filter_map(|(atom, pairs)| {
            let tokens: Vec<String> = pairs
                .iter()
                .filter(|(k, _)| k == "USE")
                .flat_map(|(_, v)| v.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect();
            (!tokens.is_empty()).then(|| (atom.clone(), tokens))
        })
        .collect();
    config.package_env_vars = package_env_expanded
        .iter()
        .filter_map(|(atom, pairs)| {
            let vars: Vec<(String, String)> =
                pairs.iter().filter(|(k, _)| k != "USE").cloned().collect();
            (!vars.is_empty()).then(|| (atom.clone(), vars))
        })
        .collect();

    // package.use.mask/package.use.force: repo-level (every repo, not
    // just main -- see the package.use bullet just above for the same
    // `::repo`-scoping this now applies here too) plus every profile
    // level's own file (in chain order) -- NO
    // user-level source at all, unlike package.use: confirmed by reading
    // UseManager.__init__'s own file/variable table (the "user config"
    // section lists only "package.use -> _pusedict", nothing for
    // mask/force). Unlike package.mask/.unmask, real UseManager.py never
    // merges an overlay's own file with its master's own at load time
    // (no `stack_lists`-equivalent combination here at all -- confirmed
    // by reading `_parse_repository_files_to_dict_of_tuples`/`_of_dicts`,
    // which just parse each repo's own file independently); the masters
    // chain is only consulted later, per-package, in
    // `getUseMask`/`getUseForce` (`repos = masters + [pkg.repo]`, each
    // repo's own already-independent dict appended in that order) -- so
    // no `stack_mask_lines`-style merge is needed here either, just the
    // same scope-then-append `package.use` above already does. Flat-
    // concatenated the same way package_use already is; which entry
    // actually wins when more than one matches the same candidate is
    // decided later, at application time -- see `portage-repo`'s own doc
    // comment on why (atom-specificity ordering).
    let mut use_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.force"))?;
    let mut use_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_force = read_config_lines(&repo_location.join("profiles/package.use.force"))?;
        use_force_lines.extend(scope_repo_package_use_lines(&overlay_force, repo_name));
        let overlay_mask = read_config_lines(&repo_location.join("profiles/package.use.mask"))?;
        use_mask_lines.extend(scope_repo_package_use_lines(&overlay_mask, repo_name));
    }
    for level in &chain {
        use_force_lines.extend(read_config_lines(&level.join("package.use.force"))?);
        use_mask_lines.extend(read_config_lines(&level.join("package.use.mask"))?);
    }
    // No shorthand here either: package.use.force/.mask have no
    // user-level source at all (see this crate's own doc comment), and
    // real portage's own shorthand support is genuinely user-only.
    config.package_use_force = parse_package_use_lines(&use_force_lines, false);
    config.package_use_mask = parse_package_use_lines(&use_mask_lines, false);

    // use.stable.mask/use.stable.force (PMS 5+; real
    // eapi_supports_stable_use_forcing_and_masking's own EAPI floor,
    // always recognized here -- see this crate's own module doc comment
    // for portuale's established "no EAPI parametrization"
    // precedent): profile-chain only, same -atom-removal stacking
    // use_force/use_mask already get -- deliberately NOT folded into
    // use_flags here; see use_stable_force's own doc comment for why.
    let mut use_stable_force_sources: Vec<Vec<String>> = Vec::new();
    let mut use_stable_mask_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        use_stable_force_sources.push(read_config_lines(&level.join("use.stable.force"))?);
        use_stable_mask_sources.push(read_config_lines(&level.join("use.stable.mask"))?);
    }
    config.use_stable_force = stack_mask_lines(&use_stable_force_sources)
        .into_iter()
        .collect();
    config.use_stable_mask = stack_mask_lines(&use_stable_mask_sources)
        .into_iter()
        .collect();

    // package.use.stable.mask/package.use.stable.force: repo-level
    // (every repo, not just main -- same `::repo`-scoped, no-masters-
    // merge treatment package.use.mask/.force just above now gets,
    // confirmed by the same UseManager.__init__ table entries for
    // `_repo_pusestablemask_dict`/`_repo_pusestableforce_dict`) plus
    // every profile level's own file (in chain order) -- NO user-level
    // source at all, mirroring package_use_force/_mask's own confirmed
    // sourcing exactly. No shorthand either, same reasoning.
    let mut use_stable_force_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.force"))?;
    let mut use_stable_mask_lines: Vec<String> =
        read_config_lines(&main_repo_location.join("profiles/package.use.stable.mask"))?;
    for (repo_name, repo_location) in overlay_repos {
        let overlay_stable_force =
            read_config_lines(&repo_location.join("profiles/package.use.stable.force"))?;
        use_stable_force_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_force,
            repo_name,
        ));
        let overlay_stable_mask =
            read_config_lines(&repo_location.join("profiles/package.use.stable.mask"))?;
        use_stable_mask_lines.extend(scope_repo_package_use_lines(
            &overlay_stable_mask,
            repo_name,
        ));
    }
    for level in &chain {
        use_stable_force_lines.extend(read_config_lines(&level.join("package.use.stable.force"))?);
        use_stable_mask_lines.extend(read_config_lines(&level.join("package.use.stable.mask"))?);
    }
    config.package_use_stable_force = parse_package_use_lines(&use_stable_force_lines, false);
    config.package_use_stable_mask = parse_package_use_lines(&use_stable_mask_lines, false);

    // Per-level view of the eight sources just parsed flat above, so
    // `portage-repo` can replay real `UseManager.getUseMask`/`getUseForce`'s
    // *interleaved* per-level stacking (`for i, _ in enumerate(...):
    // append usemask[i], usestablemask[i], pusemask[i][cp],
    // pusestablemask[i][cp]` -> one `stack_lists(incremental=1)`). Level
    // 0 is synthetic: the repo-level `package.use.*` (main repo, then
    // each overlay `::repo`-scoped) that real prepends before the
    // profile loop. Levels 1.. are the profile chain, in order. The
    // global `use.mask`/`use.force` files read per level again here (the
    // flat fields stacked them into a single set); a re-read of a
    // handful of usually-absent small files, once per (memoised)
    // `resolve_config`.
    let mut levels: Vec<UseMaskForceLevel> = Vec::new();
    {
        // Level 0 (synthetic): repo-level `package.use.*` -- main repo,
        // then each overlay `::repo`-scoped, exactly the concatenation
        // the flat fields' repo-level prefix already uses.
        let repo_pkg = |file: &str| -> Result<Vec<(String, Vec<String>)>, Error> {
            let mut l = read_config_lines(&main_repo_location.join("profiles").join(file))?;
            for (repo_name, repo_location) in overlay_repos {
                l.extend(scope_repo_package_use_lines(
                    &read_config_lines(&repo_location.join("profiles").join(file))?,
                    repo_name,
                ));
            }
            Ok(parse_package_use_lines(&l, false))
        };
        levels.push(UseMaskForceLevel {
            package_use_mask: repo_pkg("package.use.mask")?,
            package_use_force: repo_pkg("package.use.force")?,
            package_use_stable_mask: repo_pkg("package.use.stable.mask")?,
            package_use_stable_force: repo_pkg("package.use.stable.force")?,
            ..Default::default()
        });
    }
    for level in &chain {
        levels.push(UseMaskForceLevel {
            use_mask: read_config_lines(&level.join("use.mask"))?,
            use_force: read_config_lines(&level.join("use.force"))?,
            use_stable_mask: read_config_lines(&level.join("use.stable.mask"))?,
            use_stable_force: read_config_lines(&level.join("use.stable.force"))?,
            package_use_mask: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.mask"))?,
                false,
            ),
            package_use_force: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.force"))?,
                false,
            ),
            package_use_stable_mask: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.stable.mask"))?,
                false,
            ),
            package_use_stable_force: parse_package_use_lines(
                &read_config_lines(&level.join("package.use.stable.force"))?,
                false,
            ),
        });
    }
    config.use_mask_force_levels = levels;

    // packages (@system): every profile level's own file, in chain
    // order, stacked with the same -atom removal semantics package.mask
    // uses (see stack_mask_lines) -- confirmed by reading
    // PackagesSystemSet.load, which calls the identical real
    // stack_lists(incremental=1) function. Only *after* stacking are the
    // "*"-prefixed lines kept (with the "*" stripped) as the real
    // @system atom list -- see the module doc comment's `packages`
    // bullet for why every other stacked line is read/stacked but never
    // contributes an atom of its own.
    let mut packages_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        packages_sources.push(read_config_lines(&level.join("packages"))?);
    }
    config.system_packages = stack_mask_lines(&packages_sources)
        .into_iter()
        .filter_map(|line| line.strip_prefix('*').map(String::from))
        .collect();

    // packages (@profile): the *same* files and the same
    // stack_lists(incremental=1) stacking, but (a) only profile levels
    // whose own repo declares `profile-set` in metadata/layout.conf are a
    // source at all, and (b) the *unstarred* stacked lines are the atoms
    // -- real `ProfilePackageSet.load` (`_sets/profiles.py:13-41`):
    //   for y in self._profiles if "profile-set" in y.profile_formats
    //   ... if str(x)[:1] != "*"
    // `@system` above deliberately has neither filter. A level outside
    // every configured repo (e.g. a bare synthetic profile dir in a unit
    // test, or the user-config profile node) does not qualify --
    // see `Config::profile_packages`'s own doc comment.
    let mut profile_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        let qualifies = repo_containing(level, &all_repos)
            .map(|(_, location)| {
                repo_profile_formats(&location)
                    .iter()
                    .any(|format| format == "profile-set")
            })
            .unwrap_or(false);
        if qualifies {
            profile_sources.push(read_config_lines(&level.join("packages"))?);
        }
    }
    config.profile_packages = stack_mask_lines(&profile_sources)
        .into_iter()
        .filter(|line| !line.starts_with('*'))
        .collect();

    // package.provided (real config.py:970-1027): every profile level's
    // own file (chain order) + the user-level
    // /etc/portage/profile/package.provided, stacked with the same
    // stack_lists(incremental=1) `-atom` removal package.mask/`packages`
    // use. See `Config::package_provided`.
    let mut pprovided_sources: Vec<Vec<String>> = Vec::new();
    for level in &chain {
        pprovided_sources.push(read_config_lines(&level.join("package.provided"))?);
    }
    pprovided_sources.push(read_config_lines(
        &config_root.join("etc/portage/profile/package.provided"),
    )?);
    config.package_provided = stack_mask_lines(&pprovided_sources);

    // license_groups: real `LicenseManager._read_license_groups`
    // (`LicenseManager.py:47`) over `LocationsManager.profile_locations`
    // (`LocationsManager.py:432`) -- which is the `profiles/` directory
    // of the *main repo and each overlay*, NOT the individual
    // profile-chain levels (real gentoo puts `license_groups` at
    // `<repo>/profiles/license_groups`, never in a `profiles/<foo>/`
    // profile dir -- verified live against a real tree). Then the
    // user-level `/etc/portage/license_groups`. "extend, don't
    // stack/replace" semantics -- see `parse_license_groups_lines`. Read
    // before ACCEPT_LICENSE/package.license below, both of which need
    // the full, final group map to expand `@group` tokens against.
    let mut license_groups: HashMap<String, Vec<String>> = HashMap::new();
    for (_name, location) in &all_repos {
        for (name, members) in parse_license_groups_lines(&read_config_lines(
            &location.join("profiles").join("license_groups"),
        )?) {
            license_groups.entry(name).or_default().extend(members);
        }
    }
    for (name, members) in parse_license_groups_lines(&read_config_lines(
        &config_root.join("etc/portage/license_groups"),
    )?) {
        license_groups.entry(name).or_default().extend(members);
    }
    config.license_groups = license_groups;

    // ACCEPT_LICENSE: last-level-wins scalar (see `accept_license`'s own
    // doc comment for why, and the real "* -@EULA" default portuale
    // replicates when it's never set anywhere at all) -- `scalars`
    // already holds whatever the profile chain + make.conf left it as,
    // via the same catch-all `scalars.insert` every other scalar
    // variable already goes through in `process_lines`/
    // `process_make_conf_file`.
    let accept_license_str = scalars
        .get("ACCEPT_LICENSE")
        .cloned()
        .unwrap_or_else(|| "* -@EULA".to_string());
    let accept_license_tokens: Vec<String> = accept_license_str
        .split_whitespace()
        .map(String::from)
        .collect();
    config.accept_license = expand_license_tokens(&accept_license_tokens, &config.license_groups);

    // package.license: user-level only -- see `package_license`'s own
    // doc comment for the deliberately-not-replicated `profile-license`
    // profile-format and `*/*`-global-extraction real quirks.
    let package_license_lines =
        read_config_lines(&config_root.join("etc/portage/package.license"))?;
    config.package_license = parse_package_license_lines(&package_license_lines)
        .into_iter()
        .map(|(atom, tokens)| (atom, expand_license_tokens(&tokens, &config.license_groups)))
        .collect();

    // ACCEPT_PROPERTIES/ACCEPT_RESTRICT: last-level-wins scalars, real
    // "*" default (see `accept_properties`'s own doc comment) -- no
    // `@group` expansion for either, unlike ACCEPT_LICENSE/
    // package.license just above.
    config.accept_properties = scalars
        .get("ACCEPT_PROPERTIES")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);
    config.accept_restrict = scalars
        .get("ACCEPT_RESTRICT")
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| vec!["*".to_string()]);

    // package.properties/package.accept_restrict: user-level only, same
    // "atom + raw tokens" shape package.license already reads (reused
    // directly -- see parse_package_license_lines's own doc comment).
    config.package_properties = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.properties"),
    )?);
    config.package_accept_restrict = parse_package_license_lines(&read_config_lines(
        &config_root.join("etc/portage/package.accept_restrict"),
    )?);

    // PKGDIR: last-level-wins scalar, real "/var/cache/binpkgs" default
    // (see `pkgdir`'s own doc comment).
    config.pkgdir = scalars
        .get("PKGDIR")
        .cloned()
        .unwrap_or_else(|| "/var/cache/binpkgs".to_string());

    // binrepos.conf + PORTAGE_BINHOST (see `binrepos`'s own doc comment).
    // Real `BinRepoConfigLoader._parse` runs `_recursive_file_list` over
    // the path, so `etc/portage/binrepos.conf` may be a **directory** of
    // `*.conf` fragments (the standard portage "file or dir" pattern, and
    // how the release profile ships it -- `gentoobinhost.conf`), not just
    // a single file.
    config.binrepos = parse_binrepos(
        &read_config_lines(&config_root.join("etc/portage/binrepos.conf"))
            .unwrap_or_default()
            .join("\n"),
        scalars
            .get("PORTAGE_BINHOST")
            .map(String::as_str)
            .unwrap_or(""),
    );

    // Real `config.py.__init__`: `PORTAGE_CONFIGROOT` is stamped into the
    // settings, and `CBUILD` defaults to `CHOST` when unset anywhere
    // (`config.py`'s own `if self.get("CBUILD") is None and
    // self.get("CHOST")`). Both surface in `emerge --info`.
    scalars
        .entry("PORTAGE_CONFIGROOT".to_string())
        .or_insert_with(|| {
            let s = config_root.to_string_lossy();
            if s.is_empty() {
                "/".to_string()
            } else {
                s.into_owned()
            }
        });
    if !scalars.contains_key("CBUILD")
        && let Some(chost) = scalars.get("CHOST").cloned()
    {
        scalars.insert("CBUILD".to_string(), chost);
    }

    config.other_vars = scalars;

    // #105: freeze the immutable USE-context digest now that the config
    // is complete. The `'backtrack` loop mutates only `autounmask_use`
    // (on clones), which the memo keys hash against this base.
    config.use_context_base = Some(use_context_base_fingerprint(&config));
    // #114: and the visibility base (masks + accept lists).
    config.is_visible_base = Some(is_visible_base_fingerprint(&config));

    Ok(config)
}
