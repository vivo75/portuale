# crates/portage-repo/src/resolve/slot_conflict_inputs.rs
Builds the handler's input from `SlotConflict` records (plan 02.589 R4, backlog #597).

## fn solve_slot_conflict_records
- Records of one `(category, package, slot)` are joined into one handler conflict, because Portage keeps one conflict per slot and its limits count slots (SC:110-114).
- Parents come from the records, so a parent whose atom no instance fully satisfies is not seen (the records file a parent only under an instance it satisfies). Portage's parent set also holds such atoms. No contract case depends on this.
- Installed instances read IUSE/USE/REQUIRED_USE from the vdb; tree instances from the candidate and its md5-cache entry.

## fn unevaluated_atom
- The records keep the evaluated atom (conditionals resolved against the parent's USE), but the search needs the form written in the parent's dependency string (`atom.unevaluated_atom`, SC:1054-1113). It is recovered by scanning the parent's `*DEPEND` strings for the token that evaluates to the stored atom. The tree ebuild is preferred over the vdb, like Portage's dynamic deps. When nothing matches, the evaluated atom stands in; that loses only the forcing of a parent flag through `flag?` / `flag=`.
