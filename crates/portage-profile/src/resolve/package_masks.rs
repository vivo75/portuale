//! `package.mask`/`package.unmask` and `package.accept_keywords`.

use crate::resolve::*;

/// The values [`fold_package_masks_and_keywords`] reads from its caller.
pub(crate) struct FoldPackageMasksAndKeywordsArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) main_repo_name: &'a str,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) repo_masters: &'a HashMap<String, Vec<PathBuf>>,
    pub(crate) chain: &'a [PathBuf],
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// `package.mask`/`package.unmask` and `package.accept_keywords`.
pub(crate) fn fold_package_masks_and_keywords(
    args: FoldPackageMasksAndKeywordsArgs<'_>,
) -> Result<(), Error> {
    let FoldPackageMasksAndKeywordsArgs {
        main_repo_location,
        main_repo_name,
        overlay_repos,
        repo_masters,
        chain,
        config_root,
        config,
        warnings,
    } = args;
    let all_repos: Vec<(String, PathBuf)> =
        std::iter::once((main_repo_name.to_string(), main_repo_location.to_path_buf()))
            .chain(overlay_repos.iter().cloned())
            .collect();
    let repo_rules = repo_rules_for(&all_repos, warnings);
    // Every repo's own repo-level package.mask/.unmask gets `::repo`-scoped,
    // including the main repo's. Profile-chain and user-level entries stay
    // unscoped. Portage: MaskManager repo_pkgmasklines/repo_pkgunmasklines
    //
    // Every source is validated first (Portage `grabfile_package` with the
    // repo's own rules: file-adjacent EAPI, no wildcards); user-level files
    // are S6 scope and stay raw.
    let main_rules = repo_file_rules(&repo_rules, main_repo_name);
    let main_repo_mask_lines = filter_atom_lines(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.mask"))?,
        &main_rules,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        warnings,
    );
    let main_repo_unmask_lines = filter_atom_lines(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.unmask"))?,
        &main_rules,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        warnings,
    );
    let mut mask_sources: Vec<Vec<String>> =
        vec![scope_repo_mask_lines(&main_repo_mask_lines, main_repo_name)];
    let mut unmask_sources: Vec<Vec<String>> = vec![scope_repo_mask_lines(
        &main_repo_unmask_lines,
        main_repo_name,
    )];
    for (repo_name, repo_location) in overlay_repos {
        // An overlay's package.mask stacks on its declared masters' masks
        // (default: the main repo alone), then gets `::repo`-scoped.
        // Portage: MaskManager.py per-master `stack_lists`
        let masters = repo_masters
            .get(repo_name)
            .cloned()
            .unwrap_or_else(|| vec![main_repo_location.to_path_buf()]);
        let overlay_rules = repo_file_rules(&repo_rules, repo_name);
        let mut mastered_lines_stack: Vec<Vec<String>> = Vec::with_capacity(masters.len() + 1);
        for master_location in &masters {
            // Masters validate under their own rules, like Portage's
            // per-repo `grab_pmask` cache.
            let master_name = all_repos
                .iter()
                .find(|(_, loc)| loc == master_location)
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
            let master_rules = if master_name.is_empty() {
                NodeAtomRules {
                    eapi: Some("0".to_string()),
                    allow_repo: false,
                    allow_build_id: false,
                    allow_wildcard: false,
                }
            } else {
                repo_file_rules(&repo_rules, &master_name)
            };
            mastered_lines_stack.push(filter_atom_lines(
                &read_config_lines_with_sources(&master_location.join("profiles/package.mask"))?,
                &master_rules,
                AtomLineKind::File {
                    packages_file: false,
                },
                WARN_RANK_MASK,
                warnings,
            ));
        }
        mastered_lines_stack.push(filter_atom_lines(
            &read_config_lines_with_sources(&repo_location.join("profiles/package.mask"))?,
            &overlay_rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        ));
        let mastered_mask_lines = stack_mask_lines(&mastered_lines_stack);
        mask_sources.push(scope_repo_mask_lines(&mastered_mask_lines, repo_name));
        // package.unmask gets no masters treatment: Portage stacks only each
        // repo's own lines there. Portage: MaskManager.py package.unmask loop
        unmask_sources.push(scope_repo_mask_lines(
            &filter_atom_lines(
                &read_config_lines_with_sources(&repo_location.join("profiles/package.unmask"))?,
                &overlay_rules,
                AtomLineKind::File {
                    packages_file: false,
                },
                WARN_RANK_MASK,
                warnings,
            ),
            repo_name,
        ));
    }
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        mask_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.mask"))?,
            &rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        ));
        unmask_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.unmask"))?,
            &rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        ));
    }
    mask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.mask"),
    )?);
    unmask_sources.push(read_config_lines(
        &config_root.join("etc/portage/package.unmask"),
    )?);

    config.package_mask = stack_mask_lines(&mask_sources);
    config.package_unmask = stack_mask_lines(&unmask_sources);

    // package.accept_keywords: profile-chain then user-level, flat
    // concatenation (no `-atom` removal exists for this file). No repo-level
    // source exists in Portage. Chain lines validate with the node's rules
    // (no wildcards, use dropped) but never with build-ids: Portage does
    // not pass `allow_build_id` there even with the `build-id` format
    // (S0 `formats` cell still warns on `=…-1-2`); user lines are S6 scope.
    // Portage: KeywordsManager.py:49-60
    let mut accept_keywords_lines: Vec<String> = Vec::new();
    for level in chain {
        let mut rules = rules_for_node(level, &all_repos, &repo_rules)?;
        rules.allow_build_id = false;
        accept_keywords_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.accept_keywords"))?,
            &rules,
            AtomLineKind::Dict { allow_use: false },
            WARN_RANK_KEYWORDS,
            warnings,
        ));
    }
    accept_keywords_lines.extend(read_config_lines(
        &config_root.join("etc/portage/package.accept_keywords"),
    )?);
    config.package_accept_keywords = parse_package_accept_keywords_lines(&accept_keywords_lines);
    // A bare atom (empty token list) gets the implicit `~` + plain global
    // ACCEPT_KEYWORDS meaning, computed against the fully-resolved global
    // set. Sorted for deterministic tests; downstream folds into a set.
    // Portage: KeywordsManager `accept_keywords_defaults`
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
    Ok(())
}
