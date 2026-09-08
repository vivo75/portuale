use super::*;
use portage_vdb::{EntryKey, InstalledDb};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// One hand-written entry: `(category, pf, files)`.
type HandEntry<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

/// A `files` VDB written by hand.
fn write_vdb(root: &Path, entries: &[HandEntry]) {
    for (cat, pf, files) in entries {
        let dir = root.join("var/db/pkg").join(cat).join(pf);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in *files {
            std::fs::write(dir.join(name), data).unwrap();
        }
    }
}

fn installed_reverse_dependents_for(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    portage_repo::installed_reverse_dependents(root, category, package, version)
}
