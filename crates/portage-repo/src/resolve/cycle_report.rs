//! What the circular-dependency report says about each cycle edge: the USE
//! flags responsible for it, test dependencies, masked `||` alternatives and
//! the search limit for USE changes.

use crate::*;

/// Default number of USE flags searched per cycle edge.
// Portage: resolver/circular_dependency.py, MAX_AFFECTING_USE
pub const CIRCULAR_MAX_USE_FLAGS_DEFAULT: usize = 10;

/// The search limit for USE changes: `PORTAGE_CIRCULAR_MAX_USE_FLAGS` when
/// it is an integer of at least 1, else the default. The second value is
/// the rejected setting, for the `!!! Invalid ...` warning.
// Portage: resolver/circular_dependency.py, _get_max_affecting_use
pub fn circular_max_use_flags(config: &portage_profile::Config) -> (usize, Option<String>) {
    let Some(value) =
        portage_profile::env_over_config_scalar(config, "PORTAGE_CIRCULAR_MAX_USE_FLAGS")
    else {
        return (CIRCULAR_MAX_USE_FLAGS_DEFAULT, None);
    };
    if value.is_empty() {
        return (CIRCULAR_MAX_USE_FLAGS_DEFAULT, None);
    }
    match python_int(&value) {
        Some(n) if n >= 1 => (usize::try_from(n).unwrap_or(usize::MAX), None),
        _ => (CIRCULAR_MAX_USE_FLAGS_DEFAULT, Some(value)),
    }
}

/// Python's `int(str)` for base 10: surrounding whitespace, one sign, and
/// single underscores between digits.
pub(crate) fn python_int(value: &str) -> Option<i128> {
    let s = value.trim();
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let n: i128 = digits.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

/// The package on the depending side of a cycle edge, with the metadata and
/// USE the report reads.
pub(crate) struct CycleParent {
    pub(crate) metadata: std::sync::Arc<HashMap<String, String>>,
    /// The USE in effect, autounmask USE changes included.
    pub(crate) use_flags: HashSet<String>,
}

impl CycleParent {
    /// Loads `cpv` from the repositories. `None` when it has no ebuild.
    pub(crate) fn load(
        cpv: &str,
        repos: &[RepoConfig],
        config: &portage_profile::Config,
        autounmask_use_changes: &[AutounmaskChange],
    ) -> Option<Self> {
        let (category, package, version) = split_cpv(cpv)?;
        let candidates = list_candidates(repos, &category, &package).ok()?;
        let candidate = candidates.iter().find(|c| c.version == version)?;
        let metadata = repo_aux_metadata(
            &candidate.repo_location,
            &category,
            &format!("{package}-{version}"),
        )
        .ok()?;
        let parent_str = candidate.match_str(&category, &package);
        let mut use_flags: HashSet<String> = effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &candidate.iuse,
            keywords: &candidate.keywords,
            candidate_str: &parent_str,
            build_id: candidate.build_id.as_deref(),
            category: &category,
            package: &package,
        })
        .as_ref()
        .clone();
        for ch in autounmask_use_changes {
            if matches_config_entry(&ch.atom, &parent_str, &category, &package) {
                for tok in ch.token.split_whitespace() {
                    if let Some(flag) = tok.strip_prefix('-') {
                        use_flags.remove(flag);
                    } else {
                        use_flags.insert(tok.trim_start_matches('+').to_string());
                    }
                }
            }
        }
        Some(Self {
            metadata,
            use_flags,
        })
    }

    fn get(&self, key: &str) -> &str {
        self.metadata.get(key).map(String::as_str).unwrap_or("")
    }

    /// The dependency string an edge with this priority label came from:
    /// `BDEPEND DEPEND` for build time, `RDEPEND` for run time, `PDEPEND`
    /// for post. `None` for an edge USE flags cannot influence.
    // Portage: resolver/circular_dependency.py, _dep_string
    pub(crate) fn dep_string(&self, priority_label: &str) -> Option<String> {
        if priority_label.starts_with("buildtime") {
            Some(format!("{} {}", self.get("BDEPEND"), self.get("DEPEND")))
        } else if priority_label.starts_with("runtime_post") {
            Some(self.get("PDEPEND").to_string())
        } else if priority_label.starts_with("runtime") {
            Some(self.get("RDEPEND").to_string())
        } else {
            None
        }
    }
}

/// The token of `dep` that pulls in `child_cpv`: the first non-blocker atom
/// on the child's `cat/pkg` that survives `dep` reduced with `use_flags`.
/// Compared later as the unevaluated token.
// Portage: resolver/circular_dependency.py, _parent_atom
fn parent_atom(
    dep_tokens: &[String],
    use_flags: &HashSet<String>,
    child_cpv: &str,
) -> Option<String> {
    let (ccat, cpkg, _) = split_cpv(child_cpv)?;
    let reduced = portage_use_reduce::use_reduce_flat(
        dep_tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .ok()?;
    reduced.into_iter().find(|t| {
        parse_dep_atom(t, "cycle_report").is_some_and(|a| {
            a.blocker == portage_dep::Blocker::None && a.category == ccat && a.package == cpkg
        })
    })
}

/// Whether `dep` still contains `atom` when reduced with `use_flags`. An
/// unparsable string counts as still depending, so no flag is blamed.
// Portage: resolver/circular_dependency.py, _depends_on
fn depends_on(dep_tokens: &[String], use_flags: &HashSet<String>, atom: &str) -> bool {
    portage_use_reduce::use_reduce_flat(
        dep_tokens,
        use_flags,
        portage_use_reduce::MatchMode::Normal,
    )
    .map_or(true, |reduced| reduced.iter().any(|t| t == atom))
}

fn toggled(use_flags: &HashSet<String>, flag: &str) -> HashSet<String> {
    let mut out = use_flags.clone();
    if !out.remove(flag) {
        out.insert(flag.to_string());
    }
    out
}

/// The USE flags of `parent` the edge to `child_cpv` is blamed on, sorted,
/// each with whether it is enabled: a flag the dependency goes away without,
/// or, when no single flag is enough but all together are, each flag that
/// keeps it alive alone.
// Portage: resolver/circular_dependency.py, _affecting_use
pub(crate) fn edge_affecting_use(
    parent: &CycleParent,
    child_cpv: &str,
    priority_label: &str,
) -> Vec<(String, bool)> {
    let Some(dep) = parent.dep_string(priority_label) else {
        return Vec::new();
    };
    let tokens: Vec<String> = dep.split_whitespace().map(str::to_string).collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let Some(atom) = parent_atom(&tokens, &parent.use_flags, child_cpv) else {
        return Vec::new();
    };
    let Some(affecting) = portage_dep::extract_affecting_use(&dep, &atom) else {
        return Vec::new();
    };
    let use_flags = &parent.use_flags;
    let mut flipped = use_flags.clone();
    for flag in &affecting {
        flipped = toggled(&flipped, flag);
    }
    let mut responsible: BTreeSet<String> = affecting
        .iter()
        .filter(|f| !depends_on(&tokens, &toggled(use_flags, f), &atom))
        .cloned()
        .collect();
    if !depends_on(&tokens, &flipped, &atom) {
        responsible.extend(
            affecting
                .iter()
                .filter(|f| depends_on(&tokens, &toggled(&flipped, f), &atom))
                .cloned(),
        );
    }
    responsible
        .into_iter()
        .map(|f| {
            let on = use_flags.contains(&f);
            (f, on)
        })
        .collect()
}

/// Per-edge facts of a cycle report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReportFacts {
    /// `edge_use[i]` belongs to the edge `cycle[i] -> cycle[(i + 1) % len]`:
    /// the responsible flags, sorted, each with whether it is enabled.
    pub edge_use: Vec<Vec<(String, bool)>>,
    /// Cycle members whose edge is blamed on `test`, sorted by cpv.
    pub test_dep_parents: Vec<String>,
    /// `(masked alternative, cycle member it could replace)`, sorted by
    /// member then alternative.
    pub masked_alternatives: Vec<(String, String)>,
}

/// Arguments of [`cycle_report_facts`].
pub struct CycleReportFactsArgs<'a> {
    pub cycle: &'a [String],
    /// [`cycle_edge_labels`] for `cycle`.
    pub edge_labels: &'a [String],
    pub repos: &'a [RepoConfig],
    pub config: &'a portage_profile::Config,
    pub autounmask_use_changes: &'a [AutounmaskChange],
    /// The target root, for the installed check of masked alternatives.
    pub root: &'a Path,
}

/// The facts the text report prints beside the cycle.
// Portage: resolver/circular_dependency.py, circular_dependency_handler
pub fn cycle_report_facts(args: CycleReportFactsArgs<'_>) -> CycleReportFacts {
    let CycleReportFactsArgs {
        cycle,
        edge_labels,
        repos,
        config,
        autounmask_use_changes,
        ..
    } = args;
    let n = cycle.len();
    let mut facts = CycleReportFacts::default();
    let mut test_parents: BTreeSet<String> = BTreeSet::new();
    let mut alternatives: BTreeSet<(String, String)> = BTreeSet::new();
    for i in 0..n {
        let parent_cpv = &cycle[i];
        let child_cpv = &cycle[(i + 1) % n];
        let label = edge_labels
            .get(i)
            .map(String::as_str)
            .unwrap_or("buildtime");
        let Some(parent) = CycleParent::load(parent_cpv, repos, config, autounmask_use_changes)
        else {
            facts.edge_use.push(Vec::new());
            continue;
        };
        let flags = edge_affecting_use(&parent, child_cpv, label);
        if flags.iter().any(|(f, _)| f == "test") {
            test_parents.insert(parent_cpv.clone());
        }
        facts.edge_use.push(flags);
        for alt in masked_alternatives_for_edge(&parent, child_cpv, label, &args) {
            alternatives.insert((child_cpv.clone(), alt));
        }
    }
    facts.test_dep_parents = test_parents.into_iter().collect();
    facts.masked_alternatives = alternatives
        .into_iter()
        .map(|(member, alt)| (alt, member))
        .collect();
    facts
}

/// One node of a reduced dependency string.
enum DepTree {
    Atom(String),
    AnyOf(Vec<DepTree>),
    AllOf(Vec<DepTree>),
}

/// Parses a reduced token stream (`||`, `(`, `)` and atoms) into a tree.
fn parse_dep_tree(tokens: &[String], pos: &mut usize) -> Vec<DepTree> {
    let mut out = Vec::new();
    while *pos < tokens.len() {
        let tok = tokens[*pos].as_str();
        *pos += 1;
        match tok {
            ")" => break,
            "(" => out.push(DepTree::AllOf(parse_dep_tree(tokens, pos))),
            "||" => {
                if tokens.get(*pos).map(String::as_str) == Some("(") {
                    *pos += 1;
                }
                out.push(DepTree::AnyOf(parse_dep_tree(tokens, pos)));
            }
            _ => out.push(DepTree::Atom(tok.to_string())),
        }
    }
    out
}

fn flatten_atoms<'a>(node: &'a DepTree, out: &mut Vec<&'a str>) {
    match node {
        DepTree::Atom(a) => out.push(a),
        DepTree::AnyOf(children) | DepTree::AllOf(children) => {
            for c in children {
                flatten_atoms(c, out);
            }
        }
    }
}

/// The atoms of every `|| ( )` group holding `atom`, except those of the
/// choice holding it.
// Portage: resolver/circular_dependency.py, _iter_any_of_alternatives
fn any_of_alternatives<'a>(nodes: &'a [DepTree], atom: &str, out: &mut Vec<&'a str>) {
    for node in nodes {
        match node {
            DepTree::Atom(_) => {}
            DepTree::AllOf(children) => any_of_alternatives(children, atom, out),
            DepTree::AnyOf(choices) => {
                let flat: Vec<Vec<&str>> = choices
                    .iter()
                    .map(|c| {
                        let mut v = Vec::new();
                        flatten_atoms(c, &mut v);
                        v
                    })
                    .collect();
                if flat.iter().any(|c| c.contains(&atom)) {
                    for c in flat.iter().filter(|c| !c.contains(&atom)) {
                        out.extend(c.iter().copied());
                    }
                }
                any_of_alternatives(choices, atom, out);
            }
        }
    }
}

/// Whether every mask on a candidate can be lifted by configuration:
/// keywords (`~arch` or missing), `package.mask` and licences.
// Portage: resolver/circular_dependency.py, _maskable_by_config
fn maskable_by_config(reasons: &[String]) -> bool {
    !reasons.is_empty()
        && reasons.iter().all(|r| {
            r == "package.mask"
                || r == "LICENSE"
                || r.ends_with(" license(s)")
                || r == "missing keyword"
                || (r.starts_with('~') && r.ends_with(" keyword"))
        })
}

/// The best masked package for each `||` alternative of the edge's atom
/// that has no installed or visible match and only config-liftable masks.
// Portage: resolver/circular_dependency.py, _find_masked_alternatives
fn masked_alternatives_for_edge(
    parent: &CycleParent,
    child_cpv: &str,
    priority_label: &str,
    args: &CycleReportFactsArgs<'_>,
) -> Vec<String> {
    let &CycleReportFactsArgs {
        repos,
        config,
        root,
        ..
    } = args;
    let Some(dep) = parent.dep_string(priority_label) else {
        return Vec::new();
    };
    let tokens: Vec<String> = dep.split_whitespace().map(str::to_string).collect();
    let Some(atom) = parent_atom(&tokens, &parent.use_flags, child_cpv) else {
        return Vec::new();
    };
    let Ok(reduced) = portage_use_reduce::use_reduce_structured(
        &tokens,
        &parent.use_flags,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return Vec::new();
    };
    let tree = parse_dep_tree(&reduced, &mut 0);
    let mut alt_atoms = Vec::new();
    any_of_alternatives(&tree, &atom, &mut alt_atoms);
    let mut out = Vec::new();
    for alt in alt_atoms {
        let Some(a) = parse_dep_atom(alt, "cycle_report") else {
            continue;
        };
        if a.blocker != portage_dep::Blocker::None {
            continue;
        }
        let installed: Vec<String> = installed_versions(root, &a.category, &a.package)
            .into_iter()
            .map(|v| format!("{}/{}-{v}", a.category, a.package))
            .collect();
        let installed_refs: Vec<&str> = installed.iter().map(String::as_str).collect();
        if portage_dep::match_from_list(alt, &installed_refs).is_some_and(|m| !m.is_empty()) {
            continue;
        }
        let Ok(cands) = list_candidates(repos, &a.category, &a.package) else {
            continue;
        };
        let matching: Vec<&Candidate> = cands
            .iter()
            .filter(|c| {
                let s = c.match_str(&a.category, &a.package);
                portage_dep::match_from_list(alt, &[s.as_str()]).is_some_and(|m| !m.is_empty())
            })
            .collect();
        if matching
            .iter()
            .any(|c| is_visible(c, &a.category, &a.package, config))
        {
            continue;
        }
        if let Some(best) = matching
            .into_iter()
            .filter(|c| {
                maskable_by_config(&candidate_masking_reasons(
                    c,
                    &a.category,
                    &a.package,
                    config,
                ))
            })
            .max_by(|x, y| vercmp_ordering(&x.version, &y.version))
        {
            out.push(format!("{}/{}-{}", a.category, a.package, best.version));
        }
    }
    out
}

/// The changes of one USE solution: `(flag, enabled)`.
pub type UseSolution = Vec<(String, bool)>;

/// One edge of the shortest cycle in a [`CycleReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleReportEdge {
    pub parent: String,
    pub child: String,
    /// The edge's last priority, as `str(priority)` prints it.
    pub priority: String,
    /// The USE flags the edge is blamed on, sorted, without their state.
    pub affecting_use: Vec<String>,
}

/// Everything a circular-dependency report says, before any rendering: the
/// text report and `--circular-deps-report=json` both read this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleReport {
    /// Every cycle of the graph, as `get_cycles` enumerates them.
    pub cycles: Vec<Vec<String>>,
    /// The first strictly shortest of `cycles`, as one edge per member:
    /// `edges[i]` goes from `shortest[i - 1]` to `shortest[i]`.
    pub shortest_cycle: Vec<CycleReportEdge>,
    /// Per package whose USE would change (sorted by cpv), the solutions
    /// (flag, enabled) in report order.
    pub solutions: Vec<(String, Vec<UseSolution>)>,
    pub test_dep_parents: Vec<String>,
    /// `(cycle member, masked alternatives)`, sorted.
    pub masked_alternatives: Vec<(String, Vec<String>)>,
    pub search_truncated: Vec<String>,
    pub max_affecting_use: usize,
}

/// Arguments of [`build_cycle_report`].
pub struct CycleReportArgs<'a> {
    /// [`GraphResult::report_cycles`]; when empty `fallback` stands in.
    pub cycles: &'a [Vec<String>],
    /// The cycle the resolver reported, used when `cycles` is empty.
    pub fallback: &'a [String],
    pub repos: &'a [RepoConfig],
    pub config: &'a portage_profile::Config,
    pub autounmask_use_changes: &'a [AutounmaskChange],
    pub entries: &'a [GraphEntry],
    pub root: &'a Path,
    pub max_affecting_use: usize,
}

/// The data of the circular-dependency report.
// Portage: resolver/circular_dependency.py, circular_dependency_handler
pub fn build_cycle_report(args: CycleReportArgs<'_>) -> CycleReport {
    let CycleReportArgs {
        cycles,
        fallback,
        repos,
        config,
        autounmask_use_changes,
        entries,
        root,
        max_affecting_use,
    } = args;
    let cycles: Vec<Vec<String>> = if cycles.is_empty() && !fallback.is_empty() {
        vec![fallback.to_vec()]
    } else {
        cycles.to_vec()
    };
    let mut report = CycleReport {
        max_affecting_use,
        ..CycleReport::default()
    };
    let mut shortest: &[String] = &[];
    for cycle in &cycles {
        if shortest.is_empty() || cycle.len() < shortest.len() {
            shortest = cycle;
        }
    }
    let shortest = shortest.to_vec();
    report.cycles = cycles;
    if shortest.is_empty() {
        return report;
    }
    let edge_labels = cycle_edge_labels(entries, &shortest);
    let facts = cycle_report_facts(CycleReportFactsArgs {
        cycle: &shortest,
        edge_labels: &edge_labels,
        repos,
        config,
        autounmask_use_changes,
        root,
    });
    let n = shortest.len();
    for pos in 0..n {
        let edge = (pos + n - 1) % n;
        report.shortest_cycle.push(CycleReportEdge {
            parent: shortest[edge].clone(),
            child: shortest[pos].clone(),
            priority: edge_labels[edge].clone(),
            affecting_use: facts
                .edge_use
                .get(edge)
                .map(|flags| flags.iter().map(|(f, _)| f.clone()).collect())
                .unwrap_or_default(),
        });
    }
    let search = circular_dep_search(CircularDepSearchArgs {
        cycle: &shortest,
        repos,
        config,
        autounmask_use_changes,
        entries,
        max_affecting_use,
    });
    // Suggestions arrive sorted by package then change set.
    for s in search.suggestions {
        match report.solutions.last_mut() {
            Some((pkg, sols)) if *pkg == s.parent_cpv => sols.push(s.changes),
            _ => report.solutions.push((s.parent_cpv, vec![s.changes])),
        }
    }
    report.search_truncated = search.search_truncated;
    report.test_dep_parents = facts.test_dep_parents;
    for (alternative, member) in facts.masked_alternatives {
        match report.masked_alternatives.last_mut() {
            Some((m, alts)) if *m == member => alts.push(alternative),
            _ => report.masked_alternatives.push((member, vec![alternative])),
        }
    }
    report
}
