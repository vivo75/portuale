# crates/required-use-harness/src/main.rs
Neutral CLI test-harness binary for REQUIRED_USE checking.

## module
- See LLM/agent-context.md and portage-required-use/src/lib.rs's doc comment for the checking semantics.
- Same argv/output contract as python/required_use_harness.py, which wraps Portage's `portage.dep.check_required_use` directly.
- The CLI shell already splits argv on whitespace, the same convention use-reduce-harness's own "reduce" op uses.
- `reduce` renders Portage's `check_required_use(...).tounicode()` through `human_readable_required_use`; the message shape matches Portage's depgraph.py "The following REQUIRED_USE flag constraints are unsatisfied:" line.
