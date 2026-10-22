use super::*;

use portage_util::TempDir;

fn prio(word: u64) -> DepPriority {
    DepPriority {
        buildtime: word & 1 != 0,
        runtime: word & 2 != 0,
        runtime_post: word & 4 != 0,
        buildtime_slot_op: word & 8 != 0,
        runtime_slot_op: word & 16 != 0,
        optional: word & 32 != 0,
        satisfied: word & 64 != 0,
    }
}

/// A `Digraph` over `0..n` with deduped edges, mirroring
/// `Digraph::add_edge` semantics.
fn test_graph(n: usize, edges: &[(usize, usize, DepPriority)]) -> Digraph {
    let mut g = Digraph {
        n,
        children: vec![Vec::new(); n],
        parents: vec![Vec::new(); n],
        order: (0..n).collect(),
        installed: vec![false; n],
        alive: vec![true; n],
    };
    for &(p, c, pr) in edges {
        g.add_edge(p, c, pr);
    }
    g
}

/// Every filter the loop can ask about: `None` plus each rung of
/// both ladders.
fn all_filters() -> Vec<Option<Ignore>> {
    let mut out = vec![None];
    out.extend(NORMAL.ignore.iter().copied());
    out.extend(SATISFIED.ignore.iter().copied());
    out
}

/// Assert frontier/direct agreement on a graph snapshot: every
/// level's ready set equals the corresponding scan, and `is_leaf`
/// agrees on every alive node.
fn assert_equivalent(g: &Digraph, fr: &mut SerializeFrontier) {
    for ig in all_filters() {
        let level = fr.level_of(ig).expect("every loop filter has a level");
        let mut ready = fr.ready_nodes(level, &g.alive);
        ready.sort_unstable();
        let mut direct = g.leaf_nodes(ig);
        direct.sort_unstable();
        assert_eq!(ready, direct, "ready_nodes != leaf_nodes for {ig:?}");
    }
    for node in 0..g.n {
        if !g.alive[node] {
            continue;
        }
        for ig in all_filters() {
            let level = fr.level_of(ig).unwrap();
            assert_eq!(
                fr.is_leaf(node, level),
                g.is_leaf(node, ig),
                "is_leaf disagrees on node {node} for {ig:?}"
            );
        }
    }
}

/// Deterministic xorshift64* -- no rand dependency for a unit test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Cycle node sets, order-insensitive (rotations of one ring are
/// distinct records, like Portage).
fn cycle_sets(cycles: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = cycles
        .iter()
        .map(|c| {
            let mut s = c.clone();
            s.sort_unstable();
            s
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A minimal `New` `GraphEntry` for `build_digraph` tests --
/// mirrors `synthetic_installed_entry`'s full field list.
fn new_entry(category: &str, package: &str, version: &str, deps: Vec<DepEdge>) -> GraphEntry {
    GraphEntry {
        discovery: 0,
        category: category.to_string(),
        package: package.to_string(),
        outcome: PretendOutcome::New {
            version: version.to_string(),
        },
        blockers: Vec::new(),
        slot: Some(version.to_string()),
        sub_slot: Some(version.to_string()),
        repo_name: None,
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
        build_time: None,
        deps,
    }
}

// -----------------------------------------------------------------
// Dep-key / priority mapping
// -----------------------------------------------------------------

fn dep_metadata(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn tokens(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// The `aub0` shape: A needs `d[-foo]`, B needs `d[foo]`, C needs
/// `>=d-1`; `d-0` declares `foo` (on by default), `d-1` does not.
/// Portage: _create_graph walks the arguments LIFO and fails at the
/// first contradiction or unsatisfied atom.
fn aub0_entries(order: &[&str]) -> Vec<GraphEntry> {
    let rt = DepPriority {
        runtime: true,
        ..DepPriority::default()
    };
    let mut entries: Vec<GraphEntry> = order
        .iter()
        .map(|p| {
            let atom = match *p {
                "a" => "t/d[-foo]",
                "b" => "t/d[foo]",
                _ => ">=t/d-1",
            };
            let mut e = new_entry("t", p, "0", vec![plain_edge(atom, "t", "d", rt)]);
            e.slot = Some("0".into());
            e.sub_slot = Some("0".into());
            e
        })
        .collect();
    for v in ["0", "1"] {
        let mut d = new_entry("t", "d", v, Vec::new());
        d.slot = Some("0".into());
        d.sub_slot = Some("0".into());
        entries.push(d);
    }
    entries
}

fn aub0_use(entries: &[GraphEntry]) -> impl Fn(usize) -> Option<ReplayUse> + '_ {
    move |i: usize| {
        let e = &entries[i];
        (e.package == "d").then(|| {
            let d0 = outcome_version(e) == Some("0");
            ReplayUse {
                iuse: [if d0 { "foo" } else { "bar" }.to_string()].into(),
                enabled: if d0 {
                    ["foo".to_string()].into()
                } else {
                    HashSet::new()
                },
                fixed: HashSet::new(),
            }
        })
    }
}

fn plain_edge(atom: &str, category: &str, package: &str, priority: DepPriority) -> DepEdge {
    DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: category.to_string(),
        package: package.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: 3,
    }
}

// -----------------------------------------------------------------
// Installed / vdb-backed inputs
// -----------------------------------------------------------------

/// A scratch vdb: `var/db/pkg/<cat>/<pf>/` with the given files.
/// Each test builds the exact installed shape it needs under a unique
/// root (avoids the per-root installed-packages cache).
type VdbFile<'a> = (&'a str, &'a str);

type VdbEntry<'a> = (&'a str, &'a str, &'a [VdbFile<'a>]);

fn mo_tmp_vdb(tag: &str, entries: &[VdbEntry<'_>]) -> std::path::PathBuf {
    let root = TempDir::new(&format!("portuale-mo-vdb-{}-{}", std::process::id(), tag)).keep();
    for (cat, pf, files) in entries {
        let dir = root.join("var/db/pkg").join(cat).join(pf);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, value) in *files {
            std::fs::write(dir.join(name), value.as_bytes()).unwrap();
        }
    }
    root
}

fn fixtures_vdb_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn installed_by_cp(rows: &[(&str, &str, &[&str])]) -> HashMap<(String, String), Vec<String>> {
    let mut m = HashMap::new();
    for (cat, pkg, cands) in rows {
        m.insert(
            (cat.to_string(), pkg.to_string()),
            cands.iter().map(|s| s.to_string()).collect(),
        );
    }
    m
}

fn runtime_edge(atom: &str, cat: &str, pkg: &str) -> DepEdge {
    plain_edge(
        atom,
        cat,
        pkg,
        DepPriority {
            runtime: true,
            ..DepPriority::default()
        },
    )
}

/// A runtime edge tagged as Portage's `RDEPEND` key (`key == 0`), which
/// `seed_toolchain_asap` reads providers from.
fn rdepend_edge(atom: &str, cat: &str, pkg: &str) -> DepEdge {
    let mut edge = runtime_edge(atom, cat, pkg);
    edge.key = 0;
    edge
}

mod alt_edges;
mod build;
mod cycles;
mod debug;
mod digraph;
mod installed_closure;
mod prelude;
mod priority;
mod replay;
mod select_nodes;
mod serialize;
mod tree_display;
