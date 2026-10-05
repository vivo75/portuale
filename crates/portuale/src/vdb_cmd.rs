// The `vdb` applet: convert and verify installed-package databases
// between backends.
//
//   portuale vdb convert --from KIND:PATH --to KIND:PATH [--force]
//   portuale vdb verify  KIND:PATH KIND:PATH
//   portuale vdb verify  --against KIND:PATH KIND:PATH
//
// The work is `portage_vdb::copy_all` / `portage_vdb::verify`; this file
// is argument parsing, backend opening and reporting only.
//
// `files:PATH` names a ROOT (its VDB directory plus the D4 stores
// `var/lib/portage/{world,world_sets,preserved_libs_registry}` and
// `var/cache/edb/counter` beneath it): a files destination needs those
// stores, and they live outside the VDB directory. Convenience for
// reading and writing alike: a PATH that is the VDB directory means the
// root three levels up (the host VDB is `files:/`). A source PATH
// that holds no VDB and is not one is read as a bare VDB
// directory (entries only, no D4 stores); a bare directory is refused as
// a destination.
//
//   portuale vdb mount [--foreground] [--allow-other] KIND:PATH MOUNTPOINT
//
// `mount` serves a read-only FUSE view of the database (the view is
// `vdb_view.rs`, the `fuser` adapter `vdb_fuse.rs`; feature `vdb-fuse`).
//
//   portuale vdb status  KIND:PATH
//   portuale vdb sweep   (--remove CAT/PF)... | --all  KIND:PATH
//
//   portuale vdb rebuild-index KIND:PATH
//
// `rebuild-index` recomputes a database's derived index (`owner`,
// `dep_atom`, `needed`) from the stored entry files in one transaction.
//
// `status` lists the entries left mid-merge: `merging`
// rows on a database, `-MERGING-<pf>` directories on files. `sweep` deletes
// the named ones in one write transaction; it refuses a key that is not
// pending, so an installed entry is never removed by it.
//
// Exit codes: 0 success / verify equal / status with nothing pending,
// 1 verify found differences / status found pending entries,
// 2 usage or I/O error.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use portage_vdb::{BackendKind, EntryKey, FilesDb, InstalledDb};

struct Spec {
    kind: BackendKind,
    path: PathBuf,
}

/// The scratch directory of a `--rw` mount of `db`: named after the
/// database file, so the next mount of the same database clears what a
/// daemon that died left behind (`RwView::new` empties it).
#[cfg(feature = "vdb-fuse")]
fn rw_scratch(db: &Path) -> PathBuf {
    use std::hash::{Hash as _, Hasher as _};
    let abs = std::fs::canonicalize(db).unwrap_or_else(|_| db.to_path_buf());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    abs.hash(&mut h);
    std::env::temp_dir().join(format!("portuale-vdb-rw-{:016x}", h.finish()))
}

/// What [`import_stores`] copied.
pub(crate) struct StoresReport {
    pub world: usize,
    pub world_sets: usize,
    pub preserved_libs: usize,
    pub config_memory: usize,
    pub counter: Option<i64>,
}

impl std::fmt::Display for StoresReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "world {} atoms, {} sets; preserved libs {}; config memory {}; counter {}",
            self.world,
            self.world_sets,
            self.preserved_libs,
            self.config_memory,
            self.counter.map_or("none".to_string(), |c| c.to_string())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }
}
