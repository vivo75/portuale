# crates/portage-repo/src/resolve/held_back_use.rs
Updates `-u` leaves alone for an installed consumer's `[use]` dependency.

## fn held_back_use_updates
- Backlog #699: with `installed w605x-1` built with the flag and a world consumer `w605x[w605flag]`, `-puD @world` keeps `w605x-1` (`select_best_candidate`'s early `AlreadyInstalled` for a `[use]` atom only the installed instance meets, `pretend.rs`) and printed nothing. That function sees no parent and runs once per backtrack pass, so the report is a scan after the final plan instead.
- The scan builds a `RevDepPin` for every installed in-scope consumer atom that carries `[use]` deps and names a kept package, and lets `constraint_withheld_updates` decide which candidates the atom keeps out (it already re-checks `[use]` deps against the candidate's own USE). `use_unsat_candidates_for_atom` then supplies the `Change USE:` rows; a row without one (flag masked or forced, `Missing IUSE`) reports nothing, because the user could not act on it.
- Atoms inside a `||` group are skipped: a consumer that can do without the package holds nothing back (cell A, `w605cona`).
- Gate `ctx.update`: without `-u` nothing was asked to move. A note only: the plan, the exit code and `skipped_updates` stay as they were. Portage 3.0.83 fails the same root with the autounmask hint (comparison only, charter property 5 decides).
