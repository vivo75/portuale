//! `--unmerge`: listing and removing installed packages.

use crate::pretend::*;

/// Real `unmerge.py:355-447`'s "still listed in the following package
/// sets" check for one selected `category/package-version`: the names of
/// the user-editable sets (`installed_sets` -- `EditablePackageSet`s
/// reached via `world_sets`) that still directly list a matching atom,
/// *minus* any set whose matching atom is also satisfied by an installed
/// newer version of the same cp in a different slot (real
/// `unmerge.py:421-441`'s `higher_slot`: `pkg.slot_atom != inst_pkg.
/// slot_atom` after the descending-order `pkg >= inst_pkg` break --
/// removing this version leaves that set satisfied). Shared by
/// `run_unmerge_pretend` and `run_prune_pretend`, matching real
/// portage's own single `_unmerge_display` handling every
/// `unmerge_action`.
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
    /// Real `unmerge_action` -- `"unmerge"`, `"rage-clean"` (both remove
    /// exactly the given atoms; `--rage-clean` only differs by skipping
    /// `CLEAN_DELAY` + prerm/postrm, invisible at `--pretend`),
    /// `"depclean"`, `"prune"`. Threaded into the `Couldn't find ... to
    /// <action>` / `removal by <action>` / `Portage to <action> itself`
    /// messages (real `f"... {unmerge_action}"`).
    pub(crate) action: &'a str,
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    /// The `repos.conf` repos -- threaded into the post-removal GLEP 42
    /// count notice (backlog #196 fix round 1: real `post_emerge` after
    /// uninstall actions, `actions.py:4164-4175`).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    /// Real `_unmerge_display`'s `ordered` flag (`unmerge.py:459`): when
    /// `true` the per-package blocks are rendered in `targets` order and
    /// *not* regrouped/re-sorted by `cat/pn`. Only `--depclean`'s own
    /// topologically-sorted cleanlist sets this (`run_depclean_pretend`);
    /// a plain `--unmerge`/`-C` from the CLI is always unordered.
    pub(crate) preserve_order: bool,
    /// `false` for a real `emerge -C <atom>` (no `--pretend`): the
    /// `>>> These are the packages that would be unmerged:` header is
    /// suppressed and `execute_unmerge` runs after the display. `true`
    /// for `-pC` and for the `--depclean`/`--prune` preview reuse.
    pub(crate) pretend: bool,
    /// `--ask`/`-a`: prompt `Would you like to unmerge these packages?`
    /// before the removal (real `unmerge.py:621`), then run the
    /// `CLEAN_DELAY` countdown. Always `false` when `pretend` is `true`.
    pub(crate) ask: bool,
    /// `--shell bash|brush`: which real shell backend runs each removed
    /// package's `pkg_prerm`/`pkg_postrm` (and the `FEATURES=unmerge-backup`
    /// `quickpkg`). Inert when `pretend` is `true`.
    pub(crate) shell: ebuild_phases::ShellBackend,
    /// `--debug`/`-d`: `PORTAGE_DEBUG=1` for the `pkg_prerm`/`pkg_postrm`
    /// phases (and the `unmerge-backup` `quickpkg`). Inert when `pretend`.
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Real `--quiet` (`noiselimit < 0`, `actions.py:3907-3908`): the
    /// post-removal info-regen einfo lines are suppressed. `noinfo` is the
    /// resolved-config `FEATURES` read (`config_features_list`), computed
    /// by each caller from the `config` already in its scope.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

pub(crate) fn run_unmerge_pretend(args: RunUnmergePretendArgs<'_>) -> ExitCode {
    let RunUnmergePretendArgs {
        targets,
        action,
        root,
        config_root,
        config,
        repos,
        preserve_order,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
    } = args;
    if targets.is_empty() {
        eprintln!("emerge: no package atoms given to --{action}");
        return ExitCode::from(1);
    }

    // Expand every `@set` target into its member atoms first (real
    // `root_config.sets[s].getAtoms()` / `_iter_atoms_for_pkg`), so the
    // vdb-matching loop below only ever sees ordinary atoms.
    // `@world`/`@selected`/`@system`/`@installed` are the built-in sets;
    // anything else `@name` is a custom set file (recursively expanded,
    // cycle-guarded).
    let mut expanded: Vec<String> = Vec::new();
    // Real `root_config.setconfig.active` -- the sets the user passed as
    // `-C` targets. Excluded from the "still listed in package sets"
    // check below (their members are being removed from the set anyway).
    let mut active_sets: HashSet<String> = HashSet::new();
    for target in targets {
        if let Some(name) = target.strip_prefix('@') {
            active_sets.insert(name.to_string());
        }
        match *target {
            "@world" | "@selected" => match expand_selected(root, config_root) {
                Ok(atoms) => expanded.extend(atoms),
                Err(e) => {
                    eprintln!("{e}");
                    return ExitCode::from(1);
                }
            },
            "@profile" => expanded.extend(config.profile_packages.iter().cloned()),
            "@system" => expanded.extend(config.system_packages.iter().cloned()),
            "@installed" => expanded.extend(installed_set_atoms(root)),
            "@preserved-rebuild" => expanded.extend(preserved_rebuild_atoms(root)),
            other if other.starts_with('@') => {
                let mut seen = HashSet::new();
                match resolve_custom_set(config_root, &other[1..], &mut seen) {
                    Ok(atoms) => expanded.extend(atoms),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                }
            }
            other => match resolve_vdb_path_arg(other, root) {
                Ok(Some(atom)) => expanded.push(atom),
                Ok(None) => expanded.push(other.to_string()),
                Err(code) => return code,
            },
        }
    }

    // Real `_unmerge_display` (`unmerge.py:195`) prints this header only
    // under `--pretend`/`--ask`, before the per-atom matching loop -- so
    // it shows even when nothing ends up selected, but a real `emerge -C`
    // removal run doesn't print it at all.
    if pretend {
        println!(
            "{}",
            color.c(
                "darkgreen",
                ">>> These are the packages that would be unmerged:"
            )
        );
    }

    // Real `PORTAGE_PACKAGE_ATOM` -- the one package `unmerge` always
    // refuses to select, moving it to `protected` with an eerror note.
    let portage_self = ("sys-apps".to_string(), "portage".to_string());

    // Per-`(category, package)` accumulators, real portage's own `pkgmap`
    // entry with its `selected`/`protected`/`omitted` sets, keyed by cp
    // so multiple atoms hitting the same cp merge (real `unordered`
    // dedup). `all_selected` dedups a version picked by two atoms.
    let mut per_cp: HashMap<(String, String), (Vec<String>, Vec<String>)> = HashMap::new();
    let mut all_selected: HashSet<(String, String, String)> = HashSet::new();
    let mut order: Vec<(String, String)> = Vec::new();

    for atom_str in &expanded {
        // A bare name (no `/`) borrows its category from the vdb: real
        // `vartree.dep_match`'s own "null category" lookup. Ambiguous
        // (installed under >1 category) is a hard error, real
        // `AmbiguousPackageName`.
        let matches: Vec<(String, String, String, String)> = if !atom_str.contains('/') {
            let mut found: Vec<(String, String, String, String)> = Vec::new();
            for (cat, pkg, version, slot) in installed_cp_versions(root) {
                if pkg == *atom_str {
                    found.push((cat, pkg, version, slot));
                }
            }
            let cats: HashSet<&String> = found.iter().map(|(c, _, _, _)| c).collect();
            if cats.len() > 1 {
                eprintln!(
                    "\n!!! The short package name \"{atom_str}\" is ambiguous. Please specify"
                );
                eprintln!("!!! one of the following fully-qualified package names instead:\n");
                let mut names: Vec<String> = cats
                    .into_iter()
                    .map(|c| format!("    {c}/{atom_str}"))
                    .collect();
                names.sort();
                for n in names {
                    println!("{n}");
                }
                return ExitCode::from(1);
            }
            found
        } else {
            let Some(atom) = parse_atom(atom_str) else {
                eprintln!("emerge: invalid atom {atom_str:?}");
                return ExitCode::from(1);
            };
            portage_repo::installed_candidates(root, &atom.category, &atom.package)
                .into_iter()
                .filter(|(version, slot, sub_slot)| {
                    let cs = format!(
                        "{}/{}-{version}:{slot}/{sub_slot}",
                        atom.category, atom.package
                    );
                    match_from_list(atom_str, &[cs.as_str()]).is_some_and(|m| !m.is_empty())
                })
                .map(|(version, slot, _sub)| {
                    (atom.category.clone(), atom.package.clone(), version, slot)
                })
                .collect()
        };

        if matches.is_empty() {
            println!("\n--- Couldn't find '{atom_str}' to {action}.");
            continue;
        }

        for (cat, pkg, version, _slot) in matches {
            let cp = (cat.clone(), pkg.clone());
            let entry = per_cp.entry(cp.clone()).or_insert_with(|| {
                order.push(cp.clone());
                (Vec::new(), Vec::new())
            });
            let key = (cat, pkg, version.clone());
            if all_selected.insert(key) {
                entry.0.push(version);
            }
        }
    }

    if all_selected.is_empty() {
        println!("\n>>> No packages selected for removal by {action}");
        return ExitCode::from(1);
    }

    // `sys-apps/portage` self-protection: real portage moves it out of
    // `selected` into `protected` and prints the note.
    if let Some((selected, protected)) = per_cp.get_mut(&portage_self)
        && !selected.is_empty()
    {
        for v in selected.drain(..) {
            eprintln!(
                "!!! Not unmerging package sys-apps/portage-{v} since there is no valid \
                     reason for Portage to {action} itself."
            );
            all_selected.remove(&(portage_self.0.clone(), portage_self.1.clone(), v.clone()));
            protected.push(v);
        }
    }
    // Recheck: the self-skip may have emptied the only selection.
    if all_selected.is_empty() {
        println!("\n>>> No packages selected for removal by {action}");
        return ExitCode::from(1);
    }

    // Real `syslist = root_config.sets["system"].getAtoms()` -> the
    // `@system` cps, for the "is part of your system profile" warning.
    let syslist: HashSet<(String, String)> = config
        .system_packages
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    // Real `_unmerge_display`'s "still listed in the following package
    // sets" warning: a `selected` package that a user-editable set
    // (reached via `world_sets`) still lists would be re-pulled on the
    // next `@world` update, so it's flagged. Real `unmerge.py:421-441`
    // suppresses the flag for a set when an installed *higher-versioned*
    // instance of the same cp *in a different slot* also matches the set
    // atom (`pkg.slot_atom != inst_pkg.slot_atom` after the `pkg >=
    // inst_pkg` break -> "a newer version, other slot") -- removing this
    // version leaves the set satisfied by that one.
    let installed_sets: Vec<(String, Vec<String>)> = match collect_installed_sets(config_root, root)
    {
        Ok(sets) => sets
            .into_iter()
            .filter(|(name, _)| !active_sets.contains(name))
            .collect(),
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    };
    let mut selected_sorted: Vec<&(String, String, String)> = all_selected.iter().collect();
    selected_sorted.sort();
    for (cat, pkg, version) in selected_sorted {
        let mut parents = still_listed_parents(root, &installed_sets, cat, pkg, version);
        if !parents.is_empty() {
            parents.sort_unstable();
            println!(
                "{}",
                color.c(
                    "WARN",
                    &format!("Package {cat}/{pkg}-{version} is going to be unmerged,")
                )
            );
            println!(
                "{}",
                color.c("WARN", "but still listed in the following package sets:")
            );
            println!("    {}\n", parents.join(", "));
        }
    }

    let vercmp_key = |a: &str, b: &str| {
        portage_versions::vercmp(a, b)
            .map(|c| c.cmp(&0))
            .unwrap_or_else(|| a.cmp(b))
    };
    if !preserve_order {
        order.sort();
    }
    let mut all_selected_display: Vec<String> = Vec::new();
    // `(category, package, version)` of every `selected` version, in the
    // same order the preview blocks render -- real `unmerge()`'s own
    // removal loop walks `pkgmap` in exactly this order.
    let mut removal_list: Vec<(String, String, String)> = Vec::new();
    for cp in &order {
        let (selected, protected) = per_cp.get_mut(cp).unwrap();
        if selected.is_empty() {
            continue;
        }
        selected.sort_by(|a, b| vercmp_key(a, b));
        protected.sort_by(|a, b| vercmp_key(a, b));
        // `omitted` = every other installed version of this cp, real
        // `vartree.dep_match(cp)` minus selected/protected.
        let mut omitted: Vec<String> = portage_repo::installed_candidates(root, &cp.0, &cp.1)
            .into_iter()
            .map(|(v, _, _)| v)
            .filter(|v| !selected.contains(v) && !protected.contains(v))
            .collect();
        omitted.sort_by(|a, b| vercmp_key(a, b));

        // Real `_unmerge_display`: `if not (protected or omitted) and cp
        // in syslist` -- a cp that would be *fully* removed and is a
        // `@system` member. To stderr (real `writemsg_level(...,
        // level=logging.WARNING)`).
        if protected.is_empty() && omitted.is_empty() && syslist.contains(cp) {
            eprintln!(
                "{}",
                color.c(
                    "BAD",
                    &format!(
                        "\n\n!!! '{}/{}' is part of your system profile.",
                        cp.0, cp.1
                    )
                )
            );
            eprintln!(
                "{}",
                color.c("WARN", "!!! Unmerging it may be damaging to your system.\n")
            );
        }

        println!("\n {}/{}", cp.0, cp.1);
        print_unmerge_row("selected", selected, color);
        print_unmerge_row("protected", protected, color);
        print_unmerge_row("omitted", &omitted, color);

        for v in selected.iter() {
            all_selected_display.push(format!("={}/{}-{v}", cp.0, cp.1));
            removal_list.push((cp.0.clone(), cp.1.clone(), v.clone()));
        }
    }

    all_selected_display.sort();
    println!(
        "\nAll selected packages: {}",
        all_selected_display.join(" ")
    );
    println!(
        "\n>>> {} packages are slated for removal.",
        color.c("UNMERGE_WARN", "'Selected'")
    );
    println!(
        ">>> {} and {} packages will not be removed.",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );

    // Real `unmerge()` (`unmerge.py:637-699`): once `_unmerge_display`
    // returns EX_OK and we're not in `--pretend`, the packages are
    // actually removed.
    if !pretend {
        if ask && !ask_confirm(color, "Would you like to unmerge these packages?") {
            return ExitCode::from(130);
        }
        clean_delay_countdown();
        return execute_unmerge(ExecuteUnmergeArgs {
            removal_list: &removal_list,
            root,
            shell,
            debug,
            color,
            quiet,
            noinfo,
            repos,
            config,
        });
    }
    ExitCode::SUCCESS
}

/// Real `"unmerge-backup" in self.settings.features` -- not a
/// `make.globals` default token, so absent unless `FEATURES` names it.
///
/// This is the **raw process-env fallback**, for the CLI boundaries
/// with no resolved config (`ebuild <file>`) and for the two
/// `execute_unmerge` reads (`unmerge-backup`, `split-elog`) that
/// predate its resolved `config` param (backlog #196 fix round 1) and
/// keep the old fallback. Every build/merge path reads the
/// resolved list instead: `config_features_list`/`config_features_string`
/// here, `MergeOptions::set_resolved_features`, and
/// `ebuild_phases::features_string`/`emerge_build::resolved_features`.
pub(crate) fn feature_enabled(token: &str) -> bool {
    std::env::var("FEATURES")
        .map(|f| f.split_whitespace().any(|t| t == token))
        .unwrap_or(false)
}
/// Arguments of [`execute_unmerge`].
pub(crate) struct ExecuteUnmergeArgs<'a> {
    pub(crate) removal_list: &'a [(String, String, String)],
    pub(crate) root: &'a Path,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Real `--quiet` (`noiselimit < 0`): suppresses the info-regen einfo
    /// lines. `noinfo` is the resolved-config `FEATURES` read, threaded
    /// from the caller (real `"noinfo" not in settings.features`,
    /// `post_emerge.py:127`).
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
    /// Backlog #196 fix round 1: real calls `post_emerge` after the
    /// uninstall actions (clean/depclean/prune/unmerge/rage-clean --
    /// `actions.py:4164-4175`, except deselect/buildpkgonly/fetchonly/
    /// pretend), whose tail is the GLEP 42 count notice. `repos` +
    /// `config` feed that notice; the vdb-changed gate is structural
    /// (every caller only reaches here with a non-empty `removal_list`
    /// and `pretend == false`).
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
    // #333: `FEATURES=unmerge-orphans` keeps what the resolved
    // `CONFIG_PROTECT` protects (real `dblink.isprotected`), not the
    // process env's value.
    options.apply_config_protect(config);
    // Real `dblink._pre_unmerge_backup`: `FEATURES=unmerge-backup` -> a
    // `quickpkg` of each package before it's removed. Its `BINPKG_FORMAT`
    // follows the same resolved chain as every other `PackageOptions`
    // construction site (backlog #173: calling env over
    // `make.conf`/profile/`make.globals`, real `config` precedence) --
    // resolved here, since this path carries no graph entry. A failed
    // resolution keeps the old env-only fallback inside
    // `package_options_from_env`'s `None` arm. `resolved_features` stays
    // `None` on purpose: the `FEATURES` tokens keep their raw
    // process-env fallback (only the format joins the chain).
    let unmerge_config =
        ebuild_phases::resolve_unmerge_config(&portage_repo::config_root_from_env(), root);
    let backup = feature_enabled("unmerge-backup")
        .then(|| package_options_from_env(shell, debug, None, unmerge_config.as_ref()));
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
        if let Err(e) = ebuild_merge::unmerge_one_installed(
            root,
            category,
            package,
            &pf,
            &[],
            &scratch,
            &portage_tmpdir,
            &options,
            backup.as_ref(),
            false,
            &BTreeMap::new(),
            // Standalone `emerge -C`: no replacing package, so neither
            // feed (backlog #224).
            &[],
            // The entry is deleted at once, as before (S4.1 defers only
            // the replace loop's deletes).
            None,
        ) {
            // Backlog #196 fix round 1: real `post_emerge` prints the
            // notice regardless of retval -- gated on the vdb having
            // changed. `idx > 0` is that gate here (at least one
            // package was already removed this run); a failure on the
            // very first removal changed nothing and stays silent,
            // exactly like real's `:112-117` early return.
            if idx > 0 {
                display_news_notice_if_any(repos, root, config, color);
            }
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
        if let Err(e) = deselect_from_world(root, category, package) {
            // The package itself is already removed above, so the vdb
            // changed -- the notice prints (same real arm).
            display_news_notice_if_any(repos, root, config, color);
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    // Real `dblink.unmerge()` -> `self._elog_process(phasefilter=("prerm",
    // "postrm"))`: hand each removed package's `pkg_prerm`/`pkg_postrm`
    // `elog`/`ewarn`/`eerror` output to the `echo`/`save`/`save_summary`
    // modules. `${T}` for a removal is `unmerge_one_installed`'s own
    // `run_phase_from_saved_env` builddir (`<PORTAGE_TMPDIR>/portage/
    // <cat>/<pf>/temp`), which only ever holds prerm/postrm logs here.
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
        split_elog: feature_enabled("split-elog"),
    });

    // Real `post_emerge()`: after `emerge -C` / `--depclean` / `--prune`
    // removed something, regenerate any stale GNU info directory index,
    // then warn about any library the removal preserved.
    if let Err(e) = crate::info_files::post_merge_info_update(root, color, quiet, noinfo) {
        // Every removal already landed above, so the vdb changed -- the
        // notice prints (same real arm as below).
        display_news_notice_if_any(repos, root, config, color);
        eprintln!("emerge: {e}");
        return ExitCode::from(1);
    }
    // Real `post_emerge()`: after `emerge -C` / `--depclean` / `--prune`
    // removed something, warn about any library the removal preserved.
    crate::preserved_libs::show_preserved_libs_notice(root, color, false, false);
    // Backlog #196 fix round 1: real `post_emerge()`'s tail
    // (`post_emerge.py:155`) after the uninstall actions
    // (`actions.py:4164-4175` -- clean/depclean/prune/unmerge/
    // rage-clean; NOT deselect -- `run_deselect` never reaches here --
    // and never under `--pretend`, which returns before any removal).
    // The vdb-changed gate is structural: every caller only reaches
    // here with a non-empty `removal_list`, so an empty selection
    // (which returns before removal) prints no notice, like real's
    // `:112-117` early return.
    if !removal_list.is_empty() {
        display_news_notice_if_any(repos, root, config, color);
    }

    ExitCode::SUCCESS
}

/// Real `WorldSelectedPackagesSet.cleanPackage` (`_sets/files.py:336`),
/// called by real `unmerge()` right after each package is removed: every
/// world atom whose `cp` equals the removed package's `category/package`
/// is dropped **unless** some installed version still satisfies it; all
/// other atoms are kept untouched. The file is then rewritten sorted
/// (real `.write`), with comment and `@set` lines not carried forward --
/// matching real portage and `update_world_file`. A world file that ends
/// up empty is written empty (real `replace([])`).
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

/// One `    selected: 1.0 ` / `   protected: none ` / `     omitted: ...`
/// row of `_unmerge_display`'s per-package block: the label
/// right-justified into 14 columns (real `(mytype + ": ").rjust(14)`),
/// then each version followed by a trailing space, or the literal
/// `none ` when empty -- reproduced faithfully, trailing spaces and all.
/// Real `_unmerge_display`: each `selected` version is
/// `colorize("UNMERGE_WARN", v + " ")` (red), each `protected`/`omitted`
/// version `colorize("GOOD", v + " ")` (green) -- the label and `none`
/// stay plain.
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

/// Every installed `(category, package, version, slot)` in the vdb under
/// `root` -- real `vartree.dbapi.cpv_all()` split out, used only for
/// `--unmerge`/`-C`'s own bare-name ("null category") resolution.
pub(crate) fn installed_cp_versions(root: &Path) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    // The backend's listing (`files`: the same walk, each category and
    // entry directory tested with `is_dir`; `-MERGING-<pf>` entries, which
    // are never a `--unmerge`/`-C` resolution candidate, are skipped).
    let Ok(entries) = portage_vdb::for_root(root).entries() else {
        return out;
    };
    for key in entries {
        let (category, dirname) = (key.category, key.pf);
        if let Some((name, version)) = split_pf(&dirname) {
            // #116: through the vdb seam; the main slot only, as
            // before (an absent `SLOT` is `""`, like the old
            // `unwrap_or_default`).
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

/// Real `action_depclean`'s own argument handling (`actions.py:848-863`),
/// shared by `--depclean` and `--prune`: resolve each bare name against
/// the vdb null-category scan (ambiguous -> `!!! ... ambiguous` +
/// `Err(1)`), then check every resulting atom -- one matching nothing
/// prints `--- Couldn't find 'X' to <action>.` (stderr), and if *none*
/// match, print `>>> No packages selected for removal by <action>` and
/// return `Err(1)`. An empty `targets` is `Ok(vec![])` (the full,
/// no-args form).
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
