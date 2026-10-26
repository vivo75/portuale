//! `--fetchonly` / `--fetch-all-uri`: download the sources of a merge list, or list where
//! they would come from under `--pretend`.

use crate::emerge_build::*;

/// Arguments of [`run_fetchonly`].
pub struct RunFetchonlyArgs<'a> {
    pub entries: &'a [GraphEntry],
    pub config: &'a portage_profile::Config,
    pub repos: &'a [RepoConfig],
    pub root: &'a Path,
    /// `$PKGDIR`, where a remote binary lands unless its binrepo sets `location`.
    pub pkgdir: &'a Path,
    pub portage_tmpdir: &'a Path,
    pub distdir: &'a Path,
    pub config_root: &'a Path,
    pub shell: ebuild_phases::ShellBackend,
    pub debug: bool,
    /// `--pretend`: list the URIs, fetch nothing.
    pub pretend: bool,
    /// `--fetch-all-uri`: every file of `SRC_URI`, whatever the USE set.
    pub all_uri: bool,
    /// `--keep-going`: a single failed package is summarised at the end too.
    pub keep_going: bool,
}

/// The `* Messages for package <cpv> merged to <root>:` summary of a package whose fetch
/// failed, printed after its `Failed to emerge` tail: it carries what `pkg_nofetch` logged.
// Portage: Scheduler.py (_failed_pkg_msg then elog_process via _elog_listener), mod_echo
fn print_fetch_failure_elog(
    env: &ebuild_phases::Environment,
    cpv: &str,
    root: &Path,
    features: &str,
    color: &crate::color::Colorizer,
) {
    crate::elog::process_batch(crate::elog::ProcessBatchArgs {
        logdir: &crate::elog::logdir(root),
        root_display: &root.display().to_string(),
        items: &[(cpv.to_string(), env.t())],
        phases: None,
        color,
        split_elog: features.split_whitespace().any(|t| t == "split-elog"),
    });
    ebuild_phases::remove_nofetch_scratch(env);
}

/// One remote binary entry of a fetch-only run.
struct RemoteBinaryJob<'a> {
    entry: &'a GraphEntry,
    config: &'a portage_profile::Config,
    root: &'a Path,
    pkgdir: &'a Path,
    pretend: bool,
    progress: mrg_director::MergeProgress,
    color: &'a crate::color::Colorizer,
}

impl RemoteBinaryJob<'_> {
    /// Lists (`--pretend`) or downloads the binary into its binrepo's `location` (else
    /// `$PKGDIR`); a binary already there is not fetched again but still verified.
    /// Returns `false` when the package failed.
    // Portage: _emerge/Binpkg.py (fetchonly arm), BinpkgFetcher.py, BinpkgVerifier.py
    fn run(&self, first_job: &mut bool) -> bool {
        let entry = self.entry;
        let Some(version) = entry_version(entry) else {
            return true;
        };
        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
        let local = portage_repo::BinaryIndex::from_pkgdir(self.pkgdir);
        let remote = mrg_director::RemoteBinhostIndex {
            config: self.config,
            root: self.root,
            local: &local,
        };
        use mrg_director::BinpkgIndex as _;
        let Some((binrepo_name, record)) = remote.metadata_with_source_instance(
            &entry.category,
            &entry.package,
            version,
            entry.build_id.as_deref(),
        ) else {
            eprintln!("emerge: {cpv}: not in any binhost `Packages` index");
            return false;
        };
        let Some(binrepo) = self.config.binrepos.iter().find(|b| b.name == binrepo_name) else {
            return true;
        };
        let rel = record
            .get("PATH")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}/{}-{version}.tbz2", entry.category, entry.package));
        let dir = crate::emerge_getbinpkg::binrepo_download_dir(
            self.config,
            binrepo,
            &crate::emerge_getbinpkg::eprefix_from_env(),
            self.pkgdir,
        );
        let dest = dir.join(&rel);
        let uri = format!("{}/{rel}", binrepo.sync_uri.trim_end_matches('/'));
        if self.pretend {
            if !dest.is_file() {
                println!();
                println!("{uri}");
            }
            return true;
        }
        if std::mem::take(first_job) {
            println!();
        }
        println!(
            "{}",
            fetching_line(entry, version, self.progress, self.root, self.color)
        );
        let fetched = if dest.is_file() {
            Ok(dest.clone())
        } else {
            crate::emerge_getbinpkg::download_and_verify(
                crate::emerge_getbinpkg::DownloadAndVerifyArgs {
                    sync_uri: &binrepo.sync_uri,
                    record: &record,
                    category: &entry.category,
                    package: &entry.package,
                    version,
                    pkgdir: &dir,
                    config: self.config,
                },
            )
        };
        match fetched {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let filter = crate::emerge_getbinpkg::checksum_filter_from_config(self.config);
                if let Some(names) = crate::emerge_getbinpkg::verified_digest_names_filtered(
                    &record,
                    filter.as_deref(),
                ) {
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();
                    crate::fetch::print_verified_names(&name, &names);
                }
                true
            }
            Err(e) => {
                if let crate::emerge_getbinpkg::DownloadError::MissingDigests { partial } = &e {
                    print!("{}", crate::binpkg::missing_digests_block(partial));
                } else {
                    eprintln!("!!! {e}");
                }
                print!(
                    "{}",
                    crate::emerge_getbinpkg::failed_pkg_msg(&cpv, self.root, None)
                );
                false
            }
        }
    }
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
        pkgdir,
        portage_tmpdir,
        distdir,
        config_root,
        shell,
        debug,
        pretend,
        all_uri,
        keep_going,
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
    // `(cpv:slot/sub::repo, ebuild scheduled for merge to 'root')` of every fetch failure.
    let mut failed_pkgs: Vec<String> = Vec::new();
    // The Scheduler's own blank line before the first job (Portage's `-f` output has two
    // blank lines after the resolution notice where a merge has one).
    let mut first_job = true;
    for (idx, entry) in entries.iter().enumerate() {
        if entry.source == CandidateSource::Binary && entry.remote_binary {
            let job = RemoteBinaryJob {
                entry,
                config,
                root,
                pkgdir,
                pretend,
                progress: progress[idx],
                color: &color,
            };
            if !job.run(&mut first_job) {
                failed = true;
            }
            continue;
        }
        if !scheduler_needs_build(entry) {
            continue;
        }
        let Some(version) = entry_version(entry) else {
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
                        let cpv = format!("{}/{}-{version}", entry.category, entry.package);
                        print!(
                            "{}",
                            crate::emerge_getbinpkg::failed_pkg_msg(&cpv, root, None)
                        );
                        print_fetch_failure_elog(&env, &cpv, root, &features, &color);
                        failed_pkgs.push(failed_pkg_label(entry, &cpv_slot, root));
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
            let cpv = format!("{}/{}-{version}", entry.category, entry.package);
            print!(
                "{}",
                crate::emerge_getbinpkg::failed_pkg_msg(&cpv, root, None)
            );
            print_fetch_failure_elog(&env, &cpv, root, &features, &color);
            failed_pkgs.push(failed_pkg_label(entry, &cpv_slot, root));
            failed = true;
        }
    }
    print_failed_pkgs_summary(&failed_pkgs, keep_going, &color);
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> crate::color::Colorizer {
        crate::color::Colorizer::new(false)
    }

    #[test]
    fn one_failure_is_summarised_only_under_keep_going() {
        let labels = vec![" (a/b-1:0/0::r, ebuild scheduled for merge to '/x')".to_string()];
        assert_eq!(failed_pkgs_summary(&labels, false, &plain()), "");
        assert_eq!(
            failed_pkgs_summary(&labels, true, &plain()),
            " * \n * The following package has failed to build, install, or execute postinst:\n \
             * \n *  (a/b-1:0/0::r, ebuild scheduled for merge to '/x')\n * \n"
        );
    }

    /// The `--fetchonly` success line for a remote binary names the same
    /// digests the merge path prints: both go through the shared selection
    /// the verifier checks, which is the filtered set Portage's
    /// `_display_success` prints after verifying.
    // Portage: `lib/_emerge/BinpkgVerifier.py`, `_display_success`
    #[test]
    fn fetchonly_success_line_uses_the_same_names() {
        let record: std::collections::HashMap<String, String> = [
            ("SIZE", "16"),
            ("RMD160", "r"),
            ("WHIRLPOOL", "w"),
            ("SHA512", "s"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(
            crate::emerge_getbinpkg::verified_digest_names_filtered(&record, None),
            Some(vec![
                "RMD160".to_string(),
                "SHA512".to_string(),
                "WHIRLPOOL".to_string(),
                "size".to_string()
            ])
        );
    }

    #[test]
    fn several_failures_wrap_the_heading_at_72_columns() {
        let labels = vec![" (a/b-1)".to_string(), " (c/d-1)".to_string()];
        assert_eq!(
            failed_pkgs_summary(&labels, false, &plain()),
            " * \n * The following 2 packages have failed to build, install, or execute\n \
             * postinst:\n * \n *  (a/b-1)\n *  (c/d-1)\n * \n"
        );
    }
}
