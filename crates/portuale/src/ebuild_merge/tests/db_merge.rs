use super::*;
use portage_vdb::{EntryKey, InstalledDb};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn installed_reverse_dependents_for(
    root: &Path,
    category: &str,
    package: &str,
    version: &str,
) -> Vec<String> {
    portage_repo::installed_reverse_dependents(root, category, package, version)
}
