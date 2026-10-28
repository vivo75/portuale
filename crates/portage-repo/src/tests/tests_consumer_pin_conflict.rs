//! A contradicted installed-consumer pin is a slot conflict that backtracks
//! over every choice, as Portage's `_slot_confict_backtrack` does.
//!
//! Expected values come from host Portage 3.0.83 (`emerge --debug` traces and
//! the Portage resolver's own backtracking log on scratch copies of the
//! fixtures), cited per test. `D` = `lib/_emerge/depgraph.py`,
//! `B` = `lib/_emerge/resolver/backtracking.py`.

use super::*;

/// A scratch ROOT: the fixtures' `var/` (vdb and world) with `world` atoms
/// appended, beside an empty `distfiles/`. Repositories and config stay the
/// shared fixtures'.
pub(super) fn hermetic_root(world: &[&str]) -> TempDir {
    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(".pkg.portage_lockfile") {
                continue;
            }
            let kind = entry.file_type().unwrap();
            let dest = to.join(&name);
            if kind.is_symlink() {
                std::os::unix::fs::symlink(fs::read_link(entry.path()).unwrap(), dest).unwrap();
            } else if kind.is_dir() {
                copy_tree(&entry.path(), &dest);
            } else {
                fs::copy(entry.path(), dest).unwrap();
            }
        }
    }
    let base = TempDir::new("portage-repo-consumer-pin");
    copy_tree(&fixtures_root().join("var"), &base.join("var"));
    fs::create_dir_all(base.join("distfiles")).unwrap();
    let world_file = base.join("var/lib/portage/world");
    let mut text = fs::read_to_string(&world_file).unwrap_or_default();
    for atom in world {
        text.push_str(atom);
        text.push('\n');
    }
    fs::write(world_file, text).unwrap();
    base
}

/// The request `emerge --pretend <atoms>` makes against `root`, with the
/// command line's `--backtrack=20`.
pub(super) fn request_in(root: &Path, atoms: &[&str]) -> ResolveRequest {
    let config_root = fixtures_root();
    let config = portage_profile::resolve_config(portage_profile::ResolveConfigArgs {
        config_root: &config_root,
        main_repo_location: &config_root.join("repo"),
        overlay_repos: &[("overlay".to_string(), config_root.join("overlay"))],
        repo_aliases: &[],
        main_repo_name: "testrepo",
        repo_masters: &HashMap::new(),
        eroot: root,
    })
    .expect("fixture config resolves");
    let atoms: Vec<String> = atoms.iter().map(|a| (*a).to_string()).collect();
    ResolveRequest {
        backtrack_max: 20,
        ..ResolveRequest::new(
            &config_root,
            root,
            &atoms,
            &config,
            &config_root.join("distfiles"),
        )
    }
}

pub(super) fn resolve_in(root: &Path, atoms: &[&str]) -> GraphResult {
    request_in(root, atoms)
        .resolve()
        .unwrap_or_else(|e| panic!("resolving {atoms:?} failed: {e}"))
}

/// The second walk of an `emerge --pretend` run (`_complete_graph`): the
/// first walk's merges are locked and reused, and the world atoms seed the
/// required-set walk. The installed-consumer pins of the keeper, igw, cgp
/// and triangle shapes act only here.
pub(super) fn complete_request_in(root: &Path, atoms: &[&str]) -> ResolveRequest {
    let first = resolve_in(root, atoms);
    let merges: Vec<&GraphEntry> = first
        .entries
        .iter()
        .filter(|e| {
            matches!(
                e.outcome,
                PretendOutcome::New { .. }
                    | PretendOutcome::Upgrade { .. }
                    | PretendOutcome::Downgrade { .. }
                    | PretendOutcome::Reinstall { .. }
            )
        })
        .collect();
    let locked: Vec<String> = merges
        .iter()
        .map(|e| format!("{}/{}", e.category, e.package))
        .collect();
    let mut seen = HashSet::new();
    let multi_slot: HashSet<(String, String)> = first
        .entries
        .iter()
        .map(|e| (e.category.clone(), e.package.clone()))
        .filter(|k| !seen.insert(k.clone()))
        .collect();
    let reuse: HashMap<(String, String), PretendOutcome> = merges
        .iter()
        .filter(|e| !matches!(e.outcome, PretendOutcome::Reinstall { .. }))
        .map(|e| ((e.category.clone(), e.package.clone()), e.outcome.clone()))
        .filter(|(k, _)| !multi_slot.contains(k))
        .collect();
    let seeds: Vec<String> = fs::read_to_string(root.join("var/lib/portage/world"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .map(String::from)
        .collect();
    let mut req = request_in(root, atoms);
    req.config.blocker_retry_seed_atoms = seeds.clone();
    req.config.complete_seed_atoms = seeds;
    req.config.complete_locked_merges = locked;
    req.complete = true;
    req.complete_reuse = reuse;
    req
}

pub(super) fn resolve_complete_in(root: &Path, atoms: &[&str]) -> GraphResult {
    complete_request_in(root, atoms)
        .resolve()
        .unwrap_or_else(|e| panic!("complete walk of {atoms:?} failed: {e}"))
}

/// One conflict atom of a choice: `(parent cat/pkg-ver, atom)`; the parent is
/// empty for an argument.
type TracedParent = (String, String);

pub(super) struct TracedChoice {
    /// `cat/pkg-ver`, with ` [installed]` when the mask hides the installed instance.
    target: String,
    /// The extra versions masked in the same node.
    similar: Vec<String>,
    /// The choice's conflict atoms, sorted.
    atoms: Vec<TracedParent>,
}

pub(super) struct TracedPass {
    /// The runtime masks this pass ran with, sorted.
    masks_in: Vec<String>,
    kind: &'static str,
    /// For `SlotConflict`: the choices in creation order (the last is popped first).
    choices: Vec<TracedChoice>,
    /// For `MissingDep`: the masked parent.
    missing_owner: Option<String>,
    /// The pass's slot-conflict records after the feedback decision.
    conflicts: Vec<SlotConflict>,
    /// How many search nodes this pass's feedback added.
    children: usize,
    /// Whether Portage counts this pass as a try: a pass that asks for a
    /// restart always, any other failed pass only while unexplored nodes remain
    /// (depgraph.py `_backtrack_depgraph`, D:12407-12418).
    counted: bool,
}

fn parent_text(((cat, pkg, ver), atom): &((String, String, String), String)) -> TracedParent {
    let who = if cat.is_empty() {
        String::new()
    } else {
        format!("{cat}/{pkg}-{ver}")
    };
    (who, atom.clone())
}

/// Runs the backtracking search the way the driver does, pass by pass, and
/// records what each pass fed back.
pub(super) fn trace_search(req: &ResolveRequest) -> Vec<TracedPass> {
    let req = req.clone();
    let ctx = ResolveCtx::new(&req).expect("ctx");
    let mut bt = Backtracker::new(
        std::cmp::max(1, ctx.backtrack_max.div_ceil(2)),
        BacktrackParams::initial(&req),
    );
    let mut out: Vec<TracedPass> = Vec::new();
    let mut first = true;
    // The driver's count: one per loop iteration beyond the first, and a
    // failed walk counts only while unexplored nodes remain.
    let mut passes: u64 = 0;
    while let Some(params) = bt.get() {
        let mut masks_in: Vec<String> = params
            .runtime_pkg_mask
            .values()
            .flatten()
            .map(|m| m.neg.clone())
            .collect();
        masks_in.sort_unstable();
        let mut traced = TracedPass {
            masks_in,
            kind: "Settle",
            choices: Vec::new(),
            missing_owner: None,
            conflicts: Vec::new(),
            children: 0,
            counted: false,
        };
        let mut pass = match run_pass(&ctx, &params, first) {
            Ok(pass) => pass,
            Err(_) if !first => {
                traced.kind = "Failed";
                bt.adopt_current(params.clone());
                traced.counted = !bt.unexplored.is_empty();
                out.push(traced);
                if passes >= u64::from(ctx.backtrack_max) {
                    break;
                }
                if !bt.unexplored.is_empty() {
                    passes += 1;
                }
                continue;
            }
            Err(e) => panic!("root pass: {e}"),
        };
        passes += 1;
        first = false;
        let restarts = passes - 1;
        let decision = collect_feedback(&ctx, &params, &mut pass, ctx.config);
        let before = bt.nodes.len();
        let mut stop = false;
        match decision {
            PassDecision::Settle { .. } => stop = true,
            PassDecision::DeadEnd { params } => {
                traced.kind = "DeadEnd";
                bt.adopt_current(*params);
                traced.counted = !bt.unexplored.is_empty();
                stop = restarts >= u64::from(ctx.backtrack_max);
            }
            PassDecision::Feedback(kind) => {
                traced.kind = match &kind {
                    BacktrackFeedback::Config { .. } => "Config",
                    BacktrackFeedback::RevDep { .. } => "RevDep",
                    BacktrackFeedback::Circular { .. } => "Circular",
                    BacktrackFeedback::SlotConflict { .. } => "SlotConflict",
                    BacktrackFeedback::MissingDep { .. } => "MissingDep",
                };
                match &kind {
                    BacktrackFeedback::SlotConflict { choices, .. } => {
                        for c in choices {
                            let (cat, pkg, ver) = &c.target;
                            let mut atoms: Vec<TracedParent> =
                                c.parents.iter().map(parent_text).collect();
                            atoms.sort();
                            traced.choices.push(TracedChoice {
                                target: format!(
                                    "{cat}/{pkg}-{ver}{}",
                                    if c.installed { " [installed]" } else { "" }
                                ),
                                similar: c
                                    .similar
                                    .iter()
                                    .map(|s| {
                                        format!("{}/{}-{}", s.target.0, s.target.1, s.target.2)
                                    })
                                    .collect(),
                                atoms,
                            });
                        }
                    }
                    BacktrackFeedback::MissingDep { owner, .. } => {
                        traced.missing_owner = Some(format!("{}/{}", owner.0, owner.1));
                    }
                    _ => {}
                }
                traced.counted = true;
                if restarts >= u64::from(ctx.backtrack_max) {
                    stop = true;
                } else {
                    bt.feedback(kind);
                }
            }
        }
        traced.conflicts = pass.slot_conflicts.clone();
        traced.children = bt.nodes.len() - before;
        out.push(traced);
        if stop {
            break;
        }
    }
    out
}

/// The tries Portage prints as `backtrack: N/20` for this search.
pub(super) fn portage_tries(trace: &[TracedPass]) -> usize {
    trace.iter().filter(|p| p.counted).count()
}

pub(super) fn kinds(trace: &[TracedPass]) -> Vec<&'static str> {
    trace.iter().map(|p| p.kind).collect()
}

pub(super) fn atoms_of(choice: &TracedChoice) -> Vec<(&str, &str)> {
    choice
        .atoms
        .iter()
        .map(|(who, atom)| (who.as_str(), atom.as_str()))
        .collect()
}

fn entry_of<'a>(result: &'a GraphResult, cat: &str, pkg: &str) -> &'a GraphEntry {
    result
        .entries
        .iter()
        .find(|e| e.category == cat && e.package == pkg)
        .unwrap_or_else(|| panic!("no entry for {cat}/{pkg}"))
}

#[test]
fn first_popped_choice_masks_the_merge_version_opposed_by_the_consumer_atoms() {
    // Portage 3.0.83, `emerge --debug dev-libs/p563pull` on a scratch copy of
    // the fixtures: the first `_slot_confict_backtrack` call lists the
    // installed consumer's atoms `=p563prov-5.42*` and `p563prov:0/5.42=` as
    // parents next to the puller's `=p563prov-5.44*` (D:2160-2165); the choice
    // that masks 5.44 carries the two consumer atoms, the one that masks the
    // installed 5.42 carries the puller's atom, and the longer conflict list is
    // popped first (D:2235, D:2273, B:136).
    // Portage: depgraph.py, _slot_confict_backtrack.
    let root = hermetic_root(&[]);
    let trace = trace_search(&request_in(&root, &["dev-libs/p563pull"]));
    let first = &trace[0];
    assert_eq!(first.kind, "SlotConflict", "kinds: {:?}", kinds(&trace));
    assert_eq!(first.choices.len(), 2);
    let popped_first = first.choices.last().unwrap();
    assert_eq!(popped_first.target, "dev-libs/p563prov-5.44");
    let con = "dev-libs/p563con-1";
    assert_eq!(
        atoms_of(popped_first),
        vec![
            (con, "=dev-libs/p563prov-5.42*"),
            (con, "dev-libs/p563prov:0/5.42="),
        ],
        "the consumer's selected atom and its built atom, sub-slot kept"
    );
    let other = &first.choices[0];
    assert_eq!(other.target, "dev-libs/p563prov-5.42 [installed]");
    assert_eq!(
        atoms_of(other),
        vec![("dev-libs/p563pull-2", "=dev-libs/p563prov-5.44*")]
    );
}

#[test]
fn contradicted_pin_backtracks_over_every_choice_like_portage() {
    // Portage 3.0.83, `emerge --pretend =dev-libs/paired-2.0` with
    // `dev-libs/keeper` in world (keeper-1.0 records `=paired-1.0`): rc 1,
    // `backtrack: 3/20`, then the residual slot conflict of the final pass
    // (`get_best_run`, B:287-298; D:5613-5616). The argument atom is a
    // conflict atom of the installed instance's choice (D:2160-2165); it never
    // counts as a masked parent (B:162).
    let root = hermetic_root(&["dev-libs/keeper"]);
    let trace = trace_search(&complete_request_in(&root, &["=dev-libs/paired-2.0"]));
    assert_eq!(portage_tries(&trace), 3, "kinds: {:?}", kinds(&trace));
    let result = resolve_complete_in(&root, &["=dev-libs/paired-2.0"]);
    assert_eq!(
        result.slot_conflicts.len(),
        1,
        "the residual conflict is reported: {:?}",
        result.slot_conflicts
    );
    let c = &result.slot_conflicts[0];
    assert_eq!(
        c.instances
            .iter()
            .map(|i| (i.version.as_str(), i.installed))
            .collect::<Vec<_>>(),
        vec![("2.0", false), ("1.0", true)]
    );
}

#[test]
fn t3_choice_sequence_matches_portage() {
    // Portage 3.0.83, `emerge --debug -pv dev-libs/p589cpull` (T3: installed
    // `p589cvirt-1` with `|| ( =p589cprov-5.42* ... )`, no slot operator):
    // 4/20, `N p589cpull-1`, skip row `=p589cprov-5.42*`. Pass 0 has two
    // choices with one atom each, so the version order stands and the
    // installed 5.42 is popped first (D:2235, D:2273, B:136). Pass 1 masks the
    // 5.42 ebuild and the installed 5.42; the installed instance stays in the
    // graph (D:8803-8834), the same conflict forms again, and the child that
    // masks only the installed instance is a duplicate (B:126). Pass 2 has a
    // missing dependency (D:5691-5693) whose child is rejected
    // (B:160-174); pass 3 masks 5.44 alone, with a missing dependency; pass 4
    // succeeds.
    let root = hermetic_root(&[]);
    let trace = trace_search(&request_in(&root, &["dev-libs/p589cpull"]));
    assert_eq!(
        kinds(&trace),
        vec![
            "SlotConflict",
            "SlotConflict",
            "MissingDep",
            "MissingDep",
            "Settle"
        ]
    );
    assert_eq!(trace[0].masks_in, Vec::<String>::new());
    assert_eq!(
        trace[1].masks_in,
        vec![
            "!=dev-libs/p589cprov-5.42",
            "!=dev-libs/p589cprov-5.42[installed]"
        ]
    );
    assert_eq!(trace[1].children, 1, "only the 5.44 child is new");
    assert_eq!(
        trace[2].children, 0,
        "the missing-dependency child is rejected"
    );
    assert_eq!(trace[3].masks_in, vec!["!=dev-libs/p589cprov-5.44"]);
    let result = resolve_in(&root, &["dev-libs/p589cpull"]);
    assert_eq!(result.backtrack_restarts, 4, "Portage prints 4/20");
    assert!(
        matches!(
            &entry_of(&result, "dev-libs", "p589cpull").outcome,
            PretendOutcome::New { version } if version == "1"
        ),
        "the old puller merges"
    );
    let rows: Vec<&str> = result
        .skipped_updates
        .iter()
        .map(|r| r.atom.as_str())
        .collect();
    assert_eq!(rows, vec!["=dev-libs/p589cprov-5.42*"]);
}

#[test]
fn masked_installed_instance_reforms_the_consumer_conflict() {
    // T3, the pass after "mask the 5.42 ebuild and the installed 5.42": the
    // installed instance is re-added from the raw vdb with no mask check
    // (D:8803-8834), so the conflict forms again with the installed 5.42's
    // parents `(p589cvirt-1, =5.42*)` ("backtracking loop detected",
    // D:2171-2181).
    let root = hermetic_root(&[]);
    let trace = trace_search(&request_in(&root, &["dev-libs/p589cpull"]));
    let loop_pass = &trace[1];
    let record = loop_pass
        .conflicts
        .iter()
        .find(|c| c.package == "p589cprov")
        .expect("the conflict forms again");
    let installed = record
        .instances
        .iter()
        .find(|i| i.installed)
        .expect("the installed 5.42 is a slot node");
    assert_eq!(installed.version, "5.42");
    let parents: Vec<(&str, &str)> = installed
        .parents
        .iter()
        .map(|p| {
            (
                p.parent_cpv.split(':').next().unwrap_or(""),
                p.atom.as_str(),
            )
        })
        .collect();
    assert_eq!(
        parents,
        vec![("dev-libs/p589cvirt-1", "=dev-libs/p589cprov-5.42*")]
    );
}

#[test]
fn loop_pass_mask_installed_choice_is_a_duplicate() {
    // In the loop pass the choice that masks only the installed 5.42 yields a
    // node equal to the one just run, which the backtracker drops (B:126);
    // only the 5.44 child is added.
    let root = hermetic_root(&[]);
    let trace = trace_search(&request_in(&root, &["dev-libs/p589cpull"]));
    let loop_pass = &trace[1];
    assert_eq!(loop_pass.choices.len(), 2, "both choices are offered");
    assert_eq!(loop_pass.children, 1, "one of them is a duplicate");
}

#[test]
fn walker_conflict_is_fed_back_before_a_consumer_pin_conflict() {
    // Portage feeds back only the first conflict of a pass (B:180-185), and
    // the walker's own conflicts come first; the contradicted consumer pin is
    // the residual one (D:2131-2144).
    let dir = slotundo_temp_dir("10a-walker-first");
    let mk = |name: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join("USE"), "\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("pinner-1.0", "=dev-libs/souprov-1.0");
    mk("souprov-1.0", "");
    let config = test_config();
    let repos = find_repos(&fixtures_root()).expect("fixture repos");
    let reachable: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "pinner".to_string())]);
    let opts = CtxOpts161 {
        reachable,
        backtrack_max: 20,
        ..Default::default()
    };
    let ctx = ctx_161(&dir, &config, repos, &opts);
    let souprov_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "souprov".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("2".into()),
        repo_name: Some("testrepo".into()),
        ..graph_entry("dev-libs", "souprov", "2.0")
    };
    let walker_instance = |version: &str, atom: &str| SlotConflictInstance {
        version: version.to_string(),
        sub_slot: "0".to_string(),
        repo_name: "testrepo".to_string(),
        use_display: Vec::new(),
        parents: vec![SlotConflictParent {
            parent_cpv: format!("dev-libs/walkerparent-1:0/0::testrepo{version}"),
            atom: atom.to_string(),
            use_display: Vec::new(),
            installed: false,
        }],
        installed: false,
    };
    let walker = SlotConflict {
        category: "dev-libs".to_string(),
        package: "walker".to_string(),
        slot: "0".to_string(),
        resolved_version: "2".to_string(),
        conflicting_atom: ">=dev-libs/walker-2".to_string(),
        instances: vec![
            walker_instance("2", ">=dev-libs/walker-2"),
            walker_instance("1", "<dev-libs/walker-2"),
        ],
    };
    let mut pass = pass_161();
    pass.entries = vec![souprov_upgrade];
    pass.slot_conflicts = vec![walker];
    pass.slot_want.insert(
        ("dev-libs".to_string(), "souprov".to_string()),
        vec![">=dev-libs/souprov-2.0".to_string()],
    );
    pass.slot_pullers.insert(
        ("dev-libs".to_string(), "souprov".to_string()),
        vec![(
            String::new(),
            String::new(),
            String::new(),
            ">=dev-libs/souprov-2.0".to_string(),
        )],
    );
    let decision = collect_feedback(&ctx, &BacktrackParams::default(), &mut pass, &config);
    let PassDecision::Feedback(BacktrackFeedback::SlotConflict { choices, .. }) = decision else {
        panic!("the walker's conflict is fed back");
    };
    assert!(
        choices.iter().all(|c| c.target.1 == "walker"),
        "the first conflict's choices: {:?}",
        choices.iter().map(|c| &c.target).collect::<Vec<_>>()
    );
    assert_eq!(
        pass.slot_conflicts
            .iter()
            .map(|c| c.package.as_str())
            .collect::<Vec<_>>(),
        vec!["walker", "souprov"],
        "the consumer-pin conflict is filed after the walker's"
    );
}

#[test]
fn consumer_pin_conflict_waits_for_a_missing_dependency() {
    // `dev-libs/p569pull dev-libs/t8x`: pass 0 has a plain missing dependency
    // (`t8x`); Portage returns before `altlist` (D:5691-5696) and feeds back
    // only that. The consumer-pin conflict is fed back in a later pass:
    // Portage 3.0.83 gives rc 0 and `backtrack: 3/20`.
    let root = hermetic_root(&[]);
    let trace = trace_search(&request_in(&root, &["dev-libs/p569pull", "dev-libs/t8x"]));
    assert_eq!(trace[0].kind, "MissingDep", "kinds: {:?}", kinds(&trace));
    assert_eq!(
        trace[1].kind,
        "SlotConflict",
        "the heal fires in a later pass: {:?}",
        kinds(&trace)
    );
    let result = resolve_in(&root, &["dev-libs/p569pull", "dev-libs/t8x"]);
    assert_eq!(result.backtrack_restarts, 3, "Portage prints 3/20");
}

#[test]
fn cgp_version_break_down_follows_portage_try_by_try() {
    // `<dev-libs/cgp1x-1`: choice[0] masks {2, 1, 1-installed} for the
    // argument atom, choice[1] masks {0.1} for `cgp1a >=cgp1x-1`; choice[1] is
    // explored first. Try 1 is a dead end on the argument. Try 2 runs the
    // other node: the installed 1 stays in the graph, the conflict forms
    // again, `mask 1-installed` is a duplicate and `mask 0.1` is added. Try 3
    // is a dead end. 3/20 (D:2220-2304, B:126, B:186-195, D:12414-12439).
    let root = hermetic_root(&["dev-libs/cgp1a"]);
    let trace = trace_search(&complete_request_in(&root, &["<dev-libs/cgp1x-1"]));
    assert_eq!(trace.len(), 4, "kinds: {:?}", kinds(&trace));
    assert_eq!(trace[0].kind, "SlotConflict");
    assert_eq!(
        trace[0].choices.last().unwrap().target,
        "dev-libs/cgp1x-0.1"
    );
    assert_eq!(trace[0].choices[0].target, "dev-libs/cgp1x-1 [installed]");
    assert_eq!(
        trace[0].choices[0].similar,
        vec!["dev-libs/cgp1x-2"],
        "the installed 1 is masked with the ebuild 2 (D:2253-2264)"
    );
    assert_eq!(trace[1].masks_in, vec!["!=dev-libs/cgp1x-0.1"]);
    assert_eq!(
        trace[2].masks_in,
        vec![
            "!=dev-libs/cgp1x-1",
            "!=dev-libs/cgp1x-1[installed]",
            "!=dev-libs/cgp1x-2"
        ]
    );
    assert_eq!(trace[2].children, 1);
    assert_eq!(
        trace[3].masks_in,
        vec![
            "!=dev-libs/cgp1x-0.1",
            "!=dev-libs/cgp1x-1",
            "!=dev-libs/cgp1x-1[installed]",
            "!=dev-libs/cgp1x-2"
        ]
    );
    assert_eq!(portage_tries(&trace), 3, "Portage prints 3/20");
    let result = resolve_complete_in(&root, &["<dev-libs/cgp1x-1"]);
    assert_eq!(result.slot_conflicts.len(), 1, "the residual is reported");
}

#[test]
fn cgp_version_break_up_follows_portage_try_by_try() {
    // `>=dev-libs/cgp1x-2`: choice[0] masks {2} for `cgp1a <cgp1x-2`, choice[1]
    // masks {1, 0.1, 1-installed} for the argument; choice[1] first. Try 1
    // runs the node with 1, 0.1 and the installed 1 masked; the conflict forms
    // again and `mask 2` is added. Try 2 is a dead end on the argument, try 3
    // too. 3/20.
    let root = hermetic_root(&["dev-libs/cgp1a"]);
    let trace = trace_search(&complete_request_in(&root, &[">=dev-libs/cgp1x-2"]));
    assert_eq!(trace.len(), 4, "kinds: {:?}", kinds(&trace));
    assert_eq!(trace[0].choices[1].target, "dev-libs/cgp1x-1 [installed]");
    assert_eq!(trace[0].choices[1].similar, vec!["dev-libs/cgp1x-0.1"]);
    assert_eq!(
        trace[1].masks_in,
        vec![
            "!=dev-libs/cgp1x-0.1",
            "!=dev-libs/cgp1x-1",
            "!=dev-libs/cgp1x-1[installed]"
        ]
    );
    assert_eq!(
        trace[2].masks_in,
        vec![
            "!=dev-libs/cgp1x-0.1",
            "!=dev-libs/cgp1x-1",
            "!=dev-libs/cgp1x-1[installed]",
            "!=dev-libs/cgp1x-2"
        ]
    );
    assert_eq!(trace[3].masks_in, vec!["!=dev-libs/cgp1x-2"]);
    assert_eq!(portage_tries(&trace), 3, "Portage prints 3/20");
    let result = resolve_complete_in(&root, &[">=dev-libs/cgp1x-2"]);
    assert_eq!(result.slot_conflicts.len(), 1, "the residual is reported");
}

#[test]
fn igw_plain_backtracks_like_portage_in_both_forms() {
    // `dev-libs/igw0a` in world, no `--ignore-world`: Portage 3.0.83 prints
    // `backtrack: 3/20` for `>=dev-libs/igw0x-2` and `<dev-libs/igw0x-1`, rc 1.
    let root = hermetic_root(&["dev-libs/igw0a"]);
    for atom in [">=dev-libs/igw0x-2", "<dev-libs/igw0x-1"] {
        let trace = trace_search(&complete_request_in(&root, &[atom]));
        assert_eq!(
            portage_tries(&trace),
            3,
            "{atom}: kinds {:?}",
            kinds(&trace)
        );
        let result = resolve_complete_in(&root, &[atom]);
        assert_eq!(result.slot_conflicts.len(), 1, "{atom}");
    }
}

#[test]
fn keeper_needer_backtracks_like_portage() {
    // `dev-libs/keeper` in world, `emerge --pretend dev-libs/needer`: Portage
    // 3.0.83 prints `backtrack: 4/20`, rc 1.
    let root = hermetic_root(&["dev-libs/keeper"]);
    let trace = trace_search(&complete_request_in(&root, &["dev-libs/needer"]));
    assert_eq!(portage_tries(&trace), 4, "kinds: {:?}", kinds(&trace));
}

#[test]
fn triangle_backtracks_like_portage_in_both_argument_orders() {
    // `emerge --pretend dev-libs/needer dev-libs/othermod` and the reversed
    // order on the shared fixtures: Portage 3.0.83 prints `backtrack: 4/20`,
    // rc 1 (the harvested corpus holds 3/20).
    let root = hermetic_root(&[]);
    for atoms in [
        ["dev-libs/needer", "dev-libs/othermod"],
        ["dev-libs/othermod", "dev-libs/needer"],
    ] {
        let trace = trace_search(&complete_request_in(&root, &atoms));
        assert_eq!(
            portage_tries(&trace),
            4,
            "{atoms:?}: kinds {:?}",
            kinds(&trace)
        );
    }
}

#[test]
fn printed_count_of_the_dead_end_cells_matches_portage() {
    // The driver prints Portage's `backtracked` (D:12407-12418: a restart
    // always counts, any other failed pass only while unexplored nodes
    // remain). R6 (#600); oracle rows in
    // `LLM/evidence/2026-10-26-575-residues/r6/captures/full-table.md`.
    let keeper = hermetic_root(&["dev-libs/keeper"]);
    let igw = hermetic_root(&["dev-libs/igw0a"]);
    let cgp = hermetic_root(&["dev-libs/cgp1a"]);
    let plain = hermetic_root(&[]);
    let cells: [(&TempDir, &[&str], u64); 9] = [
        (&keeper, &["=dev-libs/paired-2.0"], 3),
        (&keeper, &["dev-libs/needer"], 4),
        (&igw, &[">=dev-libs/igw0x-2"], 3),
        (&igw, &["<dev-libs/igw0x-1"], 3),
        (&cgp, &[">=dev-libs/cgp1x-2"], 3),
        (&cgp, &["<dev-libs/cgp1x-1"], 3),
        (&plain, &["dev-libs/needer", "dev-libs/othermod"], 4),
        (&plain, &["dev-libs/othermod", "dev-libs/needer"], 4),
        (&keeper, &["dev-libs/needer", "dev-libs/othermod"], 4),
    ];
    for (root, atoms, portage) in cells {
        let result = resolve_complete_in(root, atoms);
        assert_eq!(result.backtrack_restarts, portage, "{atoms:?}");
    }
}
