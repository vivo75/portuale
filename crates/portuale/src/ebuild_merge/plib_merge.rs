//! Preserved libraries during a merge.

use crate::ebuild_merge::*;

/// The replacing package's `NEEDED.ELF.2` lines from its `-MERGING-<pf>` temporary
/// entry, which enumeration skips; empty when there is no replacing package.
// Portage: vartree.py, dblink.treewalk
pub(crate) fn replacement_needed_entries(
    root: &Path,
    category: &str,
    new_pf: &str,
) -> Vec<(String, Vec<crate::needed_elf::NeededEntry>)> {
    let Some(text) = portage_vdb::for_root(root)
        .read_pending_file(
            &portage_vdb::EntryKey::new(category, new_pf),
            "NEEDED.ELF.2",
        )
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return Vec::new();
    };
    let entries = crate::needed_elf::NeededEntry::parse_file(&text);
    if entries.is_empty() {
        return Vec::new();
    }
    vec![(format!("{category}/{new_pf}"), entries)]
}
