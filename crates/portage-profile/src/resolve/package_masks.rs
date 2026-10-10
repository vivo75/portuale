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
    for level in chain {
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
    Ok(())
}
