# #305 (feat#157) — S2.8 round trip on the corpus

Binary: portuale `f0f98a62` (default features, vdb-sqlite on). Run
2026-10-04 by the coordinator in systemd scope `oc-305-s28-host`; the host
VDB was only read.

## Host (`/var/db/pkg`, 2,130 entries, 75,611 files, 143,941,725 bytes)

| Step | Command | Result | Time |
|---|---|---|---|
| files → sqlite | `portuale vdb convert --from files:/ --to sqlite:host.db` | 2130 entries; world 207 atoms, 0 sets; preserved libs 0; config memory 0; counter 23336; rc 0 | 6.2 s |
| sqlite → files | `portuale vdb convert --from sqlite:host.db --to files:hostroot` | same counts, rc 0 | 0.5 s |
| verify | `portuale vdb verify files:/ sqlite:host.db` | `equal: 2130 entries compared`, rc 0 | 0.5 s |
| verify | `portuale vdb verify files:/ files:hostroot` | `equal: 2130 entries compared`, rc 0 | 0.6 s |
| byte diff | `diff -r /var/db/pkg hostroot/var/db/pkg` | no output, rc 0; apparent size identical (143,941,725) | |
| corpus stats | `docs/evidence/305-s0-corpus.py hostroot/var/db/pkg` | 2130 entries, 75,611 files, 129 empty files, 1,500 entries without `metadata`, **6 stale stamps** — identical to the source (the stale ones stayed stale) | |

The sqlite file is 343,932,928 bytes (WAL checkpointed): about 2.4× the
apparent VDB size, mostly the uncompressed-in-page CONTENTS and
`environment.bz2` blobs plus the derived `owner` table.

## Fixtures

The pmtest fixture VDB (131 entries) round-trips files → sqlite → files
with `verify` rc 0 in `portuale/src/vdb_cmd.rs`'s tests (S2.7), which also
show that a one-byte change and a world change are reported. The
second fixture (`quickpkgroot`, 1 entry) is covered by the S2.8 corpus
test.
