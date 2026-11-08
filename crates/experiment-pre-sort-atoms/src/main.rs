//! Experiment: for every atom of `@world` (world file + `@system` +
//! `@profile`) and, transitively, of the dependencies of the version
//! Portuale would pick for it, build the ordered list of matching ebuilds.
//!
//! Order: visible before masked, then highest version first, then highest
//! repo priority -- the order `select_best_candidate` compares in. The
//! work runs level by level over a breadth-first frontier, each level
//! spread across all CPU cores with `std::thread::scope`.
//!
//! Second pass (`reorder`): all atoms aimed at one package slot are
//! collected, and the versions that satisfy every one of them move to the
//! front. The first pass follows the visible best ebuild of every slot an atom
//! can match, not only the highest one. A slot with no visible version satisfying all of its requesters
//! is reported INVALID (with `--unique`, also one with several); it is
//! still listed. The dependency edges come from the first pass and are not
//! re-walked after the reorder.
//!
//! Known cuts: USE-dependencies (`[foo]`) are not used to filter, virtuals
//! are ordinary packages.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

mod memo;
mod reorder;
mod sets;

use memo::Memo;
use portage_dep::Blocker;
use portage_profile::{Config, ResolveConfigArgs};
use portage_repo::{Candidate, EffectiveUseFlagsArgs, RepoConfig};
use portage_use_reduce::{
    AltPreference, MatchMode, UseReduceFlatDisjunctiveArgs, use_reduce_flat_disjunctive,
};
use sets::SetResolver;
use std::sync::Arc;

/// What the workers share: the tree, what is installed, and verdicts
/// memoised per candidate / atom.
pub(crate) struct Memos<'a> {
    repos: &'a [RepoConfig],
    /// `cat/pkg` of everything installed, to prefer installed `||` branches.
    installed: HashSet<String>,
    visible: Memo<bool>,
    atom_available: Memo<bool>,
    deps: Memo<Arc<BTreeSet<String>>>,
}

impl<'a> Memos<'a> {
    fn new(repos: &'a [RepoConfig], root: &Path) -> Self {
        let installed = portage_repo::all_installed_packages(root)
            .iter()
            .map(|p| format!("{}/{}", p.category, p.package))
            .collect();
        Self {
            repos,
            installed,
            visible: Memo::new(),
            atom_available: Memo::new(),
            deps: Memo::new(),
        }
    }

    pub(crate) fn visible(
        &self,
        c: &Candidate,
        category: &str,
        package: &str,
        config: &Config,
    ) -> bool {
        self.visible
            .get_or_compute(c.match_str(category, package), || {
                portage_repo::is_visible(c, category, package, config)
            })
    }

    fn deps(
        &self,
        c: &Candidate,
        category: &str,
        package: &str,
        config: &Config,
    ) -> Arc<BTreeSet<String>> {
        self.deps
            .get_or_compute(c.match_str(category, package), || {
                Arc::new(self.dependency_atoms(c, category, package, config))
            })
    }

    /// Whether some visible ebuild matches `atom_str`.
    fn atom_available(&self, atom_str: &str, config: &Config) -> bool {
        self.atom_available
            .get_or_compute(atom_str.to_string(), || {
                let Some(atom) = portage_dep::parse_atom(atom_str) else {
                    return false;
                };
                let Ok(all) =
                    portage_repo::list_candidates(self.repos, &atom.category, &atom.package)
                else {
                    return false;
                };
                all.iter().any(|c| {
                    let s = c.match_str(&atom.category, &atom.package);
                    portage_dep::match_from_list(atom_str, &[s.as_str()])
                        .is_some_and(|m| !m.is_empty())
                        && self.visible(c, &atom.category, &atom.package, config)
                })
            })
    }

    /// How good one `||` branch is, in Portage's terms: a branch whose
    /// packages are all installed beats one that is merely available, and
    /// one with an atom no visible ebuild satisfies is never chosen.
    // Portage: dep_check.py, dep_zapdeps
    fn branch_preference(&self, branch: &[String], config: &Config) -> AltPreference {
        let atoms: Vec<portage_dep::Atom> = branch
            .iter()
            .filter_map(|t| portage_repo::parse_dep_atom(t, "experiment-pre-sort-atoms"))
            .filter(|a| a.blocker == Blocker::None)
            .collect();
        let all_available = branch
            .iter()
            .filter(|t| {
                portage_repo::parse_dep_atom(t, "experiment-pre-sort-atoms")
                    .is_some_and(|a| a.blocker == Blocker::None)
            })
            .all(|t| self.atom_available(t, config));
        if !all_available {
            AltPreference::Unsatisfiable
        } else if atoms.iter().all(|a| {
            self.installed
                .contains(&format!("{}/{}", a.category, a.package))
        }) {
            AltPreference::Installed
        } else {
            AltPreference::Available
        }
    }

    /// Non-blocker atoms of every `*DEPEND` of `best`, with the conditionals
    /// evaluated against the package's effective USE and every `||` group
    /// resolved to the one branch Portage would take.
    fn dependency_atoms(
        &self,
        best: &Candidate,
        category: &str,
        package: &str,
        config: &Config,
    ) -> BTreeSet<String> {
        let pf = format!("{package}-{}", best.version);
        let Ok(meta) = portage_repo::repo_aux_metadata(&best.repo_location, category, &pf) else {
            return BTreeSet::new();
        };
        let use_set = portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &best.iuse,
            keywords: &best.keywords,
            candidate_str: &best.match_str(category, package),
            build_id: None,
            category,
            package,
        });
        let mut out = BTreeSet::new();
        for key in DEP_KEYS {
            let Some(text) = meta.get(key) else { continue };
            let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
            // First listed branch wins a tie.
            let mut first = |_: &[Vec<String>]| 0usize;
            let Ok(flat) = use_reduce_flat_disjunctive(
                UseReduceFlatDisjunctiveArgs {
                    tokens: &tokens,
                    uselist: &use_set,
                    mode: MatchMode::Normal,
                    empty_groups_always_true: true,
                    tie_break: &mut first,
                },
                &mut |branch| self.branch_preference(branch, config),
            ) else {
                continue;
            };
            for tok in flat {
                if let Some(a) = portage_repo::parse_dep_atom(&tok, "experiment-pre-sort-atoms")
                    && a.blocker == Blocker::None
                {
                    out.insert(tok);
                }
            }
        }
        out
    }
}

const DEP_KEYS: [&str; 5] = ["DEPEND", "RDEPEND", "PDEPEND", "BDEPEND", "IDEPEND"];

/// The result for one atom: for every slot it can match, the best ebuild
/// of that slot (as `cat/pkg-ver`) with the atoms it depends on.
struct AtomResult {
    per_slot: Vec<(String, Arc<BTreeSet<String>>)>,
}

struct Options {
    root: PathBuf,
    config_root: PathBuf,
    threads: usize,
    only_best: bool,
    unique: bool,
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(msg) = run(&opts) {
        eprintln!("experiment-pre-sort-atoms: {msg}");
        std::process::exit(1);
    }
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        root: PathBuf::from("/"),
        config_root: PathBuf::from("/"),
        threads: std::thread::available_parallelism().map_or(1, usize::from),
        only_best: false,
        unique: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--root" => opts.root = value("--root")?.into(),
            "--config-root" => opts.config_root = value("--config-root")?.into(),
            "--threads" => {
                opts.threads = value("--threads")?
                    .parse::<usize>()
                    .map_err(|e| format!("--threads: {e}"))?
                    .max(1);
            }
            "--best-only" => opts.only_best = true,
            "--unique" => opts.unique = true,
            "-h" | "--help" => {
                return Err(
                    "usage: experiment-pre-sort-atoms [--root DIR] [--config-root DIR] \
                            [--threads N] [--best-only] [--unique]"
                        .into(),
                );
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(opts)
}

fn run(opts: &Options) -> Result<(), String> {
    let repos = portage_repo::find_repos_quiet(&opts.config_root).map_err(|e| e.to_string())?;
    let config = load_config(&opts.config_root, &opts.root, &repos)?;
    let memo = Memos::new(&repos, &opts.root);
    let seeds = world_atoms(&opts.root, &opts.config_root, &config)?;

    // atom -> who asked for it.
    let mut requesters: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for seed in &seeds {
        requesters
            .entry(seed.clone())
            .or_default()
            .insert("@world".into());
    }
    let mut frontier: Vec<String> = seeds.into_iter().collect();
    let mut done: BTreeSet<String> = BTreeSet::new();

    while !frontier.is_empty() {
        let results = parallel_map(&frontier, opts.threads, |atom| {
            process_atom(atom, &repos, &config, &memo)
        });
        let mut next = BTreeSet::new();
        for (atom, res) in frontier.iter().zip(results) {
            done.insert(atom.clone());
            for (by, deps) in res.per_slot {
                for dep in deps.iter().cloned() {
                    requesters
                        .entry(dep.clone())
                        .or_default()
                        .insert(by.clone());
                    if !done.contains(&dep) {
                        next.insert(dep);
                    }
                }
            }
        }
        frontier = next.into_iter().collect();
    }

    // Second pass, one job per package.
    let mut by_cp: BTreeMap<(String, String), Vec<&String>> = BTreeMap::new();
    for atom in &done {
        if let Some(a) = portage_dep::parse_atom(atom) {
            by_cp.entry((a.category, a.package)).or_default().push(atom);
        }
    }
    let jobs: Vec<_> = by_cp.into_iter().collect();
    let ctx = reorder::Ctx {
        requesters: &requesters,
        repos: &repos,
        config: &config,
        memo: &memo,
        unique: opts.unique,
    };
    let sets: Vec<Vec<reorder::SlotSet>> =
        parallel_map(&jobs, opts.threads, |((cat, pkg), atoms)| {
            reorder::reorder(cat, pkg, atoms, &ctx)
        });

    let (mut invalid, mut unneeded, mut total) = (0, 0, 0);
    for set in sets.iter().flatten() {
        total += 1;
        invalid += usize::from(set.invalid);
        unneeded += usize::from(set.unneeded);
        set.print(opts.only_best);
    }
    eprintln!(
        "{} atoms, {total} package slots, {invalid} invalid, {unneeded} unneeded",
        done.len()
    );
    Ok(())
}

/// Runs `f` over `items` on `threads` workers pulling from a shared
/// index; results keep the order of `items`.
fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..threads.min(items.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, AtomicOrdering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    out.lock().expect("results lock").push((i, r));
                }
            });
        }
    });
    let mut out = out.into_inner().expect("results lock");
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

/// The best ebuild of every slot the atom matches (visible first, then
/// highest version, then highest repo priority) and its dependency atoms.
fn process_atom(atom_str: &str, repos: &[RepoConfig], config: &Config, memo: &Memos) -> AtomResult {
    let empty = AtomResult {
        per_slot: Vec::new(),
    };
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return empty;
    };
    let Ok(all) = portage_repo::list_candidates(repos, &atom.category, &atom.package) else {
        return empty;
    };
    let mut best: BTreeMap<&str, (&Candidate, bool)> = BTreeMap::new();
    for c in all.iter().filter(|c| {
        let s = c.match_str(&atom.category, &atom.package);
        portage_dep::match_from_list(atom_str, &[s.as_str()]).is_some_and(|m| !m.is_empty())
    }) {
        let visible = memo.visible(c, &atom.category, &atom.package, config);
        let slot = best.entry(c.slot.as_str()).or_insert((c, visible));
        if rank(c, visible, slot.0, slot.1) == Ordering::Greater {
            *slot = (c, visible);
        }
    }
    AtomResult {
        per_slot: best
            .into_values()
            // A masked best is never installed: its dependencies don't count.
            .filter(|(_, visible)| *visible)
            .map(|(c, _)| {
                (
                    format!("{}/{}-{}", atom.category, atom.package, c.version),
                    memo.deps(c, &atom.category, &atom.package, config),
                )
            })
            .collect(),
    }
}

/// `Greater` when `a` should come before `b`.
pub(crate) fn rank(a: &Candidate, av: bool, b: &Candidate, bv: bool) -> Ordering {
    av.cmp(&bv)
        .then_with(|| version_cmp(&a.version, &b.version))
        .then_with(|| a.repo_priority.cmp(&b.repo_priority))
}

pub(crate) fn version_cmp(a: &str, b: &str) -> Ordering {
    portage_versions::vercmp(a, b).map_or(Ordering::Equal, |n| n.cmp(&0))
}

fn load_config(config_root: &Path, root: &Path, repos: &[RepoConfig]) -> Result<Config, String> {
    let main = repos
        .iter()
        .find(|r| r.is_main)
        .ok_or("no main repo found in repos.conf")?;
    let overlays: Vec<(String, PathBuf)> = repos
        .iter()
        .filter(|r| !r.is_main)
        .map(|r| (r.name.clone(), r.location.clone()))
        .collect();
    let masters = repos
        .iter()
        .map(|r| (r.name.clone(), r.masters.clone()))
        .collect();
    let aliases: Vec<(String, PathBuf)> = repos
        .iter()
        .flat_map(|r| r.aliases.iter().map(|a| (a.clone(), r.location.clone())))
        .collect();
    portage_profile::resolve_config(ResolveConfigArgs {
        config_root,
        main_repo_location: &main.location,
        overlay_repos: &overlays,
        repo_aliases: &aliases,
        main_repo_name: &main.name,
        repo_masters: &masters,
        eroot: root,
    })
    .map_err(|e| e.to_string())
}

/// `@world` as `portage.conf` defines it (`@profile @selected @system`):
/// the world file, every set named in `world_sets` (expanded through
/// `sets.conf`), `@system` and `@profile`.
fn world_atoms(
    root: &Path,
    config_root: &Path,
    config: &Config,
) -> Result<BTreeSet<String>, String> {
    let mut atoms = BTreeSet::new();
    SetResolver::new(root, config_root).expand("selected", &mut atoms);
    atoms.extend(
        config
            .system_packages
            .iter()
            .map(|a| a.trim_start_matches('*').to_string()),
    );
    atoms.extend(config.profile_packages.iter().cloned());
    atoms.retain(|a| portage_dep::parse_atom(a).is_some());
    Ok(atoms)
}
