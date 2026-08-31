//! The remote entry points: resolve, plan, and run each unit.

use crate::remote::*;

/// BFS-drop the transitive dependents of a failed package (Portage
/// `_calc_resume_list`): every skipped cp records the failed cpv that
/// doomed it, for the `skipped (… failed)` trailer.
pub(crate) fn drop_dependents(
    dependents: &std::collections::HashMap<(String, String), Vec<(String, String)>>,
    skip: &mut std::collections::HashMap<(String, String), String>,
    failed_cp: (String, String),
    failed_cpv: &str,
) {
    let mut queue = vec![failed_cp];
    while let Some(x) = queue.pop() {
        if let Some(deps) = dependents.get(&x) {
            for p in deps {
                if skip.insert(p.clone(), failed_cpv.to_string()).is_none() {
                    queue.push(p.clone());
                }
            }
        }
    }
}
