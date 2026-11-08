---
name: portage-source-map
description: Use when you need to find where a behaviour lives in Portage's own Python source before changing portuale — locating a resolver decision, a merge or VDB rule, a config chain step, an output site, or an ebuild phase detail, and needing the exact file and line to read instead of grepping blindly.
---

# Navigating Portage's source

Portage is the teacher, not the oracle (`docs/charter.md`). When portuale needs
to learn how Portage solved a problem, the first question is *where Portage
implements it*, and the second is *whether the answer is in Python, in the bash
runtime, or in the C fast path*. What Portage does is a hint for what portuale
should do, to be judged against the PMS, the Devmanual and the charter's
properties.

## Where the checkout is

`3rdparty/portage`, pinned to tag `portage-3.0.83` by
`3rdparty/repos.toml`; `$PORTUALE_PORTAGE_CHECKOUT` overrides it at runtime.
The checkout is **not needed to run portuale** — every phase helper is native
since #326 S2–S7. It remains the comparison reference
(`PORTUALE_PYTHON_HELPERS=real`), the
source of the D4 fixture generators, and the source of the vendored
`bin/` + `cnf/sets/portage.conf`.

## Where the answer usually is

Derived from the citations already recorded in `LLM/code-notes/` (1.468
distinct `file:line` references across 174 files):

| You are looking for | Read |
|---|---|
| Dependency resolution, backtracking, slot conflicts, `_want_*`, masks, `SELECTED` | `lib/_emerge/depgraph.py` — 462 citations, by far the densest |
| What `emerge` actually does with the graph: actions, display, world updates, depclean | `lib/_emerge/actions.py` (151) |
| Installed-package database, `dblink`, unmerge, preserved libs, collision protect | `lib/portage/dbapi/vartree.py` (147) |
| The configuration chain: `make.conf`, profiles, `make.globals`, masks, `package.env` | `lib/portage/package/ebuild/config.py` (115) |
| Binary packages, `gpkg`, binhost index | `lib/portage/dbapi/bintree.py` (110), `lib/portage/gpkg.py` (32) |
| Phase execution, `doebuild`, the environment handed to a phase | `lib/portage/package/ebuild/doebuild.py` (73) |
| Argument parsing and the two-pass root discovery | `lib/_emerge/main.py` (72) |
| Fetch, `SRC_URI`, mirrors, digests | `lib/portage/package/ebuild/fetch.py` (70) |
| Output text, colours, `[ebuild N]` rows, `--tree` | `lib/_emerge/output.py` (70), `lib/portage/elog` |
| Scheduling, jobs, pid-sandbox, process groups | `lib/_emerge/Scheduler.py` (56) |
| Which flags a run means (`--update`, `--deep`, bdeps) | `lib/_emerge/create_depgraph_params.py` (40) |
| Atom parsing grammar | `lib/portage/dep/__init__.py`, `dep_check.py` (33) |

The most-cited landmarks, worth reading before touching the area:

| Citation | What it answers |
|---|---|
| `depgraph.py:5577-5605` | `--update` intent and `_want_update_pkg` |
| `depgraph.py:10365-10383` | the final merge-list construction |
| `depgraph.py:4186-4193`, `:4253-4289` | slot-conflict handling |
| `dep_check.py:1052` | the dependency check entry used by the resolver |
| `create_depgraph_params.py:169-175` | how `--deep`/`--update`/bdeps become parameters |
| `post_emerge.py:112-117`, `:155` | what prints after the merge, and when |
| `config.py:2691-2699` | config reload, and why an error can repeat N times |
| `vartree.py:4409-4418` | preserved-libs decision |
| `doebuild.py:697-750` | the phase environment |
| `Scheduler.py:1595-1602` | process-group and signal handling |
| `actions.py:460-462` | world-file rewrite |

## How to find the rest

The full citation index lives in `LLM/code-notes/`: every notes file carries a
`// Portage: <file>, <function>` line for the symbol it documents. So:

```
grep -rn 'Portage:' LLM/code-notes | grep -i <concept>
python3 LLM/tools/code_notes_index.py            # regenerate the index
```

That is cheaper and more precise than grepping the checkout, because a human
already established the correspondence.

## Three traps that produce wrong conclusions

- **The C fast path.** Installed-package and some atom validation go through a
  C parser that disagrees with the Python one — it rejects `::repo` and
  build-id atoms the Python path accepts, and produces different error text.
  On this host the C path is taken. Verify which path ran before citing
  "Portage says".
- **EAPI is applied after the grammar.** `_get_atom_re` is a module global and
  ignores `eapi_attrs`; only `slot_operator`, `slot_deps`, `use_deps`,
  `use_dep_defaults`, `strong_blocks` and `repo_deps` are consulted by atom
  parsing. An atom can be grammatical and still illegal for the EAPI.
- **A source reading is not evidence of behaviour.** Three items in one batch
  misread `depgraph.py` and the "gap" did not exist. Confirm with a probe.

## Python, bash, or Rust

Portuale vendors the whole bash phase runtime into `./bin/` and answers every
`portage`-importing `.py` helper natively (`crates/portuale/src/helpers/`). So a
behaviour can live in three places, and the fix belongs wherever the *contract*
is, not wherever Portage happened to put it:

- **bash** — `bin/ebuild.sh`, `bin/phase-helpers.sh`, vendored from upstream;
- **Rust** — `crates/`, for everything portuale answers natively;
- **Python** — only the comparison reference and the fixture generators.

## Known bugs not to copy

`LLM/portage-quirks.md` records confirmed Portage defects (Q-numbers) with the
decision to not reproduce them. Check it before implementing a behaviour that
looks like a bug — it probably is one.

## Where to look

| Question | Source |
|---|---|
| Symbol → Portage file:line | `LLM/code-notes/` (483 files) |
| Confirmed upstream defects | `LLM/portage-quirks.md` |
| The pin, the overlay and the runtime-independence decision | `3rdparty/repos.toml`, `LLM/agent-context.md` |
| Which Portage version is the reference | tag `portage-3.0.83` (`3rdparty/repos.toml`) |

## History

The plans, reviews and captures this rule came from are archived: see
[`LLM/ARCHIVE.md`](../../ARCHIVE.md); `git log` and the backlog entries in
`LLM/backlog-tasks.md` hold the per-item story.
