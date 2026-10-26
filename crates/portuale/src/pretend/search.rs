//! `--list-sets` and `--search`.

use crate::pretend::*;

/// Every defined package-set name, sorted, one per line: the built-in
/// names plus every file under `<config_root>/etc/portage/sets/`.
/// Portage: _emerge/actions.py
pub(crate) fn run_list_sets(config_root: &Path) -> ExitCode {
    let mut names = defined_set_names(config_root);
    names.sort();
    names.dedup();
    for name in names {
        println!("{name}");
    }
    ExitCode::SUCCESS
}

/// The defined package-set names -- see `run_list_sets`. Also used by
/// `run_search`'s own set-name matching.
pub(crate) fn defined_set_names(config_root: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Some(text) = crate::ebuild_phases::package_sets_conf() {
        let mut section: Option<String> = None;
        let mut multiset = false;
        for line in text.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                if let Some(prev) = section.take()
                    && !multiset
                {
                    names.push(prev);
                }
                section = Some(name.to_string());
                multiset = false;
            } else if line
                .split_whitespace()
                .collect::<String>()
                .eq_ignore_ascii_case("multiset=true")
            {
                multiset = true;
            }
        }
        if let Some(prev) = section
            && !multiset
        {
            names.push(prev);
        }
    }
    if let Ok(entries) = portage_util::read_dir_entries(&config_root.join("etc/portage/sets")) {
        for e in entries {
            if e.path().is_file()
                && let Some(n) = e.file_name().to_str()
            {
                names.push(n.to_string());
            }
        }
    }
    names
}

/// Arguments of [`run_search`].
pub(crate) struct RunSearchArgs<'a> {
    pub(crate) terms: &'a [&'a str],
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a Path,
    /// The run-shared allow-list, built once by the caller.
    pub(crate) categories: &'a portage_repo::Categories,
    pub(crate) searchdesc: bool,
    pub(crate) verbose: bool,
    pub(crate) fuzzy: bool,
    pub(crate) regex_auto: bool,
    pub(crate) search_similarity: f64,
    pub(crate) color: &'a Colorizer,
}

/// Case-insensitive substring search of every `category/package` in the
/// configured repos (package-name half only, unless the key holds `/`),
/// plus defined set names. `-S` also matches `DESCRIPTION`. `--fuzzy-search`
/// (default on) lands near-misses at `--search-similarity`% (default 80);
/// `--regex-search-auto` (default on) matches regex-looking keys as regex.
/// V1 cuts: no search index, no binpkg results, an approximate
/// mask/keyword filter, no `Size of files`.
/// Portage: _emerge/actions.py, action_search; _emerge/search.py
pub(crate) fn run_search(args: RunSearchArgs<'_>) -> ExitCode {
    let RunSearchArgs {
        terms,
        repos,
        root,
        categories,
        searchdesc,
        verbose,
        fuzzy,
        regex_auto,
        search_similarity,
        color,
    } = args;
    if terms.is_empty() {
        println!("emerge: no search terms provided.");
        return ExitCode::SUCCESS;
    }

    let all_cp = portage_repo::all_cp(repos, &categories.set);
    let set_names = defined_set_names(&config_root_from_env());

    for term in terms {
        let forced_regex = term.starts_with('%');
        let key = term.trim_start_matches('%');
        let match_category = key.contains('/');
        let cutoff = search_similarity / 100.0;

        // `%`-forced, or auto-detected (a regex metacharacter present and
        // the key compiles). Fuzzy is disabled for a regex search.
        // Portage: _emerge/search.py
        let looks_regex =
            key.contains(['^', '$', '*', '[', ']', '{', '}', '|', '?']) || key.contains(".+");
        let re: Option<regex::Regex> = if forced_regex || (regex_auto && looks_regex) {
            regex::RegexBuilder::new(key)
                .case_insensitive(true)
                .build()
                .ok()
        } else {
            None
        };
        let use_fuzzy = fuzzy && re.is_none();

        // `re.search` (regex) or a plain case-insensitive substring.
        let needle = key.to_lowercase();
        let literal_or_re = |hay: &str| -> bool {
            match &re {
                Some(r) => r.is_match(hay),
                None => hay.to_lowercase().contains(&needle),
            }
        };
        // Fuzzy match: for a `cat/pkg` key the halves score independently
        // and both must pass.
        // Portage: _emerge/search.py, fuzzy_search_part
        let fuzzy_hit = |match_string: &str| -> bool {
            if !use_fuzzy {
                return false;
            }
            let (kparts, mparts): (Vec<&str>, Vec<&str>) = if match_category {
                (
                    key.splitn(2, '/').collect(),
                    match_string.splitn(2, '/').collect(),
                )
            } else {
                (vec![key], vec![match_string])
            };
            kparts
                .iter()
                .zip(mparts.iter())
                .all(|(k, m)| difflib::ratio(&m.to_lowercase(), &k.to_lowercase()) >= cutoff)
        };

        let mut hits: Vec<(String, bool)> = Vec::new(); // (cp, masked)
        for cp in &all_cp {
            let hay = if match_category {
                cp.as_str()
            } else {
                cp.rsplit('/').next().unwrap_or(cp)
            };
            let name_match = literal_or_re(hay) || fuzzy_hit(hay);
            let (cat, pkg) = cp.split_once('/').unwrap_or((cp.as_str(), ""));
            let cands = portage_repo::list_candidates(repos, cat, pkg).unwrap_or_default();
            let best = search_best_candidate(&cands);
            let desc_match = !name_match
                && searchdesc
                && best.as_ref().is_some_and(|c| {
                    portage_repo::repo_aux_metadata(
                        &c.repo_location,
                        cat,
                        &format!("{pkg}-{}", c.version),
                    )
                    .ok()
                    .and_then(|m| m.get("DESCRIPTION").cloned())
                    .is_some_and(|d| literal_or_re(&d))
                });
            if name_match || desc_match {
                let masked = !best.as_ref().is_some_and(search_candidate_visible);
                hits.push((cp.clone(), masked));
            }
        }
        // Set names match with the same regex (never fuzzy), against the
        // last `/` component unless matching categories.
        // Portage: _emerge/search.py
        let mut set_hits: Vec<String> = set_names
            .iter()
            .filter(|s| {
                let hay = if match_category {
                    s.as_str()
                } else {
                    s.rsplit('/').next().unwrap_or(s)
                };
                literal_or_re(hay)
            })
            .cloned()
            .collect();
        set_hits.sort();

        let star = color.c("GOOD", "*");
        print!("Searching...\n\n");
        print!(
            "\x08\x08  \n[ Results for search key : {} ]\n",
            color.c("bold", key)
        );
        let total = hits.len() + set_hits.len();
        for name in &set_hits {
            println!("{star}  {}", color.c("bold", name));
        }
        for (cp, masked) in &hits {
            let (cat, pkg) = cp.split_once('/').unwrap_or((cp.as_str(), ""));
            let cands = portage_repo::list_candidates(repos, cat, pkg).unwrap_or_default();
            let best = search_best_candidate(&cands);
            if *masked {
                println!(
                    "{star}  {} {}",
                    color.c("bold", cp),
                    color.c("BAD", "[ Masked ]")
                );
            } else {
                println!("{star}  {}", color.c("bold", cp));
            }
            if verbose && let Some(c) = &best {
                let meta = portage_repo::repo_aux_metadata(
                    &c.repo_location,
                    cat,
                    &format!("{pkg}-{}", c.version),
                )
                .unwrap_or_default();
                let g = |k: &str| color.c("darkgreen", k);
                let installed = portage_repo::installed_versions(root, cat, pkg);
                let inst = if installed.is_empty() {
                    "[ Not Installed ]".to_string()
                } else {
                    installed.join(" ")
                };
                println!("      {} {}", g("Latest version available:"), c.version);
                println!("      {} {inst}", g("Latest version installed:"));
                println!(
                    "      {}      {}",
                    g("Homepage:"),
                    meta.get("HOMEPAGE").map(String::as_str).unwrap_or("")
                );
                println!(
                    "      {}   {}",
                    g("Description:"),
                    meta.get("DESCRIPTION").map(String::as_str).unwrap_or("")
                );
                println!(
                    "      {}       {}\n",
                    g("License:"),
                    meta.get("LICENSE").map(String::as_str).unwrap_or("")
                );
            }
        }
        println!(
            "[ Applications found : {} ]\n",
            color.c("bold", &total.to_string())
        );
    }
    ExitCode::SUCCESS
}

/// The highest-version candidate, a version tie broken toward the
/// higher-priority repo: `run_search`'s "Latest version" pick.
/// Portage: portdbapi.xmatch
pub(crate) fn search_best_candidate(
    cands: &[portage_repo::Candidate],
) -> Option<portage_repo::Candidate> {
    cands
        .iter()
        .max_by(|a, b| {
            portage_versions::vercmp(&a.version, &b.version)
                .unwrap_or(0)
                .cmp(&0)
                .then_with(|| a.repo_priority.cmp(&b.repo_priority))
        })
        .cloned()
}

/// Approximate `bestmatch-visible` check: the candidate carries a
/// non-`-`-prefixed `KEYWORDS` token for the profile arch. Portage runs
/// the full mask/keyword visibility check instead.
pub(crate) fn search_candidate_visible(c: &portage_repo::Candidate) -> bool {
    c.keywords
        .iter()
        .any(|k| matches!(k.as_str(), "amd64" | "~amd64" | "*" | "~*" | "**"))
}

/// The ambiguous-short-name report: the same renderer fed a fixed package
/// list instead of a query match, always with the full metadata block.
/// Portage: _emerge/depgraph.py, ambiguous_package_name
pub(crate) fn render_ambiguous_search_output(
    key: &str,
    cps: &[String],
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    color: &Colorizer,
) {
    print!(
        "\x08\x08  \n[ Results for search key : {} ]\n",
        color.c("bold", key)
    );
    let star = color.c("GOOD", "*");
    for cp in cps {
        let (cat, pkg) = cp.split_once('/').unwrap_or((cp.as_str(), ""));
        let cands = portage_repo::list_candidates(repos, cat, pkg).unwrap_or_default();
        let best = search_best_candidate(&cands);
        let masked = !best.as_ref().is_some_and(search_candidate_visible);
        if masked {
            println!(
                "{star}  {} {}",
                color.c("bold", cp),
                color.c("BAD", "[ Masked ]")
            );
        } else {
            println!("{star}  {}", color.c("bold", cp));
        }
        if let Some(c) = &best {
            let meta = portage_repo::repo_aux_metadata(
                &c.repo_location,
                cat,
                &format!("{pkg}-{}", c.version),
            )
            .unwrap_or_default();
            let g = |k: &str| color.c("darkgreen", k);
            let installed = portage_repo::installed_versions(root, cat, pkg);
            let inst = if installed.is_empty() {
                "[ Not Installed ]".to_string()
            } else {
                installed.join(" ")
            };
            println!("      {} {}", g("Latest version available:"), c.version);
            println!("      {} {inst}", g("Latest version installed:"));
            println!(
                "      {}      {}",
                g("Homepage:"),
                meta.get("HOMEPAGE").map(String::as_str).unwrap_or("")
            );
            println!(
                "      {}   {}",
                g("Description:"),
                meta.get("DESCRIPTION").map(String::as_str).unwrap_or("")
            );
            println!(
                "      {}       {}\n",
                g("License:"),
                meta.get("LICENSE").map(String::as_str).unwrap_or("")
            );
        }
    }
    println!(
        "[ Applications found : {} ]\n",
        color.c("bold", &cps.len().to_string())
    );
    eprintln!("!!! The short ebuild name \"{key}\" is ambiguous. Please specify");
    eprintln!("!!! one of the above fully-qualified ebuild names instead.\n");
}
