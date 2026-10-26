//! The per-edge facts of the circular-dependency report. Expected values are
//! Portage captures of the same report shapes.

use super::*;

fn fixture_config() -> (portage_profile::Config, Vec<RepoConfig>) {
    let root = fixtures_root();
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
    let repos = find_repos(&root).expect("repos");
    (config, repos)
}

/// The facts for the one cycle `atom` resolves to, keyed by the edge's
/// depending package: `(owner cpv, flags)` for every edge.
fn facts_for(atom: &str) -> (Vec<String>, CycleReportFacts) {
    let (config, repos) = fixture_config();
    let result = graph_result_real(atom);
    assert_eq!(result.circular_deps.len(), 1, "{atom}: one cycle");
    let cycle = result.circular_deps[0].clone();
    let labels = cycle_edge_labels(&result.entries, &cycle);
    let root = fixtures_root();
    let facts = cycle_report_facts(CycleReportFactsArgs {
        cycle: &cycle,
        edge_labels: &labels,
        repos: &repos,
        config: &config,
        autounmask_use_changes: &result.autounmask_use_changes,
        root: &root,
    });
    (cycle, facts)
}

fn edge_use_of(cycle: &[String], facts: &CycleReportFacts, owner: &str) -> Vec<(String, bool)> {
    let i = cycle
        .iter()
        .position(|c| c == owner)
        .expect("owner in cycle");
    facts.edge_use[i].clone()
}

fn flags(list: &[(&str, bool)]) -> Vec<(String, bool)> {
    list.iter().map(|(f, on)| (f.to_string(), *on)).collect()
}

#[test]
fn edge_use_names_the_enabled_flag_that_pulls_the_dependency_in() {
    let (cycle, facts) = facts_for("dev-libs/usecyclea");
    assert_eq!(
        edge_use_of(&cycle, &facts, "dev-libs/usecyclea-1.0"),
        flags(&[("x", true)])
    );
    assert_eq!(
        edge_use_of(&cycle, &facts, "dev-libs/usecycleb-1.0"),
        flags(&[])
    );
    assert!(facts.test_dep_parents.is_empty());
    assert!(facts.masked_alternatives.is_empty());
}

#[test]
fn edge_use_names_a_disabled_flag_under_a_negated_conditional() {
    let (cycle, facts) = facts_for("dev-libs/cddnega");
    assert_eq!(
        edge_use_of(&cycle, &facts, "dev-libs/cddnega-1"),
        flags(&[("system-b", false)])
    );
}

#[test]
fn edge_use_blames_nothing_when_the_dependency_is_also_unconditional() {
    let (_, facts) = facts_for("dev-libs/cddunra");
    assert!(facts.edge_use.iter().all(Vec::is_empty), "{facts:?}");
}

#[test]
fn edge_use_blames_every_flag_when_only_all_of_them_together_drop_it() {
    let (cycle, facts) = facts_for("dev-libs/cddseva");
    assert_eq!(
        edge_use_of(&cycle, &facts, "dev-libs/cddseva-1"),
        flags(&[("doc", true), ("nls", true)])
    );
}

#[test]
fn a_test_flag_edge_names_its_owner_for_the_features_test_pointer() {
    let (_, facts) = facts_for("dev-libs/cddtsta");
    assert_eq!(
        facts.test_dep_parents,
        vec!["dev-libs/cddtsta-1".to_string()]
    );
    let (_, facts) = facts_for("dev-libs/cddtdoa");
    assert_eq!(
        facts.test_dep_parents,
        vec!["dev-libs/cddtdoa-1".to_string()]
    );
}

#[test]
fn masked_bootstrap_alternatives_are_listed_when_config_can_unmask_them() {
    for (atom, alt, member) in [
        (
            "dev-lang/cddgo",
            "dev-lang/cddgo-bootstrap-1",
            "dev-lang/cddgo-2",
        ),
        (
            "dev-lang/cddgou",
            "dev-lang/cddgou-bootstrap-1",
            "dev-lang/cddgou-2",
        ),
        // `-amd64` alone reads as a missing keyword, which config can lift.
        (
            "dev-lang/cddgok",
            "dev-lang/cddgok-bootstrap-1",
            "dev-lang/cddgok-2",
        ),
        (
            "dev-lang/cddgop",
            "dev-lang/cddgop-bootstrap-1",
            "dev-lang/cddgop-2",
        ),
    ] {
        let (_, facts) = facts_for(atom);
        assert_eq!(
            facts.masked_alternatives,
            vec![(alt.to_string(), member.to_string())],
            "{atom}"
        );
    }
}

#[test]
fn the_use_change_search_gives_up_beyond_the_limit_and_runs_when_raised() {
    let (config, repos) = fixture_config();
    let result = graph_result_real("dev-libs/cddtrna");
    let cycle = &result.circular_deps[0];
    let search = |max| {
        circular_dep_search(CircularDepSearchArgs {
            cycle,
            repos: &repos,
            config: &config,
            autounmask_use_changes: &result.autounmask_use_changes,
            entries: &result.entries,
            max_affecting_use: max,
        })
    };
    let capped = search(CIRCULAR_MAX_USE_FLAGS_DEFAULT);
    assert!(capped.suggestions.is_empty());
    assert_eq!(
        capped.search_truncated,
        vec!["dev-libs/cddtrna-1".to_string()]
    );
    let raised = search(16);
    assert!(raised.search_truncated.is_empty());
    assert_eq!(raised.suggestions.len(), 1);
    assert_eq!(raised.suggestions[0].changes.len(), 12);
    assert!(raised.suggestions[0].changes.iter().all(|(_, on)| !on));
}

#[test]
fn the_search_limit_parses_like_python_int() {
    assert_eq!(python_int("16"), Some(16));
    assert_eq!(python_int(" +1_6 "), Some(16));
    assert_eq!(python_int("0"), Some(0));
    assert_eq!(python_int("-3"), Some(-3));
    assert_eq!(python_int("abc"), None);
    assert_eq!(python_int("1__6"), None);
    assert_eq!(python_int("_16"), None);
    assert_eq!(python_int(""), None);
}

/// The data step of `--circular-deps-report=json` for one atom.
fn report_for(atom: &str, max: usize) -> CycleReport {
    let (config, repos) = fixture_config();
    let result = graph_result_real(atom);
    build_cycle_report(CycleReportArgs {
        cycles: &result.report_cycles,
        fallback: &result.circular_deps[0],
        repos: &repos,
        config: &config,
        autounmask_use_changes: &result.autounmask_use_changes,
        entries: &result.entries,
        root: &fixtures_root(),
        max_affecting_use: max,
    })
}

/// Portage's JSON report for the same USE-cycle fixture.
#[test]
fn json_report_lists_both_rotations_and_the_flag_solution() {
    let r = report_for("dev-libs/usecyclea", CIRCULAR_MAX_USE_FLAGS_DEFAULT);
    let a = "dev-libs/usecyclea-1.0".to_string();
    let b = "dev-libs/usecycleb-1.0".to_string();
    assert_eq!(
        r.cycles,
        vec![vec![b.clone(), a.clone()], vec![a.clone(), b.clone()]]
    );
    // The shortest cycle is the first one: edge 0 is the wrap-around a -> b.
    let edge = |p: &str, c: &str, flags: &[&str]| CycleReportEdge {
        parent: p.to_string(),
        child: c.to_string(),
        priority: "buildtime".to_string(),
        affecting_use: flags.iter().map(|f| f.to_string()).collect(),
    };
    assert_eq!(
        r.shortest_cycle,
        vec![edge(&a, &b, &["x"]), edge(&b, &a, &[])]
    );
    assert_eq!(r.solutions, vec![(a, vec![flags(&[("x", false)])])]);
    assert!(r.test_dep_parents.is_empty() && r.search_truncated.is_empty());
    assert_eq!(r.max_affecting_use, 10);
}

#[test]
fn json_report_carries_test_parents_masked_alternatives_and_truncation() {
    let r = report_for("dev-libs/cddtsta", 10);
    assert_eq!(r.test_dep_parents, vec!["dev-libs/cddtsta-1".to_string()]);
    assert_eq!(
        r.solutions,
        vec![(
            "dev-libs/cddtsta-1".to_string(),
            vec![flags(&[("test", false)])]
        )]
    );
    let r = report_for("dev-lang/cddgo", 10);
    assert_eq!(r.cycles, vec![vec!["dev-lang/cddgo-2".to_string()]]);
    assert_eq!(r.shortest_cycle.len(), 1);
    assert_eq!(r.shortest_cycle[0].parent, r.shortest_cycle[0].child);
    assert_eq!(
        r.masked_alternatives,
        vec![(
            "dev-lang/cddgo-2".to_string(),
            vec!["dev-lang/cddgo-bootstrap-1".to_string()]
        )]
    );
    let r = report_for("dev-libs/cddtrna", 10);
    assert!(r.solutions.is_empty());
    assert_eq!(r.search_truncated, vec!["dev-libs/cddtrna-1".to_string()]);
    let r = report_for("dev-libs/cddseva", 10);
    assert_eq!(
        r.solutions,
        vec![(
            "dev-libs/cddseva-1".to_string(),
            vec![flags(&[("doc", false), ("nls", false)])]
        )]
    );
}
