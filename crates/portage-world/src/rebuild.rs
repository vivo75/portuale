//! Installed packages a plan keeps but must build again.

use std::collections::{BTreeSet, HashMap};

use portage_dep::SlotOperator;
use portage_profile::Config;
use portage_repo::EffectiveUseFlagsArgs;
use portage_vdb::EntryKey;

use crate::{DEP_KEYS, SlotSet, World};

/// The reached slots whose installed version a plan keeps, with the entry
/// their `VDB` holds and the version.
fn kept(world: &World) -> impl Iterator<Item = (&SlotSet, EntryKey)> {
    world.slots.iter().filter_map(|s| {
        let reached = world.reached.contains(&s.node());
        let version = s
            .installed
            .as_deref()
            .filter(|_| reached && s.change().is_none())?;
        let (category, package) = s.cp.split_once('/')?;
        Some((s, EntryKey::new(category, format!("{package}-{version}"))))
    })
}

/// `cat/pkg:slot` of each kept installed package whose recorded `:slot/sub-slot=`
/// dependency no longer matches the version the plan chooses for the provider.
/// A `:*` dependency never counts, nor does a provider the plan leaves as it is
/// (a binding that was already stale is not this plan's doing).
///
/// PMS `dependencies.tex`, `sec:slot-dep`: `:=` binds the consumer to the
/// slot and sub-slot it was built against.
pub(crate) fn slot_operator(world: &World, root: &std::path::Path) -> BTreeSet<String> {
    let db = portage_vdb::for_root(root);
    let sub_slot: HashMap<String, &str> = world
        .slots
        .iter()
        .filter(|s| world.reached.contains(&s.node()) && s.change().is_some())
        .filter_map(|s| Some((s.node(), s.chosen()?.sub_slot.as_str())))
        .collect();
    let broken = |key: &EntryKey| {
        DEP_KEYS.iter().any(|dep| {
            let text = db.aux_get(key, dep).ok().flatten().unwrap_or_default();
            text.split_whitespace()
                .filter_map(portage_dep::parse_atom)
                .filter(|a| a.slot_operator == Some(SlotOperator::Equals))
                .any(|a| {
                    let (Some(slot), Some(recorded)) = (&a.slot, &a.sub_slot) else {
                        return false;
                    };
                    let provider = format!("{}/{}:{slot}", a.category, a.package);
                    sub_slot.get(&provider).is_some_and(|now| now != recorded)
                })
        })
    };
    kept(world)
        .filter(|(_, key)| broken(key))
        .map(|(s, _)| s.node())
        .collect()
}

/// `cat/pkg:slot` of every kept installed package that has a version to
/// install: with everything built, each is built again.
pub(crate) fn every_kept(world: &World) -> BTreeSet<String> {
    kept(world)
        .filter(|(s, _)| s.chosen().is_some())
        .map(|(s, _)| s.node())
        .collect()
}

/// `cat/pkg:slot` of each kept installed package whose recorded `USE`, cut to
/// the `IUSE` of the version kept, differs from the configured one.
///
/// PMS `dependencies.tex`: USE is restricted to `IUSE_EFFECTIVE`.
pub(crate) fn changed_use(
    world: &World,
    root: &std::path::Path,
    config: &Config,
) -> BTreeSet<String> {
    let db = portage_vdb::for_root(root);
    let names = |text: &str| -> BTreeSet<String> {
        text.split_whitespace()
            .map(|f| f.trim_start_matches(['+', '-']).to_string())
            .collect()
    };
    kept(world)
        .filter(|(s, key)| {
            let Some(chosen) = s.chosen() else {
                return false;
            };
            let iuse = names(&chosen.iuse);
            let recorded = db.aux_get(key, "USE").ok().flatten().unwrap_or_default();
            let recorded: BTreeSet<String> =
                names(&recorded).intersection(&iuse).cloned().collect();
            let (category, package) = s.cp.split_once('/').unwrap_or_default();
            let configured = portage_repo::effective_use_flags(EffectiveUseFlagsArgs {
                config,
                iuse: &chosen.iuse,
                keywords: &chosen.keywords,
                candidate_str: &chosen.match_str(category, package),
                build_id: None,
                category,
                package,
            });
            let configured: BTreeSet<String> = configured
                .iter()
                .filter(|f| iuse.contains(*f))
                .cloned()
                .collect();
            recorded != configured
        })
        .map(|(s, _)| s.node())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{Order, testkit::analyse_fixture_with};

    /// PMS `dependencies.tex`: with everything built, every slot `@world`
    /// reaches is a row, so the rows are the reached slots that have a chosen
    /// version; the installed one that is kept is a `Rebuild`.
    #[test]
    fn emptytree_has_a_row_for_every_reached_slot_with_a_version() {
        let installed = [("dev-libs/newpkg-1.0", "0")];
        let world = analyse_fixture_with(&["dev-libs/withdeps"], &installed, |o| {
            o.rebuilds = true;
            o.emptytree = true;
        });
        let reached = world
            .slots
            .iter()
            .filter(|s| world.reached.contains(&s.node()) && s.chosen().is_some())
            .count();
        let rows = world.ordered(&Order::Alpha);
        assert!(reached > 1);
        assert_eq!(rows.len(), reached);
        let kept = rows.iter().find(|s| s.cp == "dev-libs/newpkg").unwrap();
        assert_eq!(world.change(kept), Some(crate::Change::Rebuild));
    }
}
