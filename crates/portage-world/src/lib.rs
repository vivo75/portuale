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
//! A kept package that must be built again ([`World::rebuilds`]) follows every
//! edge, like a built one; with `emptytree` that is every reached package.
//!
//! A `[foo]` / `[-foo]` USE dependency filters the versions an atom matches
//! by the candidate's own USE; the parent-relative forms (`[foo?]`, `[foo=]`)
//! are first evaluated against the USE of the package that asks.
//!
//! Known cuts: a repository without `metadata/md5-cache` is read through the
//! aux-metadata provider the caller registers (`portage_repo::register_aux_metadata_provider`);
//! an ebuild the provider cannot read is no candidate and is listed in
//! `World::unreadable`; virtuals are ordinary packages; a USE-dependency flag the
//! candidate does not declare counts as declared when it is enabled, so an
//! implicit-IUSE flag that is off is judged undeclared.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

mod memo;
mod order;
mod rebuild;
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
        analyse_fixture_with(world, installed, |_| {})
    }

    /// [`analyse_fixture`] with `tune` applied to the [`Options`] first.
    pub(crate) fn analyse_fixture_with(
        world: &[&str],
        installed: &[(&str, &str)],
        tune: impl FnOnce(&mut Options),
    ) -> World {
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
        tune(&mut opts);
        analyse(&opts).expect("the fixture tree analyses")
    }
}

pub use order::{Order, Pass, Row};
pub use rebuild::RefusedBinding;
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
    /// Plan modes name the best slot of a slotless atom even when an older
    /// slot is installed; the check counts installed slots as meeting it. The
    /// same flag gates the plan-only `||` preference: a branch every atom of
    /// which the best visible candidate of its slot matches outranks a merely
    /// installed one.
    newest_slot: bool,
    /// `(cat/pkg, slot)` -> installed version, to tell a rebuild from a keep.
    installed_versions: HashMap<(String, String), String>,
    /// `cat/pkg:slot` of installed packages a plan builds again: they follow
    /// every dependency, as a built package does.
    rebuilds: BTreeSet<String>,
    /// Follow `DEPEND`/`BDEPEND`/`IDEPEND` of an installed, unchanged
    /// package too (`--with-bdeps`); Portage does not by default.
    with_bdeps: bool,
    visible: Memo<bool>,
    use_sets: Memo<Arc<HashSet<String>>>,
    atom_available: Memo<bool>,
    /// `||` branch -> how good it is, memoised: the plan-mode rank scans
    /// every atom's candidates, like [`Memos::atom_available`] does.
    branch_preference: Memo<AltPreference>,
    /// Dependency atom -> the `*DEPEND` variables naming it (bits of [`DEP_KEYS`]).
    deps: Memo<Arc<DepAtoms>>,
}

impl<'a> Memos<'a> {
    fn new(
        repos: &'a [RepoConfig],
        root: &Path,
        with_bdeps: bool,
        rebuilds: BTreeSet<String>,
        newest_slot: bool,
    ) -> Self {
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
            rebuilds,
            newest_slot,
            with_bdeps,
            visible: Memo::new(),
            use_sets: Memo::new(),
            atom_available: Memo::new(),
            branch_preference: Memo::new(),
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

    fn deps(&self, c: &Candidate, category: &str, package: &str, config: &Config) -> Arc<DepAtoms> {
        // An installed package that stays as it is needs only its run-time
        // dependencies; one that is built or rebuilt needs them all.
        // Portage: depgraph.py, _add_pkg_deps (built pkgs, bdeps unset)
        let installed = self
            .installed_versions
            .get(&(format!("{category}/{package}"), c.slot.clone()))
            .map(String::as_str)
            .filter(|_| {
                !self
                    .rebuilds
                    .contains(&format!("{category}/{package}:{}", c.slot))
            });
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
    /// one with an atom no visible ebuild satisfies is never chosen. In plan
    /// mode a branch every non-blocker atom of which the best visible
    /// candidate of its slot matches outranks a merely installed one, so the
    /// plan takes the arm that admits the newest version.
    // Portage: dep_check.py, dep_zapdeps
    pub(crate) fn branch_preference(&self, branch: &[String], config: &Config) -> AltPreference {
        self.branch_preference
            .get_or_compute(branch.join("\n"), || {
                self.branch_preference_uncached(branch, config)
            })
    }

    /// [`branch_preference`](Self::branch_preference) without the memo.
    fn branch_preference_uncached(&self, branch: &[String], config: &Config) -> AltPreference {
        let kept: Vec<(&String, portage_dep::Atom)> = branch
            .iter()
            .filter_map(|t| {
                let atom = portage_repo::parse_dep_atom(t, "portage-world")?;
                (atom.blocker == Blocker::None).then_some((t, atom))
            })
            .collect();
        if !kept.iter().all(|(t, _)| self.atom_available(t, config)) {
            return AltPreference::Unsatisfiable;
        }
        let installed = kept.iter().all(|(_, a)| {
            self.installed
                .contains(&format!("{}/{}", a.category, a.package))
        });
        if !self.newest_slot {
            return if installed {
                AltPreference::Installed
            } else {
                AltPreference::Available
            };
        }
        let best_visible = kept.iter().all(|(t, a)| {
            let Ok(all) = portage_repo::list_candidates(self.repos, &a.category, &a.package) else {
                return false;
            };
            let mut best: Option<&Candidate> = None;
            for c in all.iter() {
                // No slot named: the package's best visible; a slot named:
                // the best visible in that main slot (never a parallel one).
                if a.slot.as_deref().is_some_and(|s| s != c.slot) {
                    continue;
                }
                if !self.visible(c, &a.category, &a.package, config) {
                    continue;
                }
                if best.is_none_or(|b| rank(c, true, b, true) == Ordering::Greater) {
                    best = Some(c);
                }
            }
            best.is_some_and(|c| self.atom_matches(t, c, (&a.category, &a.package), config))
        });
        match (best_visible, installed) {
            (true, true) => AltPreference::BestVisibleInstalled,
            (true, false) => AltPreference::BestVisibleAvailable,
            (false, true) => AltPreference::Installed,
            (false, false) => AltPreference::Available,
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
    ) -> BTreeMap<String, u8> {
        let pf = format!("{package}-{}", best.version);
        let Ok(meta) = portage_repo::repo_aux_metadata(&best.repo_location, category, &pf) else {
            return BTreeMap::new();
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
        let mut out = BTreeMap::new();
        for key in keys {
            let Some(text) = meta.get(*key) else { continue };
            let bit = DEP_KEYS
                .iter()
                .position(|k| k == key)
                .map_or(0, |i| 1u8 << i);
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
                if let Some(a) = portage_repo::parse_dep_atom(&tok, "portage-world")
                    && a.blocker == Blocker::None
                {
                    // `[flag?]`, `[flag=]` ... become plain `[flag]`/`[-flag]`
                    // against this package's USE; a dropped one leaves none.
                    let atom =
                        portage_dep::evaluate_atom_conditionals(&tok, &use_set).unwrap_or(tok);
                    *out.entry(atom).or_default() |= bit;
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
/// Dependency atom -> the `*DEPEND` variables naming it (bits of [`DEP_KEYS`]).
type DepAtoms = BTreeMap<String, u8>;
/// Every `*DEPEND` variable; an edge's kind mask has bit `i` set for `DEP_KEYS[i]`.
pub const DEP_KEYS: [&str; 5] = ["DEPEND", "RDEPEND", "PDEPEND", "BDEPEND", "IDEPEND"];

/// The result for one atom: for every slot it can match, the best ebuild
/// of that slot (as `cat/pkg-ver`) with the atoms it depends on.
struct AtomResult {
    /// `(cat/pkg-ver, cat/pkg:slot, dependency atoms with their kinds)` per slot.
    per_slot: Vec<(String, String, Arc<DepAtoms>)>,
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
    /// Work out which kept packages a plan must build again
    /// ([`World::rebuilds`]); a check of `@world` alone does not need them.
    /// It also narrows a slotless atom to its best slot (a plan); a check
    /// keeps the installed slots instead.
    pub rebuilds: bool,
    /// With `rebuilds`: an installed package whose recorded `USE`, cut to the `IUSE` of the
    /// version kept, differs from the configured one is built again.
    pub newuse: bool,
    /// With `rebuilds`: everything is built, so every reached slot with a
    /// chosen version is a row and a kept one is built again.
    pub emptytree: bool,
    /// Sets added to the seeds besides `@world` (`--set NAME`).
    pub extra_sets: Vec<String>,
    /// Atoms and `@set` names to start from instead of `@world`; empty
    /// means `@world`.
    pub targets: Vec<String>,
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
            rebuilds: false,
            newuse: false,
            emptytree: false,
            extra_sets: Vec::new(),
            targets: Vec::new(),
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
    /// `(slot, slot it depends on)` -> the `*DEPEND` variables behind that
    /// edge, as bits of [`DEP_KEYS`].
    pub edge_kinds: BTreeMap<(String, String), u8>,
    /// The slots the first pass followed (`cat/pkg:slot`): the ones `@world`
    /// actually needs. An atom without a slot aims at every slot of its
    /// package, so a [`SlotSet`] alone does not tell.
    pub reached: BTreeSet<String>,
    /// The reached slots whose installed version a plan keeps but builds
    /// again (`cat/pkg:slot`): a recorded `:slot/sub-slot=` dependency no longer
    /// matches the provider the plan chooses, or (with `newuse`) the USE changed.
    pub rebuilds: BTreeSet<String>,
    /// The stale slot-operator bindings the plan does not repair
    /// ([`RefusedBinding`]): kept consumers the scan refused, each named once.
    pub refused: Vec<RefusedBinding>,
    /// `cat/pkg:slot` -> the enabled USE flags of the version a plan would
    /// install there, for each reached slot with a change.
    pub planned_use: BTreeMap<String, BTreeSet<String>>,
    /// `cat/pkg-ver` of the ebuilds of every reached package whose metadata
    /// could not be read: they are no candidates, so their dependencies
    /// are missing from the walk. Sorted.
    pub unreadable: Vec<String>,
}

impl World {
    /// The unreadable ebuilds of the package `atom` names.
    #[must_use]
    pub fn unreadable_for(&self, atom: &str) -> Vec<&str> {
        let Some(a) = portage_dep::parse_atom(atom) else {
            return Vec::new();
        };
        let prefix = format!("{}/{}-", a.category, a.package);
        self.unreadable
            .iter()
            .map(String::as_str)
            .filter(|u| {
                u.strip_prefix(&prefix)
                    .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
            })
            .collect()
    }

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
    // A package that must be built again follows more dependencies, which
    // can need more rebuilds: walk again until the set stops growing.
    let mut rebuilds = BTreeSet::new();
    loop {
        let mut world = walk(opts, (&repos, &config), &rebuilds)?;
        if !opts.rebuilds {
            return Ok(world);
        }
        let mut stale = if opts.emptytree {
            rebuild::StaleBindings {
                rebuild: rebuild::every_kept(&world, &opts.root),
                refused: Vec::new(),
            }
        } else {
            let memos = Memos::new(
                &repos,
                &opts.root,
                opts.with_bdeps,
                rebuilds.clone(),
                opts.rebuilds,
            );
            rebuild::slot_operator(&world, &opts.root, &memos, &config)
        };
        if opts.newuse {
            stale
                .rebuild
                .extend(rebuild::changed_use(&world, &opts.root, &config));
        }
        if stale.rebuild.is_subset(&rebuilds) {
            world.rebuilds = rebuilds;
            world.refused = stale.refused;
            return Ok(world);
        }
        rebuilds.extend(stale.rebuild);
    }
}

/// One walk of `@world`, with the `rebuilds` followed as built packages.
fn walk(
    opts: &Options,
    (repos, config): (&[RepoConfig], &Config),
    rebuilds: &BTreeSet<String>,
) -> Result<World, String> {
    let memo = Memos::new(
        repos,
        &opts.root,
        opts.with_bdeps,
        rebuilds.clone(),
        opts.rebuilds,
    );
    let seeds = world_atoms(opts, repos, config)?;

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
    let mut node_atoms: BTreeMap<String, BTreeMap<String, u8>> = BTreeMap::new();

    while !frontier.is_empty() {
        let results = parallel_map(&frontier, opts.threads, |atom| {
            process_atom(atom, repos, config, &memo)
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
                let kinds = node_atoms.entry(node).or_default();
                for (dep, mask) in deps.iter() {
                    *kinds.entry(dep.clone()).or_default() |= mask;
                }
                for dep in deps.keys().cloned() {
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
    let unreadable = unreadable_of(&jobs, repos, opts.threads);
    let ctx = reorder::Ctx {
        requesters: &requesters,
        repos,
        config,
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
    let planned_use = planned_use(&slots, &reached, rebuilds, config);
    // slot -> the slots it depends on (itself left out).
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut edge_kinds: BTreeMap<(String, String), u8> = BTreeMap::new();
    for (node, atoms) in node_atoms {
        let to = edges.entry(node.clone()).or_default();
        for (atom, mask) in &atoms {
            for dep in atom_nodes.get(atom).into_iter().flatten() {
                if *dep != node {
                    to.insert(dep.clone());
                    *edge_kinds.entry((node.clone(), dep.clone())).or_default() |= mask;
                }
            }
        }
    }
    Ok(World {
        atoms: done.len(),
        slots,
        unmatched,
        edges,
        edge_kinds,
        reached,
        rebuilds: BTreeSet::new(),
        refused: Vec::new(),
        planned_use,
        unreadable,
    })
}

/// The enabled USE of the chosen version of every reached slot a plan changes.
fn planned_use(
    slots: &[SlotSet],
    reached: &BTreeSet<String>,
    rebuilds: &BTreeSet<String>,
    config: &Config,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for slot in slots {
        let Some(best) = slot.chosen() else { continue };
        if !reached.contains(&slot.node())
            || (slot.change().is_none() && !rebuilds.contains(&slot.node()))
        {
            continue;
        }
        let (category, package) = slot.cp.split_once('/').unwrap_or_default();
        let flags = portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
            config,
            iuse: &best.iuse,
            keywords: &best.keywords,
            candidate_str: &best.match_str(category, package),
            build_id: None,
            category,
            package,
        });
        out.insert(slot.node(), flags.iter().cloned().collect());
    }
    out
}

/// The unreadable ebuilds of every reached package, sorted and unique.
fn unreadable_of<T: Sync>(
    jobs: &[((String, String), T)],
    repos: &[RepoConfig],
    threads: usize,
) -> Vec<String> {
    let found = parallel_map(jobs, threads, |((cat, pkg), _)| {
        portage_repo::unreadable_ebuilds(repos, cat, pkg)
    });
    let mut all: Vec<String> = found.into_iter().flatten().collect();
    all.sort();
    all.dedup();
    all
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
        narrow_slotless(
            &mut best,
            memo.installed_slots.get(&cp),
            memo.newest_slot,
            |a, b| rank(a.0, a.1, b.0, b.1),
        );
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

/// An atom without a slot is one requirement, not one per slot. A check is
/// met by the visible slots already installed, else by the best slot alone;
/// a plan names the best slot alone, so the newest version wins.
fn narrow_slotless<T>(
    best: &mut BTreeMap<&str, (T, bool)>,
    installed: Option<&HashSet<String>>,
    newest_slot: bool,
    cmp: impl Fn(&(T, bool), &(T, bool)) -> Ordering,
) {
    if best.len() < 2 {
        return;
    }
    let keep: Vec<&str> = if newest_slot {
        Vec::new()
    } else {
        best.iter()
            .filter(|(slot, (_, visible))| {
                *visible
                    && installed.is_some_and(|h| h.contains(slot.split('/').next().unwrap_or(slot)))
            })
            .map(|(slot, _)| *slot)
            .collect()
    };
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

/// The atoms of the three sets `@world` is made of.
pub struct SeedSets {
    pub system: BTreeSet<String>,
    pub profile: BTreeSet<String>,
    /// The world file and every set named in `world_sets`.
    pub selected: BTreeSet<String>,
}

/// `@system`, `@profile` and `@selected`, each on its own.
///
/// # Errors
/// The repositories or the configuration cannot be read.
pub fn seed_sets(opts: &Options) -> Result<SeedSets, String> {
    let repos = portage_repo::find_repos_quiet(&opts.config_root).map_err(|e| e.to_string())?;
    let config = load_config(&opts.config_root, &opts.root, &repos)?;
    let mut selected = BTreeSet::new();
    SetResolver::new(&opts.root, &opts.config_root, &repos, &config)
        .expand("selected", &mut selected);
    selected.retain(|a| portage_dep::parse_atom(a).is_some());
    Ok(SeedSets {
        system: system_atoms(&config),
        profile: profile_atoms(&config),
        selected,
    })
}

fn system_atoms(config: &Config) -> BTreeSet<String> {
    config
        .system_packages
        .iter()
        .map(|a| a.trim_start_matches('*').to_string())
        .filter(|a| portage_dep::parse_atom(a).is_some())
        .collect()
}

fn profile_atoms(config: &Config) -> BTreeSet<String> {
    config
        .profile_packages
        .iter()
        .filter(|a| portage_dep::parse_atom(a).is_some())
        .cloned()
        .collect()
}

/// The seed atoms: `opts.targets` when given, else `@world` as
/// `portage.conf` defines it (`@profile @selected @system`: the world file,
/// every set named in `world_sets` expanded through `sets.conf`, `@system`
/// and `@profile`), plus `opts.extra_sets` either way.
fn world_atoms(
    opts: &Options,
    repos: &[RepoConfig],
    config: &Config,
) -> Result<BTreeSet<String>, String> {
    let (root, config_root) = (&opts.root, &opts.config_root);
    let mut atoms = BTreeSet::new();
    let mut sets = SetResolver::new(root, config_root, repos, config);
    let world = ["@world".to_string()];
    let targets = if opts.targets.is_empty() {
        &world[..]
    } else {
        &opts.targets[..]
    };
    for target in targets {
        match target.as_str() {
            "@world" => {
                sets.expand("selected", &mut atoms);
                atoms.extend(system_atoms(config));
                atoms.extend(profile_atoms(config));
            }
            "@system" => atoms.extend(system_atoms(config)),
            "@profile" => atoms.extend(profile_atoms(config)),
            other => match other.strip_prefix('@') {
                Some(name) => sets.expand(name, &mut atoms),
                None => {
                    atoms.insert(other.to_string());
                }
            },
        }
    }
    for name in &opts.extra_sets {
        sets.expand(name, &mut atoms);
    }
    atoms.retain(|a| portage_dep::parse_atom(a).is_some());
    Ok(atoms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots as `(slot, version, visible)`; version is the rank.
    fn narrow(
        slots: &[(&'static str, u32, bool)],
        installed: &[&str],
        newest_slot: bool,
    ) -> Vec<&'static str> {
        let mut best: BTreeMap<&str, (u32, bool)> =
            slots.iter().map(|&(s, v, vis)| (s, (v, vis))).collect();
        let have: HashSet<String> = installed.iter().map(|s| s.to_string()).collect();
        narrow_slotless(&mut best, Some(&have), newest_slot, |a, b| {
            a.1.cmp(&b.1).then(a.0.cmp(&b.0))
        });
        best.into_keys().collect()
    }

    #[test]
    fn installed_slot_meets_the_atom() {
        // 1.90 is installed; the newer 1.99 is not needed for a slotless atom.
        assert_eq!(
            narrow(&[("1.90", 90, true), ("1.99", 99, true)], &["1.90"], false),
            ["1.90"]
        );
    }

    /// Charter property 3: a plan names the newest slot even when an older
    /// one is installed; the installed slot is listed only when another atom
    /// reaches it.
    #[test]
    fn plan_mode_takes_the_best_slot_only_even_when_an_older_one_is_installed() {
        assert_eq!(
            narrow(&[("1.90", 90, true), ("1.99", 99, true)], &["1.90"], true),
            ["1.99"]
        );
    }

    #[test]
    fn every_installed_slot_is_kept() {
        assert_eq!(
            narrow(
                &[("1", 1, true), ("2", 2, true), ("3", 3, true)],
                &["1", "3"],
                false,
            ),
            ["1", "3"]
        );
    }

    #[test]
    fn nothing_installed_takes_the_best_slot_only() {
        assert_eq!(
            narrow(&[("1.90", 90, true), ("1.99", 99, true)], &[], false),
            ["1.99"]
        );
    }

    #[test]
    fn a_visible_slot_beats_a_higher_masked_one() {
        // The experiment's `rank` puts visible first; so does the cmp here.
        let mut best: BTreeMap<&str, (u32, bool)> =
            [("a", (5, false)), ("b", (1, true))].into_iter().collect();
        narrow_slotless(&mut best, None, false, |x, y| {
            x.1.cmp(&y.1).then(x.0.cmp(&y.0))
        });
        assert_eq!(best.into_keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn an_installed_but_masked_slot_does_not_count() {
        assert_eq!(
            narrow(&[("1", 1, false), ("2", 2, true)], &["1"], false),
            ["2"]
        );
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
        assert_eq!(narrow(&[("0", 1, false)], &[], false), ["0"]);
    }

    /// The slots `parent` (`cat/pkg:0`) depends on, as the first pass followed them.
    fn edges_of(world: &World, parent: &str) -> BTreeSet<String> {
        world
            .edges
            .get(parent)
            .unwrap_or_else(|| panic!("{parent} was not followed"))
            .clone()
    }

    /// `odw0a` names one package per `*DEPEND` variable, and is given as a
    /// target while `@world` is empty: every edge carries its variable's bit.
    #[test]
    fn a_target_is_followed_and_each_edge_knows_its_dependency_kind() {
        let world = testkit::analyse_fixture_with(&[], &[], |o| {
            o.targets = vec!["dev-libs/odw0a".into()];
        });
        let kinds: Vec<(String, &str)> = world
            .edge_kinds
            .iter()
            .filter(|((from, _), _)| from == "dev-libs/odw0a:0")
            .map(|((_, to), mask)| {
                let key = DEP_KEYS[mask.trailing_zeros() as usize];
                assert_eq!(mask.count_ones(), 1);
                (to.clone(), key)
            })
            .collect();
        let expected = [
            ("dev-libs/odw0b:0", "DEPEND"),
            ("dev-libs/odw0c:0", "RDEPEND"),
            ("dev-libs/odw0d:0", "PDEPEND"),
            ("dev-libs/odw0e:0", "IDEPEND"),
            ("dev-libs/odw0f:0", "BDEPEND"),
        ]
        .map(|(to, key)| (to.to_string(), key));
        assert_eq!(kinds, expected);
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

    /// Charter properties 1 and 5 (PMS, metadata cache: an entry may be
    /// missing): an ebuild whose metadata cannot be read is reported, not
    /// taken for a missing package.
    #[test]
    fn an_unreadable_ebuild_is_reported() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status()
            .expect("cp runs");
        assert!(copied.success());
        let dir = tree.path().join("repo/dev-libs/w692unread");
        std::fs::create_dir_all(&dir).expect("package dir");
        std::fs::write(
            dir.join("w692unread-1.ebuild"),
            "EAPI=8\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n",
        )
        .expect("ebuild without a cache entry");
        for threads in [1, 4] {
            let world = testkit::analyse_fixture_with(&["dev-libs/w692unread"], &[], |o| {
                o.config_root = tree.path().to_path_buf();
                o.threads = threads;
            });
            assert_eq!(world.unreadable, ["dev-libs/w692unread-1"]);
            assert_eq!(
                world.unreadable_for("dev-libs/w692unread"),
                ["dev-libs/w692unread-1"]
            );
        }
    }

    /// A repository ebuild with its md5-cache entry, in the tree copy `root`.
    fn add_ebuild(root: &Path, cpv: &str, slot: &str, rdepend: &str) {
        add_ebuild_fields(root, cpv, slot, &[("RDEPEND", rdepend)]);
    }

    /// An ebuild like [`add_ebuild`], with arbitrary metadata variables
    /// (`BDEPEND`, `DEPEND`, ...) besides `EAPI`, `SLOT` and `KEYWORDS`.
    fn add_ebuild_fields(root: &Path, cpv: &str, slot: &str, fields: &[(&str, &str)]) {
        let (category, pf) = cpv.split_once('/').expect("category/pf");
        let package = pf.rsplit_once('-').expect("pf").0;
        let mut vars = vec![("EAPI", "8"), ("SLOT", slot), ("KEYWORDS", "amd64")];
        vars.extend_from_slice(fields);
        let text: String = vars.iter().map(|(k, v)| format!("{k}=\"{v}\"\n")).collect();
        let dir = root.join("repo").join(category).join(package);
        std::fs::create_dir_all(&dir).expect("package dir");
        std::fs::write(dir.join(format!("{pf}.ebuild")), &text).expect("ebuild");
        let md5 = std::process::Command::new("md5sum")
            .arg(dir.join(format!("{pf}.ebuild")))
            .output()
            .expect("md5sum runs")
            .stdout;
        let md5 = String::from_utf8_lossy(&md5);
        let cache = root.join("repo/metadata/md5-cache").join(category);
        std::fs::create_dir_all(&cache).expect("cache dir");
        let mut entry: Vec<String> = vars.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        entry.sort();
        let entry =
            entry.concat() + &format!("_md5_={}\n", md5.split(' ').next().unwrap_or_default());
        std::fs::write(cache.join(pf), entry).expect("cache entry");
    }

    /// PMS `dependencies.tex`, `sec:slot-dep`: a consumer is rebuilt when the
    /// plan moves the sub-slot its `:=` dependency recorded, and when the
    /// recorded sub-slot already differs from the installed provider's (an
    /// earlier move the plan leaves alone, backlog #741); a `:*` consumer,
    /// and one whose other recorded pins forbid the plan, are not.
    #[test]
    fn a_sub_slot_change_rebuilds_the_slot_operator_consumers_only() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(tree.path(), "dev-libs/w693prov-2", "0/2", "");
        add_ebuild(tree.path(), "dev-libs/w693calm-1", "0/1", "");
        add_ebuild(
            tree.path(),
            "dev-libs/w693cons-1",
            "0",
            "dev-libs/w693prov:=",
        );
        add_ebuild(
            tree.path(),
            "dev-libs/w693star-1",
            "0",
            "dev-libs/w693prov:*",
        );
        add_ebuild(
            tree.path(),
            "dev-libs/w693stale-1",
            "0",
            "dev-libs/w693calm:=",
        );
        let world = [
            "dev-libs/w693cons",
            "dev-libs/w693star",
            "dev-libs/w693stale",
        ];
        let installed = [
            ("dev-libs/w693prov-1", "0/1"),
            ("dev-libs/w693calm-1", "0/1"),
            ("dev-libs/w693cons-1", "0"),
            ("dev-libs/w693star-1", "0"),
            ("dev-libs/w693stale-1", "0"),
        ];
        let world = testkit::analyse_fixture_with(&world, &installed, |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
            for (cpf, recorded) in [
                ("w693cons-1", "dev-libs/w693prov:0/1="),
                ("w693star-1", "dev-libs/w693prov:*"),
                ("w693stale-1", "dev-libs/w693calm:0/9="),
            ] {
                std::fs::write(
                    o.root.join("var/db/pkg/dev-libs").join(cpf).join("RDEPEND"),
                    recorded,
                )
                .expect("recorded RDEPEND");
            }
        });
        assert_eq!(
            world.rebuilds.iter().collect::<Vec<_>>(),
            ["dev-libs/w693cons:0", "dev-libs/w693stale:0"]
        );
    }

    /// PMS `dependencies.tex`, `sec:slot-dep`: a kept consumer whose recorded
    /// `:=` binding names a sub-slot the installed provider no longer has is
    /// rebuilt even though the plan leaves the provider alone (backlog #741).
    #[test]
    fn a_kept_package_with_a_stale_binding_is_rebuilt() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(tree.path(), "dev-libs/w741prov-2", "0/2", "");
        add_ebuild(
            tree.path(),
            "dev-libs/w741user-1",
            "0",
            "dev-libs/w741prov:=",
        );
        let installed = [("dev-libs/w741prov-2", "0/2"), ("dev-libs/w741user-1", "0")];
        let world = testkit::analyse_fixture_with(&["dev-libs/w741user"], &installed, |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
            std::fs::write(
                o.root
                    .join("var/db/pkg/dev-libs/w741user-1")
                    .join("RDEPEND"),
                "dev-libs/w741prov:0/1=",
            )
            .expect("recorded RDEPEND");
        });
        assert_eq!(
            world.rebuilds.iter().collect::<Vec<_>>(),
            ["dev-libs/w741user:0"]
        );
        assert!(world.refused.is_empty(), "{:?}", world.refused);
    }

    /// Charter property 1 with the owner's guard: a stale consumer whose
    /// recorded versioned dep forbids the plan's chosen provider version is
    /// refused, not scheduled, so the rebuild cannot hold the provider back.
    #[test]
    fn a_rebuild_that_would_hold_back_a_changed_provider_is_refused() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(tree.path(), "dev-libs/w741core-2", "0/2", "");
        add_ebuild(tree.path(), "dev-libs/w741user-1", "0", "dev-libs/w741core");
        let installed = [("dev-libs/w741core-1", "0/1"), ("dev-libs/w741user-1", "0")];
        let world = testkit::analyse_fixture_with(&["dev-libs/w741user"], &installed, |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
            std::fs::write(
                o.root
                    .join("var/db/pkg/dev-libs/w741user-1")
                    .join("RDEPEND"),
                "=dev-libs/w741core-1* dev-libs/w741core:0/9=",
            )
            .expect("recorded RDEPEND");
        });
        assert!(world.rebuilds.is_empty(), "{:?}", world.rebuilds);
        assert_eq!(
            world.refused,
            [crate::rebuild::RefusedBinding {
                consumer: "dev-libs/w741user".to_string(),
                atom: "=dev-libs/w741core-1*".to_string(),
                provider: Some("dev-libs/w741core-2".to_string()),
            }]
        );
    }

    /// A stale consumer with no visible ebuild cannot be rebuilt: it is
    /// refused, with the provider the binding names when it is installed.
    #[test]
    fn a_consumer_with_no_visible_ebuild_is_not_rebuilt() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(tree.path(), "dev-libs/w741prov-2", "0/2", "");
        let installed = [("dev-libs/w741prov-2", "0/2"), ("dev-libs/w741gone-1", "0")];
        let world = testkit::analyse_fixture_with(&["dev-libs/w741gone"], &installed, |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
            std::fs::write(
                o.root
                    .join("var/db/pkg/dev-libs/w741gone-1")
                    .join("RDEPEND"),
                "dev-libs/w741prov:0/1=",
            )
            .expect("recorded RDEPEND");
        });
        assert!(world.rebuilds.is_empty(), "{:?}", world.rebuilds);
        assert_eq!(
            world.refused,
            [crate::rebuild::RefusedBinding {
                consumer: "dev-libs/w741gone".to_string(),
                atom: "dev-libs/w741prov:0/1=".to_string(),
                provider: None,
            }]
        );
    }

    /// A tree shaped like backlog #746: `w746auto` builds with `w746help`,
    /// which needs `w746loc` at run time, which needs `w746get`.
    fn build_time_in_fact_tree(root: &Path) {
        add_ebuild_fields(
            root,
            "dev-libs/w746auto-1",
            "0",
            &[("BDEPEND", "dev-libs/w746help")],
        );
        add_ebuild(root, "dev-libs/w746help-1", "0", "dev-libs/w746loc");
        add_ebuild(root, "dev-libs/w746loc-1", "0", "dev-libs/w746get");
        add_ebuild(root, "dev-libs/w746get-1", "0", "");
    }

    /// Charter property 2 (PMS 8.1: a build tool must be usable, backlog
    /// #746): a runtime edge out of a package the plan builds with is a
    /// build-time need in fact, so the cut spares it.
    #[test]
    fn a_build_tool_makes_its_runtime_closure_build_time() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        build_time_in_fact_tree(tree.path());
        let world = testkit::analyse_fixture_with(&["dev-libs/w746auto"], &[], |o| {
            o.config_root = tree.path().to_path_buf();
        });
        let used = crate::order::build_time_used(&world.edge_kinds);
        assert!(used.contains("dev-libs/w746help:0"));
        assert!(!crate::order::is_cuttable_runtime_edge(
            &world.edge_kinds,
            &used,
            &(
                "dev-libs/w746help:0".to_string(),
                "dev-libs/w746loc:0".to_string()
            ),
        ));
    }

    /// Charter property 2 (backlog #746): a runtime edge out of a package
    /// nothing builds with is not a build-time need in fact, so the cut can
    /// still serialize the cycle there.
    #[test]
    fn a_runtime_dependency_of_a_runtime_dependency_is_not_build_time() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        build_time_in_fact_tree(tree.path());
        let world = testkit::analyse_fixture_with(&["dev-libs/w746auto"], &[], |o| {
            o.config_root = tree.path().to_path_buf();
        });
        let used = crate::order::build_time_used(&world.edge_kinds);
        assert!(!used.contains("dev-libs/w746loc:0"));
        assert!(crate::order::is_cuttable_runtime_edge(
            &world.edge_kinds,
            &used,
            &(
                "dev-libs/w746loc:0".to_string(),
                "dev-libs/w746get:0".to_string()
            ),
        ));
    }

    /// The fast plan agrees with the resolver: a rebuild re-records the
    /// chosen ebuild's live dependencies, so when the tree no longer names
    /// the stale provider there is nothing left to repair -- the consumer
    /// stays out silently, neither rebuilt nor refused.
    #[test]
    fn a_consumer_whose_tree_dropped_the_dependency_is_left_alone() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(tree.path(), "dev-libs/w741prov-2", "0/2", "");
        add_ebuild(tree.path(), "dev-libs/w741solo-1", "0", "");
        let installed = [("dev-libs/w741prov-2", "0/2"), ("dev-libs/w741solo-1", "0")];
        let world = testkit::analyse_fixture_with(&["dev-libs/w741solo"], &installed, |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
            std::fs::write(
                o.root
                    .join("var/db/pkg/dev-libs/w741solo-1")
                    .join("RDEPEND"),
                "dev-libs/w741prov:0/1=",
            )
            .expect("recorded RDEPEND");
        });
        assert!(world.rebuilds.is_empty(), "{:?}", world.rebuilds);
        assert!(world.refused.is_empty(), "{:?}", world.refused);
    }

    /// The openssl shape of plan 02.740: `w740user` reaches
    /// `|| ( dev-libs/w740compat:3 dev-libs/w740prov:0/3 )` and the kept
    /// `w740sudo` (like the host's profile `@system` sudo) reaches
    /// `dev-libs/w740prov:=`; `w740prov-3` (`0/3`) is installed while
    /// `w740prov-4` (`0/4`) and `w740compat-3` are visible.
    fn newest_arm_tree(root: &Path) {
        add_ebuild(
            root,
            "dev-libs/w740user-1",
            "0",
            "|| ( dev-libs/w740compat:3 dev-libs/w740prov:0/3 )",
        );
        add_ebuild(root, "dev-libs/w740sudo-1", "0", "dev-libs/w740prov:=");
        add_ebuild(root, "dev-libs/w740prov-3", "0/3", "");
        add_ebuild(root, "dev-libs/w740prov-4", "0/4", "");
        add_ebuild(root, "dev-libs/w740compat-3", "3", "");
    }

    /// The analysis of [`newest_arm_tree`] in plan (`true`) or check
    /// (`false`) mode.
    fn newest_arm_world(rebuilds: bool) -> World {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        newest_arm_tree(tree.path());
        let installed = [("dev-libs/w740prov-3", "0/3"), ("dev-libs/w740sudo-1", "0")];
        testkit::analyse_fixture_with(
            &["dev-libs/w740user", "dev-libs/w740sudo"],
            &installed,
            |o| {
                o.config_root = tree.path().to_path_buf();
                o.rebuilds = rebuilds;
            },
        )
    }

    /// The chosen version of `cp:slot`, when the world reaches it.
    fn chosen_version(world: &World, cp: &str, slot: &str) -> Option<String> {
        world
            .slots
            .iter()
            .find(|s| s.cp == cp && s.slot == slot)
            .and_then(|s| s.chosen())
            .map(|c| c.version.clone())
    }

    /// What a plan would do to `cp:slot`: new, upgraded, or nothing.
    fn slot_change(world: &World, cp: &str, slot: &str) -> Option<Change> {
        world
            .slots
            .iter()
            .find(|s| s.cp == cp && s.slot == slot)
            .and_then(|s| s.change())
    }

    /// Charter property 3 (plan 02.740): in plan mode the `||` branch every
    /// atom of which the best visible candidate of its slot matches wins over
    /// the installed-matching branch, so the plan takes the compat arm and
    /// upgrades the provider instead of pinning the installed sub-slot.
    #[test]
    fn a_plan_prefers_the_branch_that_admits_the_best_version() {
        let world = newest_arm_world(true);
        assert_eq!(
            slot_change(&world, "dev-libs/w740compat", "3"),
            Some(Change::New)
        );
        assert_eq!(
            chosen_version(&world, "dev-libs/w740prov", "0"),
            Some("4".to_string())
        );
        assert_eq!(
            slot_change(&world, "dev-libs/w740prov", "0"),
            Some(Change::Upgrade)
        );
    }

    /// Plan 02.740: a check keeps today's `||` order (the installed arm
    /// wins), so it still sees the installed sub-slot and no compat package.
    #[test]
    fn the_same_branch_choice_in_check_mode_keeps_the_installed_one() {
        let world = newest_arm_world(false);
        assert_eq!(
            chosen_version(&world, "dev-libs/w740prov", "0"),
            Some("3".to_string())
        );
        assert_eq!(chosen_version(&world, "dev-libs/w740compat", "3"), None);
    }

    /// Plan 02.740 (the selenium `|| ( firefox firefox-bin )` shape, with
    /// `firefox-bin` installed): two best-visible arms tie on the new rank,
    /// so the installed one wins and no extra package is pulled in.
    #[test]
    fn a_best_visible_tie_prefers_the_installed_branch() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(
            tree.path(),
            "dev-libs/w740tie-1",
            "0",
            "|| ( dev-libs/w740a dev-libs/w740b )",
        );
        add_ebuild(tree.path(), "dev-libs/w740a-1", "0", "");
        add_ebuild(tree.path(), "dev-libs/w740b-1", "0", "");
        let world = testkit::analyse_fixture_with(
            &["dev-libs/w740tie"],
            &[("dev-libs/w740b-1", "0")],
            |o| {
                o.config_root = tree.path().to_path_buf();
                o.rebuilds = true;
            },
        );
        assert_eq!(
            chosen_version(&world, "dev-libs/w740b", "0"),
            Some("1".to_string())
        );
        assert_eq!(chosen_version(&world, "dev-libs/w740a", "0"), None);
    }

    /// Plan 02.740 (the openssl direction, pinned apart from the tie): an
    /// installed arm its slot's best visible version cannot satisfy loses to
    /// a best-visible uninstalled one, because keeping it would pin an older
    /// version (charter property 3).
    #[test]
    fn a_stale_installed_arm_loses_to_a_best_visible_uninstalled_one() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(
            tree.path(),
            "dev-libs/w740stale-1",
            "0",
            "|| ( dev-libs/w740alt dev-libs/w740old:0/1 )",
        );
        add_ebuild(tree.path(), "dev-libs/w740alt-1", "0", "");
        add_ebuild(tree.path(), "dev-libs/w740old-1", "0/1", "");
        add_ebuild(tree.path(), "dev-libs/w740old-2", "0/2", "");
        let world = testkit::analyse_fixture_with(
            &["dev-libs/w740stale"],
            &[("dev-libs/w740old-1", "0/1")],
            |o| {
                o.config_root = tree.path().to_path_buf();
                o.rebuilds = true;
            },
        );
        assert_eq!(
            slot_change(&world, "dev-libs/w740alt", "0"),
            Some(Change::New)
        );
        // The losing arm requests nothing: the installed package stays alone.
        assert_eq!(chosen_version(&world, "dev-libs/w740old", "0"), None);
    }

    /// Plan 02.740: a slot-restricted arm is judged against its own slot's
    /// best, never the package's newest slot, so `w740prov:1` stays
    /// best-visible with `w740prov-2:2` present.
    #[test]
    fn a_parallel_slot_arm_is_judged_against_its_own_slot() {
        let tree = tempfile::tempdir_in("/var/tmp").expect("temporary config root");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/.");
        let copied = std::process::Command::new("cp")
            .arg("-a")
            .arg(fixtures)
            .arg(tree.path())
            .status();
        assert!(copied.expect("cp runs").success());
        add_ebuild(
            tree.path(),
            "dev-libs/w740multi-1",
            "0",
            "|| ( dev-libs/w740prov:1 dev-libs/w740alt )",
        );
        add_ebuild(tree.path(), "dev-libs/w740prov-1", "1", "");
        add_ebuild(tree.path(), "dev-libs/w740prov-2", "2", "");
        add_ebuild(tree.path(), "dev-libs/w740alt-1", "0", "");
        let world = testkit::analyse_fixture_with(&["dev-libs/w740multi"], &[], |o| {
            o.config_root = tree.path().to_path_buf();
            o.rebuilds = true;
        });
        assert_eq!(
            chosen_version(&world, "dev-libs/w740prov", "1"),
            Some("1".to_string())
        );
        assert_eq!(
            slot_change(&world, "dev-libs/w740prov", "1"),
            Some(Change::New)
        );
    }
}
