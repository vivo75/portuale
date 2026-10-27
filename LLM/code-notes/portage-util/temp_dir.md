# crates/portage-util/src/temp_dir.rs
A scratch directory that removes itself, plus a process-level stale sweep.

## module
- Backlog #260. Every test scratch directory used to be `std::env::temp_dir().join(format!("{tag}-{pid}-{nanos}"))` with a trailing `remove_dir_all` at the end of the test -- skipped by a panic, by `panic = "abort"` (the release profile the verification pass runs under, skipping even a plain failed assert), and by a kill from outside. The result was ~85k top-level `/tmp` entries exhausting the tmpfs inodes.
- Same division of labour `ebuild_phases.rs::bin_dir` and pmtest's `_reclaim_portuale_bin_overlays` already use for the `portuale-bin.*` overlay (backlog #88).
- The prefix makes owned entries recognizable from the outside (`find /tmp -name 'portuale-td-*'` lists exactly ours).

## struct TempDir
- Derefs to `Path`, so the existing `tmp.join(..)` / `&tmp` call shapes keep working unchanged.
- Tests never `keep`: each test binds the guard and uses `dir.path()`, so the
  directory is removed on drop even under `panic = "abort"`-adjacent unwinds
  and kills (the sweep reclaims the rest on the next run). The only test-side
  keep was the guard's own self-test.
- Helpers that build trees return the guard (`TempDir`, or a tuple with the
  derived path); binding a bare `helper().join(..)` temporary drops the guard
  at once, and recreating the path then leaks it, so call sites bind first.
- The deliberate keeps left are product code only: two `tempfile` staging
  keeps removed after use, and four remote-path keeps with explicit cleanup
  (guard-equivalent `Drop`, remove-on-every-exit, documented forensics).
  `temp_dir_keep_scan` (portage-util integration test) enforces the list.

## tests: only_allowlisted_calls_may_keep_a_scratch_guard
- Source scan over `crates/`: any `.keep()` outside the allowlist fails. The
  needle is built by concatenation so the scan's own source never matches.

## fn stale_sweep_once
- Conservative in the safe direction: never delete a live run's scratch out from under it.

## fn pid_is_alive
- The workspace is a Linux/Gentoo package manager (musl static target); on a kernel without procfs this returns `true` rather than deleting a live run's directories.

## tests: temp_dir_removes_itself_on_unwind
- Backlog #260's actual failure mode; `catch_unwind` stands in for the panic.
