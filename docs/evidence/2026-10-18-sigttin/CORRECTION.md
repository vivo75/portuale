# Correction (2026-10-18, backlog #408)

The captures here are genuine; the first diagnosis drawn from them was not.
The freeze does not come from the phase inheriting the terminal as stdin: a
pty probe showed `bash -i -c true` in a `process_group(0)` child stops with
SIGTTIN even with stdin on `/dev/null` (`bash -i` uses `/dev/tty` or stderr).
The cause is the phase being a background process group of the terminal.
Fix and details: `LLM/code-notes/portuale/ebuild_phases/scheduler_registry.md`
(`PhasePlacement`), `LLM/2026-10-18-batch-plan.review.md`.
