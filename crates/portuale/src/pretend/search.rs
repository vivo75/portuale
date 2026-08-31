//! `--list-sets` and `--search`.

use crate::pretend::*;

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
