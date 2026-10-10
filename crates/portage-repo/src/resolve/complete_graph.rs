//! When `--complete-graph` turns itself on.

use crate::*;

/// Real `depgraph.py::_complete_graph` (8581-8648) auto-enable check:
/// even without `--complete-graph`, complete mode switches on when
/// `--complete-graph-if-new-use` (`complete_if_new_use`, default `"y"`)
/// or `--complete-graph-if-new-ver` (`complete_if_new_ver`, default
/// `"y"`) or `complete_if_new_slot` (= `--rebuild-if-new-slot`, default
/// `"y"`) is on *and* the freshly-built graph contains a merge that
/// changes an already-installed package. Real portage scans its digraph's
/// `operation == "merge"` nodes and compares each to the vdb match on
/// `node.slot_atom`:
///
///   - `complete_if_new_ver`: `inst_pkg < node or node < inst_pkg` (an
///     in-slot version change -- portuale's `Upgrade`/`Downgrade`), or
///     a slot/sub-slot change without a revbump (8611-8618 -- this
///     portuale's `--changed-slot` `Reinstall` with `slot_changed`, and a
///     `New` into a slot of an already-installed `cp`, `new_slot`).
///   - `complete_if_new_use`: `node.iuse.all != inst_pkg.iuse.all` or the
///     enabled-USE ∩ IUSE sets differ -- portuale's `--newuse`/
///     `--changed-use` `Reinstall`, whose `changed_flags` is real
///     `_reinstall_for_flags`'s own return value, *plus* a bare
///     reasonless `[ebuild R]` for an explicitly-requested installed
///     package (backlog #222): real compares the merge node's own USE
///     against the vdb unconditionally, while `changed_flags` is only
///     ever computed under `--newuse`/`--changed-use`. The reasonless
///     shape is detected with the same fresh `iuse.all` /
///     enabled-∩-IUSE diff real makes (`merge_use_changed_vs_installed`),
///     falling back to `changed_flags` when the merge side is
///     unreadable.
///   - `complete_if_new_slot`: `vardb.match_pkgs(Atom(node.cp))` exists,
///     same `cp`, and no installed pkg matches `node`'s slot/sub-slot
///     (8634-8645) -- a new-slot install, portuale's `New` + `new_slot`.
///
/// The CLI layer runs the first resolve, calls this on its entries, and
/// (if it returns `true` and neither `--complete-graph` nor `--deep` is
/// already in force) re-resolves with `complete = true` -- see
/// `pretend.rs`'s two-pass, which mirrors real portage's own "resolve,
/// then `_complete_graph` re-walk" structure.
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub fn complete_graph_auto_enable(
    entries: &[GraphEntry],
    if_new_use: bool,
    if_new_ver: bool,
    if_new_slot: bool,
    root: &Path,
    repos: &[RepoConfig],
    config: &portage_profile::Config,
) -> bool {
    entries.iter().any(|e| match &e.outcome {
        PretendOutcome::Upgrade { .. } | PretendOutcome::Downgrade { .. } => if_new_ver,
        PretendOutcome::New { .. } if e.new_slot => if_new_ver || if_new_slot,
        PretendOutcome::Reinstall {
            version,
            changed_flags,
            slot_changed,
            ..
        } => {
            (if_new_ver && *slot_changed)
                || (if_new_use
                    && (!changed_flags.is_empty()
                        || merge_use_changed_vs_installed(
                            root,
                            repos,
                            config,
                            &e.category,
                            &e.package,
                            version,
                        )
                        .unwrap_or(false)))
        }
        _ => false,
    })
}
