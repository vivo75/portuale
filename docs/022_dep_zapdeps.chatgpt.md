Question:

talking of opensource project  https://github.com/vivo75/portuale/  what's your opinion on backlog "22 dep_zapdeps finer choice bins".
How should an LLM split the task, how difficult is it? It's a job for a frontier LLM or can be executed by some less expensive one can do ?

Answer:
I looked at the current `portuale` tree and, more importantly, compared the remaining work against the actual upstream `dep_zapdeps` implementation. My take is:

> **This is not a frontier-only coding task. It is a medium/high-difficulty \*semantic porting\* task. A strong but cheaper coding LLM can probably implement most of it, provided the task is split into small slices. I would use a frontier model for the initial decomposition and final review, not necessarily for all the code.**

The reason is that the hard part isn't writing Rust. It's not accidentally changing Portage's preference semantics.

## What #22 actually means

The backlog currently says the remaining resolver-depth work includes the finer `dep_zapdeps` bins — `unsat_use_*` / `other_*` — and full `all_use_satisfied`.  GitHub+1 

That's a deceptively small description.

The current portuale implementation has essentially reduced Portage's choice system to:

```
Unsatisfiable
Available
Installed
```

The `portage-use-reduce` code explicitly describes this as a cut-down version of the real nine choice bins.  GitHub 

Real Portage has:

```
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

and it doesn't simply calculate one boolean "does this alternative work?". It independently calculates things like:

- is every atom available ignoring USE deps?
- are all USE deps satisfied?
- are the required USE changes unmasked?
- is the alternative already in the dependency graph?
- is it installed?
- is it installed in the exact required slot?
- is some package from it installed?
- would it introduce a downgrade?
- how many new slots would it introduce?
- does the parent want an update?

The upstream implementation does all of this before putting an alternative into one of those bins.  GitHub 

And then there is **another layer**: choices inside each bin are reordered based on upgrades, graph membership, slot count, etc.  GitHub 

So I would **not** interpret #22 as:

> "Add six more enum variants."

That would be too shallow.

------

# How I'd split it

I would make it roughly **5 implementation slices**, each independently testable.

### 1. Extract the "choice facts"

First introduce an internal representation something like:

```
ChoiceFacts {
    all_available
    all_use_satisfied
    all_use_unmasked

    all_in_graph
    all_installed
    all_installed_slots
    some_installed
    installed_any_slot

    conflict_downgrade
    installed_downgrade

    new_slot_count
    want_update

    atoms
    cp_map
    slot_map
}
```

Not necessarily those exact Rust names.

The important architectural move is:

**don't immediately encode Portage's nine bins into `AltPreference`.**

Instead:

```
alternative
    ↓
candidate analysis
    ↓
ChoiceFacts
    ↓
choice_bin()
    ↓
ordering within bin
    ↓
selected alternative
```

That is much easier to debug.

Right now `use_reduce_flat_disjunctive()` asks the caller for a single `AltPreference`, and the current implementation is deliberately simplified around that model.  GitHub 

I'd preserve the generic nature of `portage-use-reduce` as much as possible and put Portage-specific candidate analysis in `portage-repo`.

**Difficulty: 5/10.**

A cheaper coding model should handle this.

------

### 2. Separate "available" from "USE-satisfied"

This is the most important semantic correction.

Currently `atom_currently_satisfiable()` effectively says:

```
visible candidate
AND
matches atom
AND
USE deps satisfied
```

and therefore returns false when the only problem is a USE dependency.  GitHub 

But real `dep_zapdeps` deliberately does something different:

```
all_available
    = can I find a candidate ignoring USE deps?

all_use_satisfied
    = do those candidates satisfy the USE deps?

all_use_unmasked
    = if I need USE changes, are they allowed?
```

That's visible directly in the upstream implementation.  GitHub 

This distinction is crucial because:

```
A = available + USE satisfied
B = available + USE unsatisfied but unmasked
C = available + USE unsatisfied and masked
D = unavailable
```

are **not equivalent** in Portage's preference system.

This is probably the most intellectually difficult part of #22.

**Difficulty: 7/10.**

Still not frontier-only, but I would want a strong coding model here.

------

### 3. Implement the nine bins

Once the facts exist, the actual classification becomes fairly mechanical.

Conceptually:

```
if all_use_satisfied:
    if all_in_graph:
        preferred_in_graph
    elif all_installed:
        if all_installed_slots:
            preferred_installed
        else:
            preferred_any_slot
    else:
        preferred_non_installed

else:
    if !all_use_unmasked:
        other
    elif all_in_graph:
        unsat_use_in_graph
    elif all_installed_slots:
        unsat_use_installed
    else:
        unsat_use_non_installed

else:
    if all_installed:
        other_installed
    elif some_installed:
        other_installed_some
    elif installed_any_slot:
        other_installed_any_slot
    else:
        other
```

Obviously the actual control flow needs to follow Portage exactly; that's just the conceptual shape. The real code has additional downgrade/circular checks around this classification.  GitHub 

This part is **quite suitable for a cheaper LLM** once slice #2 has established the facts.

**Difficulty: 5–6/10.**

------

### 4. Don't forget within-bin ordering

This is the trap I'd explicitly put in the task description.

Real Portage does **not** simply say:

> choose the first element of the highest bin.

It does:

```
classify → bin
             ↓
       reorder within bin
             ↓
       choose first
```

The ordering considers things such as:

- fewer new slots when `minimize_slots` applies
- all-installed-slots
- upgrade vs downgrade
- whether everything is already in the graph
- whether the choice would eliminate an upgrade
- etc.

The upstream implementation has a substantial block devoted to this.  GitHub 

So I would make a conscious decision:

**Option A — #22 means only finer classification:**
 Implement bins and leave intra-bin ordering as a subsequent item.

**Option B — #22 means true `dep_zapdeps` parity:**
 Include the within-bin ordering in #22.

I strongly prefer **Option A** for an LLM-driven backlog. Otherwise the task becomes too fuzzy.

------

### 5. Differential fixtures, not just unit tests

This is where I would spend most of the verification budget.

For every bin, create a tiny synthetic `||` fixture where the only difference between alternatives is the property being tested.

For example:

```
|| (
    candidate-A   # installed
    candidate-B   # merely in graph
    candidate-C   # new
)
```

Then progressively construct cases for:

```
installed vs in-graph
same slot vs different slot
all installed vs partially installed
USE satisfied vs USE-unsatisfied
USE-unsatisfied + unmasked
USE-unsatisfied + masked
downgrade vs non-downgrade
virtual vs ordinary package
multi-atom alternative
nested || groups
backtracking after first choice fails
```

The project's existing test philosophy is particularly well suited to this: the Rust and Python implementations are exercised through shared black-box contract tests, and the project already uses real-tree probes for resolver behavior.  GitHub+1 

This is another place where a cheap model can do a lot of work.

------

# My recommended LLM allocation

I'd use a **tiered approach**.

| Work                                                  | Model                           |
| ----------------------------------------------------- | ------------------------------- |
| Read repo + upstream Portage and design decomposition | **Frontier**                    |
| Implement ChoiceFacts                                 | Mid/cheap strong coding model   |
| Implement USE-satisfaction split                      | **Frontier or strong mid-tier** |
| Implement nine-bin classification                     | Mid/cheap                       |
| Add fixtures/tests                                    | Cheap/mid                       |
| Fix compile/test failures                             | Cheap/mid                       |
| Analyze differential failures                         | Strong mid/frontier             |
| Final semantic review against upstream                | **Frontier**                    |

In other words:

**Frontier model ≠ write all the code.**

Use the expensive model where reasoning density is high.

------

# How hard is it really?

I'd score it like this:

### If you mean "implement nine enum values"

**3–4/10.**

Easy enough for a cheap coding model, but largely meaningless without the underlying semantics.

### If you mean "port the nine-bin classification correctly"

**7/10.**

This is a serious resolver task, but very tractable because Portage itself is the executable specification.

### If you mean "make portuale behaviorally match real `dep_zapdeps` including bin classification + intra-bin ordering + USE behavior + corner cases"

**8/10.**

That's where I would want a frontier model involved.

### If you also include the remaining full backtracking semantics

**9/10+**, and I'd make that a separate project.

The portuale documentation itself correctly treats the finer bins as only one part of the remaining resolver depth; full backtracking and other resolver details remain separate.  GitHub 

------

# The nice thing about this particular task

There is a huge advantage here:

**the algorithm already exists.**

You're not asking the LLM to invent dependency resolution.

You're asking it to translate:

```
Gentoo Portage Python
        ↓
semantic understanding
        ↓
existing portuale Rust architecture
        ↓
differential tests
```

That's an excellent LLM task.

And the current code has already done the hard architectural groundwork: it has `AltPreference`, disjunctive resolution, installed preference, graph preference, runtime masking for backtracking, and a large contract-test apparatus.  GitHub+1 

So I would **not** give this to an expensive frontier model as one giant:

> "Implement backlog #22."

I'd give the agent something closer to:

```
Slice 22.1
Understand upstream dep_zapdeps and write ChoiceFacts design.
Do not modify behavior.

Slice 22.2
Implement independent all_available / all_use_satisfied /
all_use_unmasked analysis.
Add differential fixtures.

Slice 22.3
Implement the nine choice bins.
Do not implement intra-bin sorting yet.

Slice 22.4
Add real Portage differential tests for each bin.

Slice 22.5
Review whether current intra-bin ordering is a separate gap.
If yes, document it; do not silently implement it.
```

That decomposition makes it **very feasible for a less expensive model**.

## My overall verdict

**I would absolutely let a non-frontier coding LLM work on #22.**

But I would *not* let a cheap model decide what #22 means.

The optimal setup is:

> **Frontier model designs the semantic boundary → cheaper model implements/test-drives individual slices → frontier model performs the final Portage-vs-Rust semantic review.**

For this repository in particular, that's likely to give you much better cost/performance than running a frontier model continuously.

And there's one especially important warning I'd put in the backlog itself:

> **"Finer choice bins" must not be implemented as a richer enum alone. First separate `all_available`, `all_use_satisfied`, and `all_use_unmasked`; otherwise the nine bins will be structurally present but semantically wrong.**

That's the part I think is most likely to fool a weaker coding agent.  GitHub+1 

 portuale repository 