# 03 — command exec: a function pipeline stage deadlocks past one pipe buffer

**Crate:** `brush-core` · **File:** `src/commands.rs` (+ `brush-shell/tests/cases/compat/pipeline.yaml`) · **Patch:** [`patches/03-function-pipeline-stage-deadlock.patch`](patches/03-function-pipeline-stage-deadlock.patch)

> This is a rebase of the never-reviewed [#1276](https://github.com/reubeno/brush/pull/1276)
> onto current `main` (the fix moved from `interp.rs` to `commands.rs`, and
> `SimpleCommand` grew `post_execute` in the meantime).

## Symptom

```console
$ brush -c 'big() { local i; for ((i=0;i<20000;i++)); do printf "%d: padding.....\n" "$i"; done; }
big | wc -l'
# hangs forever
```

Two lines, no external command, no builtin. bash prints `20000`.

## Root cause

`spawn_pipeline_processes` spawns pipeline stages in a loop, `await`ing each
stage's `execute_in_pipeline` before moving to the next:

```rust
for (i, command) in pipeline.seq.iter().enumerate() {
    // ... install this stage's stdin/stdout pipe fds ...
    let spawn_result = command.execute_in_pipeline(ctx, params).await?;   // <-- (*)
    // ...
}
```

For a non-last stage the shell is `ShellForCommand::OwnedShell`. In that case:

- **external command** → `execute_via_external` returns `StartedProcess`
  immediately; `(*)` unblocks.
- **builtin** → `execute_via_builtin_in_owned_shell` wraps the work in
  `tokio::task::spawn_blocking` and returns `StartedTask` immediately.
- **function** → `execute_via_function` `await`s `invoke_shell_function`
  **inline**. `(*)` does not return until the function body has fully run.

So the *next* stage — the one that would `read()` the far end of this stage's
stdout pipe — is never spawned. A function that writes more than the pipe buffer
(~64 KiB on Linux) before returning blocks on `write()` with no reader: a hard
deadlock.

A real shell always runs every pipeline stage as its own concurrently-running
process, so this never arises there.

## Fix

Split `execute_via_function` exactly like `execute_via_builtin`:

```rust
async fn execute_via_function(self, reg) -> Result<ExecutionSpawnResult, Error> {
    match self.shell {
        ShellForCommand::OwnedShell { target, .. } =>
            Ok(Self::execute_via_function_in_owned_shell(*target, self.params, reg,
                                                         self.command_name, self.args)),
        ShellForCommand::ParentShell(..) =>
            self.execute_via_function_in_parent_shell(reg).await,   // unchanged body
    }
}
```

`execute_via_function_in_owned_shell` mirrors
`execute_via_builtin_in_owned_shell` line for line: `spawn_blocking(move || {
rt.block_on(invoke_shell_function(...)); shell.update_last_arg_variable(...);
... })` returning `StartedTask`. The pipeline loop proceeds immediately; the
function runs concurrently on the blocking pool.

`invoke_shell_function` only ever returns `ExecutionSpawnResult::Completed`
today; the owned-shell wrapper unwraps that and `error::unimp()`s on
`StartedProcess`/`StartedTask` rather than silently dropping already-started
work.

`post_execute` (only ever the ephemeral-`Command`-scope pop from
`interp.rs`) is not run on the owned-shell path — the owned shell is discarded
after the stage, so the scope goes with it. This matches
`execute_via_builtin_in_owned_shell`, which likewise skips it.

The parent-shell path (a pipeline's *last* stage under `lastpipe` + no job
control — never needs to unblock a downstream reader) keeps the exact original
inline body, `post_execute` and all.

## Tests

`brush-shell/tests/cases/compat/pipeline.yaml` — new case
"Function stage writing more than a pipe buffer before the next stage is
spawned": `big | wc -l` with `big` emitting 5000 lines. Deadlocks (times out
under the harness's 15 s limit) without the fix; matches bash with it.

Full compat suite: 0 failed (unchanged). `cargo test -p brush-core` green.

## Note

Portage's own `bin/phase-functions.sh` no longer hits this on the portuale side
(a `${T}`-temp-file rewrite of `__save_and_filter_ebuild_env`), but any ebuild
in the wild that pipes a `pkg_*` function into a filter still needs this.
