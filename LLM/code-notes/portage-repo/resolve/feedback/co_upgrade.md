# crates/portage-repo/src/resolve/feedback/co_upgrade.rs
Co-upgrade (#698): installed holders of a pin follow an upgrade when each has a route; the route-less ones are named (#703).

## fn holder_routes
- Backlog #703: the one computation both solvers share. For one withheld provider it returns each holder's route and, when some holder has none, the `HeldBackNoRoute` record behind the "cannot follow" note. `co_upgrade_request` (default solver, holders from the skipped-update rows) and `holder_fix` in `solver_bridge.rs` (pubgrub, holders from the recorded deps the move breaks) both call it, so the rule "any route-less holder withholds the upgrade" and the note's content have one source.
- Record shape: a new `GraphResult::held_back_no_route` rather than a reason on `HeldBackUse` (#699). One group lists several holders and its closing sentence is a fact about the holder ("No visible version of X accepts ..."), whereas #699 has one consumer and a `Change USE:` command; a shared record would need an enum and two printer branches. The printer is `show_held_back_no_route` in `pretend/problem_reports.rs`, same quiet rule.
- The atom shown is the holder's recorded atom that rejects the new version, the first in sort order (a versioned `=cat/pkg-ver*` sorts before a `:slot/sub=` atom).

## fn co_upgrade_request
- Backlog #703: the notes are recorded before the `--backtrack=0` early return, so a run with no second walk still names the holder; with no world nothing is recorded (nobody is protected, #704 / #700). `driver.rs` keeps a note only for a provider that is still in the final `skipped_updates`.
- Pubgrub: a withheld provider is pinned to its installed version in the next solve round, so only the round that found the route-less holder knows it; `resolve_pubgrub_unstale` accumulates the notes across rounds and stores them on the final result. Pubgrub still prints no skipped-update block (#431 S1 cut); the note is filled from repo facts, not from the bridge's conflicts.
- Backlog numbers removed from the comment (original text): The request that lets every installed holder of a withheld provider follow its upgrade, when each of them has a route: the walk is run again with the holders as extra roots (`=cat/pkg-ver` for those that must move to a newer version, a forced reinstall for those a rebuild mends). Providers with a holder that cannot follow (or whose pin is a USE dependency) stay withheld. `None` when nothing is withheld or nothing can follow. Next to the request comes each `(holder, provider)` `(category, package)` pair, so the result of the second walk can name why a holder is in the plan. The notes (#703) name the holders that cannot follow, whether or not a second walk runs.

## fn request_updates

- Backlog #727 (owner decision 2026-11-12, smallest plan): the co-upgrade applies only to a provider the request wants updated. Portage `depgraph.py` `_want_update_pkg` returns `(arg_atoms or update) and not _too_deep(depth)`: an argument atom (or set member) naming the package, or `--update` within `--deep`. The depth of the provider is not known here, so `--update` counts only together with `--deep` (any value). The default solver calls it per skipped provider in `co_upgrade_request`; the pubgrub bridge calls it in `holder_fix` with the request as the user typed it (`asked`, without the atoms the holder pass added) and, when it is false, pins the installed version like a route-less holder (the #703 NOTE is still printed when a holder has no route). `-u` without `-D` therefore keeps a dependency provider, as Portage does.
- Backlog numbers removed from the comment (original text): Whether the request wants `provider` updated: it names the package itself (a command-line atom or a set member), or it is an `--update` that walks dependencies (`--deep`). Otherwise a withheld update stays withheld, the smallest plan (#727). Portage: depgraph.py, `_want_update_pkg` (`arg_atoms or update`, within `--deep`).

## fn name_holder_parents (solver_bridge.rs)

- Backlog #718 (a): `holder_fix` returns, next to the rebuild and atom lists, the `(holder, provider)` pair of every holder it routes; `resolve_pubgrub_unstale` collects them over its rounds and `name_holder_parents` adds the provider to `required_by` of the holder's row in the final plan, as `backtracking_resolve` does for the default solver (#713). `--tree` is not touched (owner decision 2026-11-12: it follows the real dependency edges).

## fn provider_atoms_accept
- Public since #769 S2: `portage-world`'s stale-binding scan (`rebuild.rs`, `Scan::judge`) uses it to decide whether a walked consumer's tree ebuild accepts the new provider, so a recorded pin the tree has let go is not a refusal. Differences from the holder route: the fast plan tests one candidate (the chosen ebuild) where the route searches all candidates of the holder; and it reads the tree with the planner's `||` reduction (`tree_reduced_atoms`) where the route's `provider_atoms` keeps every `||` arm as a menu (see #781 in plan `02.770`).

## module
- Backlog numbers removed from the comment (original text): Co-upgrade: an installed holder of a pin that an upgrade breaks may follow the upgrade instead of withholding it (backlog #698).  The decision uses repo facts only (no solver state), so every solver can share it. Charter property 1 (every kept package has its dependencies) holds either way; withholding is only needed when a holder cannot follow.

## fn holder_has_route
- Backlog numbers removed from the comment (original text): The version the installed holder `(category, package, version)` should move to so it can follow an upgrade of `provider` to the versions rendered in `new_provider` (`cat/pkg-ver:slot/sub::repo`): some visible candidate of the holder (a masked version is not a route the user can install, #774), in its installed slot and not older than it, whose `*DEPEND` names `provider` only in ways the new versions satisfy. A rebuild of the same version counts (its tree dependencies, not the vdb ones, are what binds).  A versioned atom on `provider` is a menu of acceptable versions (the `virtual/perl-*` shape): one of them must match. Unversioned atoms (`cat/pkg:=`, `cat/pkg:0`) must all match.

## fn tree_route_atoms
- Backlog numbers removed from the comment (original text): The atoms on `provider` that a candidate's tree dependencies *require*: every atom outside a `||` group, and for a `||` group the atoms of its arms only when every arm names `provider`. A group with an arm on another package can be met without the provider, so it constrains nothing (backlog #781).

## fn tree_follow_atoms
- Backlog numbers removed from the comment (original text): What the consumer's tree needs for the provider to be `new` (`cat/pkg-ver:slot/sub::repo`): `None` when an atom outside any `||` group, or a group whose arms all name the provider, rejects it; otherwise the atoms to ask for so that each group is met without a rejecting arm (the first arm that accepts the provider, else the first arm on other packages; backlog #808).

## fn or_group_with_an_arm_off_the_provider_constrains_nothing
- Backlog numbers removed from the comment (original text): #781: the `p569virt` shape, a version menu plus another package.

## fn follow_asks_for_the_arm_off_the_provider
- Backlog numbers removed from the comment (original text): #808: the rejecting arm is skipped, the other package is asked for.
