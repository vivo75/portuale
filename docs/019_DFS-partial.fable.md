Let me open the scope backlog to find item 19.I read the current `scope-backlog.md` and `agent-context.md`. Note that item #19's original numbered text got compacted into a `docs/history/` snapshot I couldn't open, so my reading comes from the three places the live backlog cross-references it: the masked-dependency *abort* half ("same gated-abort-path gap as #19 — parked there"), the circular-dep "partial flat list + cumulative counters (needs #19's abandon path)", and Part 4's "DFS-partial merge-list truncation". If the parked branch says something different, weigh that over me.

**What the item actually is**

Real portage's depgraph walk is a recursive DFS. When it hits something unfixable (masked-only dep, an unserializable cycle in `_serialize_tasks`), it abandons the resolve: `display()` prints the merge list *as visited so far*, the `Total: N packages (...)` counters are cumulative over that partial list, then the error block, exit 1. Portuale's walk is a single-pass BFS with a "report, don't enforce" convention: full merge list, notice appended, exit 0. So #19 is not really "truncate the list" — it's "build an abandon path", and three backlog entries are waiting on the same mechanism. That's why it got parked: it touches architecture (BFS vs DFS visitation order) and a *policy* decision (the exit-0 convention), not just code.

**My opinion**

Worth doing, but as one mechanism with three consumers, and with the policy question answered by the owner first, not by the model. The trap is trying to byte-match real's truncation point on the first pass — the truncation point is a function of DFS visit order, which portuale doesn't have. Get abort semantics, counters and exit codes right against fixtures first; chase visit-order parity as a separate slice with the real-tree L0 bed as the oracle.

**How I'd split it (each slice = Rust + Python reference + contract tests, per AGENTS.md)**

1. Oracle first. Fixtures that trigger each abort shape (masked-only dep, unserializable cycle, a mid-walk unsatisfiable atom), captured output from real 3.0.82.2 in the `TEST/` container, pinned as contract tests marked xfail. Read `depgraph.py`'s `display_problems`/`altlist`/the `_serialize_tasks` failure branch and write down, in prose, exactly which nodes appear in the partial list and how counters are computed. Mid-tier model is fine; the deliverable is a spec, not code.
2. Data model. Add an aborted outcome to `GraphResult` carrying `partial_merge_list` + reason, plumbed through both sides, behaviour-neutral, exit code behind a gate. Mid-tier.
3. Membership/order of the partial list. This is the hard one: either make the BFS walk expose a DFS-equivalent "visited before failure" prefix, or add a DFS visit-order side channel. Frontier model, with the fixtures from step 1 and L0 as the check.
4. Rendering. Partial flat list + cumulative counters through the already-ported `resolver/output.py` layer. Cheap model.
5. Wire the three consumers (masked-dep abort, circular partial list, #19 proper), flip exit codes, update `scope-backlog.md`/`what-this-proves.md`. Mid-tier.
6. L0 run, triage the divergences. Frontier — the failure mode here is a model declaring parity after fixtures pass while the real tree still diverges.

**Difficulty**

Medium-high: probably 3–4 slices. The code volume is modest; the cost is parity with a traversal-order-dependent artifact, the dual-language lockstep, and that whatever you build must not disturb the merge-order work that just shipped (`merge_order.rs`, the frontier layer).

**Frontier or cheaper?**

Split it. Steps 1, 3 and 6 need a frontier model — reading ~1–2k lines of upstream Python, reasoning about traversal order, and honest triage. Steps 2, 4, 5 are well-specified plumbing a Sonnet-class model handles fine *once step 1 exists*. What I wouldn't do is hand the whole item to a cheaper model end-to-end: it will match the fixtures and miss the real-tree order, which is precisely what parked the branch the first time.

