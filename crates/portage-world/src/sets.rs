//! Package-set expansion for `@world`: the sets named in `world_sets`, and
//! whatever `sets.conf` defines (Portage's default `portage.conf`, then
//! `<config-root>/etc/portage/sets.conf`).
//!
//! Supported classes: `StaticFileSet` (`filename`, or `directory` +
//! `multiset`), `DummyPackageSet` (`packages`), `WorldSelectedSet` and
//! friends, `EverythingSet` (`@installed`) and `ProfilePackageSet` /
//! `PackagesSystemSet`, which are resolved from the profile by the caller.
//! Sets computed from the installed packages and the repositories:
//! `VariableSet` (`@live-rebuild`, `@rust-rebuild`, …, installed metadata
//! only), `OwnerSet` (`@module-rebuild`, `@x11-module-rebuild`),
//! `ChangedDepsSet`, `RebuiltBinaries`, `PreservedLibraryConsumerSet`
//! (`@preserved-rebuild`), `UnavailableSet`, `DowngradeSet` and
//! `SubslotChangedSet`. A set of any other class (`@security`) depends on
//! state this experiment does not read; it is reported and skipped.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use portage_profile::Config;
use portage_repo::{Candidate, RepoConfig};

const GLOBAL_SETS_CONF: &str = "/usr/share/portage/config/sets/portage.conf";

enum SetDef {
    /// One file: its lines are atoms or `@set` references.
    File(PathBuf),
    /// A directory of files, one set per file name.
    Dir(PathBuf),
    /// The `packages =` list, atoms or `@set` references.
    Packages(Vec<String>),
    /// `world_sets` and the `world` file together.
    Selected,
    Installed,
    /// Installed packages whose `variable` holds an include (and no
    /// exclude); for a `*DEPEND` variable the flattened atoms are compared.
    Variable(VariableSet),
    /// Installed packages owning a file under one of `files` (globs, below
    /// the root) and none of `exclude`.
    Owner {
        files: Vec<String>,
        exclude: Vec<String>,
    },
    /// Installed packages whose `RDEPEND`/`PDEPEND` differ from their ebuild's.
    ChangedDeps,
    /// Installed packages still linking against a preserved library.
    PreservedRebuild,
    /// Installed slots whose local binary package of the same version was
    /// built at another time.
    RebuiltBinaries,
    /// Installed slots with no visible ebuild.
    Unavailable,
    /// Installed slots whose best visible ebuild is older.
    Downgrade,
    /// Installed slots whose best visible ebuild has another sub-slot.
    SubslotChanged,
    /// `@system` and `@profile`: already seeds, nothing to add here.
    FromProfile,
    Unsupported(String),
}

struct VariableSet {
    variable: String,
    includes: Vec<String>,
    excludes: Vec<String>,
    excludes_output: Vec<String>,
}

pub struct SetResolver<'a> {
    root: &'a Path,
    repos: &'a [RepoConfig],
    config: &'a Config,
    config_root: &'a Path,
    defs: HashMap<String, SetDef>,
    seen: HashSet<String>,
}

impl<'a> SetResolver<'a> {
    pub fn new(
        root: &'a Path,
        config_root: &'a Path,
        repos: &'a [RepoConfig],
        config: &'a Config,
    ) -> Self {
        let mut defs = HashMap::new();
        for path in [
            PathBuf::from(GLOBAL_SETS_CONF),
            config_root.join("etc/portage/sets.conf"),
        ] {
            if let Ok(text) = std::fs::read_to_string(&path) {
                read_sets_conf(&text, config_root, &mut defs);
            }
        }
        // The default `usersets` section, in case no sets.conf defines it.
        defs.entry("usersets".into())
            .or_insert_with(|| SetDef::Dir(config_root.join("etc/portage/sets")));
        Self {
            root,
            repos,
            config,
            config_root,
            defs,
            seen: HashSet::new(),
        }
    }

    /// Adds the atoms of set `name` (no `@`) to `out`, following nested
    /// `@set` references once each.
    pub fn expand(&mut self, name: &str, out: &mut BTreeSet<String>) {
        if !self.seen.insert(name.to_string()) {
            return;
        }
        let lines = match self.lookup(name) {
            Lookup::Lines(l) => l,
            Lookup::Nothing => return,
            Lookup::Missing => {
                eprintln!(
                    "experiment-pre-sort-atoms: set '{name}' not found (sets.conf or {}/etc/portage/sets)",
                    self.config_root.display()
                );
                return;
            }
            Lookup::Unsupported(class) => {
                eprintln!(
                    "experiment-pre-sort-atoms: set '{name}' (class {class}) is not supported, skipped"
                );
                return;
            }
        };
        for line in lines {
            match line.strip_prefix('@') {
                Some(nested) => self.expand(nested, out),
                None => {
                    out.insert(line);
                }
            }
        }
    }

    fn lookup(&self, name: &str) -> Lookup {
        match self.defs.get(name) {
            Some(SetDef::File(p)) => file_lines(p),
            Some(SetDef::Packages(p)) => Lookup::Lines(p.clone()),
            Some(SetDef::Selected) => {
                let db = portage_vdb::for_root(self.root);
                let mut lines: Vec<String> = db.world().map(|w| w.atoms).unwrap_or_default();
                lines.extend(
                    db.world_sets()
                        .map(|w| w.sets)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| format!("@{}", s.trim_start_matches('@'))),
                );
                Lookup::Lines(lines)
            }
            Some(SetDef::Installed) => Lookup::Lines(
                portage_repo::all_installed_packages(self.root)
                    .iter()
                    .map(|p| format!("{}/{}:{}", p.category, p.package, p.slot))
                    .collect(),
            ),
            Some(SetDef::Variable(v)) => Lookup::Lines(self.variable_set(v)),
            Some(SetDef::Owner { files, exclude }) => Lookup::Lines(self.owner_set(files, exclude)),
            Some(SetDef::PreservedRebuild) => {
                Lookup::Lines(portage_repo::preserved_rebuild_atoms(self.root))
            }
            Some(SetDef::RebuiltBinaries) => Lookup::Lines(self.rebuilt_binaries()),
            Some(SetDef::ChangedDeps) => Lookup::Lines(self.changed_deps()),
            Some(SetDef::Unavailable) => {
                Lookup::Lines(self.installed_slots(|_, best| best.is_none()))
            }
            Some(SetDef::Downgrade) => Lookup::Lines(self.installed_slots(|p, best| {
                best.is_some_and(|b| crate::version_cmp(&p.version, &b.version).is_gt())
            })),
            Some(SetDef::SubslotChanged) => {
                let root = self.root;
                Lookup::Lines(self.installed_slots(|p, best| {
                    let pf = format!("{}-{}", p.package, p.version);
                    let slot = portage_repo::vdb_entry_slot(root, &p.category, &pf);
                    let sub = slot.split_once('/').map_or(slot.as_str(), |(_, s)| s);
                    best.is_some_and(|b| sub != b.sub_slot)
                }))
            }
            Some(SetDef::FromProfile) => Lookup::Nothing,
            Some(SetDef::Unsupported(class)) => Lookup::Unsupported(class.clone()),
            // Not a section of its own: a file in a `multiset` directory.
            Some(SetDef::Dir(_)) | None => self
                .defs
                .values()
                .filter_map(|d| match d {
                    SetDef::Dir(dir) => Some(dir.join(name)),
                    _ => None,
                })
                .find(|p| p.is_file())
                .map_or(Lookup::Missing, |p| file_lines(&p)),
        }
    }
}

impl SetResolver<'_> {
    /// `cat/pkg:slot` of every installed package for which `keep` holds,
    /// given the best visible ebuild of the same slot.
    fn installed_slots(
        &self,
        keep: impl Fn(&portage_repo::InstalledPackage, Option<&Candidate>) -> bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for p in portage_repo::all_installed_packages(self.root).iter() {
            let slot = p.slot.split('/').next().unwrap_or_default();
            let best = portage_repo::list_candidates(self.repos, &p.category, &p.package)
                .ok()
                .and_then(|all| {
                    all.iter()
                        .filter(|c| {
                            c.slot == slot
                                && portage_repo::is_visible(c, &p.category, &p.package, self.config)
                        })
                        .max_by(|a, b| crate::rank(a, true, b, true))
                        .cloned()
                });
            if keep(p, best.as_ref()) {
                out.push(format!("{}/{}:{slot}", p.category, p.package));
            }
        }
        out
    }

    /// `OwnerSet.mapPathsToAtoms`: the `cat/pkg:slot` of every package whose
    /// `CONTENTS` lists a path matching `files`, minus the packages that also
    /// own a path matching `exclude`. Globs expand below the root.
    // Portage: _sets/dbapi.py, OwnerSet.mapPathsToAtoms
    fn owner_set(&self, files: &[String], exclude: &[String]) -> Vec<String> {
        let paths = self.expand_globs(files);
        let excluded = self.expand_globs(exclude);
        let all: Vec<&[u8]> = paths
            .iter()
            .chain(&excluded)
            .map(|p| p.as_bytes())
            .collect();
        let Ok(owners) = portage_vdb::for_root(self.root).owners(&all) else {
            return Vec::new();
        };
        let mut atoms = BTreeSet::new();
        let mut out_atoms = BTreeSet::new();
        for (path, key) in owners {
            let Some((package, _)) = portage_repo::split_pf(&key.pf) else {
                continue;
            };
            let slot = portage_repo::vdb_entry_slot(self.root, &key.category, &key.pf);
            let slot = slot.split('/').next().unwrap_or("0");
            let atom = format!("{}/{package}:{slot}", key.category);
            let path = String::from_utf8_lossy(&path)
                .trim_start_matches('/')
                .to_string();
            if excluded.iter().any(|e| e.trim_start_matches('/') == path) {
                out_atoms.insert(atom.clone());
            }
            atoms.insert(atom);
        }
        atoms.difference(&out_atoms).cloned().collect()
    }

    /// Each pattern (a path below the root, `*` and `?` allowed) expanded to
    /// the paths that exist, with a leading `/` and without the root.
    fn expand_globs(&self, patterns: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for p in patterns {
            let full = self.root.join(p.trim_start_matches('/'));
            let Some(full) = full.to_str() else { continue };
            for hit in glob::glob(full).into_iter().flatten().flatten() {
                if let Ok(rel) = hit.strip_prefix(self.root) {
                    out.push(format!("/{}", rel.display()));
                }
            }
        }
        out
    }

    /// `=cat/pkg-ver` of every installed package whose run-time dependency
    /// strings differ from its ebuild's (see
    /// [`portage_repo::installed_deps_changed`]).
    ///
    /// Not `ChangedDepsSet.load` itself: that wraps `strip_slots`, which
    /// returns `None`, in the comparison, so Portage's set is always empty
    /// (`LLM/portage-quirks.md`). This is the set the class means to build.
    // Portage: _sets/dbapi.py, ChangedDepsSet.load
    fn changed_deps(&self) -> Vec<String> {
        let categories = portage_repo::Categories::load(self.repos, self.config_root);
        let gate = portage_repo::CategoryGate::new(self.repos, &categories, self.config_root);
        portage_repo::all_installed_packages(self.root)
            .iter()
            .filter(|p| {
                portage_repo::installed_deps_changed(
                    self.root,
                    (&p.category, &p.package, &p.version),
                    gate,
                )
            })
            .map(|p| format!("={}/{}-{}", p.category, p.package, p.version))
            .collect()
    }

    /// `cat/pkg:slot` of each installed package for which `$PKGDIR` holds a
    /// binary of the same version with a non-zero `BUILD_TIME` that differs
    /// from the installed one (the newest such binary when several exist).
    /// Only the local `$PKGDIR` is read.
    // Portage: _sets/dbapi.py, RebuiltBinaries._filter
    fn rebuilt_binaries(&self) -> Vec<String> {
        let index = portage_repo::build_local_binpkg_index(self.config);
        let db = portage_vdb::for_root(self.root);
        let mut out = Vec::new();
        for p in portage_repo::all_installed_packages(self.root).iter() {
            let bin_time = portage_repo::list_binary_candidates(&index, &p.category, &p.package)
                .iter()
                .filter(|c| c.version == p.version)
                .filter_map(|c| c.build_time)
                .max()
                .unwrap_or(0);
            if bin_time == 0 {
                continue;
            }
            let key =
                portage_vdb::EntryKey::new(&p.category, format!("{}-{}", p.package, p.version));
            let installed = db
                .aux_get(&key, "BUILD_TIME")
                .ok()
                .flatten()
                .and_then(|t| t.trim().parse::<i64>().ok());
            if installed != Some(bin_time) {
                let slot = p.slot.split('/').next().unwrap_or("0");
                out.push(format!("{}/{}:{slot}", p.category, p.package));
            }
        }
        out
    }

    fn variable_set(&self, v: &VariableSet) -> Vec<String> {
        let db = portage_vdb::for_root(self.root);
        let mut out = Vec::new();
        for p in portage_repo::all_installed_packages(self.root).iter() {
            let key =
                portage_vdb::EntryKey::new(&p.category, format!("{}-{}", p.package, p.version));
            let read = |name: &str| db.aux_get(&key, name).ok().flatten().unwrap_or_default();
            let slot = p.slot.split('/').next().unwrap_or_default();
            let cp = format!("{}/{}", p.category, p.package);
            let hit = if v.variable.contains("DEPEND") {
                variable_depend_hit(v, &cp, &read(&v.variable), &read("USE"))
            } else {
                let values: HashSet<String> = read(&v.variable)
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                (v.includes.is_empty() || v.includes.iter().any(|i| values.contains(i)))
                    && !v.excludes.iter().any(|e| values.contains(e))
            };
            if hit {
                out.push(format!("{cp}:{slot}"));
            }
        }
        out
    }
}

/// `VariableSet._filter` for a `*DEPEND` variable: the package is out when
/// it matches `excludes_output`; otherwise the first flattened atom that
/// matches an exclude (out) or an include (in) decides. Atoms are compared
/// by `category/package`, which is all the shipped sets use.
// Portage: _sets/dbapi.py, VariableSet._filter
fn variable_depend_hit(v: &VariableSet, cp: &str, text: &str, use_text: &str) -> bool {
    if v.excludes_output.iter().any(|e| e == cp) {
        return false;
    }
    let tokens: Vec<String> = text.split_whitespace().map(String::from).collect();
    let enabled: HashSet<String> = use_text.split_whitespace().map(String::from).collect();
    let Ok(flat) = portage_use_reduce::use_reduce_flat(
        &tokens,
        &enabled,
        portage_use_reduce::MatchMode::Normal,
    ) else {
        return false;
    };
    for tok in flat {
        let Some(a) = portage_repo::parse_dep_atom(&tok, "experiment-pre-sort-atoms") else {
            continue;
        };
        let dep_cp = format!("{}/{}", a.category, a.package);
        if v.excludes.contains(&dep_cp) {
            return false;
        }
        if v.includes.contains(&dep_cp) {
            return true;
        }
    }
    false
}

enum Lookup {
    Lines(Vec<String>),
    Nothing,
    Missing,
    Unsupported(String),
}

fn file_lines(path: &Path) -> Lookup {
    match std::fs::read_to_string(path) {
        Ok(text) => Lookup::Lines(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect(),
        ),
        Err(_) => Lookup::Missing,
    }
}

/// Minimal `sets.conf` reader: `[section]`, `key = value`, `#` comments,
/// and `%(PORTAGE_CONFIGROOT)s`. A later definition replaces an earlier one.
fn read_sets_conf(text: &str, config_root: &Path, defs: &mut HashMap<String, SetDef>) {
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim().to_string(), HashMap::new()));
        } else if let Some((k, v)) = line.split_once('=')
            && let Some((_, keys)) = sections.last_mut()
        {
            let root = format!(
                "{}/",
                config_root.display().to_string().trim_end_matches('/')
            );
            let v = v.trim().replace("%(PORTAGE_CONFIGROOT)s", &root);
            keys.insert(k.trim().to_string(), v);
        }
    }
    for (name, keys) in sections {
        let class = keys.get("class").map(String::as_str).unwrap_or_default();
        let class = class.rsplit('.').next().unwrap_or(class);
        let def = match class {
            "StaticFileSet" => match (keys.get("filename"), keys.get("directory")) {
                (Some(f), _) => SetDef::File(PathBuf::from(f)),
                (None, Some(d)) => SetDef::Dir(PathBuf::from(d)),
                _ => SetDef::Unsupported(class.to_string()),
            },
            "DummyPackageSet" => SetDef::Packages(
                keys.get("packages")
                    .map(|p| p.split_whitespace().map(String::from).collect())
                    .unwrap_or_default(),
            ),
            "WorldSelectedSet" => SetDef::Selected,
            // The two halves of `@selected` are covered by it.
            "WorldSelectedPackagesSet" | "WorldSelectedSetsSet" => SetDef::Selected,
            "EverythingSet" => SetDef::Installed,
            "OwnerSet" => match keys.get("files") {
                Some(files) => SetDef::Owner {
                    files: shlex::split(files).unwrap_or_default(),
                    exclude: keys
                        .get("exclude-files")
                        .and_then(|e| shlex::split(e))
                        .unwrap_or_default(),
                },
                None => SetDef::Unsupported(class.to_string()),
            },
            "ChangedDepsSet" => SetDef::ChangedDeps,
            "RebuiltBinaries" => SetDef::RebuiltBinaries,
            "PreservedLibraryConsumerSet" => SetDef::PreservedRebuild,
            "UnavailableSet" => SetDef::Unavailable,
            "DowngradeSet" => SetDef::Downgrade,
            "SubslotChangedSet" => SetDef::SubslotChanged,
            // Metadata from the repositories instead of the installed
            // packages is not read.
            "VariableSet" if keys.get("metadata-source").is_none_or(|m| m == "vartree") => {
                let list = |k: &str| -> Vec<String> {
                    keys.get(k)
                        .map(|v| v.split_whitespace().map(String::from).collect())
                        .unwrap_or_default()
                };
                match keys.get("variable") {
                    Some(variable) => SetDef::Variable(VariableSet {
                        variable: variable.clone(),
                        includes: list("includes"),
                        excludes: list("excludes"),
                        excludes_output: list("excludes_output"),
                    }),
                    None => SetDef::Unsupported(class.to_string()),
                }
            }
            "ProfilePackageSet" | "PackagesSystemSet" => SetDef::FromProfile,
            other => SetDef::Unsupported(other.to_string()),
        };
        defs.insert(name, def);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(includes: &[&str], excludes: &[&str], excludes_output: &[&str]) -> VariableSet {
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        VariableSet {
            variable: "BDEPEND".into(),
            includes: list(includes),
            excludes: list(excludes),
            excludes_output: list(excludes_output),
        }
    }

    #[test]
    fn a_depend_variable_matches_an_included_package() {
        let v = variable(&["dev-lang/rust"], &[], &[]);
        assert!(variable_depend_hit(
            &v,
            "app/x",
            ">=dev-lang/rust-1.80 dev-libs/a",
            ""
        ));
        assert!(!variable_depend_hit(&v, "app/x", "dev-libs/a", ""));
    }

    #[test]
    fn an_inactive_conditional_atom_does_not_count() {
        let v = variable(&["dev-lang/rust"], &[], &[]);
        let text = "rust? ( dev-lang/rust )";
        assert!(!variable_depend_hit(&v, "app/x", text, ""));
        assert!(variable_depend_hit(&v, "app/x", text, "rust"));
    }

    #[test]
    fn the_first_matching_atom_decides_between_include_and_exclude() {
        let v = variable(&["dev-lang/rust"], &["dev-lang/rust-bin"], &[]);
        assert!(!variable_depend_hit(
            &v,
            "app/x",
            "dev-lang/rust-bin dev-lang/rust",
            ""
        ));
        assert!(variable_depend_hit(
            &v,
            "app/x",
            "dev-lang/rust dev-lang/rust-bin",
            ""
        ));
    }

    #[test]
    fn excludes_output_removes_the_package_itself() {
        let v = variable(&["dev-lang/rust"], &[], &["dev-lang/rust"]);
        assert!(!variable_depend_hit(
            &v,
            "dev-lang/rust",
            "dev-lang/rust",
            ""
        ));
    }

    #[test]
    fn sets_conf_reads_a_variable_set_and_skips_other_metadata_sources() {
        let mut defs = HashMap::new();
        read_sets_conf(
            "[live]\nclass = portage.sets.dbapi.VariableSet\nvariable = PROPERTIES\nincludes = live\n\
             [other]\nclass = portage.sets.dbapi.VariableSet\nvariable = PROPERTIES\n\
             includes = live\nmetadata-source = porttree\n\
             [down]\nclass = portage.sets.dbapi.DowngradeSet\n",
            Path::new("/"),
            &mut defs,
        );
        assert!(matches!(defs["live"], SetDef::Variable(_)));
        assert!(matches!(defs["other"], SetDef::Unsupported(_)));
        assert!(matches!(defs["down"], SetDef::Downgrade));
    }

    #[test]
    fn sets_conf_reads_an_owner_set_with_quoted_and_excluded_paths() {
        let mut defs = HashMap::new();
        read_sets_conf(
            "[mod]\nclass = portage.sets.dbapi.OwnerSet\nfiles = /lib/modules \"/opt/my dir\"\n\
             exclude-files = /usr/src/linux*\n\
             [bad]\nclass = portage.sets.dbapi.OwnerSet\n",
            Path::new("/"),
            &mut defs,
        );
        match &defs["mod"] {
            SetDef::Owner { files, exclude } => {
                assert_eq!(files, &["/lib/modules", "/opt/my dir"]);
                assert_eq!(exclude, &["/usr/src/linux*"]);
            }
            _ => panic!("not an owner set"),
        }
        assert!(matches!(defs["bad"], SetDef::Unsupported(_)));
    }

    #[test]
    fn sets_conf_reads_changed_deps_and_rebuilt_binaries() {
        let mut defs = HashMap::new();
        read_sets_conf(
            "[cd]\nclass = portage.sets.dbapi.ChangedDepsSet\n\
             [rb]\nclass = portage.sets.dbapi.RebuiltBinaries\n\
             [pr]\nclass = portage.sets.libs.PreservedLibraryConsumerSet\n\
             [sec]\nclass = portage.sets.security.NewAffectedSet\n",
            Path::new("/"),
            &mut defs,
        );
        assert!(matches!(defs["cd"], SetDef::ChangedDeps));
        assert!(matches!(defs["rb"], SetDef::RebuiltBinaries));
        assert!(matches!(defs["pr"], SetDef::PreservedRebuild));
        assert!(matches!(defs["sec"], SetDef::Unsupported(_)));
    }

    /// The five sets, written the way Portage's shipped `portage.conf` defines them (the
    /// host file is read first, but these later definitions replace it).
    const SETS_CONF: &str = "[live-rebuild]\nclass = portage.sets.dbapi.VariableSet\n\
        variable = PROPERTIES\nincludes = live\n\
        [changed-subslot]\nclass = portage.sets.dbapi.SubslotChangedSet\n\
        [unavailable]\nclass = portage.sets.dbapi.UnavailableSet\n\
        [golang-rebuild]\nclass = portage.sets.dbapi.VariableSet\nvariable = BDEPEND\n\
        includes = dev-lang/go\nexcludes_output = dev-lang/go dev-lang/go-bootstrap\n\
        [rust-rebuild]\nclass = portage.sets.dbapi.VariableSet\nvariable = BDEPEND\n\
        includes = dev-lang/rust dev-lang/rust-bin\n\
        excludes_output = dev-lang/rust dev-lang/rust-bin\n";

    /// `(category/pf, SLOT, extra vdb files)` of one installed package.
    type Installed<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

    /// `set` expanded on a private ROOT that holds only `installed`
    /// (`category/pf`, `SLOT`, extra vdb files) and the fixture repository,
    /// with the test's own `sets.conf`. Nothing is written to the fixtures.
    fn expand(set: &str, installed: &[Installed]) -> Vec<String> {
        let dir = tempfile::tempdir().expect("temporary root");
        let root = dir.path();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .canonicalize()
            .expect("fixtures");
        let etc = root.join("etc/portage");
        std::fs::create_dir_all(etc.join("repos.conf")).expect("repos.conf dir");
        std::fs::write(
            etc.join("repos.conf/repos.conf"),
            format!(
                "[DEFAULT]\nmain-repo = testrepo\n[testrepo]\nlocation = {}\n\
                 [overlay]\nlocation = {}\npriority = 10\n",
                fixtures.join("repo").display(),
                fixtures.join("overlay").display()
            ),
        )
        .expect("repos.conf");
        std::os::unix::fs::symlink(
            fixtures.join("repo/profiles/default"),
            etc.join("make.profile"),
        )
        .expect("make.profile");
        std::fs::write(etc.join("sets.conf"), SETS_CONF).expect("sets.conf");
        for (cpf, slot, files) in installed {
            let (category, _) = cpf.split_once('/').expect("category/pf");
            let entry = root.join("var/db/pkg").join(cpf);
            std::fs::create_dir_all(&entry).expect("vdb entry");
            for (name, text) in [("CATEGORY", category), ("SLOT", slot), ("EAPI", "8")]
                .iter()
                .chain(files.iter())
            {
                std::fs::write(entry.join(name), format!("{text}\n")).expect("vdb file");
            }
        }
        let repos = portage_repo::find_repos_quiet(root).expect("repos");
        let config = crate::load_config(root, root, &repos).expect("config");
        let mut out = BTreeSet::new();
        SetResolver::new(root, root, &repos, &config).expand(set, &mut out);
        out.into_iter().collect()
    }

    /// Devmanual "ebuild-writing/variables" (`PROPERTIES`): `live` marks a package built from a
    /// moving upstream; `@live-rebuild` lists the installed ones. Include, exclude, near miss.
    #[test]
    fn live_rebuild_lists_exactly_these_packages() {
        let got = expand(
            "live-rebuild",
            &[
                ("dev-libs/w701live-9999", "0", &[("PROPERTIES", "live")]),
                (
                    "dev-libs/w701notlive-1",
                    "0",
                    &[("PROPERTIES", "interactive")],
                ),
                (
                    "dev-libs/w701livelike-1",
                    "0",
                    &[("PROPERTIES", "live-ish")],
                ),
            ],
        );
        assert_eq!(got, ["dev-libs/w701live:0"]);
    }

    /// Charter property 2 (installed software follows its toolchain): `@rust-rebuild` lists
    /// installed packages with a `dev-lang/rust` build dependency, never rust itself, never an
    /// inactive `USE` branch.
    #[test]
    fn rust_rebuild_lists_exactly_these_packages() {
        let got = expand(
            "rust-rebuild",
            &[
                (
                    "dev-libs/w701userust-1",
                    "0",
                    &[("BDEPEND", "dev-lang/rust")],
                ),
                ("dev-lang/rust-bin-1", "0", &[("BDEPEND", "dev-lang/rust")]),
                (
                    "dev-libs/w701maybe-1",
                    "0",
                    &[("BDEPEND", "rust? ( dev-lang/rust )"), ("USE", "doc")],
                ),
            ],
        );
        assert_eq!(got, ["dev-libs/w701userust:0"]);
    }

    /// Same rule for `@golang-rebuild` (`dev-lang/go`, not `dev-lang/go-bootstrap` itself, not
    /// a package that only needs another Go tool).
    #[test]
    fn golang_rebuild_lists_exactly_these_packages() {
        let got = expand(
            "golang-rebuild",
            &[
                ("dev-libs/w701usego-1", "0", &[("BDEPEND", "dev-lang/go")]),
                (
                    "dev-lang/go-bootstrap-1",
                    "0",
                    &[("BDEPEND", "dev-lang/go")],
                ),
                (
                    "dev-libs/w701gotool-1",
                    "0",
                    &[("BDEPEND", "dev-go/w701tool")],
                ),
            ],
        );
        assert_eq!(got, ["dev-libs/w701usego:0"]);
    }

    /// PMS `ebuild-vars.tex`, "SLOT": a changed sub-slot is a rebuild signal. `@changed-subslot`
    /// lists the installed slot whose best visible ebuild has another sub-slot; an equal
    /// sub-slot is excluded, a masked newer ebuild is a near miss (not visible, so not a change).
    #[test]
    fn changed_subslot_lists_exactly_these_packages() {
        let got = expand(
            "changed-subslot",
            &[
                ("dev-libs/w701subup-1", "0/1", &[]),
                ("dev-libs/w701subsame-1", "0/1", &[]),
                ("dev-libs/w701submask-1", "0/1", &[]),
            ],
        );
        assert_eq!(got, ["dev-libs/w701subup:0"]);
    }

    /// Charter property 1: `@unavailable` lists installed slots no visible ebuild offers
    /// (no ebuild at all, or only a masked one); an older installed version of a visible
    /// package is a near miss.
    #[test]
    fn unavailable_lists_exactly_these_packages() {
        let got = expand(
            "unavailable",
            &[
                ("dev-libs/w701gone-1", "0", &[]),
                ("dev-libs/w701submask-1", "0/1", &[]),
                ("dev-libs/w701subsame-1", "0/1", &[]),
                ("dev-libs/w701subup-1", "0/1", &[]),
            ],
        );
        assert_eq!(got, ["dev-libs/w701gone:0", "dev-libs/w701submask:0"]);
    }
}
