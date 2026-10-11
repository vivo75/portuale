# crates/portage-repo/src/merge_order/tests/digraph.rs
`SerializeFrontier` parity with the direct digraph scans.

## fn frontier_matches_direct_scans_on_random_graphs
- 50 deterministic graphs, 1..12 nodes, random edges (usually acyclic, sometimes not -- cycles only make leaf sets smaller, never less comparable) with random 1-2-priority lists, drained in random order with retain, comparing after every mutation.

## mod digraph scheduler-loop section
- Survivor of the backlog #144 scheduler-loop work.
