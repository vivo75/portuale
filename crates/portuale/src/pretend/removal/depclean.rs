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
