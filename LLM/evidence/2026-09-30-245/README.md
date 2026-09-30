# #245 — S0 diagnosis (batch-2026-09-28 Track O), stopped at owner B7

`u245_pg.py` → `u245.txt`: real's own `ResolverPlayground` on the g216
fixture shape (`app-misc/g216top` → `dev-libs/g216mid` →
`dev-lang/g216comp`, whose BDEPEND `|| ( >=dev-lang/g216comp-1.0
dev-lang/g216boot )` closes a self-cycle), `--pretend --backtrack=0`,
with real's `digraph.order` and each node's children printed.

Real's insertion order is `g216top`, (the `app-misc/g216top` argument
node), `g216mid`, `g216comp`. `circular_dependency_handler.
_prepare_reduced_merge_list` drains leaves and, when none is left (every
node here has a remaining child — `g216comp` through its self-edge), takes
`tempgraph.order[0]`: `g216top`, then `g216mid`, then `g216comp` — the
three-row tree real prints. Portuale's `reduced_merge_order` ports the
same rule, but its scheduling graph's order puts `g216comp` first, so the
forced tree prints `g216comp` with `g216top`/`g216mid` as `[nomerge]`
ancestors and then the two again.

The difference is `_create_graph`'s insertion order — the #17 family
(deliberate cut). Owner decision (2026-09-30, B7): leave #245 open,
aligned with #17's cut; no code.
