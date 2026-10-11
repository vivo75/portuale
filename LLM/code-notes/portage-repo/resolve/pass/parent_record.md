# crates/portage-repo/src/resolve/pass/parent_record.rs
Record this atom's per-instance parent-atom row.

## fn record_parent_atom
- Backlog #59 (S1): Portage `_add_pkg`'s per-instance parent-atom record (`_add_parent_atom`, `depgraph.py:3583-3604`), taken here -- after the reinstall/autounmask outcome flips, before the `other_outcomes` dedup below -- so the child instance is the one this atom actually resolved to. An `AlreadyInstalled` instance shadowed later by a same-slot merge keeps its own row: `paired-1.0` keeps its own row under othermod's `<paired-2.0`. A removal never resolves a dependency atom (backlog #72 B3, kept in code).

