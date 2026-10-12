//! `--depclean`.

use crate::pretend::*;

/// One cleanlist package `--depclean-lib-check` keeps installed: it solely
/// provides a library still needed at link level by a package that is not
/// itself being removed.
pub(crate) struct LibConsumerProtection {
    pub(crate) provider: portage_repo::InstalledPackage,
    /// Surviving consumer cpv -> the sonames it needs from `provider`, sorted.
    pub(crate) consumers: Vec<(String, Vec<String>)>,
}

/// The `--depclean-lib-check` scan, narrowed to its pure computation: for
/// every cleanlist package, find surviving objects still linking against
/// the sonames it provides. A consumer owned by a cleanlist package itself
/// is dropped.
/// Portage: _emerge/actions.py, _calc_depclean
///
/// Documented narrowing: the multi-provider filter can under-report when
/// the only surviving provider of a soname is itself another cleanlist
/// member; the progress line prints only alongside the WARNING.
pub(crate) fn lib_consumer_scan(
    root: &Path,
    cleanlist: &[portage_repo::InstalledPackage],
) -> Vec<LibConsumerProtection> {
    let owner_entries = needed_elf::read_all_needed_entries(root);
    let map = needed_elf::rebuild(root, &owner_entries);
    let defpath = needed_elf::getlibpaths(root, None);
    let clean_cpvs: HashSet<String> = cleanlist.iter().map(|p| p.cpv()).collect();

    let mut out: Vec<LibConsumerProtection> = Vec::new();
    for pkg in cleanlist {
        let pkg_cpv = pkg.cpv();
        // The objects this package owns that provide a soname.
        let mut provided: Vec<(&str, &str)> = map
            .obj_properties
            .values()
            .filter(|props| props.owner == pkg_cpv && !props.soname.is_empty())
            .map(|props| (props.alt_paths[0].as_str(), props.soname.as_str()))
            .collect();
        provided.sort();

        let mut per_consumer: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        for (lib_path, soname) in provided {
            let Ok(consumers) = needed_elf::find_consumers(needed_elf::FindConsumersArgs {
                root,
                map: &map,
                defpath: &defpath,
                obj: lib_path,
                exclude_provider: None,
                greedy: false,
            }) else {
                continue;
            };
            for consumer_path in consumers {
                let ckey = needed_elf::obj_key(root, &consumer_path);
                let Some(cprops) = map.obj_properties.get(&ckey) else {
                    continue;
                };
                if cprops.owner.is_empty() || cprops.owner == pkg_cpv {
                    continue;
                }
                if clean_cpvs.contains(&cprops.owner) {
                    continue;
                }
                per_consumer
                    .entry(cprops.owner.clone())
                    .or_default()
                    .insert(soname.to_string());
            }
        }

        if !per_consumer.is_empty() {
            out.push(LibConsumerProtection {
                provider: pkg.clone(),
                consumers: per_consumer
                    .into_iter()
                    .map(|(c, s)| (c, s.into_iter().collect()))
                    .collect(),
            });
        }
    }
    // Sorted by provider cpv.
    out.sort_by_key(|p| p.provider.cpv());
    out
}

/// The `--depclean-lib-check` phase: run `lib_consumer_scan` on the
/// tentative cleanlist, warn about providers that must be kept, and hand
/// them to `recompute` (a second pass seeding them as roots so their own
/// deps leave the cleanlist too). A no-op when `lib_check` is off or the
/// cleanlist is already empty.
/// Portage: _emerge/actions.py, _calc_depclean
pub(crate) fn apply_depclean_lib_check(
    root: &Path,
    result: portage_repo::DepcleanResult,
    lib_check: bool,
    color: &Colorizer,
    recompute: impl FnOnce(&[portage_repo::InstalledPackage]) -> portage_repo::DepcleanResult,
) -> portage_repo::DepcleanResult {
    if !lib_check || result.cleanlist.is_empty() {
        return result;
    }
    eprintln!(">>> Checking for lib consumers...");
    let protections = lib_consumer_scan(root, &result.cleanlist);
    if protections.is_empty() {
        return result;
    }
    eprintln!(">>> Assigning files to packages...");

    // The wrap is pinned here since it never changes.
    let star = color.c("BAD", " * ");
    for line in [
        "In order to avoid breakage of link level dependencies, one or more",
        "packages will not be removed. This can be solved by rebuilding the",
        "packages that pulled them in.",
    ] {
        eprintln!("{star}{line}");
    }
    // Per-provider `pulled in by:` / `needs <sonames>` detail, framed by
    // blank ` * ` lines.
    for prot in &protections {
        eprintln!("{star}");
        eprintln!("{star}  {} pulled in by:", prot.provider.cpv());
        for (consumer, sonames) in &prot.consumers {
            eprintln!("{star}    {consumer} needs {}", sonames.join(", "));
        }
    }
    eprintln!("{star}");

    eprintln!(">>> Adding lib providers to graph...");
    let providers: Vec<portage_repo::InstalledPackage> =
        protections.iter().map(|p| p.provider.clone()).collect();
    recompute(&providers)
}

/// The unresolved-dependency halt: when a kept installed package has a hard
/// runtime dependency nothing installed satisfies, print the block and exit
/// 1 without removing anything. Returns `Some(exit 1)` when it halted,
/// `None` to carry on. `is_prune` adds the prune-only `use --nodeps` trailer.
/// Portage: _emerge/actions.py, _calc_depclean
pub(crate) fn depclean_unresolved_halt(
    unresolved: &[(String, String)],
    is_prune: bool,
    color: &Colorizer,
) -> Option<ExitCode> {
    if unresolved.is_empty() {
        return None;
    }
    let star = color.c("BAD", " * ");
    eprintln!("{star}Dependencies could not be completely resolved due to");
    eprintln!("{star}the following required packages not being installed:");
    for (atom, parent) in unresolved {
        eprintln!("{star}");
        eprintln!("{star}  {atom} pulled in by:");
        eprintln!("{star}    {parent}");
    }
    eprintln!("{star}");
    // Pinned wrap width; it never changes.
    eprintln!("{star}Have you forgotten to do a complete update prior to depclean? The");
    eprintln!("{star}most comprehensive command for this purpose is as follows:");
    eprintln!("{star}");
    eprintln!(
        "{star}  {}",
        color.c(
            "GOOD",
            "emerge --update --newuse --deep --with-bdeps=y @world"
        )
    );
    eprintln!("{star}");
    eprintln!("{star}Note that the --with-bdeps=y option is not required in many");
    eprintln!("{star}situations. Refer to the emerge manual page (run `man emerge`)");
    eprintln!("{star}for more information about --with-bdeps.");
    eprintln!("{star}");
    eprintln!("{star}Also, note that it may be necessary to manually uninstall");
    eprintln!("{star}packages that no longer exist in the repository, since it may not");
    eprintln!("{star}be possible to satisfy their dependencies.");
    if is_prune {
        eprintln!("{star}");
        eprintln!(
            "{star}If you would like to ignore dependencies then use {}.",
            color.c("GOOD", "--nodeps")
        );
    }
    Some(ExitCode::from(1))
}

/// Arguments of [`run_depclean_pretend`].
pub(crate) struct RunDepcleanPretendArgs<'a> {
    pub(crate) targets: &'a [&'a str],
    pub(crate) root: &'a Path,
    pub(crate) config_root: &'a Path,
    pub(crate) config: &'a portage_profile::Config,
    /// The `repos.conf` repos, for the post-removal GLEP 42 count notice.
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    /// `--depclean <atoms> --deselect=n` keeps the `world` set as a
    /// protection root (a world member named as an arg is kept).
    pub(crate) deselect: bool,
    /// `false` for a non-`--pretend` run: after the preview the cleanlist
    /// is really removed, and the stats block reads `Number removed:`
    /// instead of `Number to remove:`.
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Suppression of the post-removal info-regen block; computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

/// `emerge --depclean`: the packages nothing in `@world` ∪ `@system` needs
/// at runtime are the cleanlist. The per-package block is exactly
/// `run_unmerge_pretend`'s (each cleanlist cpv passed as an `=cat/pkg-ver`
/// atom), and without `--pretend` that same call really removes them.
/// With `<atoms>`, the named packages get deselected and the cleanlist is
/// just the `args`-matched packages nothing else needs; the `* ` advisory
/// block shows only with no args. See `portage_repo::depclean_cleanlist`.
/// Portage: _emerge/actions.py, action_depclean; _calc_depclean
pub(crate) fn run_depclean_pretend(args: RunDepcleanPretendArgs<'_>) -> ExitCode {
    let RunDepcleanPretendArgs {
        targets,
        root,
        config_root,
        config,
        repos,
        verbose,
        lib_check,
        deselect,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
    } = args;
    // Bare-name targets get their category from the vdb; shared with `--prune`.
    // Portage: _emerge/actions.py, action_depclean
    let args = match resolve_cleanup_args(targets, root, "depclean") {
        Ok(a) => a,
        Err(code) => return code,
    };

    // The advisory block: only with no package arguments, non-quiet. The
    // first paragraph hinges purely on `--depclean-lib-check=n`, since
    // portuale has no preserve-libs feature.
    // Portage: _emerge/actions.py, action_depclean
    if args.is_empty() {
        // Each line is a yellow ` * ` prefix; backtick-wrapped commands
        // inside the text render green.
        let star = color.c("WARN", " * ");
        let green_ticks = |text: &str| -> String {
            let mut out = String::new();
            let mut rest = text;
            while let Some(open) = rest.find('`') {
                out.push_str(&rest[..open]);
                let after = &rest[open + 1..];
                if let Some(close) = after.find('`') {
                    out.push_str(&color.c("GOOD", &format!("`{}`", &after[..close])));
                    rest = &after[close + 1..];
                } else {
                    out.push('`');
                    rest = after;
                }
            }
            out.push_str(rest);
            out
        };
        // `None` = leading bare newline; `Some("")` = a prefixed blank line;
        // `Some(text)` = an advisory line. The first paragraph shows only
        // when `--depclean-lib-check=n`.
        let libcheck_off_paragraph = [
            Some("Depclean may break link level dependencies. Thus, it is"),
            Some("recommended to use a tool such as `revdep-rebuild` (from"),
            Some("app-portage/gentoolkit) in order to detect such breakage."),
            Some(""),
        ];
        for line in [None]
            .into_iter()
            .chain(libcheck_off_paragraph.into_iter().filter(|_| !lib_check))
            .chain([
                Some("Always study the list of packages to be cleaned for any obvious"),
                Some("mistakes. Packages that are part of the world set will always"),
                Some("be kept.  They can be manually added to this set with"),
                Some("`emerge --noreplace <atom>`.  Packages that are listed in"),
                Some("package.provided (see portage(5)) will be removed by"),
                Some("depclean, even if they are part of the world set."),
                Some(""),
                Some("As a safety measure, depclean will not remove any packages"),
                Some("unless *all* required dependencies have been resolved.  As a"),
                Some("consequence of this, it often becomes necessary to run "),
                Some("`emerge --update --newuse --deep @world` prior to depclean."),
            ])
        {
            match line {
                None => println!(),
                Some(text) => println!("{star}{}", green_ticks(text)),
            }
        }
    }

    // `@world` = the `world` file plus each `world_sets` nested set's own
    // atoms, kept as `(atom, label)` pairs for the `--verbose` reverse-dep
    // display.
    let mut world_seeds: Vec<(String, String)> = Vec::new();
    match read_world_atoms(root) {
        Ok(atoms) => world_seeds.extend(atoms.into_iter().map(|a| (a, "@selected".to_string()))),
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    match read_world_sets(root) {
        Ok(names) => {
            for name in names {
                let mut seen = HashSet::new();
                match resolve_custom_set(config_root, &name, &mut seen) {
                    Ok(out) => {
                        let label = format!("@{name}");
                        world_seeds.extend(out.atoms.into_iter().map(|a| (a, label.clone())));
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(1);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("emerge: {e}");
            return ExitCode::from(1);
        }
    }
    let world_atom_count = world_seeds
        .iter()
        .map(|(a, _)| a.clone())
        .collect::<HashSet<_>>()
        .len();

    let result = portage_repo::depclean_cleanlist(portage_repo::DepcleanCleanlistArgs {
        root,
        world_seeds: &world_seeds,
        system_atoms: &config.system_packages,
        args: &args,
        deselect,
        lib_protected_providers: &[],
        package_provided: &config.package_provided,
    });
    // The `unresolved_deps()` safety halt, checked before the lib scan.
    // Portage: _emerge/actions.py, _calc_depclean
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, false, color) {
        return code;
    }
    // The `--depclean-lib-check` phase: a cleanlist package still needed
    // at link level by a survivor is kept, with its own deps.
    // Portage: _emerge/actions.py, _calc_depclean
    let result = apply_depclean_lib_check(root, result, lib_check, color, |providers| {
        portage_repo::depclean_cleanlist(portage_repo::DepcleanCleanlistArgs {
            root,
            world_seeds: &world_seeds,
            system_atoms: &config.system_packages,
            args: &args,
            deselect,
            lib_protected_providers: providers,
            package_provided: &config.package_provided,
        })
    });

    // The reverse-dep blocks come right after the advisory and before the
    // removal-order line / empty message.
    // Portage: _emerge/actions.py, create_cleanlist
    if verbose {
        for (pkg, lines) in &result.kept_parents {
            println!("  {} pulled in by:", pkg.cpv());
            for line in lines {
                println!("    {line}");
            }
            println!();
        }
    }

    let installed_total = portage_repo::all_installed_packages(root).len();
    let stats = || {
        println!("Packages installed:   {installed_total}");
        println!("Packages in world:    {world_atom_count}");
        println!("Packages in system:   {}", config.system_packages.len());
        println!("Required packages:    {}", result.required_count);
        if pretend {
            println!("Number to remove:     {}", result.cleanlist.len());
        } else {
            println!("Number removed:       {}", result.cleanlist.len());
        }
    };

    if result.cleanlist.is_empty() {
        println!(">>> No packages selected for removal by depclean");
        // Real `create_cleanlist`: `if "--verbose" not in myopts`.
        if !verbose {
            println!(">>> To see reverse dependencies, use --verbose");
        }
        stats();
        return ExitCode::SUCCESS;
    }

    // This line only when there's something to remove; then the
    // per-package block renders through the shared unmerge path.
    // Portage: _emerge/actions.py, _calc_depclean
    println!(">>> Calculating removal order...");
    let cpv_atoms: Vec<String> = result
        .cleanlist
        .iter()
        .map(|p| format!("={}", p.cpv()))
        .collect();
    let cpv_refs: Vec<&str> = cpv_atoms.iter().map(String::as_str).collect();
    // Without `--pretend`, the shared unmerge path removes them.
    // Portage: _emerge/actions.py, action_depclean
    let unmerge_rc = run_unmerge_pretend(RunUnmergePretendArgs {
        targets: &cpv_refs,
        action: "unmerge",
        root,
        config_root,
        config,
        repos,
        preserve_order: result.ordered,
        pretend,
        ask,
        shell,
        debug,
        color,
        quiet,
        noinfo,
    });
    // The stats block prints after the removal returns. A mid-removal
    // failure still prints it here, then propagates the code: a harmless
    // cosmetic divergence on the error path only.
    stats();
    unmerge_rc
}
