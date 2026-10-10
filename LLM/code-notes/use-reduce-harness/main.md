# crates/use-reduce-harness/src/main.rs
Neutral CLI test-harness binary for `use_reduce(flat=True)`.

## module
- See LLM/agent-context.md and portage-use-reduce/src/lib.rs's doc comment for the reduction semantics.
- Same argv/output contract as python/use_reduce_harness.py.
- The CLI shell already splits argv on whitespace, which is exactly what `use_reduce`'s own `depstr.split()` first step does internally.
