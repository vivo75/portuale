//! `package.use` from repos, profiles and the user, plus the FEATURES=test and env.d USE.

use crate::resolve::*;

/// The values [`fold_package_use`] reads from its caller.
pub(crate) struct FoldPackageUseArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) main_repo_name: &'a str,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) chain: &'a [PathBuf],
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) scalars: &'a HashMap<String, String>,
    pub(crate) eroot: &'a Path,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// `package.use` from repos, profiles and the user, plus the FEATURES=test and env.d USE.
pub(crate) fn fold_package_use(args: FoldPackageUseArgs<'_>) -> Result<(), Error> {
    let FoldPackageUseArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        chain,
        config_root,
        config,
        scalars,
        eroot,
        warnings,
    } = args;
    // package.use: three sources into three fields at three USE_ORDER positions
    // (repo `::repo`-scoped, profile-chain, user-level with USE_EXPAND shorthand).
    // Purely additive; parsed independently. Portage: UseManager
    //
    // Repo and chain lines validate first (node rules, use dropped, no
    // wildcards); the user file is S6 scope and stays raw. Chain lines were
    // already validated (and warned) in `load_scalar_layers`: identical
    // warnings dedupe at the end of `resolve_config`.
    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();
    let repo_rules = repo_rules_for(&all_repos, warnings);
    let dict = AtomLineKind::Dict { allow_use: false };
    let mut repo_use_lines: Vec<String> = filter_atom_lines(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.use"))?,
        &repo_file_rules(&repo_rules, main_repo_name),
        dict,
        WARN_RANK_USE,
        warnings,
    );
    for (repo_name, repo_location) in overlay_repos {
        let overlay_use_lines = filter_atom_lines(
            &read_config_lines_with_sources(&repo_location.join("profiles/package.use"))?,
            &repo_file_rules(&repo_rules, repo_name),
            dict,
            WARN_RANK_USE,
            warnings,
        );
        repo_use_lines.extend(scope_repo_package_use_lines(&overlay_use_lines, repo_name));
    }
    let mut profile_use_lines: Vec<String> = Vec::new();
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        profile_use_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.use"))?,
            &rules,
            dict,
            WARN_RANK_USE,
            warnings,
        ));
    }
    let user_use_lines = read_config_lines(&config_root.join("etc/portage/package.use"))?;
    config.package_use_repo = parse_package_use_lines(&repo_use_lines, false);

    // Repo `make.defaults` USE (global half of `configdict["repo"]`): each repo's
    // values in file order; main applies to every package. Portage: config.py
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

    // `configdict["features"]["USE"]`: `test` when FEATURES names it.
    // Portage: config.py. FEATURES is a last-wins scalar here.
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

    // `configdict["env.d"]["USE"]`: the lowest USE_ORDER tier, from
    // `<eroot>/etc/profile.env`. Practically always empty.
    config.envd_use_tokens = read_envd_use_tokens(eroot);

    config.package_use = parse_package_use_lines(&profile_use_lines, false);
    config.package_use_user = parse_package_use_lines(&user_use_lines, true);

    // The user `package.use` `*/*` entry pops into the global USE (like make.conf
    // USE=) and leaves the per-package dict. Portage: UseManager extract_global_USE_changes
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
