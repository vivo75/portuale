# #305 (feat#157) — P0.2 baseline

Branch point: portuale `3e131368` (code identical to `main` `e2d411ce`;
the commits between them are docs only), pmtest `main` (`b4b5d17a`).
Recorded 2026-10-04. Every later step compares against these numbers.

| Check | Command | Result |
|---|---|---|
| fmt | `cargo fmt --check` (in `rust/`) | clean |
| clippy | `cargo clippy --release --all-targets` | **4 pre-existing warnings**, all in `portage-repo/src/lib.rs`: `too_many_arguments` (`slot_operator_slot_change_probe`), `manual_div_ceil`, 2 × `cloned_ref_to_slice_refs` (test code). V-std = no new warnings. |
| cargo test | `cargo test --release` | green: portage-repo 965, portuale 746, portage-profile 82, portage-dep 55, portage-fetch 48, portage-required-use 20, portage-use-reduce 19, mrg-director 17, portage-util 8, portage-versions 5, shuffle_seed_freeze 1 |
| pmtest suite | `python3 -m pytest pytests-contract-suite -q` (in `../pmtest`) | **2248 passed, 37 skipped, 2 xfailed** (394 s) |
| L0 | `differential-test-bed/run/l0-resolver.sh` | `l0-20261004T172739Z`: 120 probes, 101 clean, parity 0.842, 8 explained, **31 unexplained** (= main's known set), invariants 0 violations; rc 1 as on main |
| L1 merge gate, build | `l1-merge-from-binpkg.sh atomlists/l1-merge-gate.txt` | `l1-20261004T173503Z`: 0 hard / 0 unexplained / 0 payload / 7 mtime-only, rc 0 |
| L1 merge gate, re-merge | `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1 …` same list | `l1-20261004T180824Z`: glibc + bash merged 2/2 (merge_rc 0), 0 hard / 0 unexplained / 0 payload / 1415 mtime-only, rc 0 |

Host VDB syscall counts (from S1.2's parity check, baseline binary, lines
of `strace -f -e trace=openat,statx,newfstatat,stat,lstat` naming
`/var/db/pkg`):

| Command | openat | statx | total |
|---|---|---|---|
| `emerge -p --depclean` | 14346 | 49547 | 63893 |
| `emerge -pv sys-apps/portage` | 16355 | 59194 | 75549 |
