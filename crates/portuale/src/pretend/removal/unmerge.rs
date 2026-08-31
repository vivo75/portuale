//! `--unmerge`: listing and removing installed packages.

use crate::pretend::*;

/// The "still listed in the following package sets" check for one selected
/// `category/package-version`: names of the user-editable sets that still
/// directly list a matching atom, minus any set whose atom is also satisfied
/// by an installed newer version of the same cp in a different slot.
/// Shared by the unmerge and prune paths.
/// Portage: unmerge.py, _unmerge_display
pub(crate) fn still_listed_parents<'a>(
    root: &Path,
    installed_sets: &'a [(String, Vec<String>)],
    cat: &str,
    pkg: &str,
    version: &str,
) -> Vec<&'a str> {
    let installed = portage_repo::installed_candidates(root, cat, pkg);
    let selected_slot = installed
        .iter()
        .find(|(v, _, _)| v == version)
        .map(|(_, s, _)| s.as_str());
    let covered_by_higher_slot = |atom_str: &str| -> bool {
        installed.iter().any(|(v, s, ss)| {
            portage_versions::vercmp(v, version).is_some_and(|c| c > 0)
                && Some(s.as_str()) != selected_slot
                && match_from_list(atom_str, &[format!("{cat}/{pkg}-{v}:{s}/{ss}").as_str()])
                    .is_some_and(|m| !m.is_empty())
        })
    };
    let candidate = format!("{cat}/{pkg}-{version}");
    let mut parents: Vec<&str> = Vec::new();
    for (set_name, atoms) in installed_sets {
        let listed = atoms.iter().any(|atom_str| {
            parse_atom(atom_str).is_some_and(|a| a.category == cat && a.package == pkg)
                && match_from_list(atom_str, &[candidate.as_str()]).is_some_and(|m| !m.is_empty())
                && !covered_by_higher_slot(atom_str)
        });
        if listed {
            parents.push(set_name);
        }
    }
    parents
}
/// Arguments of [`run_unmerge_pretend`].
pub(crate) struct RunUnmergePretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    /// `"unmerge"`, `"rage-clean"` (both remove exactly the given atoms;
    /// `--rage-clean` only differs by skipping `CLEAN_DELAY` + prerm/postrm,
    /// invisible at `--pretend`), `"depclean"`, `"prune"`. Threaded into the
    /// `Couldn't find ... to <action>` / `removal by <action>` messages.
    pub(crate) action: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    /// The `repos.conf` repos, for the post-removal GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    /// When `true` the per-package blocks render in `targets` order, not
    /// regrouped by `cat/pn`. Only `--depclean`'s topologically-sorted
    /// cleanlist sets this; a plain `--unmerge`/`-C` is always unordered.
    pub(crate) preserve_order: bool,
    /// `false` for a non-`--pretend` run: the header is suppressed and the
    /// removal runs after the display. `true` for `-pC` and for the
    /// `--depclean`/`--prune` preview reuse.
    pub(crate) pretend: bool,
    /// `--ask`: prompt before the removal, then run the `CLEAN_DELAY`
    /// countdown. Always `false` when `pretend` is `true`.
    pub(crate) ask: bool,
    /// Which shell backend runs each removed package's
    /// `pkg_prerm`/`pkg_postrm`. Inert when `pretend` is `true`.
    pub(crate) shell: ebuild_phases::ShellBackend,
    /// `PORTAGE_DEBUG=1` for the `pkg_prerm`/`pkg_postrm` phases.
    /// Inert when `pretend`.
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the post-removal info-regen einfo lines, plus the
    /// resolved-config `FEATURES` read; computed by each caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

/// Whether a `FEATURES` token is on in the raw process env. This is the
/// fallback for the CLI boundaries with no resolved config; every
/// build/merge path reads the resolved list instead.
pub(crate) fn feature_enabled(token: &str) -> bool {
    std::env::var("FEATURES")
        .map(|f| f.split_whitespace().any(|t| t == token))
        .unwrap_or(false)
}
/// The resolved `FEATURES` driving `emerge -C`, plus its `unmerge-backup`
/// gate: the folded config value already stacks the calling env, so a
/// `make.conf`-only token still backs up.
/// Portage: dbapi/vartree.py, dblink._pre_unmerge_backup
pub(crate) fn unmerge_backup_features(config: &portage_profile::Config) -> (bool, String) {
    let resolved = config_features_string(config);
    let backup = resolved.split_whitespace().any(|t| t == "unmerge-backup");
    (backup, resolved)
}
/// Arguments of [`execute_unmerge`].
pub(crate) struct ExecuteUnmergeArgs<'a> {
    pub(crate) removal_list: &'a [(String, String, String)],
    pub(crate) root: &'a Path,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the info-regen einfo lines, plus the resolved-config
    /// `FEATURES` read; threaded from the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// The `repos.conf` repos + resolved config, for the post-removal
    /// GLEP 42 count notice. The vdb-changed gate is structural (callers
    /// only reach here with a non-empty `removal_list` and no `--pretend`).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) config: &'a portage_profile::Config,
}

pub(crate) fn execute_unmerge(args: ExecuteUnmergeArgs<'_>) -> ExitCode {
    let ExecuteUnmergeArgs {
        removal_list,
        root,
        shell,
        debug,
        color,
        quiet,
        noinfo,
        repos,
        config,
    } = args;
    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
    let mut options = ebuild_merge::MergeOptions::from_env(shell, debug);
    // `FEATURES=unmerge-orphans` keeps what the resolved `CONFIG_PROTECT`
    // protects, not the process env's value.
    options.apply_config_protect(config);
    // Resolved `FEATURES` for the backup gate and both option sets, as the
    // merge paths do.
    // Portage: dbapi/vartree.py, dblink._pre_unmerge_backup
    let (backup_wanted, resolved_features) = unmerge_backup_features(config);
    options.set_resolved_features(&resolved_features);
    // `FEATURES=unmerge-backup` -> a `quickpkg` of each package before it's
    // removed. `BINPKG_FORMAT` follows the resolved chain; a failed
    // resolution keeps the old env-only fallback.
    let unmerge_config =
        ebuild_phases::resolve_unmerge_config(&portage_repo::config_root_from_env(), root);
    let backup = backup_wanted.then(|| {
        package_options_from_env(
            shell,
            debug,
            Some(&resolved_features),
            unmerge_config.as_ref(),
        )
    });
    let scratch = portage_tmpdir.join("portage").join("_unmerge_src");
    let total = removal_list.len();
    for (idx, (category, package, version)) in removal_list.iter().enumerate() {
        let pf = format!("{package}-{version}");
        println!(
            ">>> Unmerging ({} of {}) {}/{}...",
            color.c("MERGE_LIST_PROGRESS", &(idx + 1).to_string()),
            color.c("MERGE_LIST_PROGRESS", &total.to_string()),
            category,
            pf
        );
        if let Err(e) = ebuild_merge::unmerge_one_installed(ebuild_merge::UnmergeOneInstalledArgs {
            root,
            category,
            package,
            pf: &pf,
            also_keep: &[],
            scratch_dir: &scratch,
            portage_tmpdir: &portage_tmpdir,
            options: &options,
            backup: backup.as_ref(),
            is_replacement: false,
            replacement_preserved: &BTreeMap::new(),
            replacement_needed: &[],
            deferred_delete: None,
        }) {
            // The notice prints when the vdb changed (at least one package
            // was already removed); a failure on the very first removal
            // changed nothing and stays silent.
            // Portage: post_emerge.py, post_emerge
            if idx > 0 {
                display_news_notice_if_any(repos, root, config, color);
            }
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
        if let Err(e) = deselect_from_world(root, category, package) {
            // The package itself is already removed above, so the notice prints.
            // Portage: post_emerge.py, post_emerge
            display_news_notice_if_any(repos, root, config, color);
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    // Hand each removed package's `pkg_prerm`/`pkg_postrm` elog output to
    // the `echo`/`save`/`save_summary` modules.
    // Portage: dblink.py, unmerge
    let items: Vec<(String, std::path::PathBuf)> = removal_list
        .iter()
        .map(|(category, package, version)| {
            let pf = format!("{package}-{version}");
            let t_dir = portage_tmpdir
                .join("portage")
                .join(category)
                .join(&pf)
                .join("temp");
            (format!("{category}/{pf}"), t_dir)
        })
        .collect();
    crate::elog::process_batch(crate::elog::ProcessBatchArgs {
        logdir: &crate::elog::logdir(root),
        root_display: &root.display().to_string(),
        items: &items,
        phases: Some(&["prerm", "postrm"]),
        color,
        split_elog: resolved_features
            .split_whitespace()
            .any(|t| t == "split-elog"),
    });

    // After the removal, regenerate any stale GNU info directory index,
    // then warn about any library the removal preserved.
    // Portage: post_emerge.py, post_emerge
    if let Err(e) = crate::info_files::post_merge_info_update(root, color, quiet, noinfo) {
        // Every removal already landed above, so the notice prints.
        // Portage: post_emerge.py, post_emerge
        display_news_notice_if_any(repos, root, config, color);
        eprintln!("emerge: {e}");
        return ExitCode::from(1);
    }
    crate::preserved_libs::show_preserved_libs_notice(root, color, false, false);
    // The GLEP 42 count notice tail after the uninstall actions.
    // Portage: post_emerge.py, post_emerge
    if !removal_list.is_empty() {
        display_news_notice_if_any(repos, root, config, color);
    }

    ExitCode::SUCCESS
}

/// Drop every world atom for the removed package unless some installed
/// version still satisfies it; all other atoms are kept untouched. The file
/// is rewritten sorted, with comment and `@set` lines not carried forward.
/// Portage: _sets/files.py, WorldSelectedPackagesSet.cleanPackage
pub(crate) fn deselect_from_world(
    root: &Path,
    category: &str,
    package: &str,
) -> Result<(), String> {
    let mykey = format!("{category}/{package}");
    let mut current = read_world_atoms(root)?;
    let before = current.len();
    current.retain(|raw| {
        let Some(atom) = parse_atom(raw) else {
            return true;
        };
        if format!("{}/{}", atom.category, atom.package) != mykey {
            return true;
        }
        portage_repo::installed_candidates(root, &atom.category, &atom.package)
            .into_iter()
            .any(|(v, slot, sub_slot)| {
                let cs = format!("{}/{}-{v}:{slot}/{sub_slot}", atom.category, atom.package);
                match_from_list(raw, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
            })
    });
    if current.len() == before {
        return Ok(());
    }
    current.sort();
    current.dedup();
    let db = portage_vdb::for_root(root);
    let mut txn = db.begin_write().map_err(|e| e.to_string())?;
    txn.set_world(&portage_vdb::World { atoms: current })
        .map_err(|e| e.to_string())?;
    txn.commit().map_err(|e| e.to_string())?;
    Ok(())
}

/// One `selected:` / `protected:` / `omitted:` row of the per-package block:
/// the label right-justified into 14 columns, then each version followed by
/// a trailing space, or the literal `none ` when empty, trailing spaces
/// included. `selected` versions render red, `protected`/`omitted` green.
/// Portage: unmerge.py, _unmerge_display
pub(crate) fn print_unmerge_row(label: &str, versions: &[String], color: &Colorizer) {
    let head = format!("{label}: ");
    let padded = format!("{head:>14}");
    if versions.is_empty() {
        println!("{padded}none ");
    } else {
        let key = if label == "selected" {
            "UNMERGE_WARN"
        } else {
            "GOOD"
        };
        let mut line = padded;
        for v in versions {
            line.push_str(&color.c(key, &format!("{v} ")));
        }
        println!("{line}");
    }
}

/// Every installed `(category, package, version, slot)` in the vdb: used only
/// for the bare-name ("null category") resolution.
/// Portage: vartree.py, dbapi.cpv_all
pub(crate) fn installed_cp_versions(root: &Path) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    // `-MERGING-<pf>` entries are never a resolution candidate, so skipped.
    let Ok(entries) = portage_vdb::for_root(root).entries() else {
        return out;
    };
    for key in entries {
        let (category, dirname) = (key.category, key.pf);
        if let Some((name, version)) = split_pf(&dirname) {
            // Through the vdb seam; the main slot only (an absent `SLOT`
            // reads as `""`, like the old default).
            let slot = portage_repo::vdb_entry_slot(root, &category, &dirname)
                .split('/')
                .next()
                .unwrap_or("0")
                .to_string();
            out.push((category, name, version, slot));
        }
    }
    out
}

/// Shared `--depclean`/`--prune` argument handling: resolve each bare name
/// against the vdb null-category scan, then check every resulting atom. One
/// matching nothing prints `--- Couldn't find 'X' to <action>`; if none
/// match, print the empty message and return `Err(1)`. Empty `targets` is
/// `Ok(vec![])` (the full, no-args form).
/// Portage: _emerge/actions.py, action_depclean
pub(crate) fn resolve_cleanup_args(
    targets: &[&str],
    root: &Path,
    action: &str,
) -> Result<Vec<String>, ExitCode> {
    let mut args: Vec<String> = Vec::new();
    let installed_scan = installed_cp_versions(root);
    for t in targets {
        if t.contains('/') {
            args.push((*t).to_string());
        } else {
            let cats: HashSet<&String> = installed_scan
                .iter()
                .filter(|(_, p, _, _)| p == *t)
                .map(|(c, _, _, _)| c)
                .collect();
            match cats.len() {
                0 => args.push((*t).to_string()), // let the "Couldn't find" path report it
                1 => args.push(format!("{}/{t}", cats.into_iter().next().unwrap())),
                _ => {
                    eprintln!("\n!!! The short package name \"{t}\" is ambiguous. Please specify");
                    eprintln!("!!! one of the following fully-qualified package names instead:\n");
                    let mut names: Vec<String> =
                        cats.into_iter().map(|c| format!("    {c}/{t}")).collect();
                    names.sort();
                    for n in names {
                        println!("{n}");
                    }
                    return Err(ExitCode::from(1));
                }
            }
        }
    }

    if !args.is_empty() {
        let mut any_matched = false;
        for a in &args {
            let matched = portage_dep::parse_atom(a).is_some_and(|atom| {
                portage_repo::installed_candidates(root, &atom.category, &atom.package)
                    .iter()
                    .any(|(v, s, sub)| {
                        let cs = format!("{}/{}-{v}:{s}/{sub}", atom.category, atom.package);
                        match_from_list(a, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                    })
            });
            if matched {
                any_matched = true;
            } else {
                eprintln!(
                    "--- Couldn't find '{}' to {action}.",
                    a.strip_prefix("null/").unwrap_or(a)
                );
            }
        }
        if !any_matched {
            println!(">>> No packages selected for removal by {action}");
            return Err(ExitCode::from(1));
        }
    }
    Ok(args)
}
