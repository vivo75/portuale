# crates/harness-common/src/lib.rs
Shared CLI scaffolding for portuale's neutral test-harness binaries.

## module
- Every harness is a thin per-crate `dispatch(op, args)` over the library surface plus the same two-mode driver: one operation per process invocation in correctness mode, or a stdin "batch" loop in benchmark mode (see `LLM/agent-context.md`, "Test/benchmark harness architecture").
- Covers versions-harness, atom-harness, use-reduce-harness, required-use-harness.
- The argv/output contract implemented here is identical to the Python-side harnesses. NOTE: the Python mirror no longer exists (see AGENTS.md rule 4).

## fn run_batch
- Stop with `ExitCode::FAILURE` at the first dispatch error; trailing newlines never terminate the loop.

## fn main_dispatch
- A single op prints its result on stdout (exit 0) or its error on stderr (exit 2).
