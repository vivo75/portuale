//! Installed packages pulled into the merge-order graph.

use crate::merge_order::*;

/// Whether an installed package matches `edge`'s atom. For a slot-operator edge
/// the match must carry the resolved child's own slot/sub-slot.
/// `child_slot` `None` means no resolved child (plain atom check).
pub(crate) fn edge_satisfied_with(
    installed_by_cp: &HashMap<(String, String), Vec<String>>,
    edge: &DepEdge,
    child_slot: Option<(&str, &str)>,
) -> bool {
    let Some(cands) = installed_by_cp.get(&(edge.category.clone(), edge.package.clone())) else {
        return false;
    };
    let refs: Vec<&str> = cands.iter().map(String::as_str).collect();
    let Some(matched) = portage_dep::match_from_list(&edge.atom, &refs) else {
        return false;
    };
    if matched.is_empty() {
        return false;
    }
    if edge.priority.buildtime_slot_op || edge.priority.runtime_slot_op {
        let Some((slot, sub_slot)) = child_slot else {
            return true;
        };
        return matched.iter().any(|m| {
            portage_dep::parse_candidate(m).is_some_and(|c| {
                c.slot.as_deref() == Some(slot) && c.sub_slot.as_deref() == Some(sub_slot)
            })
        });
    }
    true
}
