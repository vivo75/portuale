//! #573: the skipped-update block names every installed consumer Portage lists,
//! not just the first in-graph consumer that shares the withheld pin atom.
//!
//! Expected values come from host Portage 3.0.83 on the same scratch ROOT
//! (`LLM/evidence/2026-10-28-573/captures/portage-p569prov.stderr` for the
//! in-graph cell, `portage-p573up.stderr` for the complete-mode cell), never
//! from a second portuale run.

use super::tests_consumer_pin_conflict::{hermetic_root, request_in};
use super::*;

/// The scratch ROOT of `LLM/evidence/2026-10-28-573/probe-p573.sh`: the shared
/// fixture vdb plus the #573 recorded installed consumers (`extra`), built
/// beside the shared tree so the fixture vdb itself stays untouched.
fn p573_root(world: &[&str], extra: &[(&str, &str)]) -> TempDir {
    let base = hermetic_root(world);
    let recorded = [
        (
            "dev-libs/p573world-1",
            "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=",
        ),
        (
            "dev-libs/p573virt-1",
            "=dev-libs/p569prov-5.42* dev-libs/p569prov:0/5.42=",
        ),
        ("dev-libs/p573real-1", "dev-libs/p569prov:0/5.42="),
    ];
    for (cpv, rdepend) in recorded.iter().chain(extra.iter()) {
        let dir = base.join("var/db/pkg").join(cpv);
        fs::create_dir_all(&dir).unwrap();
        let (category, _) = cpv.split_once('/').unwrap();
        fs::write(dir.join("CATEGORY"), format!("{category}\n")).unwrap();
        fs::write(dir.join("EAPI"), "8\n").unwrap();
        fs::write(dir.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        fs::write(dir.join("SLOT"), "0\n").unwrap();
        fs::write(dir.join("repository"), "testrepo\n").unwrap();
    }
    base
}

/// The skip block as `(recorded atom, consumer)` rows, sorted.
fn rows(result: &GraphResult) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = result
        .skipped_updates
        .iter()
        .map(|row| (row.atom.clone(), row.consumer_cpv.clone()))
        .collect();
    rows.sort();
    rows
}

/// The merge-bound entries as `(cat/pkg, version)`, in list order.
fn merges(result: &GraphResult) -> Vec<(String, String)> {
    result
        .entries
        .iter()
        .filter_map(|entry| {
            let version = match &entry.outcome {
                PretendOutcome::New { version } | PretendOutcome::Reinstall { version, .. } => {
                    version
                }
                PretendOutcome::Upgrade { to, .. } | PretendOutcome::Downgrade { to, .. } => to,
                PretendOutcome::AlreadyInstalled { .. }
                | PretendOutcome::Uninstall { .. }
                | PretendOutcome::NoVisibleCandidate => return None,
            };
            Some((
                // match-str-ok: merge-list entry (Option slot/repo), not a Candidate
                format!("{}/{}-{version}", entry.category, entry.package),
                version.clone(),
            ))
        })
        .collect()
}

/// The request `emerge --pretend` makes against this ROOT. `resolve_in` alone
/// leaves `complete_seed_atoms` empty and `selective` on, which the CLI never
/// does for a plain `emerge <atom>`: the reachability sets stay empty (no
/// installed consumer is in scope) and selective mode keeps the installed
/// version instead of considering the withheld one
/// (`portuale/src/pretend/resolve_plan.rs:128,304`).
fn seeded_request(root: &Path, world: &[&str], atoms: &[&str], selective: bool) -> ResolveRequest {
    let mut req = request_in(root, atoms);
    req.selective = selective;
    req.config.complete_seed_atoms = world.iter().map(|w| (*w).to_string()).collect();
    req
}

/// Two installed consumers recording byte-identical atoms are both parents of
/// the withheld update: Portage prints four rows (each consumer's built and
/// menu atom), not one consumer's two.
#[test]
fn two_consumers_sharing_a_pin_atom_both_render() {
    let root = p573_root(&["dev-libs/p573virt", "dev-libs/p573world"], &[]);
    let result = seeded_request(
        &root,
        &["dev-libs/p573virt", "dev-libs/p573world"],
        &["dev-libs/p569prov"],
        false,
    )
    .resolve()
    .expect("the in-graph cell resolves");
    let expected: Vec<(String, String)> = [
        "dev-libs/p573world-1:0/0::testrepo",
        "dev-libs/p573virt-1:0/0::testrepo",
    ]
    .iter()
    .flat_map(|consumer| {
        [
            (
                "dev-libs/p569prov:0/5.42=".to_string(),
                (*consumer).to_string(),
            ),
            (
                "=dev-libs/p569prov-5.42*".to_string(),
                (*consumer).to_string(),
            ),
        ]
    })
    .collect();
    let mut expected = expected;
    expected.sort();
    assert_eq!(
        rows(&result),
        expected,
        "Portage's four-row block (captures/portage-p569prov.stderr)"
    );
    assert!(
        !rows(&result)
            .iter()
            .any(|(_, consumer)| consumer.starts_with("dev-libs/p573real-1")),
        "nothing here walks the vdb, so the out-of-graph consumer stays unlisted"
    );
}

/// A consumer Portage reaches only through complete mode's installed-vdb walk
/// is a parent there and nowhere else: the settled `p573up` upgrade turns the
/// walk on, `p573host`'s installed dependency reaches `p573real`, and the row
/// appears without the merge list moving.
#[test]
fn out_of_graph_consumer_is_reported_when_complete_mode_walks_the_vdb() {
    let root = p573_root(
        &[
            "dev-libs/p573up",
            "dev-libs/p573virt",
            "dev-libs/p573world",
            "dev-libs/p573host",
        ],
        &[
            ("dev-libs/p573up-1", "dev-libs/p569prov:0/5.42="),
            ("dev-libs/p573host-1", "dev-libs/p573real"),
        ],
    );
    let mut req = seeded_request(
        &root,
        &[
            "dev-libs/p573up",
            "dev-libs/p573virt",
            "dev-libs/p573world",
            "dev-libs/p573host",
        ],
        &["dev-libs/p569prov", "dev-libs/p573up", "dev-libs/p573host"],
        true,
    );
    req.update = true;
    let result = req.resolve().unwrap();
    assert_eq!(
        merges(&result),
        vec![("dev-libs/p573up-2".to_string(), "2".to_string())],
        "Portage's merge list for this cell is the one settled upgrade"
    );
    assert!(
        rows(&result).contains(&(
            "dev-libs/p569prov:0/5.42=".to_string(),
            "dev-libs/p573real-1:0/0::testrepo".to_string()
        )),
        "the complete walk reaches p573real-1 (captures/portage-p573up.stderr): {:?}",
        rows(&result)
    );
}
