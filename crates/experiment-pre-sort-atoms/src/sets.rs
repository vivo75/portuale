//! Package-set expansion for `@world`: the sets named in `world_sets`, and
//! whatever `sets.conf` defines (Portage's default `portage.conf`, then
//! `<config-root>/etc/portage/sets.conf`).
//!
//! Supported classes: `StaticFileSet` (`filename`, or `directory` +
//! `multiset`), `DummyPackageSet` (`packages`), `WorldSelectedSet` and
//! friends, `EverythingSet` (`@installed`) and `ProfilePackageSet` /
//! `PackagesSystemSet`, which are resolved from the profile by the caller.
//! A set of any other class (`@security`, `@preserved-rebuild`, …) depends
//! on state this experiment does not read; it is reported and skipped.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    /// `@system` and `@profile`: already seeds, nothing to add here.
    FromProfile,
    Unsupported(String),
}

pub struct SetResolver<'a> {
    root: &'a Path,
    config_root: &'a Path,
    defs: HashMap<String, SetDef>,
    seen: HashSet<String>,
}

impl<'a> SetResolver<'a> {
    pub fn new(root: &'a Path, config_root: &'a Path) -> Self {
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
            "ProfilePackageSet" | "PackagesSystemSet" => SetDef::FromProfile,
            other => SetDef::Unsupported(other.to_string()),
        };
        defs.insert(name, def);
    }
}
