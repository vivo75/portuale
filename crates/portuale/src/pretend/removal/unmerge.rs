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
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
    /// Whether this is a genuine top-level `-C`/`--unmerge` or
    /// `--rage-clean` run (not the shared display path reused by
    /// `--depclean`/`--prune`/`--clean`, which pass `action: "unmerge"`
    /// for the wording): only then does Portage print the
    /// `can only be used with specific package names` no-atoms usage.
    /// The depclean-safety note is narrower still: `action == "unmerge"`
    /// only (never `--rage-clean`). Set at the two standalone call
    /// sites only. Portage: _emerge/actions.py, run_action
    pub(crate) safety_note: bool,
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

/// Per `(category, package)`: the selected and the protected versions.
type PerCpSelection = HashMap<(String, String), (Vec<String>, Vec<String>)>;

fn vercmp_key(a: &str, b: &str) -> std::cmp::Ordering {
    portage_versions::vercmp(a, b)
        .map(|c| c.cmp(&0))
        .unwrap_or_else(|| a.cmp(b))
}

pub(crate) fn run_unmerge_pretend(args: RunUnmergePretendArgs<'_>) -> ExitCode {
    let RunUnmergePretendArgs {
        targets,
        action,
        safety_note,
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
        // Portage prints the no-atoms usage on stdout, naming the action,
        // for a genuine top-level `unmerge`/`rage-clean` run only (a bare
        // `--clean` cleans globally instead -- different path, out of scope).
        // Portage: _emerge/unmerge.py, unmerge
        if safety_note {
            println!("\nemerge {action} can only be used with specific package names\n");
            return ExitCode::from(1);
        }
        eprintln!("emerge: no package atoms given to --{action}");
        return ExitCode::from(1);
    }

    // The depclean-safety note prints for every genuine top-level `-C` run
    // (pretend or real), before anything else: Portage prints it in
    // `run_action` exactly when `action == "unmerge"` -- never for
    // `--rage-clean` (or the depclean/prune/clean display reuse).
    // Each line is Portage's `ewarn` rendering: the ` * ` span in WARN
    // (yellow under `--color=y`, plain otherwise), the body unstyled.
    // (`--quiet-unmerge-warn` is not a parsed portuale flag, so `--quiet`
    // is the only gate.) Portage: _emerge/actions.py, run_action;
    // lib/portage/output.py, EOutput.ewarn
    if safety_note && action == "unmerge" && !quiet {
        for line in [
            "This action can remove important packages! In order to be safer, use",
            "`emerge -pv --depclean <atom>` to check for reverse dependencies before",
            "removing packages.",
        ] {
            eprintln!("{}{line}", color.c("WARN", " * "));
        }
    }

    // Expand every `@set` target into member atoms first, so the
    // vdb-matching loop below only ever sees ordinary atoms.
    // Portage: _emerge/actions.py, unmerge
    let mut expanded: Vec<String> = Vec::new();
    // The sets passed as `-C` targets are excluded from the "still listed
    // in package sets" check below (their members are being removed anyway).
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
                    Ok(out) => expanded.extend(out.atoms),
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

    // The `>>> Using system located in ROOT tree` line (non-`/` ROOT) and,
    // under `--pretend`/`--ask`, the `>>> These are the packages that would
    // be unmerged:` header share one helper with the `--clean`/`--prune`
    // path, each carrying Portage's leading blank line.
    // Portage: unmerge.py, _unmerge_display
    print_unmerge_display_header(UnmergeDisplayHeaderArgs {
        root,
        pretend,
        quiet,
        color,
    });

    // The one package `unmerge` always refuses to select, moving it to
    // `protected` with an eerror note.
    // Portage: unmerge.py, _unmerge_display
    let portage_self = ("sys-apps".to_string(), "portage".to_string());

    // Per-`(category, package)` accumulators keyed by cp, so multiple
    // atoms hitting the same cp merge. `all_selected` dedups a version
    // picked by two atoms.
    // Portage: unmerge.py, _unmerge_display
    let mut per_cp: PerCpSelection = HashMap::new();
    let mut all_selected: HashSet<(String, String, String)> = HashSet::new();
    let mut order: Vec<(String, String)> = Vec::new();

    if let Some(code) = select_unmerge_targets(SelectUnmergeTargetsArgs {
        expanded: &expanded,
        root,
        action,
        per_cp: &mut per_cp,
        order: &mut order,
        all_selected: &mut all_selected,
    }) {
        return code;
    }

    if all_selected.is_empty() {
        println!("\n>>> No packages selected for removal by {action}");
        return ExitCode::from(1);
    }

    // `sys-apps/portage` self-protection: move it from `selected` to
    // `protected` with the note.
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

    // The `@system` cps, for the "is part of your system profile" warning.
    // Portage: _emerge/actions.py, unmerge
    let syslist: HashSet<(String, String)> = config
        .system_packages
        .iter()
        .filter_map(|a| parse_atom(a))
        .map(|a| (a.category, a.package))
        .collect();

    // The "still listed in the following package sets" warning: a selected
    // package a user-editable set still lists would be re-pulled on the
    // next `@world` update, so it's flagged (minus the higher-slot cover).
    // Portage: unmerge.py, _unmerge_display
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

    if !preserve_order {
        order.sort();
    }
    let mut all_selected_display: Vec<String> = Vec::new();
    // `(category, package, version)` of every `selected` version, in the
    // same order the preview blocks render; the removal loop walks this order.
    let mut removal_list: Vec<(String, String, String)> = Vec::new();
    print_unmerge_selection(PrintUnmergeSelectionArgs {
        order: &order,
        per_cp: &mut per_cp,
        root,
        syslist: &syslist,
        color,
        all_selected_display: &mut all_selected_display,
        removal_list: &mut removal_list,
    });

    all_selected_display.sort();
    println!(
        "\nAll selected packages: {}",
        all_selected_display.join(" ")
    );
    println!(
        "\n>>> {} packages are slated for removal.",
        color.c("UNMERGE_WARN", "'Selected'")
    );
    // Portage's legend block ends with a blank line (unmerge.py:563-569).
    println!(
        ">>> {} and {} packages will not be removed.\n",
        color.c("GOOD", "'Protected'"),
        color.c("GOOD", "'omitted'")
    );

    // Once the display is accepted and we're not in `--pretend`, the
    // packages are actually removed.
    // Portage: unmerge.py, unmerge
    if !pretend {
        if ask && !ask_confirm(color, "Would you like to unmerge these packages?") {
            return ExitCode::from(130);
        }
        clean_delay_countdown(Some(config));
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
            autoclean: false,
        });
    }
    ExitCode::SUCCESS
}

/// The values [`print_unmerge_display_header`] reads from its caller.
pub(crate) struct UnmergeDisplayHeaderArgs<'a> {
    pub(crate) root: &'a Path,
    pub(crate) pretend: bool,
    pub(crate) quiet: bool,
    pub(crate) color: &'a Colorizer,
}

/// Portage `_unmerge_display`'s two opening lines as text: the
/// `>>> Using system located in ROOT tree <root>/` notice for a non-`/`
/// ROOT (every removal display: `-C`, `--clean`, `--depclean`), and the
/// `>>> These are the packages that would be unmerged:` header under
/// `--pretend`/`--ask`. Both carry a leading blank line unless `--quiet`
/// (backlog #521).
// Portage: _emerge/unmerge.py, _unmerge_display
pub(crate) fn unmerge_display_header_text(
    root: &Path,
    pretend: bool,
    quiet: bool,
    color: &Colorizer,
) -> String {
    let newline = if quiet { "" } else { "\n" };
    let mut out = String::new();
    if root != Path::new("/") {
        out.push_str(&color.c(
            "darkgreen",
            &format!(
                "{newline}>>> Using system located in ROOT tree {}/",
                root.display()
            ),
        ));
        out.push('\n');
    }
    if pretend && !quiet {
        out.push_str(&color.c(
            "darkgreen",
            &format!("{newline}>>> These are the packages that would be unmerged:"),
        ));
        out.push('\n');
    }
    out
}

/// Prints [`unmerge_display_header_text`].
pub(crate) fn print_unmerge_display_header(args: UnmergeDisplayHeaderArgs<'_>) {
    let UnmergeDisplayHeaderArgs {
        root,
        pretend,
        quiet,
        color,
    } = args;
    print!(
        "{}",
        unmerge_display_header_text(root, pretend, quiet, color)
    );
}

/// The values [`print_unmerge_selection`] reads from its caller.
pub(crate) struct PrintUnmergeSelectionArgs<'a> {
    pub(crate) order: &'a [(String, String)],
    pub(crate) per_cp: &'a mut PerCpSelection,
    pub(crate) root: &'a Path,
    pub(crate) syslist: &'a HashSet<(String, String)>,
    pub(crate) color: &'a Colorizer,
    pub(crate) all_selected_display: &'a mut Vec<String>,
    pub(crate) removal_list: &'a mut Vec<(String, String, String)>,
}

/// Print the per-package unmerge selection.
pub(crate) fn print_unmerge_selection(args: PrintUnmergeSelectionArgs<'_>) {
    let PrintUnmergeSelectionArgs {
        order,
        per_cp,
        root,
        syslist,
        color,
        all_selected_display,
        removal_list,
    } = args;
    for cp in order {
        let (selected, protected) = per_cp.get_mut(cp).unwrap();
        if selected.is_empty() {
            continue;
        }
        selected.sort_by(|a, b| vercmp_key(a, b));
        protected.sort_by(|a, b| vercmp_key(a, b));
        // `omitted` = every other installed version of this cp.
        let mut omitted: Vec<String> = portage_repo::installed_candidates(root, &cp.0, &cp.1)
            .into_iter()
            .map(|(v, _, _)| v)
            .filter(|v| !selected.contains(v) && !protected.contains(v))
            .collect();
        omitted.sort_by(|a, b| vercmp_key(a, b));

        // A cp that would be fully removed and is a `@system` member warns
        // on stderr.
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
}

/// The values [`select_unmerge_targets`] reads from its caller.
pub(crate) struct SelectUnmergeTargetsArgs<'a> {
    pub(crate) expanded: &'a [String],
    pub(crate) root: &'a Path,
    pub(crate) action: &'a str,
    pub(crate) per_cp: &'a mut PerCpSelection,
    pub(crate) order: &'a mut Vec<(String, String)>,
    pub(crate) all_selected: &'a mut HashSet<(String, String, String)>,
}

/// Select the installed versions each unmerge atom removes.
pub(crate) fn select_unmerge_targets(args: SelectUnmergeTargetsArgs<'_>) -> Option<ExitCode> {
    let SelectUnmergeTargetsArgs {
        expanded,
        root,
        action,
        per_cp,
        order,
        all_selected,
    } = args;
    for atom_str in expanded {
        // A bare name (no `/`) borrows its category from the vdb.
        // Ambiguous (installed under >1 category) is a hard error.
        // Portage: vartree.py, dep_match
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
                return Some(ExitCode::from(1));
            }
            found
        } else {
            let Some(atom) = parse_atom(atom_str) else {
                eprintln!("emerge: invalid atom {atom_str:?}");
                return Some(ExitCode::from(1));
            };
            portage_repo::installed_candidates(root, &atom.category, &atom.package)
                .into_iter()
                .filter(|(version, slot, sub_slot)| {
                    let cs = format!(
                        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
            // Portage's `writemsg` (`noiselevel=-1`): stderr, with the
            // leading blank line. Portage: _emerge/unmerge.py, unmerge
            eprintln!("\n--- Couldn't find '{atom_str}' to {action}.");
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
    None
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
    /// The post-merge auto-clean: the info-dir regen, the preserved-libs
    /// notice and the GLEP 42 notice stay with the merge's own
    /// post-merge step (which runs right after), as in Portage where
    /// they live in `post_emerge`, after the auto-clean.
    /// Portage: post_emerge.py, post_emerge
    pub(crate) autoclean: bool,
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
        autoclean,
    } = args;
    let portage_tmpdir = portage_repo::portage_tmpdir_from_env();
    let mut options = ebuild_merge::MergeOptions::from_env(shell, debug);
    // `FEATURES=unmerge-orphans` keeps what the resolved `CONFIG_PROTECT`
    // protects, not the process env's value.
    options.apply_config_protect(config);
    // `UNINSTALL_IGNORE` from the resolved scalar (the synthetic base when
    // `make.globals` was not read), not the raw process env.
    options.apply_uninstall_ignore(config);
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
            // Standalone `emerge -C`: no replacing version.
            replaced_by_version: None,
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
    // then warn about any library the removal preserved -- unless this is
    // the post-merge auto-clean, whose merge step runs all three right
    // after (see `run_autoclean_after_merge`).
    // Portage: post_emerge.py, post_emerge
    if !autoclean {
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
                // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
                        // match-str-ok: partial form (no ::repo) built from a record's or tuple's version/slot fields; match_str would add the repo
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
