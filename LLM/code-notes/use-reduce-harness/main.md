# crates/use-reduce-harness/src/main.rs
Neutral CLI test-harness binary for `use_reduce(flat=True)`.

## module
- See LLM/agent-context.md and portage-use-reduce/src/lib.rs's doc comment for the reduction semantics.
- Same argv/output contract as python/use_reduce_harness.py.
- The CLI shell already splits argv on whitespace, which is exactly what `use_reduce`'s own `depstr.split()` first step does internally.
- S4 (#341): the `validate <eapi> <validflags> <token...>` op mirrors `portage-repo`'s `dep_validate` wiring (`parse_atom_with` + `check_conditional_flags` as the walk's hook) against live Portage's `use_reduce(matchall=True, flat=True, token_class=Atom, is_valid_flag=…)` with the `_metadata_exception` message selection (`OK` / `ERR:<category>:<message>`). The Python side runs the vendored (pure-Python `Atom`) path, which equals the C path for atoms (oracle §1.4) — sound here, never for S10's native-scanner quirk.
