//! For every atom of `@world` (world file + `@system` +
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
//! An installed package that stays as it is contributes only its `RDEPEND`
//! and `PDEPEND` (like Portage without `--with-bdeps`); `--with-bdeps`
//! restores the build-time edges.
//!
//! A `[foo]` / `[-foo]` USE dependency filters the versions an atom matches
//! by the candidate's own USE; the parent-relative forms (`[foo?]`, `[foo=]`)
//! are first evaluated against the USE of the package that asks.
//!
//! Known cuts: a repository without `metadata/md5-cache` is read through the
//! aux-metadata provider the caller registers (`portage_repo::register_aux_metadata_provider`);
//! an ebuild the provider cannot read contributes nothing; virtuals are ordinary packages; a USE-dependency flag the
//! candidate does not declare counts as declared when it is enabled, so an
//! implicit-IUSE flag that is off is judged undeclared.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

mod memo;
mod order;
mod reorder;
mod sets;
/// Test-only helpers: run [`analyse`] on the shared fixture config root with
/// a private ROOT that holds the world file and the installed packages.
#[cfg(test)]
pub(crate) mod testkit {
    use std::fs;
    use std::path::Path;

    use crate::{Options, World, analyse};

    /// `@world` made of `world` atoms, with `installed` (`category/pf`, `SLOT`)
    /// in the vdb, analysed against the fixture repositories. The ROOT lives in
    /// a temporary directory; the fixtures are only read.
    pub(crate) fn analyse_fixture(world: &[&str], installed: &[(&str, &str)]) -> World {
        let root = tempfile::tempdir().expect("temporary root");
        let portage = root.path().join("var/lib/portage");
        fs::create_dir_all(&portage).expect("world dir");
        fs::write(portage.join("world"), world.join("\n") + "\n").expect("world file");
        for (cpf, slot) in installed {
            let (category, _) = cpf.split_once('/').expect("category/pf");
            let dir = root.path().join("var/db/pkg").join(cpf);
            fs::create_dir_all(&dir).expect("vdb entry");
            fs::write(dir.join("CATEGORY"), format!("{category}\n")).expect("CATEGORY");
            fs::write(dir.join("SLOT"), format!("{slot}\n")).expect("SLOT");
            fs::write(dir.join("EAPI"), "8\n").expect("EAPI");
        }
        let config_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut opts = Options::new(root.path().to_path_buf(), config_root);
        opts.threads = 2;
        analyse(&opts).expect("the fixture tree analyses")
    }
}

pub use order::Order;
pub use reorder::{Change, SlotSet};

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
    /// `cat/pkg` -> installed slots (sub-slot cut off).
    installed_slots: HashMap<String, HashSet<String>>,
    /// `(cat/pkg, slot)` -> installed version, to tell a rebuild from a keep.
    installed_versions: HashMap<(String, String), String>,
    /// Follow `DEPEND`/`BDEPEND`/`IDEPEND` of an installed, unchanged
    /// package too (`--with-bdeps`); Portage does not by default.
    with_bdeps: bool,
    visible: Memo<bool>,
    use_sets: Memo<Arc<HashSet<String>>>,
    atom_available: Memo<bool>,
    deps: Memo<Arc<BTreeSet<String>>>,
}

impl<'a> Memos<'a> {
    fn new(repos: &'a [RepoConfig], root: &Path, with_bdeps: bool) -> Self {
        let installed = portage_repo::all_installed_packages(root)
            .iter()
            .map(|p| format!("{}/{}", p.category, p.package))
            .collect();
        let mut installed_slots: HashMap<String, HashSet<String>> = HashMap::new();
        let mut installed_versions = HashMap::new();
        for p in portage_repo::all_installed_packages(root).iter() {
            let slot = p.slot.split('/').next().unwrap_or_default().to_string();
            installed_versions.insert(
                (format!("{}/{}", p.category, p.package), slot.clone()),
                p.version.clone(),
            );
            installed_slots
                .entry(format!("{}/{}", p.category, p.package))
                .or_default()
                .insert(slot);
        }
        Self {
            repos,
            installed,
            installed_slots,
            installed_versions,
            with_bdeps,
            visible: Memo::new(),
            use_sets: Memo::new(),
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

    /// The effective USE of one candidate.
    fn use_set(
        &self,
        c: &Candidate,
        category: &str,
        package: &str,
        config: &Config,
    ) -> Arc<HashSet<String>> {
        self.use_sets
            .get_or_compute(c.match_str(category, package), || {
                Arc::new(
                    portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
                        config,
                        iuse: &c.iuse,
                        keywords: &c.keywords,
                        candidate_str: &c.match_str(category, package),
                        build_id: None,
                        category,
                        package,
                    })
                    .as_ref()
                    .clone(),
                )
            })
    }

    /// Whether candidate `c` matches `atom_str`: version, slot and
    /// repository like `match_from_list`, and the atom's `[flag]` /
    /// `[-flag]` dependencies against the candidate's own USE. A flag the
    /// candidate neither declares nor enables counts as undeclared unless
    /// the dependency gives a `(+)`/`(-)` default.
    // Portage: dep/__init__.py, match_from_list (use block)
    pub(crate) fn atom_matches(
        &self,
        atom_str: &str,
        c: &Candidate,
        (category, package): (&str, &str),
        config: &Config,
    ) -> bool {
        let s = c.match_str(category, package);
        if !portage_dep::match_from_list(atom_str, &[s.as_str()]).is_some_and(|m| !m.is_empty()) {
            return false;
        }
        if !atom_str.contains('[') {
            return true;
        }
        let enabled = self.use_set(c, category, package, config);
        use_deps_hold(atom_str, &c.iuse, &enabled)
    }

    fn deps(
        &self,
        c: &Candidate,
        category: &str,
        package: &str,
        config: &Config,
    ) -> Arc<BTreeSet<String>> {
        // An installed package that stays as it is needs only its run-time
        // dependencies; one that is built or rebuilt needs them all.
        // Portage: depgraph.py, _add_pkg_deps (built pkgs, bdeps unset)
        let installed = self
            .installed_versions
            .get(&(format!("{category}/{package}"), c.slot.clone()))
            .map(String::as_str);
        let keys = dependency_keys(self.with_bdeps, installed, &c.version);
        let kept = keys.len() == RUNTIME_KEYS.len();
        let key = format!("{}{}", c.match_str(category, package), u8::from(kept));
        self.deps.get_or_compute(key, || {
            Arc::new(self.dependency_atoms(c, (category, package), config, keys))
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
                    self.atom_matches(atom_str, c, (&atom.category, &atom.package), config)
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
        (category, package): (&str, &str),
        config: &Config,
        keys: &[&str],
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
        for key in keys {
            let Some(text) = meta.get(*key) else { continue };
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
                    // `[flag?]`, `[flag=]` ... become plain `[flag]`/`[-flag]`
                    // against this package's USE; a dropped one leaves none.
                    out.insert(
                        portage_dep::evaluate_atom_conditionals(&tok, &use_set).unwrap_or(tok),
                    );
                }
            }
        }
        out
    }
}

/// The `*DEPEND` variables to follow: an installed version that stays as it
/// is needs only its run-time ones, unless `--with-bdeps`.
fn dependency_keys(
    with_bdeps: bool,
    installed: Option<&str>,
    version: &str,
) -> &'static [&'static str] {
    if !with_bdeps && installed == Some(version) {
        &RUNTIME_KEYS
    } else {
        &DEP_KEYS
    }
}

/// Whether the `[flag]` / `[-flag]` dependencies of `atom_str` hold for a
/// package declaring `iuse` and enabling `enabled`. A flag the package does
/// not declare is judged by the dependency's `(+)`/`(-)` default; a flag
/// that is enabled counts as declared (implicit IUSE).
fn use_deps_hold(atom_str: &str, iuse: &str, enabled: &HashSet<String>) -> bool {
    let Some(use_deps) = portage_dep::parse_atom(atom_str).and_then(|a| a.use_deps) else {
        return true;
    };
    let mut declared: HashSet<String> = iuse
        .split_whitespace()
        .map(|f| f.trim_start_matches(['+', '-']).to_string())
        .collect();
    declared.extend(enabled.iter().cloned());
    portage_dep::use_deps_satisfied(&use_deps, &declared, enabled)
}

const RUNTIME_KEYS: [&str; 2] = ["RDEPEND", "PDEPEND"];
const DEP_KEYS: [&str; 5] = ["DEPEND", "RDEPEND", "PDEPEND", "BDEPEND", "IDEPEND"];

/// The result for one atom: for every slot it can match, the best ebuild
/// of that slot (as `cat/pkg-ver`) with the atoms it depends on.
struct AtomResult {
    /// `(cat/pkg-ver, cat/pkg:slot, dependency atoms)` per slot.
    per_slot: Vec<(String, String, Arc<BTreeSet<String>>)>,
    /// No ebuild of any repository matches the atom (the package is gone,
    /// or no version or USE setting fits it): it points at no slot at all.
    unmatched: bool,
}

/// What one analysis reads and how.
pub struct Options {
    pub root: PathBuf,
    pub config_root: PathBuf,
    pub threads: usize,
    /// More than one satisfying version also counts as invalid.
    pub unique: bool,
    pub with_bdeps: bool,
    /// Sets added to the seeds besides `@world` (`--set NAME`).
    pub extra_sets: Vec<String>,
}

impl Options {
    /// `root` and `config_root` as given, every core, no extra sets.
    #[must_use]
    pub fn new(root: PathBuf, config_root: PathBuf) -> Self {
        Options {
            root,
            config_root,
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            unique: false,
            with_bdeps: false,
            extra_sets: Vec::new(),
        }
    }
}

/// The outcome of [`analyse`]: every package slot with its versions in final order.
pub struct World {
    /// How many distinct atoms were followed.
    pub atoms: usize,
    pub slots: Vec<SlotSet>,
    /// Atoms no ebuild matches at all, with who asked for each.
    pub unmatched: Vec<(String, Vec<String>)>,
    /// `cat/pkg:slot` -> the slots it depends on, as the first pass followed them.
    pub edges: BTreeMap<String, BTreeSet<String>>,
    /// The slots the first pass followed (`cat/pkg:slot`): the ones `@world`
    /// actually needs. An atom without a slot aims at every slot of its
    /// package, so a [`SlotSet`] alone does not tell.
    pub reached: BTreeSet<String>,
}

impl World {
    /// Slots no visible version satisfies the requesters of.
    pub fn invalid(&self) -> impl Iterator<Item = &SlotSet> {
        self.slots.iter().filter(|s| s.invalid)
    }
}

/// The seed atoms: `@world` and the sets named in `opts.extra_sets`.
///
/// # Errors
/// The repositories or the configuration cannot be read.
pub fn seeds(opts: &Options) -> Result<BTreeSet<String>, String> {
    let repos = portage_repo::find_repos_quiet(&opts.config_root).map_err(|e| e.to_string())?;
    let config = load_config(&opts.config_root, &opts.root, &repos)?;
    world_atoms(opts, &repos, &config)
}

/// Follows `@world` and its dependencies and orders the versions of every slot.
///
/// # Errors
/// The repositories or the configuration cannot be read.
pub fn analyse(opts: &Options) -> Result<World, String> {
    let repos = portage_repo::find_repos_quiet(&opts.config_root).map_err(|e| e.to_string())?;
    let config = load_config(&opts.config_root, &opts.root, &repos)?;
    let memo = Memos::new(&repos, &opts.root, opts.with_bdeps);
    let seeds = world_atoms(opts, &repos, &config)?;

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
    let mut unmatched: BTreeSet<String> = BTreeSet::new();
    // atom -> the slots it reaches; slot -> the atoms it depends on.
    let mut atom_nodes: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut node_atoms: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    while !frontier.is_empty() {
        let results = parallel_map(&frontier, opts.threads, |atom| {
            process_atom(atom, &repos, &config, &memo)
        });
        let mut next = BTreeSet::new();
        for (atom, res) in frontier.iter().zip(results) {
            done.insert(atom.clone());
            if res.unmatched {
                unmatched.insert(atom.clone());
            }
            for (by, node, deps) in res.per_slot {
                atom_nodes
                    .entry(atom.clone())
                    .or_default()
                    .insert(node.clone());
                node_atoms
                    .entry(node)
                    .or_default()
                    .extend(deps.iter().cloned());
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

    let slots: Vec<SlotSet> = sets.into_iter().flatten().collect();
    let unmatched = unmatched
        .into_iter()
        .map(|atom| {
            let by = requesters
                .get(&atom)
                .map_or_else(Vec::new, |r| r.iter().cloned().collect());
            (atom, by)
        })
        .collect();
    let reached: BTreeSet<String> = atom_nodes.values().flatten().cloned().collect();
    // slot -> the slots it depends on (itself left out).
    let edges = node_atoms
        .into_iter()
        .map(|(node, atoms)| {
            let to: BTreeSet<String> = atoms
                .iter()
                .filter_map(|a| atom_nodes.get(a))
                .flatten()
                .filter(|n| **n != node)
                .cloned()
                .collect();
            (node, to)
        })
        .collect();
    Ok(World {
        atoms: done.len(),
        slots,
        unmatched,
        edges,
        reached,
    })
}

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
    let empty = |unmatched| AtomResult {
        per_slot: Vec::new(),
        unmatched,
    };
    let Some(atom) = portage_dep::parse_atom(atom_str) else {
        return empty(false);
    };
    let Ok(all) = portage_repo::list_candidates(repos, &atom.category, &atom.package) else {
        return empty(true);
    };
    let mut best: BTreeMap<&str, (&Candidate, bool)> = BTreeMap::new();
    for c in all
        .iter()
        .filter(|c| memo.atom_matches(atom_str, c, (&atom.category, &atom.package), config))
    {
        let visible = memo.visible(c, &atom.category, &atom.package, config);
        let slot = best.entry(c.slot.as_str()).or_insert((c, visible));
        if rank(c, visible, slot.0, slot.1) == Ordering::Greater {
            *slot = (c, visible);
        }
    }
    let unmatched = best.is_empty();
    if atom.slot.is_none() {
        let cp = format!("{}/{}", atom.category, atom.package);
        narrow_slotless(&mut best, memo.installed_slots.get(&cp), |a, b| {
            rank(a.0, a.1, b.0, b.1)
        });
    }
    AtomResult {
        per_slot: best
            .into_values()
            // A masked best is never installed: its dependencies don't count.
            .filter(|(_, visible)| *visible)
            .map(|(c, _)| {
                (
                    format!("{}/{}-{}", atom.category, atom.package, c.version),
                    format!("{}/{}:{}", atom.category, atom.package, c.slot),
                    memo.deps(c, &atom.category, &atom.package, config),
                )
            })
            .collect(),
        unmatched,
    }
}

/// An atom without a slot is one requirement, not one per slot: it is met
/// by the visible slots already installed, else by the best slot alone.
fn narrow_slotless<T>(
    best: &mut BTreeMap<&str, (T, bool)>,
    installed: Option<&HashSet<String>>,
    cmp: impl Fn(&(T, bool), &(T, bool)) -> Ordering,
) {
    if best.len() < 2 {
        return;
    }
    let keep: Vec<&str> = best
        .iter()
        .filter(|(slot, (_, visible))| {
            *visible
                && installed.is_some_and(|h| h.contains(slot.split('/').next().unwrap_or(slot)))
        })
        .map(|(slot, _)| *slot)
        .collect();
    if keep.is_empty() {
        let top = best
            .iter()
            .max_by(|a, b| cmp(a.1, b.1))
            .map(|(slot, _)| *slot);
        best.retain(|slot, _| Some(*slot) == top);
    } else {
        best.retain(|slot, _| keep.contains(slot));
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
    opts: &Options,
    repos: &[RepoConfig],
    config: &Config,
) -> Result<BTreeSet<String>, String> {
    let (root, config_root) = (&opts.root, &opts.config_root);
    let mut atoms = BTreeSet::new();
    let mut sets = SetResolver::new(root, config_root, repos, config);
    sets.expand("selected", &mut atoms);
    for name in &opts.extra_sets {
        sets.expand(name, &mut atoms);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots as `(slot, version, visible)`; version is the rank.
    fn narrow(slots: &[(&'static str, u32, bool)], installed: &[&str]) -> Vec<&'static str> {
        let mut best: BTreeMap<&str, (u32, bool)> =
            slots.iter().map(|&(s, v, vis)| (s, (v, vis))).collect();
        let have: HashSet<String> = installed.iter().map(|s| s.to_string()).collect();
        narrow_slotless(&mut best, Some(&have), |a, b| {
            a.1.cmp(&b.1).then(a.0.cmp(&b.0))
        });
        best.into_keys().collect()
    }

    #[test]
    fn installed_slot_meets_the_atom() {
        // 1.90 is installed; the newer 1.99 is not needed for a slotless atom.
        assert_eq!(
            narrow(&[("1.90", 90, true), ("1.99", 99, true)], &["1.90"]),
            ["1.90"]
        );
    }

    #[test]
    fn every_installed_slot_is_kept() {
        assert_eq!(
            narrow(
                &[("1", 1, true), ("2", 2, true), ("3", 3, true)],
                &["1", "3"]
            ),
            ["1", "3"]
        );
    }

    #[test]
    fn nothing_installed_takes_the_best_slot_only() {
        assert_eq!(
            narrow(&[("1.90", 90, true), ("1.99", 99, true)], &[]),
            ["1.99"]
        );
    }

    #[test]
    fn a_visible_slot_beats_a_higher_masked_one() {
        // The experiment's `rank` puts visible first; so does the cmp here.
        let mut best: BTreeMap<&str, (u32, bool)> =
            [("a", (5, false)), ("b", (1, true))].into_iter().collect();
        narrow_slotless(&mut best, None, |x, y| x.1.cmp(&y.1).then(x.0.cmp(&y.0)));
        assert_eq!(best.into_keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn an_installed_but_masked_slot_does_not_count() {
        assert_eq!(narrow(&[("1", 1, false), ("2", 2, true)], &["1"]), ["2"]);
    }

    #[test]
    fn an_unchanged_installed_version_follows_runtime_edges_only() {
        assert_eq!(dependency_keys(false, Some("1.0"), "1.0"), RUNTIME_KEYS);
    }

    #[test]
    fn a_rebuilt_or_new_version_follows_every_edge() {
        assert_eq!(dependency_keys(false, Some("1.0"), "1.1"), DEP_KEYS);
        assert_eq!(dependency_keys(false, None, "1.0"), DEP_KEYS);
    }

    #[test]
    fn with_bdeps_follows_every_edge_even_when_unchanged() {
        assert_eq!(dependency_keys(true, Some("1.0"), "1.0"), DEP_KEYS);
    }

    fn flags(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_use_dep_needs_the_flag_enabled() {
        let atom = "dev-libs/foo[ssl]";
        assert!(use_deps_hold(atom, "+ssl doc", &flags(&["ssl"])));
        assert!(!use_deps_hold(atom, "ssl doc", &flags(&[])));
    }

    #[test]
    fn a_negative_use_dep_needs_the_flag_disabled() {
        let atom = "dev-libs/foo[-ssl]";
        assert!(use_deps_hold(atom, "ssl", &flags(&[])));
        assert!(!use_deps_hold(atom, "ssl", &flags(&["ssl"])));
    }

    #[test]
    fn an_undeclared_flag_follows_the_default_or_fails() {
        assert!(!use_deps_hold("dev-libs/foo[ssl]", "doc", &flags(&[])));
        assert!(use_deps_hold("dev-libs/foo[ssl(+)]", "doc", &flags(&[])));
        assert!(!use_deps_hold("dev-libs/foo[ssl(-)]", "doc", &flags(&[])));
    }

    #[test]
    fn an_atom_without_use_deps_always_holds() {
        assert!(use_deps_hold("dev-libs/foo", "", &flags(&[])));
    }

    #[test]
    fn a_single_slot_is_left_alone() {
        assert_eq!(narrow(&[("0", 1, false)], &[]), ["0"]);
    }

    /// The slots `parent` (`cat/pkg:0`) depends on, as the first pass followed them.
    fn edges_of(world: &World, parent: &str) -> BTreeSet<String> {
        world
            .edges
            .get(parent)
            .unwrap_or_else(|| panic!("{parent} was not followed"))
            .clone()
    }

    fn set(slots: &[&str]) -> BTreeSet<String> {
        slots.iter().map(|s| s.to_string()).collect()
    }

    /// One parent per (form, own-flag state). Each asks for `w701flagon`
    /// (flag ON) and `w701flagoff` (flag OFF) with the same relative `[...]`,
    /// so which child it still reaches tells how the form was evaluated:
    /// `[flag]` reaches the ON child, `[-flag]` the OFF one, a dropped
    /// dependency both.
    fn relative_use_edges(form: &str, state: &str) -> BTreeSet<String> {
        let parent = format!("dev-libs/w701par-{form}-{state}");
        let world = testkit::analyse_fixture(&[&parent], &[]);
        edges_of(&world, &format!("{parent}:0"))
    }

    const ON: &str = "dev-libs/w701flagon:0";
    const OFF: &str = "dev-libs/w701flagoff:0";

    /// PMS `dependencies.tex`, "2-style and 4-style USE dependencies" (`[opt?]`) / charter property 1: `[flag?]` means `[flag]`
    /// when the asking package has the flag on and no constraint when off.
    #[test]
    fn a_use_dep_if_on_follows_the_asking_package_flag() {
        assert_eq!(relative_use_edges("ifp", "on"), set(&[ON]));
        assert_eq!(relative_use_edges("ifp", "off"), set(&[ON, OFF]));
    }

    /// PMS `dependencies.tex`, "2-style and 4-style USE dependencies" (`[!opt?]`) / charter property 1: `[!flag?]` means
    /// no constraint when the asker has the flag on and `[-flag]` when off.
    #[test]
    fn a_use_dep_if_off_follows_the_asking_package_flag() {
        assert_eq!(relative_use_edges("ifn", "on"), set(&[ON, OFF]));
        assert_eq!(relative_use_edges("ifn", "off"), set(&[OFF]));
    }

    /// PMS `dependencies.tex`, "2-style and 4-style USE dependencies" (`[opt=]`) / charter property 1: `[flag=]` copies the
    /// asking package's flag state.
    #[test]
    fn a_use_dep_equal_copies_the_asking_package_flag() {
        assert_eq!(relative_use_edges("eqp", "on"), set(&[ON]));
        assert_eq!(relative_use_edges("eqp", "off"), set(&[OFF]));
    }

    /// PMS `dependencies.tex`, "2-style and 4-style USE dependencies" (`[!opt=]`) / charter property 1: `[!flag=]` is the
    /// opposite of the asking package's flag state.
    #[test]
    fn a_use_dep_opposite_inverts_the_asking_package_flag() {
        assert_eq!(relative_use_edges("eqn", "on"), set(&[OFF]));
        assert_eq!(relative_use_edges("eqn", "off"), set(&[ON]));
    }

    /// Charter property 1: a `||` branch whose packages are installed is
    /// taken before an earlier branch that is merely available.
    #[test]
    fn an_installed_branch_is_taken_first() {
        let world = testkit::analyse_fixture(&["dev-libs/w701or"], &[("dev-libs/w701orb-1", "0")]);
        assert_eq!(
            edges_of(&world, "dev-libs/w701or:0"),
            set(&["dev-libs/w701orb:0"])
        );
    }

    /// Charter property 1: with no branch installed, the first visible
    /// branch is taken.
    #[test]
    fn with_nothing_installed_the_first_visible_branch_is_taken() {
        let world = testkit::analyse_fixture(&["dev-libs/w701or"], &[]);
        assert_eq!(
            edges_of(&world, "dev-libs/w701or:0"),
            set(&["dev-libs/w701ora:0"])
        );
    }

    /// Charter property 1: a first branch no visible ebuild satisfies
    /// (keyworded `~amd64`) yields to the next one.
    #[test]
    fn a_masked_first_branch_yields_to_the_next() {
        let world = testkit::analyse_fixture(&["dev-libs/w701orm"], &[]);
        assert_eq!(
            edges_of(&world, "dev-libs/w701orm:0"),
            set(&["dev-libs/w701orb:0"])
        );
    }
}
