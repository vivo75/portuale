# crates/portage-repo/src/resolve/pass/entry_use_flags.rs
Computes the entry's USE flags: baked for a binary, profiled otherwise, plus any autounmask-use flip.

## fn entry_use_flags
- The baked set is what the REQUIRED_USE check (PMS 7.3.4), the `-pv` `USE="…"` line and the USE-conditional dependency walk all see, identical to what `candidate_iuse_and_use` already does everywhere else. Recomputing from the profile (`effective_use_flags`) would e.g. spuriously fail a `^^ ( elogind systemd )` REQUIRED_USE whenever the profile forces neither flag but the binary was built with exactly one.
- `--newuse`/`-N` rebuild-vs-reuse is a selection decision made upstream; if the outcome stays `Binary` here, the baked USE is authoritative.
- The recorded change mirrors Portage `_display_autounmask`'s `use_changes_msg` + `_get_dep_chain_as_comment(pkg, unsatisfied_dependency=True)`.
