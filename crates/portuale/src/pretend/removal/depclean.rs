//! `--depclean`.

use crate::pretend::*;

/// One cleanlist package `--depclean-lib-check` keeps installed: it
/// solely provides a library still needed at link level (`NEEDED.ELF.2`
/// soname) by a package that is *not* itself being removed.
pub(crate) struct LibConsumerProtection {
    pub(crate) provider: portage_repo::InstalledPackage,
    /// Surviving consumer cpv -> the sonames it needs from `provider`,
    /// sorted (real `sorted(libs)` / `sorted(consumer.mycpv ...)`).
    pub(crate) consumers: Vec<(String, Vec<String>)>,
}

/// Real `_calc_depclean`'s `--depclean-lib-check` scan (`actions.py:
/// 1381-1546`), narrowed to its pure computation. For every cleanlist
/// package, take the `NEEDED.ELF.2`-indexed objects it owns that carry a
/// `DT_SONAME`, and ask `needed_elf::find_consumers` (non-greedy, so a
/// consumer already satisfied by *another* provider of the same soname
/// is excluded -- real `actions.py:1454-1504`'s own multi-provider
/// filter) which surviving objects still link against them. A consumer
/// whose owning package is itself in the cleanlist is dropped (real
/// `lib_consumer in clean_set`).
///
/// **Documented narrowing**: real's multi-provider filter only lets an
/// *alternative* provider satisfy a consumer when that alternative's own
/// package is not also being removed; `find_consumers` is not clean-set
/// aware, so portuale can under-report in the (rare) case where the
/// only surviving provider of a soname is itself another cleanlist
/// member. Also real's intermediate `>>> Assigning files to packages...`
/// progress line can appear with no WARNING following (all consumers
/// satisfied elsewhere); portuale prints it only alongside the WARNING.
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
        // The objects this package owns that provide a soname (real
        // `pkg_dblink.getcontents()` ∩ the linkmap ∩ non-empty soname).
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
    // Real `sorted(consumer_map, key=cmp_sort_key(cmp_pkg_cpv))`.
    out.sort_by_key(|p| p.provider.cpv());
    out
}

/// Real `_calc_depclean`'s `--depclean-lib-check` phase (`actions.py:
/// 1356-1590`): run `lib_consumer_scan` on the tentative `result`, print
/// the `>>> Checking for lib consumers...` progress and, when any
/// provider must be kept, the `* ...one or more packages will not be
/// removed` WARNING (real `bad(" * ")` prefix, `logging.WARNING` ->
/// stderr) plus the per-provider `pulled in by:` / `needs <soname>`
/// detail, then hand the protected providers to `recompute` (a second
/// `depclean_cleanlist` / `prune_cleanlist` pass that seeds them as
/// roots so their own deps also leave the cleanlist). A no-op when
/// `lib_check` is off (`--depclean-lib-check=n`) or the cleanlist is
/// already empty.
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

    // Real: `"".join(bad(" * ") + f"{line}\n" for line in textwrap.wrap(
    // msg, 70))` -- the wrap is pinned here since it never changes.
    let star = color.c("BAD", " * ");
    for line in [
        "In order to avoid breakage of link level dependencies, one or more",
        "packages will not be removed. This can be solved by rebuilding the",
        "packages that pulled them in.",
    ] {
        eprintln!("{star}{line}");
    }
    // Real's second `msg` list: a blank ` * ` line, then `  <cpv> pulled
    // in by:` and `    <consumer> needs <sonames>` per provider, then a
    // trailing blank ` * ` line.
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

/// Real `_calc_depclean`'s `unresolved_deps()` halt (`actions.py:1177-1248`):
/// when a kept installed package has a hard runtime dependency no
/// installed package satisfies, depclean/prune print the `bad(" * ")`-
/// prefixed `Dependencies could not be completely resolved ...` block
/// (`logging.ERROR` -> stderr) and exit 1 without removing anything.
/// Returns `Some(exit 1)` when it halted, `None` to carry on. `is_prune`
/// adds the real prune-only `use --nodeps` trailer.
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
    // Real `textwrap.wrap(..., 65)` -- pinned, it never changes.
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
    /// The `repos.conf` repos -- threaded into the post-removal GLEP 42
    /// count notice (backlog #196 fix round 1).
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) verbose: bool,
    pub(crate) lib_check: bool,
    /// Real `action_depclean`'s `deselect = myopts.get("--deselect") !=
    /// "n"` -- `--depclean <atoms> --deselect=n` keeps the `world` set as
    /// a protection root (a world member named as an arg is kept). See
    /// `depclean_cleanlist`'s own `deselect` param.
    pub(crate) deselect: bool,
    /// `false` for a real `emerge --depclean` (no `--pretend`): after the
    /// preview, `run_unmerge_pretend` removes the cleanlist, and the stats
    /// block reads `Number removed:` instead of `Number to remove:`
    /// (real `action_depclean:908-911`).
    pub(crate) pretend: bool,
    pub(crate) ask: bool,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
    /// Threaded into the post-removal info-regen block (real `--quiet`
    /// suppression + resolved-config `noinfo`); computed by the caller.
    pub(crate) quiet: bool,
    pub(crate) noinfo: bool,
}

/// Real `emerge --pretend --depclean` / `-pc` (real `action_depclean` +
/// `_calc_depclean`, no package arguments): the packages nothing in
/// `@world` ∪ `@system` needs, at runtime, are the cleanlist. Real
/// `action_depclean` literally feeds its cleanlist to `unmerge(...,
/// "unmerge", cleanlist, ordered=ordered)`, so the per-package block
/// here is exactly `run_unmerge_pretend`'s (each cleanlist cpv passed as
/// an `=cat/pkg-ver` atom) -- and **without `--pretend` that same
/// `run_unmerge_pretend` really removes them** (`execute_unmerge` +
/// per-package `deselect_from_world`), after the safety halt and
/// `--depclean-lib-check` have already run. See
/// `portage_repo::depclean_cleanlist`'s own doc comment for the graph
/// and its documented narrowings.
///
/// `emerge -pc <atoms>` (the `--depclean <atoms>` narrowing): the world
/// "selected" plain atoms are dropped (the named packages get deselected
/// *and* removed), every other installed package becomes a protected
/// root, and the cleanlist is just the `args`-matched packages nothing
/// else needs -- see `depclean_cleanlist`'s own doc comment. Real
/// `action_depclean` only shows the `* ` advisory block with no args, so
/// this doesn't either.
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
    // Bare-name targets get their category from the vdb, then each atom
    // is checked against the vdb (real `action_depclean`, `:848-863`) --
    // shared with `--prune`.
    let args = match resolve_cleanup_args(targets, root, "depclean") {
        Ok(a) => a,
        Err(code) => return code,
    };

    // Real `action_depclean`'s own advisory block -- *only* with no
    // package arguments (`not myfiles`), non-quiet. The "Depclean may
    // break link level dependencies" first paragraph is real's own
    // `if "preserve-libs" not in features and --depclean-lib-check == "n"`
    // -- portuale has no preserve-libs feature, so it hinges purely on
    // `--depclean-lib-check=n` (`!lib_check`).
    if args.is_empty() {
        // Real `action_depclean`: each line is `colorize("WARN", " * ")`
        // (yellow) + text, and each backtick-wrapped command inside the
        // text is `good("`…`")` (green).
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
        // `None` = real's leading bare `writemsg_stdout("\n")`; `Some("")`
        // = real's `msg.append("\n")` (still `WARN(" * ")`-prefixed);
        // `Some(text)` = a ` * `-prefixed advisory line. The first
        // paragraph (`Depclean may break link level dependencies...`)
        // only when `--depclean-lib-check=n`.
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

    // @world = the `world` file (parent `@selected`, real `show_parents`)
    // plus each `world_sets` nested set's own atoms (parent `@<name>`) --
    // kept as `(atom, label)` pairs for the `--verbose` reverse-dep
    // display. `depclean_cleanlist` drops these seeds in `args` mode.
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
                    Ok(atoms) => {
                        let label = format!("@{name}");
                        world_seeds.extend(atoms.into_iter().map(|a| (a, label.clone())));
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
    // Real `_calc_depclean`'s `unresolved_deps()` safety halt
    // (`actions.py:1247`) -- checked before the lib scan.
    if let Some(code) = depclean_unresolved_halt(&result.unresolved, false, color) {
        return code;
    }
    // Real `_calc_depclean`'s `--depclean-lib-check` phase: a cleanlist
    // package still needed at link level by a survivor is kept (and its
    // own deps with it, via a second `depclean_cleanlist` pass seeding
    // the protected providers as roots).
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

    // Real `create_cleanlist`'s own `elif "--verbose": show_parents(pkg)`
    // -- the reverse-dep blocks come right after the `* ` advisory and
    // before `>>> Calculating removal order...` / the empty-cleanlist
    // message.
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

    // Real `_calc_depclean`: this line only when there's something to
    // remove; then `unmerge(..., "unmerge", cleanlist)` renders the
    // per-package block.
    println!(">>> Calculating removal order...");
    let cpv_atoms: Vec<String> = result
        .cleanlist
        .iter()
        .map(|p| format!("={}", p.cpv()))
        .collect();
    let cpv_refs: Vec<&str> = cpv_atoms.iter().map(String::as_str).collect();
    // Real `action_depclean`: `unmerge(root_config, myopts, "unmerge",
    // cleanlist, ordered=ordered)` -- the same `unmerge()` the `-C` path
    // uses. Without `--pretend`, `run_unmerge_pretend` removes them.
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
    // Real `action_depclean` prints the stats block after `unmerge()`
    // returns (`Number removed:` without `--pretend`). A mid-removal
    // failure real-`sys.exit`s before this; portuale still prints it
    // then propagates `unmerge_rc` -- a harmless cosmetic divergence on
    // the error path only.
    stats();
    unmerge_rc
}
