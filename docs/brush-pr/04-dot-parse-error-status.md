# 04 — `source`: a parse error in the sourced file must not exit the caller

**Crate:** `brush-core` · **File:** `src/shell/execution.rs` (+ `brush-shell/tests/cases/compat/builtins/dot.yaml`) · **Patch:** [`patches/04-dot-parse-error-status.patch`](patches/04-dot-parse-error-status.patch)

## Symptom

```console
$ cat bad.sh
:
echo "unterminated

$ brush -c 'source ./bad.sh || echo "caught $?"; echo after'
error: ./bad.sh: unterminated double quote ...
# nothing else: no `caught`, no `after`; exit status 2
```

bash:

```console
$ bash -c 'source ./bad.sh || echo "caught $?"; echo after'
./bad.sh: line 2: unexpected EOF while looking for matching `"'
caught 2
after
```

The failure mode that found this: real `bin/ebuild.sh:580` guards the saved
environment with `source "${T}"/environment || die "error sourcing
environment"`. Under brush the guard could never run — the parse error was
fatal to the whole script, so a broken `${T}/environment` aborted the phase
mid-source instead of calling `die` (and, before portuale's own guard, the
phase then went on to run `default` with rc 0).

## Root cause

`run_parsed_result` (`src/shell/execution.rs`) converts every parse error with
`.into_fatal()`. `Error::to_control_flow` then maps *any* fatal error in a
non-interactive shell to `ExecutionControlFlow::ExitShell`, and
`source_file` returns that result to the `.`/`source` builtin — which asks the
interpreter to exit the enclosing program. That is right for a script run on
the command line (`brush script.sh`, `brush -c`), and wrong for a sourced
file: bash's `.` returns 2 and execution continues there.

## Fix

In `source_file`, remember whether the file parsed, and after
`run_parsed_result` clear the requested shell exit when the call type is
`ScriptCallType::Source`:

```rust
let parse_failed = parse_result.is_err();
let mut result = self.run_parsed_result(parse_result, source_info, params).await;

if parse_failed && matches!(call_type, callstack::ScriptCallType::Source) {
    if let Ok(result) = &mut result {
        result.next_control_flow = ExecutionControlFlow::Normal;
    }
}
```

The error is still displayed and the exit code is still 2 (`ParseError` →
`InvalidUsage`); only the control flow changes, and only at the `source`
boundary. Top-level scripts keep the fatal behavior.

`eval` is still fatal on a parse error (bash leaves the caller running there
too) — a separate, pre-existing divergence not covered by this fix.

## Tests

`brush-shell/tests/cases/compat/builtins/dot.yaml` — two new cases using a
`test_files` fixture with an unterminated quote: a bare `source ./bad.sh`
(asserts `$?` is 2 and that the next command runs) and `source ./bad.sh ||
echo "Caught: $?"`. stderr is ignored per-case because brush and bash word
their parse diagnostics differently.

Full compat suite: 0 unexpected failures. Portuale-side, the
`a_corrupt_saved_environment_fails_the_next_phase_in_both_backends` test
corrupts a real `${T}/environment` and asserts both backends fail with real
`ebuild.sh`'s own `error sourcing environment` die.
