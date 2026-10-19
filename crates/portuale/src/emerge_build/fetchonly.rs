//! `--fetchonly` / `--fetch-all-uri`: download the sources of a merge list, or list where
//! they would come from under `--pretend`.

use crate::emerge_build::*;

/// Arguments of [`run_fetchonly`].
pub struct RunFetchonlyArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a portage_profile::Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub distdir: &'a Path,
    pub config_root: &'a Path,
    pub shell: ebuild_phases::ShellBackend,
    pub debug: bool,
    /// `--pretend`: list the URIs, fetch nothing.
    pub pretend: bool,
    /// `--fetch-all-uri`: every file of `SRC_URI`, whatever the USE set.
    pub all_uri: bool,
}

/// Fetches (or lists) the sources of every source entry of the merge list, in order, and
/// returns the exit code: 1 when any package failed. A failure does not stop the others,
/// and nothing is written to the vdb, the world file or the resume list.
// Portage: _emerge/Scheduler.py (fetchonly), _emerge/EbuildBuild.py, EbuildFetchonly.py
pub fn run_fetchonly(args: RunFetchonlyArgs<'_>) -> i32 {
    let RunFetchonlyArgs {
        entries,
        config,
        repos,
        root,
        portage_tmpdir,
        distdir,
        config_root,
        shell,
        debug,
        pretend,
        all_uri,
    } = args;
    let run_wide = run_wide_phase_env(config);
    // `FEATURES=mirror` implies `--fetch-all-uri`.
    // Portage: Scheduler.py:192-193
    let all_uri = all_uri
        || config
            .resolved_incremental("FEATURES")
            .is_some_and(|f| f.iter().any(|t| t == "mirror"));
    let progress = merge_progress_map(entries);
    let color = progress_color();
    let mut failed = false;
    // The Scheduler's own blank line before the first job (Portage's `-f` output has two
    // blank lines after the resolution notice where a merge has one).
    let mut first_job = true;
    for (idx, entry) in entries.iter().enumerate() {
        if !scheduler_needs_build(entry) {
            continue;
        }
        let Some(version) = entry_version(&entry.outcome) else {
            continue;
        };
        let Some(candidate) = locate_candidate(repos, &entry.category, &entry.package, version)
        else {
            eprintln!(
                "emerge: {}/{}-{version}: could not locate its own ebuild file",
                entry.category, entry.package
            );
            failed = true;
            continue;
        };
        let path = ebuild_path(&candidate, &entry.category, &entry.package, version);
        let cpv_slot = entry_cpv_slot(entry, version);
        let build_env = buildpkgonly_entry_build_env(BuildpkgonlyEntryBuildEnvArgs {
            config,
            repos,
            entry,
            candidate: &candidate,
            cpv_slot: &cpv_slot,
            run_wide: &run_wide,
        });
        let env_value = |key: &str| {
            build_env
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map_or("", |(_, v)| v.as_str())
                .to_string()
        };
        let (use_flags, features) = (env_value("USE"), env_value("FEATURES"));
        let env = match ebuild_phases::compute_environment(&path, portage_tmpdir) {
            Ok(env) => env,
            Err(e) => {
                eprintln!(
                    "emerge: {}/{}-{version}: {e}",
                    entry.category, entry.package
                );
                failed = true;
                continue;
            }
        };
        let fetch_args = || ebuild_phases::FetchSourcesArgs {
            env: &env,
            root,
            distdir,
            debug,
            config_root,
            shell,
            features: &features,
            use_flags: &use_flags,
        };
        if pretend {
            match ebuild_phases::list_sources(fetch_args(), all_uri) {
                Ok(files) => {
                    // A file with no URI left (`RESTRICT=fetch` bars them all)
                    // fails the listing, and the caller then runs `pkg_nofetch`.
                    // Portage: fetch.py (`fetched != 2` under listonly),
                    // EbuildFetchonly.py, doebuild.py (bug 657360)
                    let mut unlisted = false;
                    for uris in files {
                        println!();
                        if uris.is_empty() {
                            unlisted = true;
                            println!();
                        } else {
                            println!("{} ", uris.join(" "));
                        }
                    }
                    if unlisted {
                        let restrict_fetch = ebuild_phases::package_restricts_fetch(&env);
                        if restrict_fetch {
                            eprint!(
                                "{}",
                                ebuild_phases::fetch_restriction_message(
                                    &entry.category,
                                    &format!("{}-{version}", entry.package)
                                )
                            );
                        }
                        if let Ok(runtime) = ebuild_phases::shared_runtime() {
                            runtime.block_on(ebuild_phases::run_nofetch(
                                ebuild_phases::RunNofetchArgs {
                                    env: &env,
                                    root,
                                    debug,
                                    config_root,
                                    shell,
                                    restrict_fetch,
                                },
                            ));
                        }
                        print!(
                            "{}",
                            crate::emerge_getbinpkg::failed_pkg_msg(
                                &format!("{}/{}-{version}", entry.category, entry.package),
                                root,
                                None
                            )
                        );
                        failed = true;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "emerge: {}/{}-{version}: {e}",
                        entry.category, entry.package
                    );
                    failed = true;
                }
            }
            continue;
        }
        if std::mem::take(&mut first_job) {
            println!();
        }
        println!(
            "{}",
            fetching_line(entry, version, progress[idx], root, &color)
        );
        let runtime = match ebuild_phases::shared_runtime() {
            Ok(runtime) => runtime,
            Err(e) => {
                eprintln!("emerge: {e}");
                return 1;
            }
        };
        if let Err(e) = runtime.block_on(ebuild_phases::fetch_sources(fetch_args())) {
            // `<file>: …`: Portage's own wording names the file only.
            // Portage: fetch.py, `Couldn't download '%s'. Aborting.`
            match e.split_once(": ") {
                // `RESTRICT=fetch` printed its notice instead (see `fetch_sources`).
                _ if ebuild_phases::package_restricts_fetch(&env) => {}
                Some((file, _)) if !file.contains(' ') => {
                    eprintln!("!!! Couldn't download '{file}'. Aborting.");
                }
                _ => eprintln!("!!! {e}"),
            }
            print!(
                "{}",
                crate::emerge_getbinpkg::failed_pkg_msg(
                    &format!("{}/{}-{version}", entry.category, entry.package),
                    root,
                    None
                )
            );
            failed = true;
        }
    }
    i32::from(failed)
}
