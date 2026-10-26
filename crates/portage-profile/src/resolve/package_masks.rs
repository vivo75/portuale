//! `package.mask`/`package.unmask` and `package.accept_keywords`.

use crate::resolve::*;

/// The values [`fold_package_masks_and_keywords`] reads from its caller.
pub(crate) struct FoldPackageMasksAndKeywordsArgs<'a> {
    pub(crate) main_repo_location: &'a Path,
    pub(crate) main_repo_name: &'a str,
    pub(crate) overlay_repos: &'a [(String, PathBuf)],
    pub(crate) repo_masters: &'a HashMap<String, Vec<PathBuf>>,
    pub(crate) chain: &'a [PathBuf],
    pub(crate) user_node: Option<&'a UserProfileNode>,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a mut Config,
    pub(crate) warnings: &'a mut Vec<ProfileWarning>,
}

/// The plain entry strings of with-source lines (the plain-fold shape).
fn sourced_line_text(lines: &[(String, PathBuf)]) -> Vec<String> {
    lines.iter().map(|(line, _)| line.clone()).collect()
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
        user_node,
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
    // repo's own rules: file-adjacent EAPI, no wildcards); the user
    // profile validates with `eapi=None`, repo and build-id on, no
    // wildcards; user-level files with `eapi=None`, wildcards, repo and
    // build-ids on.
    let main_rules = repo_file_rules(&repo_rules, main_repo_name);
    let main_repo_mask_pairs = filter_atom_lines_with_sources(
        &read_config_lines_with_sources(&main_repo_location.join("profiles/package.mask"))?,
        &main_rules,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        warnings,
    );
    let main_repo_mask_lines = sourced_line_text(&main_repo_mask_pairs);
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
    let mut mask_pair_sources: Vec<Vec<(String, PathBuf)>> =
        vec![scope_repo_mask_lines_with_sources(
            &main_repo_mask_pairs,
            main_repo_name,
        )];
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
        let mut mastered_pair_stack: Vec<Vec<(String, PathBuf)>> =
            Vec::with_capacity(masters.len() + 1);
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
            mastered_pair_stack.push(filter_atom_lines_with_sources(
                &read_config_lines_with_sources(&master_location.join("profiles/package.mask"))?,
                &master_rules,
                AtomLineKind::File {
                    packages_file: false,
                },
                WARN_RANK_MASK,
                warnings,
            ));
        }
        mastered_pair_stack.push(filter_atom_lines_with_sources(
            &read_config_lines_with_sources(&repo_location.join("profiles/package.mask"))?,
            &overlay_rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        ));
        let mastered_lines_stack: Vec<Vec<String>> = mastered_pair_stack
            .iter()
            .map(|pairs| sourced_line_text(pairs))
            .collect();
        let mastered_mask_lines = stack_mask_lines(&mastered_lines_stack);
        mask_sources.push(scope_repo_mask_lines(&mastered_mask_lines, repo_name));
        mask_pair_sources.push(scope_repo_mask_lines_with_sources(
            &stack_mask_lines_with_sources(&mastered_pair_stack),
            repo_name,
        ));
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
        let level_mask_pairs = filter_atom_lines_with_sources(
            &read_config_lines_with_sources(&level.join("package.mask"))?,
            &rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        );
        mask_sources.push(sourced_line_text(&level_mask_pairs));
        mask_pair_sources.push(level_mask_pairs);
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
    // The user profile (`/etc/portage/profile`) is Portage's last profile
    // node: its `package.mask`/`.unmask` stack after the chain, before the
    // user files, with the node's own EAPI and no wildcards. Absent
    // without the node: without `make.profile` Portage never appends it.
    // A missing directory reads as empty, like every other absent file.
    // Portage: MaskManager.py:171-193 (the profiles loop covers the user
    // node; `portage1_directories` is true there too)
    if let Some(user) = user_node {
        let user_profile_rules = user_node_rules(user.eapi.as_deref(), UserNodeFile::Mask);
        let user_mask_pairs = filter_atom_lines_with_sources(
            &read_config_lines_with_sources(&user.path.join("package.mask"))?,
            &user_profile_rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        );
        mask_sources.push(sourced_line_text(&user_mask_pairs));
        mask_pair_sources.push(user_mask_pairs);
        unmask_sources.push(filter_atom_lines(
            &read_config_lines_with_sources(&user.path.join("package.unmask"))?,
            &user_profile_rules,
            AtomLineKind::File {
                packages_file: false,
            },
            WARN_RANK_MASK,
            warnings,
        ));
    }
    // User-level files validate like Portage's `user_config` readers:
    // `eapi=None`, wildcards and repo on, build-ids on.
    // Portage: MaskManager.py:214-232
    let user_mask_rules = user_atom_rules(true, true);
    let user_file_mask_pairs = filter_atom_lines_with_sources(
        &read_config_lines_with_sources(&config_root.join("etc/portage/package.mask"))?,
        &user_mask_rules,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        warnings,
    );
    mask_sources.push(sourced_line_text(&user_file_mask_pairs));
    mask_pair_sources.push(user_file_mask_pairs);
    unmask_sources.push(filter_atom_lines(
        &read_config_lines_with_sources(&config_root.join("etc/portage/package.unmask"))?,
        &user_mask_rules,
        AtomLineKind::File {
            packages_file: false,
        },
        WARN_RANK_MASK,
        warnings,
    ));

    config.package_mask = stack_mask_lines(&mask_sources);
    config.package_mask_files = stack_mask_lines_with_sources(&mask_pair_sources);
    debug_assert_eq!(
        config.package_mask,
        sourced_line_text(&config.package_mask_files),
        "the with-source mask stack must stay the plain stack",
    );
    config.package_unmask = stack_mask_lines(&unmask_sources);

    // package.accept_keywords: profile-chain, then the user profile node,
    // then the user files, flat concatenation (no `-atom` removal exists
    // for this file). No repo-level source exists in Portage. Chain lines
    // validate with the node's rules (no wildcards, use dropped) but never
    // with build-ids: Portage does not pass `allow_build_id` there even
    // with the `build-id` format (S0 `formats` cell still warns on
    // `=…-1-2`); the user-profile node reads with the same profile reader
    // (no wildcards, no build-ids, the node's EAPI). The user
    // `package.keywords` lines come first (Portage reads that file into
    // the same dict, then extends each atom with the accept_keywords
    // entries), validating like the user file (wildcards and build-ids
    // on, use-dep lines dropped).
    // Portage: KeywordsManager.py:25-60 (profiles, covering the user node),
    // :73-104 (user).
    // Profile-node `package.keywords` (chain and user node) is a different
    // reader below: it adds to the package's own KEYWORDS, never here.
    let mut profile_keywords_lines: Vec<String> = Vec::new();
    for level in chain {
        let rules = rules_for_node(level, &all_repos, &repo_rules)?;
        profile_keywords_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&level.join("package.keywords"))?,
            &rules,
            AtomLineKind::Dict { allow_use: false },
            WARN_RANK_KEYWORDS,
            warnings,
        ));
    }
    // The user profile's `package.keywords` stacks after the chain, with
    // the node's own EAPI and no wildcards/build-ids.
    if let Some(user) = user_node {
        let user_profile_keywords_rules =
            user_node_rules(user.eapi.as_deref(), UserNodeFile::PackageKeywords);
        profile_keywords_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&user.path.join("package.keywords"))?,
            &user_profile_keywords_rules,
            AtomLineKind::Dict { allow_use: false },
            WARN_RANK_KEYWORDS,
            warnings,
        ));
    }
    config.package_keywords_profile = parse_package_accept_keywords_lines(&profile_keywords_lines);
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
    // The user profile's `package.accept_keywords` stacks after the chain
    // (it is the last profile node), before the user file.
    if let Some(user) = user_node {
        let user_profile_accept_rules =
            user_node_rules(user.eapi.as_deref(), UserNodeFile::AcceptKeywords);
        accept_keywords_lines.extend(filter_atom_lines(
            &read_config_lines_with_sources(&user.path.join("package.accept_keywords"))?,
            &user_profile_accept_rules,
            AtomLineKind::Dict { allow_use: false },
            WARN_RANK_KEYWORDS,
            warnings,
        ));
    }
    // User-level lines validate like Portage's `user_config` reader:
    // `eapi=None`, wildcards and repo on, build-ids on, use-dep lines
    // dropped. The user `package.keywords` file is read first, into the
    // same dict; `package.accept_keywords` entries then extend each atom.
    // A non-empty read warns once, with the Python `warnings.warn`
    // rendering Portage prints (`file:line: UserWarning: msg` and the
    // source line), interpreter prefix left out like the profile
    // `virtuals` frame.
    // Portage: KeywordsManager.py:73-104 (the `warn` call is line 89)
    let user_keywords_path = config_root.join("etc/portage/package.keywords");
    let user_accept_path = config_root.join("etc/portage/package.accept_keywords");
    let user_accept_rules = user_atom_rules(true, true);
    let user_keywords_lines = filter_atom_lines(
        &read_config_lines_with_sources(&user_keywords_path)?,
        &user_accept_rules,
        AtomLineKind::Dict { allow_use: false },
        WARN_RANK_KEYWORDS,
        warnings,
    );
    if !user_keywords_lines.is_empty() {
        warnings.push(ProfileWarning {
            rank: WARN_RANK_KEYWORDS,
            text: format!(
                "portage/package/ebuild/_config/KeywordsManager.py:89: UserWarning: {} is deprecated, use {} instead\n  warnings.warn(",
                user_keywords_path.display(),
                user_accept_path.display()
            ),
        });
    }
    accept_keywords_lines.extend(user_keywords_lines);
    accept_keywords_lines.extend(filter_atom_lines(
        &read_config_lines_with_sources(&user_accept_path)?,
        &user_accept_rules,
        AtomLineKind::Dict { allow_use: false },
        WARN_RANK_KEYWORDS,
        warnings,
    ));
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

/// The `# <file>:` line and comment lines Portage prints in the unmask
/// block for a stacked `package.mask` entry: the fragment file that
/// contributed the entry (user-most source first, like Portage's reversed
/// `locations`) plus the `#` comment block directly above the matching
/// atom in that file (accumulated since the last blank line, kept only
/// when the atom line immediately follows the comments). `None` when the
/// entry is unknown, the file is unreadable, or the file no longer holds
/// the atom -- the renderer then prints the bare block, as before.
// Portage: package/ebuild/getmaskingreason.py (comment, filename)
pub fn masking_file_comment(config: &Config, mask_entry: &str) -> Option<(PathBuf, Vec<String>)> {
    let (_, file) = config
        .package_mask_files
        .iter()
        .rev()
        .find(|(entry, _)| entry == mask_entry)?;
    let text = fs::read_to_string(file).ok()?;
    let target = mask_entry
        .split_once("::")
        .map_or(mask_entry, |(atom, _)| atom);
    let opts = portage_dep::AtomOptions {
        allow_wildcard: true,
        ..Default::default()
    };
    let mut comment: Vec<String> = Vec::new();
    let mut comment_valid: i64 = -1;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            comment.clear();
            comment_valid = -1;
        } else if line.starts_with('#') {
            comment.push(line.to_string());
            comment_valid = i as i64 + 1;
        } else if line.split_once("::").map_or(line, |(atom, _)| atom) == target
            && portage_dep::parse_atom_with(line, &opts, "").is_ok()
        {
            if comment_valid != i as i64 {
                comment.clear();
            }
            return Some((file.clone(), comment));
        } else if comment_valid != -1 {
            comment_valid += 1;
        }
    }
    None
}
