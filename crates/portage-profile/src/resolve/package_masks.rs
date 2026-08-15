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
    } = args;
    // Every repo's own repo-level package.mask/.unmask gets `::repo`-scoped,
    // including the main repo's. Profile-chain and user-level entries stay
    // unscoped. Portage: MaskManager repo_pkgmasklines/repo_pkgunmasklines
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
        // An overlay's package.mask stacks on its declared masters' masks
        // (default: the main repo alone), then gets `::repo`-scoped.
        // Portage: MaskManager.py per-master `stack_lists`
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
        // package.unmask gets no masters treatment: Portage stacks only each
        // repo's own lines there. Portage: MaskManager.py package.unmask loop
        unmask_sources.push(scope_repo_mask_lines(
            &read_config_lines(&repo_location.join("profiles/package.unmask"))?,
            repo_name,
        ));
    }
    for level in chain {
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

    // package.accept_keywords: profile-chain then user-level, flat
    // concatenation (no `-atom` removal exists for this file). No repo-level
    // source exists in Portage. Portage: KeywordsManager.getPKeywords
    let mut accept_keywords_lines: Vec<String> = Vec::new();
    for level in chain {
        accept_keywords_lines.extend(read_config_lines(&level.join("package.accept_keywords"))?);
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
