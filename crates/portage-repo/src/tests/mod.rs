use super::*;

use portage_util::TempDir;

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

/// Like `resolve_update`, but with `excluded` too (`--exclude` beats `--update`).
fn resolve_update_excluded(category: &str, package: &str, excluded: &[String]) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &test_config(),
            local_binpkg: &build_local_binpkg_index(&test_config()),
        },
        &atom_str,
        &SelectOptions {
            update: true,
            excluded,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Unlike `resolve()`, uses the fixture's fully resolved config
/// (package.mask/.unmask/.accept_keywords included), not the
/// synthetic `test_config()`.
fn resolve_real(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve_real`, but with `--newuse` enabled -- for the
/// `PretendOutcome::Reinstall` tests below.
fn resolve_real_newuse(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            newuse: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Like `resolve_real`, but with `--changed-use` enabled.
fn resolve_real_changed_use(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_use: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_changed_deps(category: &str, package: &str, with_bdeps: bool) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_deps: true,
            with_bdeps,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_changed_slot(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            changed_slot: true,
            is_top_level: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

fn resolve_real_newrepo(category: &str, package: &str) -> PretendOutcome {
    let root = fixtures_root();
    let repos = find_repos(&root).expect("fixture repos.conf must resolve");
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &root,
        main_repo_location: &root.join("repo"),
        overlay_repos: &[("overlay".to_string(), root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: &root,
    })
    .expect("fixture config resolves");
    let atom_str = format!("{category}/{package}");
    resolve_pretend(
        &SelectSource {
            repos: &repos,
            root: &root,
            config: &config,
            local_binpkg: &build_local_binpkg_index(&config),
        },
        &atom_str,
        &SelectOptions {
            is_top_level: true,
            newrepo: true,
            ..SelectOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("resolve_pretend({atom_str}) failed: {e}"))
}

/// Shared builders: a synthetic `SlotConflict` over
/// `dev-libs/slotconflicttarget:0` from `(version, installed,
/// [(parent_cpv, atom, parent_installed)])` rows, plus merge `New`
/// entries in walk order for the `or_tuple` first-pulled rule.
type S2Row<'x> = (&'x str, bool, Vec<(&'x str, &'x str, bool)>);

fn s2_conflict(rows: Vec<S2Row<'_>>) -> SlotConflict {
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "slotconflicttarget".to_string(),
        slot: "0".to_string(),
        resolved_version: rows[0].0.to_string(),
        conflicting_atom: rows
            .get(1)
            .map(|r| {
                r.2.first()
                    .map(|(_, a, _)| a.to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default(),
        instances: rows
            .into_iter()
            .map(|(v, installed, parents)| SlotConflictInstance {
                version: v.to_string(),
                sub_slot: "0".to_string(),
                repo_name: "testrepo".to_string(),
                use_display: Vec::new(),
                parents: parents
                    .into_iter()
                    .map(|(cpv, atom, pinst)| SlotConflictParent {
                        parent_cpv: cpv.to_string(),
                        atom: atom.to_string(),
                        use_display: Vec::new(),
                        installed: pinst,
                    })
                    .collect(),
                installed,
            })
            .collect(),
    }
}

/// The `souprov` slot conflict for the solvable legs: 1.0 and 2.0
/// pulled, 2.0 resolved.
fn souprov_conflict_161() -> SlotConflict {
    let inst = |version: &str, sub: &str| SlotConflictInstance {
        version: version.to_string(),
        sub_slot: sub.to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: Vec::new(),
        installed: false,
    };
    SlotConflict {
        category: "dev-libs".to_string(),
        package: "souprov".to_string(),
        slot: "0".to_string(),
        resolved_version: "2.0".to_string(),
        conflicting_atom: ">=dev-libs/souprov-2.0".to_string(),
        instances: vec![inst("2.0", "2"), inst("1.0", "1")],
    }
}

/// Arguments of [`run_161`].
struct Run161Args<'a> {
    dir: &'a Path,
    atoms: &'a [String],
    update: bool,
    installed: &'a [(&'a str, &'a str, &'a str)],
    newuse: bool,
    usepkgonly: bool,
    nodeps: bool,
    selective: bool,
    usepkg: bool,
    deep: Deep,
    with_bdeps: bool,
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

/// Test shim for the pre-"Config depth" positional `effective_use_flags`
/// signature (single flat `package_use` list). Tests that exercise the split
/// call `effective_use_flags` directly.
fn euf_flat(args: EufFlatArgs<'_>) -> HashSet<String> {
    let EufFlatArgs {
        iuse,
        use_tokens,
        package_use,
        package_use_force,
        package_use_mask,
        use_force,
        use_mask,
        use_stable_force,
        use_stable_mask,
        package_use_stable_force,
        package_use_stable_mask,
        keywords,
        accept_keywords,
        package_accept_keywords,
        candidate_str,
        category,
        package,
    } = args;
    let config = portage_profile::Config {
        use_tokens: use_tokens.to_vec(),
        package_use: package_use.to_vec(),
        package_use_force: package_use_force.to_vec(),
        package_use_mask: package_use_mask.to_vec(),
        use_force: use_force.clone(),
        use_mask: use_mask.clone(),
        use_stable_force: use_stable_force.clone(),
        use_stable_mask: use_stable_mask.clone(),
        package_use_stable_force: package_use_stable_force.to_vec(),
        package_use_stable_mask: package_use_stable_mask.to_vec(),
        accept_keywords: accept_keywords.clone(),
        package_accept_keywords: package_accept_keywords.to_vec(),
        // `effective_use_flags`/`forced_or_masked_flags` now read the
        // per-level view; collapse every flat input into one
        // synthetic level (global-then-package, same as the old flat
        // application).
        use_mask_force_levels: vec![portage_profile::UseMaskForceLevel {
            use_mask: use_mask.iter().cloned().collect(),
            use_force: use_force.iter().cloned().collect(),
            use_stable_mask: use_stable_mask.iter().cloned().collect(),
            use_stable_force: use_stable_force.iter().cloned().collect(),
            package_use_mask: package_use_mask.to_vec(),
            package_use_force: package_use_force.to_vec(),
            package_use_stable_mask: package_use_stable_mask.to_vec(),
            package_use_stable_force: package_use_stable_force.to_vec(),
        }],
        ..Default::default()
    };
    // Test helper keeps the owned return; the `Rc` is unwrapped here.
    effective_use_flags(EffectiveUseFlagsArgs {
        config: &config,
        iuse,
        keywords,
        candidate_str,
        category,
        package,
    })
    .as_ref()
    .clone()
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

fn license_tokens(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

/// Direct unit legs for the masking/visibility stack: each leg observes its
/// predicate's own result, no end-to-end contract reproduction.
mod tests_164 {
    use super::*;

    /// Hand-built binary candidate: `binary_use` is the baked USE set
    /// (Portage's `pkg.use.enabled`), `iuse` its domain.
    fn binary_164(version: &str, iuse: &str, use_flags: &[&str]) -> Candidate {
        let mut c = candidate(version, &["amd64"]);
        c.source = CandidateSource::Binary;
        c.iuse = iuse.to_string();
        c.binary_use = Some(use_flags.iter().map(|s| s.to_string()).collect());
        c
    }

    #[test]
    fn keyword_masked_only_164_mask_unmask_interplay() {
        let rescued = candidate("1.0", &["~amd64"]);
        // In `package.mask` but rescued by `package.unmask`: the mask
        // gate stays shut, so a keyword-masked candidate still
        // counts as keyword-masked only ...
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/kwunmask".to_string()],
            package_unmask: vec!["dev-libs/kwunmask".to_string()],
            ..Default::default()
        };
        assert!(keyword_masked_only(
            &rescued, "dev-libs", "kwunmask", &config
        ));
        // ... while a mask with no unmask masks outright, whatever
        // the keywords say.
        let config_masked = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/kwmasked".to_string()],
            ..Default::default()
        };
        assert!(!keyword_masked_only(
            &rescued,
            "dev-libs",
            "kwmasked",
            &config_masked
        ));
    }

    /// A candidate masked by `package.mask` alone (Portage's first
    /// `_getmaskingstatus` reason).
    #[test]
    fn mask_masked_only_164_needs_a_mask_and_nothing_else() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/maskonly".to_string()],
            ..Default::default()
        };
        let masked = candidate("1.0", &["amd64"]);
        assert!(mask_masked_only(&masked, "dev-libs", "maskonly", &config));
        // No mask entry at all: not mask-masked, however visible.
        assert!(!mask_masked_only(
            &masked,
            "dev-libs",
            "maskabsent",
            &config
        ));
    }

    #[test]
    fn mask_masked_only_164_false_when_another_reason_blocks() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            package_mask: vec![
                "dev-libs/maskkw".to_string(),
                "dev-libs/masklic".to_string(),
                "dev-libs/maskprops".to_string(),
                "dev-libs/maskrestrict".to_string(),
            ],
            ..Default::default()
        };
        // Masked AND keyword-rejected.
        let kw = candidate("1.0", &["~amd64"]);
        assert!(!mask_masked_only(&kw, "dev-libs", "maskkw", &config));
        // Masked AND license-unaccepted.
        let lic = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(&lic, "dev-libs", "masklic", &config));
        // Masked AND PROPERTIES-unaccepted.
        let props = Candidate {
            properties: "interactive".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(&props, "dev-libs", "maskprops", &config));
        // Masked AND RESTRICT-unaccepted.
        let restrict = Candidate {
            restrict: "fetch".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!mask_masked_only(
            &restrict,
            "dev-libs",
            "maskrestrict",
            &config
        ));
    }

    #[test]
    fn mask_masked_only_164_unmask_cancels_the_mask() {
        // Masked AND unmasked: `package.unmask` cancels the mask (Portage's
        // unmask half), so the candidate is not mask-masked at all.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            package_mask: vec!["dev-libs/maskrescue".to_string()],
            package_unmask: vec!["dev-libs/maskrescue".to_string()],
            ..Default::default()
        };
        assert!(!mask_masked_only(
            &candidate("1.0", &["amd64"]),
            "dev-libs",
            "maskrescue",
            &config
        ));
    }

    /// A candidate masked by LICENSE alone (Portage's `LicenseManager`).
    #[test]
    fn license_masked_only_164_needs_a_license_and_only_a_license() {
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        // GPL-2 unaccepted, keywords fine: license-masked only.
        let lic_only = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(
            &lic_only, "dev-libs", "liconly", &config
        ));
        // License accepted: not license-masked at all.
        let accepted = Candidate {
            license: "MIT".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!license_masked_only(
            &accepted, "dev-libs", "licok", &config
        ));
    }

    #[test]
    fn license_masked_only_164_mask_unmask_leaves_the_license_verdict() {
        // In `package.mask` but rescued by `package.unmask`: the
        // mask gate stays shut, so the license verdict still shows.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            package_mask: vec!["dev-libs/licunmask".to_string()],
            package_unmask: vec!["dev-libs/licunmask".to_string()],
            ..Default::default()
        };
        let lic = Candidate {
            license: "GPL-2".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(&lic, "dev-libs", "licunmask", &config));
    }

    #[test]
    fn license_masked_only_164_use_conditional_follows_the_flag() {
        // Portage `use_reduce`'s own "if '?' in license_str" path: the
        // `foo? ( ... )` group only constrains once `foo` is enabled.
        // `+foo` in IUSE is enabled by default, no profile needed.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        // MIT accepted but the active conditional pulls in an
        // unaccepted GPL-2: license-masked only.
        let cond = Candidate {
            license: "MIT foo? ( GPL-2 )".to_string(),
            iuse: "+foo".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(license_masked_only(&cond, "dev-libs", "liccond", &config));
        // Same shape with the flag default-off: the group drops out,
        // MIT alone is accepted -- not license-masked.
        let cond_off = Candidate {
            license: "MIT foo? ( GPL-2 )".to_string(),
            iuse: "-foo".to_string(),
            ..candidate("1.0", &["amd64"])
        };
        assert!(!license_masked_only(
            &cond_off,
            "dev-libs",
            "liccondoff",
            &config
        ));
    }

    #[test]
    fn license_masked_only_164_invalid_is_never_license_only() {
        // `invalid` disqualifies the claim even with a genuinely unaccepted
        // LICENSE.
        let config = portage_profile::Config {
            accept_keywords: HashSet::from(["amd64".to_string()]),
            accept_license: vec!["MIT".to_string()],
            ..Default::default()
        };
        let mut invalid = binary_164("1.0", "", &[]);
        invalid.license = "GPL-2".to_string();
        invalid.binary_deps = HashMap::from([(
            "RDEPEND".to_string(),
            "bogusflag? ( dev-libs/foo )".to_string(),
        )]);
        assert!(!license_masked_only(
            &invalid,
            "dev-libs",
            "licinvalid",
            &config
        ));
    }

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
mod candidates;
mod graph;
mod license;
mod masked_deps;
mod pretend;
mod queue;
mod reinstall;
mod tests_184;
mod use_flags;
mod visibility;
