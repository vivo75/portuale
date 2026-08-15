//! Blockers against installed packages and file collisions.

use crate::ebuild_merge::*;

/// The installed-package scan + blocker-atom match half, split out so
/// `merge_binpkg` can reuse it: a binpkg's `*DEPEND` files are already
/// USE-reduced at build time. Matches every `!atom`/`!!atom` against
/// every installed entry (`category/pf:slot/sub_slot`).
// Portage: vartree.py, dblink.merge
pub(crate) fn blockers_from_flat_deps(
    root: &Path,
    flat_deps: &[String],
) -> HashSet<(String, String)> {
    (|| -> Option<HashSet<(String, String)>> {
        // Category by category, like the old walk: list the category,
        // then read each entry's `SLOT`, then the next category.
        let db = portage_vdb::for_root(root);
        let installed: Vec<(String, String, String)> = db
            .categories()
            .ok()?
            .into_iter()
            .flat_map(|category_name| {
                db.category_entries(&category_name)
                    .into_iter()
                    .flatten()
                    .map(move |pf| {
                        // Through the vdb seam, so the scan sees the same
                        // normalised `SLOT` every consumer does.
                        let slot = portage_repo::vdb_entry_slot(root, &category_name, &pf);
                        let (slot, sub_slot) = slot
                            .split_once('/')
                            .map(|(s, ss)| (s.to_string(), ss.to_string()))
                            .unwrap_or_else(|| (slot.clone(), slot.clone()));
                        let candidate_str = format!("{category_name}/{pf}:{slot}/{sub_slot}");
                        (category_name.clone(), pf, candidate_str)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let installed_strs: Vec<&str> = installed.iter().map(|(_, _, s)| s.as_str()).collect();
        let by_str: HashMap<&str, &(String, String, String)> = installed_strs
            .iter()
            .copied()
            .zip(installed.iter())
            .collect();

        let mut blocked: HashSet<(String, String)> = HashSet::new();
        for tok in flat_deps {
            let Some(dep_atom) = portage_dep::parse_atom(tok) else {
                continue;
            };
            if dep_atom.blocker == portage_dep::Blocker::None {
                continue;
            }
            if let Some(matched) = portage_dep::match_from_list(tok, &installed_strs) {
                for m in matched {
                    if let Some((category, pf, _)) = by_str.get(m) {
                        blocked.insert((category.clone(), pf.clone()));
                    }
                }
            }
        }
        Some(blocked)
    })()
    .unwrap_or_default()
}
