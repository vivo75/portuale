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
