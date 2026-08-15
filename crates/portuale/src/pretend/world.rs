//! The world and world_sets files.

use crate::pretend::*;

/// Reads `<root>/var/lib/portage/world`, one atom per line. A missing file is
/// empty, not an error. `@`-prefixed lines are dropped here; nested sets live
/// in `world_sets` (see `read_world_sets`).
/// Portage: files.py, WorldSelectedPackagesSet.
pub(crate) fn read_world_atoms(root: &Path) -> Result<Vec<String>, String> {
    portage_vdb::for_root(root)
        .world()
        .map(|w| w.atoms)
        .map_err(world_read_error)
}

/// The text the world readers always gave: `reading <path>: <io error>`
/// for a store that could not be read.
pub(crate) fn world_read_error(e: portage_vdb::Error) -> String {
    match e {
        portage_vdb::Error::Io { path, source } => {
            format!("reading {}: {source}", path.display())
        }
        e => e.to_string(),
    }
}

/// Expand one file-based set, following nested `@` references with a cycle guard.
/// A missing file is an error, not an absence.
/// Portage: _sets/__init__.py, StaticFileSet.
pub(crate) fn resolve_custom_set(
    config_root: &Path,
    name: &str,
    seen: &mut HashSet<String>,
) -> Result<Vec<String>, String> {
    if !seen.insert(name.to_string()) {
        return Ok(Vec::new());
    }
    let path = config_root.join("etc/portage/sets").join(name);
    let text =
        std::fs::read_to_string(&path).map_err(|_| format!("emerge: set '{name}' not found"))?;
    let mut atoms = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some(nested_name) = line.strip_prefix('@') {
            atoms.extend(resolve_custom_set(config_root, nested_name, seen)?);
        } else {
            atoms.push(line.to_string());
        }
    }
    Ok(atoms)
}
