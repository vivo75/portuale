# crates/portage-repo/src/resolve/pass/assemble_pass.rs
The pass result from the finished walk state.

## fn assemble_pass_result
- Backlog #90 (S1): the `WARNING: … package.provided:` block lists requested atoms in requested order (not walk order); atoms absent from argv sort last, stably. Staged oracle: `providedpkg` before `providedpkg2`. Only depth-0 pushes record, so absent-from-argv shouldn't happen.
