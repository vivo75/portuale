# crates/portage-repo/src/resolve/feedback/co_upgrade.rs
Co-upgrade (#698): installed holders of a pin follow an upgrade when each has a route; the route-less ones are named (#703).

## fn holder_routes
- Backlog #703: the one computation both solvers share. For one withheld provider it returns each holder's route and, when some holder has none, the `HeldBackNoRoute` record behind the "cannot follow" note. `co_upgrade_request` (default solver, holders from the skipped-update rows) and `holder_fix` in `solver_bridge.rs` (pubgrub, holders from the recorded deps the move breaks) both call it, so the rule "any route-less holder withholds the upgrade" and the note's content have one source.
- Record shape: a new `GraphResult::held_back_no_route` rather than a reason on `HeldBackUse` (#699). One group lists several holders and its closing sentence is a fact about the holder ("No visible version of X accepts ..."), whereas #699 has one consumer and a `Change USE:` command; a shared record would need an enum and two printer branches. The printer is `show_held_back_no_route` in `pretend/problem_reports.rs`, same quiet rule.
- The atom shown is the holder's recorded atom that rejects the new version, the first in sort order (a versioned `=cat/pkg-ver*` sorts before a `:slot/sub=` atom).

## fn co_upgrade_request
- Backlog #703: the notes are recorded before the `--backtrack=0` early return, so a run with no second walk still names the holder; with no world nothing is recorded (nobody is protected, #704 / #700). `driver.rs` keeps a note only for a provider that is still in the final `skipped_updates`.
- Pubgrub: a withheld provider is pinned to its installed version in the next solve round, so only the round that found the route-less holder knows it; `resolve_pubgrub_unstale` accumulates the notes across rounds and stores them on the final result. Pubgrub still prints no skipped-update block (#431 S1 cut); the note is filled from repo facts, not from the bridge's conflicts.

## fn request_updates

- Backlog #727 (owner decision 2026-11-12, smallest plan): the co-upgrade applies only to a provider the request wants updated. Portage `depgraph.py` `_want_update_pkg` returns `(arg_atoms or update) and not _too_deep(depth)`: an argument atom (or set member) naming the package, or `--update` within `--deep`. The depth of the provider is not known here, so `--update` counts only together with `--deep` (any value). The default solver calls it per skipped provider in `co_upgrade_request`; the pubgrub bridge calls it in `holder_fix` with the request as the user typed it (`asked`, without the atoms the holder pass added) and, when it is false, pins the installed version like a route-less holder (the #703 NOTE is still printed when a holder has no route). `-u` without `-D` therefore keeps a dependency provider, as Portage does.

