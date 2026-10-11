# crates/portuale/src/remote/ledger.rs
The server/client merge ledger: per-host provenance lines on both sides.

## struct LedgerEntry
- One repo-ledger line, shared by both sides: `<unix-ts> <repo> <commit> <cpv>` (docs/remote-merge.md §8).
- The previous struct doc also carried the Slice-2 `--remote-binpkg <file>` stage description (build the bundle server-side via `remote_bundle`, stream it to `$WORKDIR/<pf>/bundle.tar`, unpack and verify there; exit 0 = `UNPACK=ok`); that behaviour is owned and documented by `run_bundle_stage` in binpkg_flow.rs.

## fn record_server_ledger
- Pass the ledger *directory* (`<placed-PKGDIR>/remote-ledger`, or `--remote-ledger-dir` when set); the hostname file hangs off it.

## fn newest_ledger_line
- The plan's append-ordered `<unix-ts> <repo> <commit> <cpv>` line (docs/remote-merge.md §8). `None` for an absent/empty ledger: a client (or server) that has never recorded provenance.

## fn ledger_lines_match
- Strict mode (`--remote-require-ledger-match`, plan §8): both absent (fresh client plus no server record) or byte-identical counts as agreeing. Any asymmetry is drift: a client that lost its ledger, was reimaged or hand-edited, or was last merged by a different server.

## fn read_client_latest_ledger
- Plan §8's "provenance story".
