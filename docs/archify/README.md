# Archify diagrams

Ten standalone HTML diagrams of portuale, one per Archify type, generated at
commit `36b417db`. Candidate JSON is in `src/`; each `*.delivery.json` is the
delivery receipt of its HTML.

| Type | File | Shows |
|---|---|---|
| Architecture | [architecture.html](architecture.html) | Crate dependencies |
| Workflow | [workflow.html](workflow.html) | `emerge -pv <atom>` from argv to merge list |
| Sequence | [sequence.html](sequence.html) | Source merge of one package |
| Dataflow | [dataflow.html](dataflow.html) | Inputs behind a merge list |
| Lifecycle | [lifecycle.html](lifecycle.html) | One package through a source merge |
| Entity relationship | [erd.html](erd.html) | SQLite VDB schema |
| Class interface | [class.html](class.html) | `mrg-director` slots and implementations |
| Tree | [tree.html](tree.html) | Repository layout |
| Timeline | [timeline.html](timeline.html) | History from git commits |
| Waterfall | [waterfall.html](waterfall.html) | Stage timing of `emerge -puD @world` (illustrative) |

## State-machine diagrams

Six lifecycle diagrams of machines the code implements without naming them,
generated at commit `2055f4dc`; findings in [../state-machines.md](../state-machines.md).

| File | Shows |
|---|---|
| [state-phases.html](state-phases.html) | One ebuild phase: skip gates, run, stamp |
| [state-backtracking.html](state-backtracking.html) | Backtracking driver loop |
| [state-resolver-pass.html](state-resolver-pass.html) | One resolver pass over the queue |
| [state-resume.html](state-resume.html) | Merge-list entry and `--resume` |
| [state-process-cancel.html](state-process-cancel.html) | One phase child and the two kill paths |
| [state-merge-path.html](state-merge-path.html) | Merge of one package to a published VDB entry |
