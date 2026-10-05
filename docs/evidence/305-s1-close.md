# #305 (feat#157) — S1 closed: the refactor is behaviour-neutral on `files`

Code under test: portuale `47b98f4f` (S1.1–S1.9: every installed-database
access in production code goes through `portage_vdb::for_root`, guarded by
`portage-vdb/tests/no_hand_built_vdb_paths.rs`). Run 2026-10-04 in systemd
scope `oc-305-s110` (`helpers/scoperun.sh`). Compare with
[`305-baseline.md`](305-baseline.md).

| Check | Baseline (P0.2) | S1 close | Equal |
|---|---|---|---|
| `cargo fmt --check` | clean | clean | yes |
| `cargo clippy --release --all-targets` | 4 warnings | 4 warnings (the same) | yes |
| `cargo test --release` | all green | 1987 passed, 1 failed: `emerge_build::tests::a_hard_failure_kills_still_running_builds_instead_of_waiting_them_out`, which ran while a musl container build loaded the host; alone it passes 3/3. Timing-sensitive test from before #305 (residue). | yes |
| pmtest suite | 2248 passed, 37 skipped, 2 xfailed | 2248 passed, 37 skipped, 2 xfailed | yes |
| L0 | 120 probes, 101 clean, 0.842, 8 explained, 31 unexplained | `l0-20261004T210702Z`: 120, 101, 0.842, 8, 31 | yes |
| L1 gate, build | 0 hard / 0 unexplained, 7 mtime-only | `l1-20261004T211452Z`: 0 / 0, 7 mtime-only | yes |
| L1 gate, re-merge (glibc + bash) | merged 2/2, 0 / 0, 1415 mtime-only | `l1-20261004T214842Z`: merged 2/2, 0 / 0, 1415 mtime-only | yes |

Per-step syscall checks (in each commit message) showed identical VDB
syscall sequences for resolve, merge, unmerge, world and quickpkg paths,
with three documented reorders/reductions (S1.5 `blockers_from_flat_deps`
d_type listing, S1.6 `emerge -C <name>` listing order, S1.7 shadow
`is_dir` stats) and none in output.
