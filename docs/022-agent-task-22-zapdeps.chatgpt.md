# Backlog #22 — `dep_zapdeps` finer choice bins

## Objective

Implement the finer `dep_zapdeps` choice-bin semantics in Portuale, bringing the resolver closer to Gentoo Portage behavior.

The goal is **semantic compatibility with Portage**, not merely adding more enum variants.

The implementation must distinguish at least:

- availability independent of USE dependency satisfaction;
- USE dependency satisfaction;
- whether required USE changes are unmasked/allowed;
- graph membership;
- installed status;
- installed slot status;
- partial installation / any-slot installation;
- the resulting Portage preference bin.

Use the upstream Portage implementation as the semantic reference:

- `lib/portage/dep/dep_check.py`
- specifically the `dep_zapdeps` logic and the code immediately surrounding its choice classification and ordering.

## Important architectural rule

Do **not** implement this task as:

```text
current AltPreference
    ↓
add more enum variants
```

Instead, introduce an intermediate analysis layer:

```text
candidate alternatives
        ↓
candidate facts
        ↓
Portage choice-bin classification
        ↓
ordering within the selected bin
        ↓
selected alternative
```

The important conceptual distinction is:

```text
all_available
    !=
all_use_satisfied
    !=
all_use_unmasked
```

An alternative can have an available package while still failing its USE dependencies.

That distinction is fundamental to `dep_zapdeps`.

------

# Scope

## In scope

Implement the finer choice-bin classification corresponding to Portage's `dep_zapdeps`.

At minimum, the implementation must be able to distinguish the following semantic categories:

```text
preferred_in_graph
preferred_non_installed

unsat_use_in_graph
unsat_use_installed
unsat_use_non_installed

other_installed
other_installed_some
other_installed_any_slot
other
```

The exact Rust representation is an implementation decision.

The classification must be based on independently computed candidate facts rather than inferred from a single "satisfiable" boolean.

## Explicitly out of scope unless required by an existing contract

Do not silently expand this task into a complete resolver rewrite.

In particular, treat these as separate concerns unless existing tests demonstrate that they are inseparable from the bin classification:

- full backtracking;
- complete intra-bin ordering;
- unrelated resolver optimizations;
- unrelated masking changes;
- broad dependency-parser changes;
- unrelated slot-selection changes.

If correct bin classification exposes a missing prerequisite, document it and implement the smallest necessary prerequisite.

------

# Reference semantics

Portage conceptually distinguishes at least these properties.

## Availability

Determine whether an alternative has viable package candidates **without treating USE dependency failure as package unavailability**.

Conceptually:

```text
all_available
```

answers:

> "Can this atom/alternative be satisfied by visible candidates if USE dependency satisfaction is ignored?"

This must not be equivalent to the current Portuale notion of "currently satisfiable".

## USE satisfaction

Determine separately:

```text
all_use_satisfied
```

This answers:

> "Do the selected candidates satisfy their USE dependencies?"

An alternative may therefore be:

```text
available = true
use_satisfied = false
```

That is an important and valid state.

## USE masking / permission

Determine separately whether required USE changes are allowed:

```text
all_use_unmasked
```

Do not collapse:

```text
USE-unsatisfied
```

and:

```text
USE-unsatisfied + masked/unavailable USE change
```

into the same semantic state.

------

# Candidate facts

Create an internal representation appropriate for Portuale's architecture.

The following is conceptual, not a required Rust API:

```text
ChoiceFacts {
    all_available
    all_use_satisfied
    all_use_unmasked

    all_in_graph

    all_installed
    all_installed_slots
    some_installed
    installed_any_slot

    downgrade-related state
    new-slot-related state
    update-related state
}
```

Only include facts actually needed by the current Portage semantics.

Avoid prematurely exposing Portage-specific implementation details through generic resolver APIs.

Prefer keeping generic resolution machinery generic and putting Portage-specific classification logic in the appropriate Portage/repository layer.

------

# Recommended implementation decomposition

## Slice 22.1 — Semantic analysis/design

Before changing behavior:

1. Read the current Portuale implementation.
2. Read the upstream Portage `dep_zapdeps` implementation.
3. Trace how Portuale currently determines:
   - candidate availability;
   - USE satisfaction;
   - graph membership;
   - installed state;
   - slot state;
   - masking.
4. Document where each required fact can be obtained.
5. Identify any information currently unavailable to the resolver.

Do not make broad code changes during this slice.

### Deliverable

A short design note or implementation plan describing:

```text
Portage fact → Portuale source/API → required adaptation
```

------

# Slice 22.2 — Separate availability from USE satisfaction

Refactor candidate evaluation so that availability and USE satisfaction are independent.

The important invariant is:

```text
available candidate
+
unsatisfied USE dependency
```

must remain distinguishable from:

```text
no available candidate
```

Add tests specifically covering this distinction.

Examples should include:

- available + USE satisfied;
- available + USE unsatisfied;
- available + USE unsatisfied + unmasked;
- available + USE unsatisfied + masked;
- genuinely unavailable candidate.

Do not implement the nine bins yet if doing so would make this slice harder to verify.

------

# Slice 22.3 — Implement finer choice-bin classification

Using the candidate facts, implement the Portage-style classification.

The classification should distinguish the semantic groups corresponding to:

```text
preferred_in_graph
preferred_non_installed

unsat_use_in_graph
unsat_use_installed
unsat_use_non_installed

other_installed
other_installed_some
other_installed_any_slot
other
```

Do not assume that the exact ordering shown above is sufficient as an implementation algorithm.

Use the upstream Portage conditions as the authoritative specification.

The classification should be deterministic.

------

# Slice 22.4 — Tests for every bin

Create focused tests where each test isolates one reason an alternative belongs to a particular bin.

Tests should cover at least:

### Graph state

- candidate already in dependency graph;
- candidate not in graph.

### Installation state

- all relevant candidates installed;
- no candidates installed;
- some candidates installed;
- installed in required slot;
- installed in another slot.

### USE state

- all USE dependencies satisfied;
- USE dependencies unsatisfied;
- USE-unsatisfied but unmasked;
- USE-unsatisfied and masked.

### Availability state

- candidate available;
- candidate unavailable;
- candidate available but failing USE constraints.

### Combinations

Add tests for combinations such as:

```text
in graph + USE satisfied
installed + USE satisfied
installed + USE unsatisfied
not installed + USE unsatisfied
partially installed
installed in another slot
```

Do not rely exclusively on unit tests for individual helper functions.

At least some tests must exercise the resolver through the public/contract-level interface.

------

# Slice 22.5 — Differential validation against Portage

Where practical, construct equivalent dependency scenarios for:

```text
Portage
vs.
Portuale
```

and compare the selected alternative / preference behavior.

Focus especially on cases where:

```text
all_available == true
all_use_satisfied == false
```

because these are precisely the cases likely to be incorrectly collapsed by a simplistic implementation.

Include nested and multi-atom disjunctions where the current test infrastructure supports them.

------

# Intra-bin ordering

Be careful not to confuse:

```text
choice-bin classification
```

with:

```text
ordering of candidates inside a bin
```

Portage performs additional preference/reordering logic after classification.

If the existing Portuale implementation already has equivalent ordering semantics, preserve and integrate them.

If not, determine whether the missing behavior belongs to this backlog item.

Do **not** silently implement a large new ordering algorithm merely because the bin enum now exists.

If ordering remains incomplete, explicitly document:

```text
#22 implements classification.
Intra-bin ordering remains a separate compatibility gap.
```

unless the repository's acceptance criteria explicitly require both.

------

# Architectural guidance

## Prefer facts over booleans with overloaded meaning

Avoid APIs where one value simultaneously means:

```text
candidate exists
candidate satisfies USE
candidate is selectable
candidate is preferred
```

These are different concepts.

Prefer something structurally equivalent to:

```text
candidate discovery
        ↓
candidate facts
        ↓
classification
        ↓
preference
```

## Keep generic code generic

Do not unnecessarily encode Gentoo/Portage-specific semantics into a generic dependency-reduction abstraction.

If `portage-use-reduce` is intended to provide generic disjunctive reduction, keep it generic where possible.

Portage-specific knowledge should live in the Portage-facing layer.

## Avoid speculative refactoring

Do not rewrite surrounding resolver code simply because it could be cleaner.

Make the smallest architectural change that permits correct semantics and good tests.

------

# Acceptance criteria

The task is complete when all of the following are true:

- Available candidates can be distinguished from USE-unsatisfied candidates.
- USE satisfaction is computed independently of basic availability.
- USE masking/unmasking is represented independently where required by Portage semantics.
- Graph membership participates correctly in classification.
- Installed/partially-installed/other-slot states participate correctly in classification.
- The finer Portage choice bins are represented and selected correctly.
- Existing resolver behavior remains unchanged for cases outside the new semantics.
- Focused tests exist for every new semantic category.
- Contract/integration tests exercise the behavior through the resolver.
- Existing test suites pass.
- No unrelated resolver behavior is changed without justification.
- Any known difference from Portage is explicitly documented.

------

# What NOT to do

Do not:

- simply add enum variants without changing candidate analysis;
- equate `USE-unsatisfied` with `unavailable`;
- use the first available candidate as a substitute for Portage's preference classification;
- infer graph/installed state from unrelated heuristics when authoritative state is available;
- rewrite the resolver wholesale;
- implement full backtracking as part of this task;
- assume bin priority alone reproduces `dep_zapdeps`;
- remove existing tests because they conflict with the new implementation;
- weaken tests to make the implementation pass;
- copy Portage Python code mechanically without adapting it to Portuale's architecture.

------

# Agent workflow

Use this workflow for each implementation slice:

1. **Inspect**
   - Read the relevant Portuale code.
   - Read the corresponding Portage implementation.
   - Identify existing tests.
2. **Explain**
   - State the semantic rule being implemented.
   - Identify the Portuale data needed to implement it.
3. **Implement**
   - Make the smallest coherent change.
4. **Test**
   - Add focused tests.
   - Run the existing relevant test suite.
5. **Compare**
   - Where possible, compare behavior with Portage.
6. **Review**
   - Check that the implementation distinguishes availability, USE satisfaction, and masking.
7. **Stop**
   - Do not continue into unrelated resolver work.

------

# Review checklist

Before declaring the task complete, answer these questions explicitly:

### Semantics

- Can an available-but-USE-unsatisfied candidate be represented?
- Can a USE-unsatisfied candidate be distinguished from an unavailable candidate?
- Can USE masking prevent an otherwise available candidate from being classified as merely "unsatisfied"?
- Are graph membership and installation state independently available?

### Classification

- Can every required Portage bin be reached?
- Are the conditions for each bin derived from Portage rather than guessed?
- Are partially installed and other-slot cases distinguished?

### Compatibility

- Do existing simple dependency cases behave exactly as before?
- Do existing USE dependency cases still resolve correctly?
- Are disjunctions with multiple alternatives handled correctly?
- Are nested disjunctions unaffected?

### Scope

- Did the change remain limited to #22?
- If another missing resolver capability was discovered, is it documented separately?
- Is any remaining difference from Portage clearly stated?

------

# Difficulty / model guidance

This task is suitable for an LLM-assisted implementation.

Recommended division of labor:

| Activity                            | Recommended model               |
| ----------------------------------- | ------------------------------- |
| Understand upstream `dep_zapdeps`   | Frontier / strongest available  |
| Design candidate facts              | Frontier or strong coding model |
| Implement candidate-fact layer      | Strong mid-tier coding model    |
| Implement bin classification        | Mid-tier coding model           |
| Add focused tests                   | Cheap coding model              |
| Fix compilation/test failures       | Cheap coding model              |
| Analyze difficult semantic failures | Strong mid-tier / frontier      |
| Final Portage-vs-Portuale review    | Frontier / strongest available  |



A cheaper coding model should be able to implement the individual slices once the semantic design is established.

The highest-risk failure mode is **not bad Rust code**.

It is producing code that compiles and passes superficial tests while incorrectly collapsing:

```text
unavailable
```

with:

```text
available but USE-unsatisfied
```

or otherwise approximating Portage's nine bins without reproducing their underlying semantics.

Therefore, spend model budget on **semantic review and differential testing**, not on routine code generation.

------

# Definition of success

The implementation should make it possible to explain any selected alternative in terms of facts such as:

```text
available:          yes
USE satisfied:      no
USE unmasked:       yes
in graph:           no
installed:          yes
required slot:      no
some installed:     yes

→ Portage choice bin: <specific bin>
```

If an agent cannot provide this kind of explanation for a difficult test case, the implementation should be considered insufficiently understood.

The ultimate goal is not to make Portuale's code look like Portage's Python.

The goal is to make **Portuale make the same semantic choice for the relevant cases**.