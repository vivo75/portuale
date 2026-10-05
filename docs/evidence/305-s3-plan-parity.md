# #305 (feat#157) — S3.3 plan parity, files vs sqlite

`mrg --pretend` must print the same plan whichever backend holds the
installed database.

## Fixtures (in `cargo test`)

`mrg.rs` test `pretend_plan_is_identical_on_files_and_sqlite` converts the
fixture root to sqlite with `portage_vdb::copy_all` and runs the built
binary once per backend for each argument set, asserting identical stdout
and stderr. The argument sets are listed in the test (widened to about ten
in S3.3).

## Host (2130 installed entries)

Measured 2026-10-04 by the coordinator, release build of `7551dc81`, the
host VDB converted with `portuale vdb convert --from files:/ --to
sqlite:host.db` (S2.8). Script `scratchpad/s32/verify.sh`, systemd scope
`oc-305-s32-verify`; two runs per backend, alternating.

| Command | files | sqlite | stdout |
|---|---|---|---|
| `mrg --pretend --depclean` | 0.55 s, 0.54 s | 0.54 s, 0.53 s | identical |
| `mrg --pretend -uDN @world` | 13.81 s, 13.72 s | 13.83 s, 13.86 s | identical |

VDB syscalls (`strace -f -e trace=openat,statx,newfstatat`, lines naming
`/var/db/pkg`):

| Command | files | sqlite |
|---|---|---|
| `--pretend --depclean` | 63,893 | 0 |
| `--pretend -uDN @world` | 417,982 | 0 |

Before S3.2's snapshot cache, sqlite took 1.10 s on `--depclean` (one SQL
query per read). With the cache, the two backends take the same time. The
447,658 `statx` calls the design measured on `-uDN @world` (§3.2) are gone
on sqlite, but they were not the bottleneck: the run is dominated by the
resolver.
