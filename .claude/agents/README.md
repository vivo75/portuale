# Subagents for the backlog-plan workflow

These map onto the slice shapes that recur in `docs/02.*.md` and the older
`docs/<tier>.<nnn>-*.opus.md` plans. The split is deliberate: the slices that
*decide* something run on a frontier model, the slices that *execute* something
already decided run on a cheap one, and the executing ones are safe to fan out.

| agent | model | slice shape it covers | parallel-safe |
|---|---|---|---|
| `real-portage-oracle` | opus | "Oracle before code": what does real 3.0.82.2 do, with `file:line` + a live probe | yes, one per question |
| `plan-reviewer` | opus | plan audits and post-phase reviews (the `D`/`G`/`P` doc) | yes, one per plan or range |
| `probe-capture` | sonnet | the `S0`/`D0` capture slices, host re-diffs — run it, transcribe it, no diagnosis | yes |
| `fixture-smith` | sonnet | the "the fixture (no product code)" slices | yes, if the fixtures don't share names |
| `bed-runner` | sonnet | L0/L1/L2/L3 + VM differential runs | yes across levels; **no** two runs of the same level at once |
| `verify-pass` | haiku | `cargo fmt/clippy/test` + the pmtest contract suite, name-diffed against a baseline | one at a time (it builds) |
| `closeout-scribe` | sonnet | AGENTS.md rules 7 and 11: backlog `DONE`, plan `Status:`, `what-this-proves.md`, findings, number allocation | yes, one per item |

## There is deliberately no "implementer" agent

The code slices stay in the main session. They are the part that needs the whole
plan, the review corrections and the operator in the loop — and AGENTS.md rule 9
makes every commit an explicit, separate request. None of these agents commits
anything; each reports the files it touched so the `portuale` and `pmtest`
commits can be split by hand.

## Typical batch

1. `plan-reviewer` on the plan, before any code. Correct the plan first — the
   2026-09-18 Tier 2 review found a false claim at the centre of its own §2.
2. `real-portage-oracle` in parallel for each open "what does real do" question
   the review surfaced.
3. Main session codes the slice.
4. `fixture-smith` and `probe-capture` in parallel for that slice's evidence.
5. `verify-pass` before the commit; `bed-runner` when the resolver, merge order,
   ebuild phases or merge code moved.
6. `closeout-scribe` on the last slice of the item.

## Invariants every one of them carries

- All artefacts in English, whatever language the request arrives in.
- Nothing commits, nothing pushes.
- No agent edits a test, fixture, oracle, threshold or allowlist to turn a red
  green (pmtest `F1`).
- No agent sets `PORTUALE_CORPUS_BLESS=1`; drift is reported, and blessing is
  the operator's call on a real capture.
- Probes build first and record HEAD + binary mtime (T8) and the full argv of
  both sides with identical effective options (T9).
- Shell scripts are never edited while a run of them is in flight.
