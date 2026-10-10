# crates/atom-harness/src/main.rs
Neutral CLI test-harness binary for portuale's atom matching.

## module
- Implements the v1 atom grammar (see `crates/portage-dep/src/lib.rs` and `LLM/agent-context.md`).
- Same argv/output contract as `python/atom_harness.py`. NOTE: the Python mirror no longer exists (see AGENTS.md rule 4); the contract description above is the surviving spec.

## fn format_use_dep
- Reconstructs `"{prefix}{flag}{default}{suffix}"`, matching Portage's `a.use.tokens` original-text preservation byte for byte, since portuale's grammar doesn't normalize anything the token regex didn't already require.

## fn format_affecting
- The dep string's tokens are whitespace-split again inside `extract_affecting_use`, hence the join-then-resplit round trip.
