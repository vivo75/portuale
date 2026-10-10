//! `package.use` from repos, profiles and the user, plus the FEATURES=test and env.d USE.

use crate::resolve::*;

/// The values [`fold_package_use`] reads from its caller.
pub(crate) struct FoldPackageUseArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) chain: &'a [PathBuf],
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) scalars: &'a HashMap<String, String>,
    pub(crate) eroot: &'a Path,
}

/// `package.use` from repos, profiles and the user, plus the FEATURES=test and env.d USE.
pub(crate) fn fold_package_use(args: FoldPackageUseArgs<'_>) -> Result<(), Error> {
    let FoldPackageUseArgs {
        main_repo_location,
        overlay_repos,
        chain,
        config_root,
        config,
        scalars,
        eroot,
    } = args;
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
    for level in chain {
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
        read_repo_make_defaults_use(&main_repo_location.join("profiles/make.defaults"), scalars),
    ))
    .chain(overlay_repos.iter().map(|(name, location)| {
        (
            name.clone(),
            read_repo_make_defaults_use(&location.join("profiles/make.defaults"), scalars),
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
    Ok(())
}
