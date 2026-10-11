# crates/portage-repo/src/resolve/pass/graph_resolved.rs
Pick the resolved candidate and graph it, reporting slot collisions.

## fn graph_resolved_entry
- Portage `matched_packages[-1]` with ebuild built first (`depgraph.py:8492`).

- Backlog #57, direction 2: Portage's tracker reports the collision either way round (S0 cells a/b are the same block); portuale's slot check only looked at `resolved_slots` (merge outcomes only). Portage merges `paired-2.0` while reporting the block.

