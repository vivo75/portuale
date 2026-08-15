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

/// Direct unit legs for the masking/visibility stack: each leg observes its
/// predicate's own result, no end-to-end contract reproduction.
mod tests_164 {
    use super::*;

    /// Arguments of [`entry_164`].
    struct Entry164Args<'a> {
        category: &'a str,
        package: &'a str,
        outcome: PretendOutcome,
        required_by: Vec<(String, String)>,
        repo: Option<&'a str>,
        source: CandidateSource,
    }

    /// Minimal `GraphEntry` for the dep-chain legs (merge-bound unless stated
    /// otherwise).
    fn entry_164(args: Entry164Args<'_>) -> GraphEntry {
        let Entry164Args {
            category,
            package,
            outcome,
            required_by,
            repo,
            source,
        } = args;
        GraphEntry {
            discovery: 0,
            category: category.to_string(),
            package: package.to_string(),
            outcome,
            blockers: Vec::new(),
            slot: Some("0".to_string()),
            sub_slot: Some("0".to_string()),
            repo_name: repo.map(str::to_string),
            oldbest: Vec::new(),
            use_flags_display: Vec::new(),
            use_expand_display: Vec::new(),
            use_expand_display_p: Vec::new(),
            keyword_mask: None,
            new_slot: false,
            interactive: false,
            fetch_restrict: false,
            fetch_restrict_satisfied: false,
            download_files: Vec::new(),
            required_by,
            source,
            provenance: VisibilityProvenance::default(),
            keyword_suggestion: None,
            use_suggestion: None,
            parent_use_suggestion: None,
            targets_running_root: false,
            remote_binary: false,
            build_id: None,
            deps: Vec::new(),
        }
    }

    fn new_164(package: &str, version: &str, parent: &str) -> GraphEntry {
        entry_164(Entry164Args {
            category: "dev-libs",
            package,
            outcome: PretendOutcome::New {
                version: version.to_string(),
            },
            required_by: vec![("dev-libs".to_string(), parent.to_string())],
            repo: Some("testrepo"),
            source: CandidateSource::Ebuild,
        })
    }
}

fn graph_entry(category: &str, package: &str, version: &str) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: Some("0".to_string()),
        sub_slot: Some("0".to_string()),
        repo_name: Some("testrepo".to_string()),
        oldbest: Vec::new(),
        use_flags_display: Vec::new(),
        use_expand_display: Vec::new(),
        use_expand_display_p: Vec::new(),
        keyword_mask: None,
        new_slot: false,
        interactive: false,
        fetch_restrict: false,
        fetch_restrict_satisfied: false,
        download_files: Vec::new(),
        required_by: Vec::new(),
        source: CandidateSource::Ebuild,
        provenance: VisibilityProvenance::default(),
        keyword_suggestion: None,
        use_suggestion: None,
        parent_use_suggestion: None,
        targets_running_root: false,
        remote_binary: false,
        build_id: None,
        deps: Vec::new(),
    }
}

mod tests_162;

mod tests_163;

mod tests_165;

mod tests_166;

mod blockers;
mod tests_184;
