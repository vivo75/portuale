//! Second pass: per package slot, put the versions that satisfy every
//! atom aimed at that slot first.

use std::collections::{BTreeMap, BTreeSet};

use portage_profile::Config;
use portage_repo::{Candidate, RepoConfig};

/// One package slot with its versions in final order.
pub struct SlotSet {
    pub cp: String,
    pub slot: String,
    /// `(candidate, visible, satisfies every requester)`, final order.
    versions: Vec<(Candidate, bool, bool)>,
    /// Each atom aimed at this slot and who asked for it.
    pub atoms: Vec<(String, Vec<String>)>,
    /// How many visible versions satisfy every atom.
    satisfying: usize,
    /// No visible version satisfies the requesters, here or in another slot.
    pub invalid: bool,
    /// Nothing satisfies them here, but another slot of the package does.
    pub unneeded: bool,
    /// The version installed in this slot, if any.
    pub installed: Option<String>,
}

/// What a plan would do to a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    New,
    Upgrade,
    Downgrade,
}

impl SlotSet {
    /// `cat/pkg:slot`, the name the dependency edges use.
    #[must_use]
    pub fn node(&self) -> String {
        format!("{}:{}", self.cp, self.slot)
    }

    /// The version a plan would install: the first visible one that satisfies
    /// every requester.
    #[must_use]
    pub fn chosen(&self) -> Option<&Candidate> {
        self.versions
            .iter()
            .find(|(_, visible, ok)| *visible && *ok)
            .map(|(c, _, _)| c)
    }

    /// What installing [`chosen`](Self::chosen) would change, `None` when the
    /// slot already has that version or has no choice.
    #[must_use]
    pub fn change(&self) -> Option<Change> {
        let chosen = self.chosen()?;
        match &self.installed {
            None => Some(Change::New),
            Some(have) => match crate::version_cmp(&chosen.version, have) {
                std::cmp::Ordering::Greater => Some(Change::Upgrade),
                std::cmp::Ordering::Less => Some(Change::Downgrade),
                std::cmp::Ordering::Equal => None,
            },
        }
    }
}

impl SlotSet {
    pub fn print(&self, only_best: bool) {
        let state = match (self.invalid, self.unneeded) {
            (true, _) => "INVALID",
            (_, true) => "unneeded (another slot serves its requesters)",
            _ => "ok",
        };
        println!(
            "{}:{}  {state} ({} satisfy)",
            self.cp, self.slot, self.satisfying
        );
        if self.invalid || self.unneeded {
            for (atom, by) in &self.atoms {
                println!("    wants {atom}  <- {}", by.join(", "));
            }
        }
        let n = if only_best { 1 } else { usize::MAX };
        for (c, visible, ok) in self.versions.iter().take(n) {
            let vis = if *visible { ' ' } else { 'M' };
            let sat = if *ok { '*' } else { ' ' };
            println!("  {sat}{vis} {}::{}", c.version, c.repo_name);
        }
    }
}

/// What every package shares: who asked for each atom, and the tree.
pub struct Ctx<'a> {
    pub requesters: &'a BTreeMap<String, BTreeSet<String>>,
    pub repos: &'a [RepoConfig],
    pub config: &'a Config,
    pub memo: &'a crate::Memos<'a>,
    /// More than one satisfying version also counts as invalid.
    pub unique: bool,
}

/// Reorders every slot of `category/package` against the `atoms` aimed at
/// the package. An atom is aimed at a slot when it matches at least one
/// version of it; a version satisfies the slot when it matches all of them.
/// A slot nothing satisfies is `invalid`, unless a visible version of
/// another slot satisfies all the same atoms (then it is `unneeded`).
pub fn reorder(category: &str, package: &str, atoms: &[&String], ctx: &Ctx<'_>) -> Vec<SlotSet> {
    let Ok(all) = portage_repo::list_candidates(ctx.repos, category, package) else {
        return Vec::new();
    };
    let matches = |atom: &str, c: &Candidate| {
        ctx.memo
            .atom_matches(atom, c, (category, package), ctx.config)
    };
    let visible = |c: &Candidate| ctx.memo.visible(c, category, package, ctx.config);
    let mut slots: BTreeMap<&str, Vec<&Candidate>> = BTreeMap::new();
    for c in all.iter() {
        slots.entry(c.slot.as_str()).or_default().push(c);
    }
    let mut out: Vec<(SlotSet, Vec<&String>)> = slots
        .iter()
        .map(|(slot, cands)| {
            let aimed: Vec<&String> = atoms
                .iter()
                .copied()
                .filter(|a| cands.iter().any(|c| matches(a, c)))
                .collect();
            let mut versions: Vec<(Candidate, bool, bool)> = cands
                .iter()
                .map(|c| {
                    let ok = aimed.iter().all(|a| matches(a, c));
                    ((*c).clone(), visible(c), ok)
                })
                .collect();
            versions.sort_by(|(a, av, aok), (b, bv, bok)| {
                bok.cmp(aok).then_with(|| crate::rank(b, *bv, a, *av))
            });
            let satisfying = versions.iter().filter(|(_, v, ok)| *v && *ok).count();
            let set = SlotSet {
                cp: format!("{category}/{package}"),
                slot: (*slot).to_string(),
                versions,
                atoms: aimed
                    .iter()
                    .map(|a| {
                        let by = ctx.requesters.get(*a).map(|r| r.iter().cloned().collect());
                        ((*a).clone(), by.unwrap_or_default())
                    })
                    .collect(),
                satisfying,
                invalid: satisfying == 0 || (ctx.unique && satisfying > 1),
                unneeded: false,
                installed: ctx
                    .memo
                    .installed_versions
                    .get(&(format!("{category}/{package}"), (*slot).to_string()))
                    .cloned(),
            };
            (set, aimed)
        })
        .collect();
    // A slot nothing satisfies is not a problem when another slot serves
    // all the atoms aimed at it.
    let served_elsewhere: Vec<bool> = out
        .iter()
        .enumerate()
        .map(|(i, (set, aimed))| {
            set.satisfying == 0
                && out.iter().enumerate().any(|(j, (other, _))| {
                    j != i
                        && other
                            .versions
                            .iter()
                            .any(|(c, v, _)| *v && aimed.iter().all(|a| matches(a, c)))
                })
        })
        .collect();
    for ((set, _), elsewhere) in out.iter_mut().zip(served_elsewhere) {
        if elsewhere {
            set.invalid = false;
            set.unneeded = true;
        }
    }
    out.into_iter().map(|(set, _)| set).collect()
}

#[cfg(test)]
mod tests {
    use crate::testkit::analyse_fixture;

    /// The versions of slot 0 of `cp` in final order, as `version` (with a
    /// trailing `*` when it satisfies every atom aimed at the slot).
    fn order(world_atoms: &[&str], cp: &str) -> Vec<String> {
        let world = analyse_fixture(world_atoms, &[]);
        let slot = world
            .slots
            .iter()
            .find(|s| s.cp == cp && s.slot == "0")
            .unwrap_or_else(|| panic!("{cp}:0 is not in the world"));
        slot.versions
            .iter()
            .map(|(c, _, ok)| format!("{}{}", c.version, if *ok { "*" } else { "" }))
            .collect()
    }

    /// PMS `names.tex`, "Version comparison" / charter property 3: the newest version
    /// (a revision after its base) leads the slot.
    #[test]
    fn the_highest_version_leads_its_slot() {
        assert_eq!(
            order(&["dev-libs/w701ver"], "dev-libs/w701ver"),
            ["2.0*", "1.1-r1*", "1.1*", "1.0*"]
        );
    }

    /// Charter property 3: with equal versions the repository of higher
    /// priority comes first (overlay 10 before testrepo -1000).
    #[test]
    fn equal_versions_follow_repo_priority() {
        let world = analyse_fixture(&["dev-libs/overlaytiepkg"], &[]);
        let slot = world
            .slots
            .iter()
            .find(|s| s.cp == "dev-libs/overlaytiepkg")
            .expect("overlaytiepkg in the world");
        let repos: Vec<&str> = slot
            .versions
            .iter()
            .map(|(c, _, _)| c.repo_name.as_str())
            .collect();
        assert_eq!(repos, ["overlay", "testrepo"]);
        assert_eq!(slot.chosen().expect("chosen").repo_name, "overlay");
    }

    /// Charter property 3: the version outranks the repository, so 2.0 in
    /// the lower-priority repo leads 1.0 in the higher-priority one.
    #[test]
    fn a_lower_priority_repo_wins_with_a_higher_version() {
        let world = analyse_fixture(&["dev-libs/w701lowhi"], &[]);
        let slot = world
            .slots
            .iter()
            .find(|s| s.cp == "dev-libs/w701lowhi")
            .expect("w701lowhi in the world");
        let got: Vec<(&str, &str)> = slot
            .versions
            .iter()
            .map(|(c, _, _)| (c.version.as_str(), c.repo_name.as_str()))
            .collect();
        assert_eq!(got, [("2.0", "testrepo"), ("1.0", "overlay")]);
    }

    /// PMS `dependencies.tex`, "Operators" (`=`): exact version and revision, so `1.1-r1` does not
    /// match `=...-1.1`; the one match leads the slot.
    #[test]
    fn an_equal_atom_keeps_one_version() {
        assert_eq!(
            order(&["=dev-libs/w701ver-1.1"], "dev-libs/w701ver"),
            ["1.1*", "2.0", "1.1-r1", "1.0"]
        );
    }

    /// PMS `dependencies.tex`, "Operators" (`<`): strictly older only; the newer ones stay behind.
    #[test]
    fn a_less_than_atom_drops_the_newer_versions() {
        assert_eq!(
            order(&["<dev-libs/w701ver-1.1"], "dev-libs/w701ver"),
            ["1.0*", "2.0", "1.1-r1", "1.1"]
        );
    }

    /// PMS `dependencies.tex`, "Operators" (`~`): every revision of the named version, none of the others.
    #[test]
    fn a_tilde_atom_keeps_every_revision_of_one_version() {
        assert_eq!(
            order(&["~dev-libs/w701ver-1.1"], "dev-libs/w701ver"),
            ["1.1-r1*", "1.1*", "2.0", "1.0"]
        );
    }

    /// Charter property 1: a version must satisfy every atom aimed at the
    /// slot, so two atoms keep only their intersection.
    #[test]
    fn two_atoms_on_one_slot_keep_their_intersection() {
        assert_eq!(
            order(
                &[">=dev-libs/w701ver-1.1", "<dev-libs/w701ver-2.0"],
                "dev-libs/w701ver"
            ),
            ["1.1-r1*", "1.1*", "2.0", "1.0"]
        );
    }
}
