# crates/portuale/src/ebuild_phases/tests/terminal.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): Phases and the controlling terminal (backlog #408).  Oracle: a probe under a real pty (`LLM/evidence/2026-10-18-408-s0/`): `bash -i -c true` in a `process_group(0)` child stops with SIGTTIN even with stdin on `/dev/null`; it finishes in the parent's group or under `setsid`. Portage: `bin/pid-ns-init` (`os.setsid()`), `doebuild.py` (`_global_pid_phases`).  Each test re-executes this test binary in a child that owns a fresh pty as its controlling terminal; the `#[ignore]`d `inner_*` tests are the bodies that run there.
