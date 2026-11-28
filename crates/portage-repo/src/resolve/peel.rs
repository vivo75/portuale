//! Mask the other builds of a version that fail the same way.

use crate::PretendOutcome;
use crate::cached_binary_index;
use crate::resolve::ctx::ResolveCtx;
use crate::resolve::pass_state::PassResult;
use crate::resolve::pretend::BinaryMask;
use crate::resolve::rev_dep::is_built_slot_op;
use portage_profile::Config;
use std::collections::{BTreeSet, HashSet};

/// Whether a build's recorded dependency strings carry the built slot
/// operator `bound` (`cat/pkg:slot/subslot=`): an `=` atom on the same
/// package with the same slot and sub-slot. USE conditions are ignored, as
/// the probe does for a replacement's atoms.
pub(crate) fn binds_same_atom(dep_strings: &[&str], bound: &str) -> bool {
    // drop-ok: an atom that does not parse cannot match; its build is left to the old one-per-restart mask.
    let Some(want) = portage_dep::parse_atom(bound) else {
        return false;
    };
    dep_strings.iter().any(|deps| {
        let toks: Vec<String> = deps.split_whitespace().map(String::from).collect();
        let Ok(tokens) = portage_use_reduce::use_reduce_flat(
            &toks,
            &HashSet::new(),
            portage_use_reduce::MatchMode::All,
        ) else {
            return false;
        };
        tokens
            .iter()
            // drop-ok: an atom that does not parse cannot match; its build is left to the old one-per-restart mask.
            .filter_map(|t| portage_dep::parse_atom(t))
            .any(|a| {
                a.blocker == portage_dep::Blocker::None
                    && a.slot_operator == Some(portage_dep::SlotOperator::Equals)
                    && (&a.category, &a.package) == (&want.category, &want.package)
                    && a.slot == want.slot
                    && a.sub_slot == want.sub_slot
            })
    })
}

/// The builds of the same version as `mask`, other than `mask` itself, whose
/// recorded dependencies bind `bound` the same way: they fail exactly as
/// `mask` does, so one restart masks them all.
pub(crate) fn sibling_masks(
    builds: &[(Option<String>, Vec<String>)],
    mask: &BinaryMask,
    bound: &str,
) -> Vec<BinaryMask> {
    builds
        .iter()
        .filter(|(id, deps)| {
            id.is_some()
                && *id != mask.3
                && binds_same_atom(&deps.iter().map(String::as_str).collect::<Vec<_>>(), bound)
        })
        .map(|(id, _)| (mask.0.clone(), mask.1.clone(), mask.2.clone(), id.clone()))
        .collect()
}

/// Every recorded build of `cat/pkg-ver` in the local `$PKGDIR` index and,
/// under `--getbinpkg`, in each binrepo's index: its `BUILD_ID` and its
/// dependency strings.
fn builds_of(
    ctx: &ResolveCtx<'_>,
    config: &Config,
    mask: &BinaryMask,
) -> Vec<(Option<String>, Vec<String>)> {
    let (cat, pkg, ver, _) = mask;
    let cpv = format!("{cat}/{pkg}-{ver}");
    let mut keys = vec!["RDEPEND", "IDEPEND", "PDEPEND"];
    if ctx.with_bdeps {
        keys.extend(["DEPEND", "BDEPEND"]);
    }
    let mut indexes = vec![ctx.local_binpkg.clone()];
    if ctx.getbinpkg {
        indexes.extend(
            config
                .binrepos
                .iter()
                .map(|b| cached_binary_index(&b.packages_dir(ctx.root))),
        );
    }
    indexes
        .iter()
        .flat_map(|index| {
            index
                .entries_for_cp(cat, pkg)
                .filter(|e| e.get("CPV") == Some(&cpv))
                .map(|e| {
                    (
                        e.get("BUILD_ID").cloned(),
                        keys.iter().filter_map(|k| e.get(*k).cloned()).collect(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The built slot-operator atoms `owner` records on dependencies that found
/// no candidate in this pass: the atoms its builds fail on.
fn unsatisfied_built_atoms(pass: &PassResult, owner: (&str, &str)) -> Vec<String> {
    let mut atoms: Vec<String> = pass
        .slot_pullers
        .iter()
        .filter(|(dep_cp, _)| {
            pass.entries.iter().any(|e| {
                (&e.category, &e.package) == (&dep_cp.0, &dep_cp.1)
                    && matches!(e.outcome, PretendOutcome::NoVisibleCandidate)
            })
        })
        .flat_map(|(_, triples)| triples)
        .filter(|(pc, pp, _, _)| (pc.as_str(), pp.as_str()) == owner)
        .filter(|(_, _, _, text)| {
            // drop-ok: an atom that does not parse cannot match; its build is left to the old one-per-restart mask.
            portage_dep::parse_atom(text)
                .is_some_and(|a| a.blocker == portage_dep::Blocker::None && is_built_slot_op(&a))
        })
        .map(|(_, _, _, text)| text.clone())
        .collect();
    atoms.sort();
    atoms.dedup();
    atoms
}

/// `masks` plus, for each, the sibling builds of the same version that bind
/// one of the owner's unsatisfied built atoms the same way. Under
/// `--usepkgonly` nothing is added: the binary result is dropped there anyway.
pub(crate) fn with_siblings(
    ctx: &ResolveCtx<'_>,
    config: &Config,
    pass: &PassResult,
    masks: BTreeSet<BinaryMask>,
) -> BTreeSet<BinaryMask> {
    if ctx.usepkgonly {
        return masks;
    }
    let mut all = masks.clone();
    for mask in &masks {
        let builds = builds_of(ctx, config, mask);
        for bound in unsatisfied_built_atoms(pass, (&mask.0, &mask.1)) {
            all.extend(sibling_masks(&builds, mask, &bound));
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUND: &str = "media-libs/libjxl:0/0.13=";

    #[test]
    fn the_same_slot_and_subslot_binding_matches() {
        assert!(binds_same_atom(
            &[">=dev-libs/a-1 media-libs/libjxl:0/0.13="],
            BOUND
        ));
    }

    #[test]
    fn use_conditions_and_version_operators_do_not_hide_the_binding() {
        assert!(binds_same_atom(
            &["foo? ( >=media-libs/libjxl-0.7:0/0.13=[abi_x86_64(-)] )"],
            BOUND
        ));
    }

    #[test]
    fn another_subslot_is_a_different_binding() {
        assert!(!binds_same_atom(&["media-libs/libjxl:0/0.12="], BOUND));
    }

    #[test]
    fn another_package_with_the_same_slot_is_not_the_binding() {
        assert!(!binds_same_atom(&["media-libs/libpng:0/0.13="], BOUND));
    }

    #[test]
    fn an_atom_without_the_equals_operator_is_not_a_built_binding() {
        assert!(!binds_same_atom(&["media-libs/libjxl:0/0.13"], BOUND));
    }

    fn build(id: &str, deps: &str) -> (Option<String>, Vec<String>) {
        (Some(id.to_string()), vec![deps.to_string()])
    }

    fn mask(id: &str) -> BinaryMask {
        (
            "sci-libs".into(),
            "gdal".into(),
            "3.13.3".into(),
            Some(id.into()),
        )
    }

    #[test]
    fn siblings_with_the_same_binding_are_masked_and_the_others_kept() {
        let builds = vec![
            build("3", "media-libs/libjxl:0/0.13="),
            build("2", "media-libs/libjxl:0/0.13="),
            build("1", "media-libs/libjxl:0/0.12="),
        ];
        let got = sibling_masks(&builds, &mask("3"), BOUND);
        assert_eq!(got, vec![mask("2")], "build 1 is bound to 0.12 and stays");
    }

    #[test]
    fn the_build_that_triggered_it_is_not_returned_again() {
        let builds = vec![build("3", "media-libs/libjxl:0/0.13=")];
        assert!(sibling_masks(&builds, &mask("3"), BOUND).is_empty());
    }

    #[test]
    fn a_build_without_an_id_cannot_be_named_and_is_left_alone() {
        let builds = vec![(None, vec!["media-libs/libjxl:0/0.13=".to_string()])];
        assert!(sibling_masks(&builds, &mask("3"), BOUND).is_empty());
    }
}
