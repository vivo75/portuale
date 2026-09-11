Question: talking of opensource project  https://github.com/vivo75/portuale/  what's your opinion on backlog "19 DFS-partial merge-list truncation (parked branch)".
How should an LLM split the task, how difficult is it? It's a job for a frontier LLM or can be executed by some less expensive one can do ?

Answer:

Yes. I looked fairly closely at the repo, including the parked `explore/dfs-graph-backtracker` branch and the commits that led to backlog item #19. My take is:

 > **#19 is deceptively hard if interpreted as “implement Portage's real `get_best_run()` semantics”, but quite manageable if interpreted as the repo's newer recommended “targeted truncation post-pass”.**

 The distinction is important.

 ## What #19 actually is

 The repo's investigation found that the spectacular merge-list discrepancy for `plasma-meta`/`podman` isn't a generic “Portuale's BFS vs Portage's DFS” problem.

 The exact trigger is:

 1. autounmask changes are required;
2. **and** there is an unresolvable circular dependency;
3. Portage's backtracking exhausts its attempts;
4. `get_best_run()` selects the deepest partial DFS graph;
5. the resulting merge list is therefore deliberately incomplete.

 For normal autounmask cases such as Firefox, Gedit, Gnome Shell, etc., Portage and Portuale actually have the same total package count. The truncation only appears in the autounmask + unresolvable-cycle combination.  GitHub

 That's a very useful discovery because it means **#19 should not be attacked as “rewrite the resolver to DFS.”**

 The parked branch tried exactly that experimentally. It produced 38 contract failures while changing essentially only representation/order, and gave essentially no improvement in merge-order parity. The branch's own conclusion is to abandon that approach and implement a targeted truncation post-pass instead.  GitHub

 That is, in my opinion, the correct direction.

 ## How I would split the work

 I would turn #19 into roughly **5 slices**, rather than give an LLM one giant instruction.

 ### Slice 1 — Reproduce the semantic boundary

 Build/pin a minimal fixture that demonstrates:

```
autounmask required
        +
unresolvable circular dependency
        ↓
partial merge list
```

 Use the existing `plasma-meta` / `podman` observations as the oracle, but create a small synthetic fixture if possible.

 The important thing is to establish:

 - full graph → full list;
- autounmask alone → full list;
- cycle alone → normal cycle handling;
- autounmask + fatal cycle → partial list;
- exact stopping point.

 This is relatively easy.

 **Difficulty: 2/5**

 A cheaper coding model should be able to do this.

---

 ### Slice 2 — Identify the existing DFS order

 This is where the repo's investigation is particularly valuable.

 `merge_order.rs` already independently reconstructs Portage's `_create_graph` DFS traversal for merge ordering. The investigation explicitly concluded that changing the _resolver's_ walk to DFS doesn't buy you anything here.  GitHub

 So the LLM should answer:

 > Given the existing resolved graph and the existing merge-order DFS, can we determine the prefix/subtree that Portage's `get_best_run()` would retain?

 This should be a **read-only analysis slice**, not implementation.

 The agent should produce something like:

```
root
 ├── A
 │    ├── B
 │    └── C
 ├── D
 │    └── E
 └── F  <-- first autounmask restart
```

 and identify:

```
retained = {root, A, B, C}
discarded = {D, E, F, ...}
```

 The branch investigation gives a strong clue: for `plasma-meta`, the retained set is essentially the reverse-reachability closure around `dev-lang/go`; for `podman`, it additionally contains `conmon` because of DFS visitation order.  GitHub

 **Difficulty: 3/5**

 This is where a decent reasoning model becomes useful.

---

 ### Slice 3 — Model “best partial run”

 This is the genuinely interesting part.

 Don't initially implement all of Portage's backtracking machinery.

 Define an internal concept along the lines of:

```
ResolutionResult {
    complete: bool,
    entries: ...,
    failure_boundary: ...,
    dfs_order: ...
}
```

 and answer one question:

 > What information does the resolver currently throw away that is necessary to reconstruct the best failed attempt?

 The repo explicitly says that a _genuine_ `get_best_run` would require a real digraph object plus abort semantics and would be a substantially larger effort.  GitHub

 This is where I would **not let a cheap model freestyle**.

 Have the LLM first write a design note and tests, before touching the algorithm.

 **Difficulty: 4/5**

---

 ### Slice 4 — Implement the targeted truncation

 If Slice 3 proves that the needed information can be recovered from the existing graph/merge-order machinery, implement:

```
normal resolution
        ↓
detect:
    autounmask restart +
    fatal circular dependency
        ↓
obtain existing DFS merge ordering
        ↓
find first restart/failure boundary
        ↓
retain DFS-partial set
        ↓
serialize only retained nodes
```

 Crucially, **don't modify the normal resolution walk**.

 The parked branch's conclusion is very explicit that the recommended implementation is a bounded post-pass over the existing DFS order, rather than a resolver-wide DFS rewrite.  GitHub

 **Difficulty: 3.5–4/5**

---

 ### Slice 5 — Differential verification

 This one is important because a superficially correct implementation could be _worse_ than doing nothing.

 The previous investigation specifically warns that an approximate BFS-prefix truncation is undesirable: it can make Portuale's total count appear closer to Portage's while producing a huge number of individual missing/extra findings.  GitHub

 So I'd require tests for:

 - `plasma-meta`
- `podman`
- Firefox
- Gedit
- Gnome Shell
- at least one ordinary circular dependency without autounmask
- at least one autounmask case without a circular dependency
- `--backtrack=0`
- normal successful resolution

 And the important invariant:

```
if this is NOT the specific fatal condition,
DO NOT TRUNCATE.
```

 **Difficulty: 3/5**

---

 # So: frontier LLM or cheaper model?

 My answer is **hybrid**.

 | Work | Model I'd use |
| --- | --- |
| Reproduce probes | Cheap/medium |
| Add fixture | Cheap/medium |
| Explore existing code | Medium |
| Design the truncation algorithm | **Frontier** |
| Implement after design is settled | Medium/cheap |
| Write regression tests | Cheap/medium |
| Review semantic correctness | **Frontier** |
| Full final differential validation | Human + strong model |

 I would **not** spend a frontier model on all five stages.

 Instead:

```
             FRONTIER
                │
       ┌────────▼────────┐
       │ Understand #19  │
       │ + design exact  │
       │ algorithm       │
       └────────┬────────┘
                │
        ┌───────▼────────┐
        │ Medium/cheap   │
        │ implementation │
        └───────┬────────┘
                │
        ┌───────▼────────┐
        │ Cheap testing  │
        │ + fixture work │
        └───────┬────────┘
                │
             FRONTIER
                │
        semantic review
```

 ## Why I wouldn't give the whole thing to a cheap model

 The dangerous part isn't Rust.

 It's **understanding what “partial” means**.

 A mediocre coding agent will very plausibly see:

 > “Portage has 4 packages, Portuale has 484. Find a DFS prefix that produces 4.”

 and produce something that makes the fixture pass.

 That would be the wrong solution.

 The investigation shows that the partial set is not simply “first N nodes”. It is determined by the failed DFS/backtracking trajectory and the point where the autounmask restart becomes relevant. `plasma-meta` and `podman` even differ in the precise retained set.  GitHub

 That's exactly the sort of subtle semantic invariant where a frontier model is worth the money.

 ## But I would _not_ call #19 a frontier-only task

 This is the important distinction.

 The repo has already done a surprisingly large amount of the difficult intellectual work:

```
if this is NOT the specific fatal condition,
DO NOT TRUNCATE.
```

 - the trigger has been isolated;
- 12 autounmask probes were compared;
- the misleading DFS-walk hypothesis was tested;
- the DFS experiment was shown to be largely a dead end;
- the existing `merge_order.rs` DFS machinery was identified as reusable;
- the recommended architecture is now a bounded post-pass.  GitHub+1

 So you're **not asking an LLM to discover the problem from scratch anymore**.

 That moves it from something like:

 > **9/10 research/architecture task**

 to approximately:

 > **6.5–7/10 implementation task**, assuming the agent receives the existing investigation and is forced to validate against real Portage.

 That's quite different.

 ## My recommendation for this repository

 I'd actually formulate the task to an agent as:

 > **Do not implement a DFS resolver. Do not change the resolver traversal order.**
>
>  First reproduce the exact `plasma-meta`/`podman` truncation boundary from the existing fixtures/findings.
>
>  Then inspect the existing `merge_order.rs` DFS traversal and determine whether it contains enough information to reproduce Portage's `get_best_run()` partial set.
>
>  If yes, implement the smallest post-resolution truncation pass possible.
>
>  If no, stop and report exactly which state/invariant is missing rather than inventing an approximation.

 That last sentence is important. **I'd give the agent permission to fail the slice.**

 The repo's `AGENTS.md` already has essentially the right discipline: ground work in actual Portage code, implement both Rust and Python sides where applicable, create isolating fixtures, and run the full verification suite rather than accepting a local passing test.  GitHub

 ### My rating

 **#19 as currently understood:**

 > **9/10 research/architecture task**

  - Algorithmic difficulty: **7/10**
- Rust implementation difficulty: **5/10**
- Codebase navigation: **7/10**
- Testing difficulty: **6/10**
- Risk of a plausible-but-wrong implementation: **high**
- Need for frontier reasoning: **yes, for the design/review**
- Need for frontier reasoning for every line of code: **no**
- Good candidate for agentic implementation: **absolutely**

 And I would **not merge the existing `explore/dfs-graph-backtracker` branch as the solution**. Its most valuable output is actually its negative result: the DFS resolution-walk experiment demonstrated that this is the wrong layer to change.  GitHub

 The current `main` backlog itself also reflects this architectural distinction: the remaining resolver gap is described as an abort-path problem adjacent to #19, while the merge-order implementation already contains substantial DFS/order machinery.  GitHub+1

 If you want, I can go one level deeper and **design the exact agent task for #19**—i.e. a sequence of 5–8 prompts/agent stages, what context each stage should receive, what it should be allowed to modify, and which stages can safely use a cheap model versus a frontier model.

