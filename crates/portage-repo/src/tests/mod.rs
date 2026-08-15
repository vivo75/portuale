use super::*;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("fixtures/ does not resolve: it is a symlink to ../pmtest/fixtures, so the sibling pmtest repo has to be checked out next to this one")
}

/// These unit tests exercise portage-repo's own resolution logic in isolation
/// from profile/make.conf/package.* parsing (portage-profile's job, tested
/// separately): visibility is pinned to a fixed "amd64"-only, no-overrides
/// config, constructed directly (not via `portage_profile::resolve_config`).
fn test_config() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}

/// Arguments of [`euf_flat`].
struct EufFlatArgs<'a> {
    iuse: &'a str,
    use_tokens: &'a [String],
    package_use: &'a [(String, Vec<String>)],
    package_use_force: &'a [(String, Vec<String>)],
    package_use_mask: &'a [(String, Vec<String>)],
    use_force: &'a HashSet<String>,
    use_mask: &'a HashSet<String>,
    use_stable_force: &'a HashSet<String>,
    use_stable_mask: &'a HashSet<String>,
    package_use_stable_force: &'a [(String, Vec<String>)],
    package_use_stable_mask: &'a [(String, Vec<String>)],
    keywords: &'a [String],
    accept_keywords: &'a HashSet<String>,
    package_accept_keywords: &'a [(String, Vec<String>)],
    candidate_str: &'a str,
    category: &'a str,
    package: &'a str,
}

fn candidate(version: &str, keywords: &[&str]) -> Candidate {
    Candidate {
        version: version.to_string(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        slot: "0".to_string(),
        sub_slot: "0".to_string(),
        repo_location: PathBuf::new(),
        repo_priority: 0,
        repo_name: "test".to_string(),
        license: String::new(),
        iuse: String::new(),
        properties: String::new(),
        restrict: String::new(),
        source: CandidateSource::Ebuild,
        binary_use: None,
        remote: false,
        build_id: None,
        build_time: None,
        binary_deps: HashMap::new(),
    }
}

mod tests_162;

mod tests_163;

mod tests_165;

mod tests_166;

mod tests_184;
