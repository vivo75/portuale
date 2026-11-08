---
name: portuale-docs-and-charter
description: Use before writing or editing any document in portuale — a plan, a status header, a backlog entry, a proof paragraph, a code note, a README — and when a document check fails. Covers which text is authoritative, which invariants the tooling enforces, and which checker to run.
---

# Documents: what is authoritative, what is checked

The charter is the source of expected behaviour; the documentation rules are
mechanical and enforced by scripts. This skill is the map between them, so you
do not have to guess which file wins.

## Authority, in order

1. **`docs/charter.md`** — what portuale is, its relation to Portage, "what
   counts as correct", and "how the code must look". It wins over any older
   passage in `LLM/` that says "drop-in", "mimic Portage" or "Portage is the
   oracle". (Older text still says so; it is history, not instruction.)
2. **The PMS, the EAPIs and the Devmanual** — expected behaviour for package
   management itself.
3. **`AGENTS.md`** — the operating rhythm and the hard process rules.
4. **`LLM/agent-context.md`** — settled goals, constraints, architecture
   decisions, and the pointers to live state.
5. Everything else in `LLM/` — history and per-slice record. Dated "memory
   notes" and slice records are **not rewritten**; a correction is appended.

`docs/` is for humans; `LLM/` is written for agents doing backlog work. Do not
move content between them without a reason.

## The invariants the tooling enforces

| Invariant | Checker | What fails |
|---|---|---|
| Every relative markdown link resolves, or is listed in `LLM/ARCHIVE.manifest.tsv` | `python3 LLM/tools/check_links.py LLM docs` | a renamed file leaves a dangling link; an archived one is accepted if listed |
| Every path a skill cites exists or is archived | `PORTUALE_DISTILL_WORK=<work dir> python3 LLM/tools/portuale-distill/f4_check.py` | a skill cites a vanished path |
| A plan's `Status:` agrees with its backlog entry, and its tier prefix matches the tier it sits in | `python3 LLM/tools/doc_status_check.py` | `done` plan with an open entry, duplicated number, unknown status, wrong tier |
| Backlog numbers are unique and never reused | same | the same number defined twice, within or across files |
| Every scope cut is acknowledged | `python3 LLM/tools/cut_registry_check.py` | an open `CUT-n` whose follow-up is DONE; an unacknowledged `Still deferred` marker in the proof file |
| `LLM/code-notes/INDEX.md` matches the notes tree | `python3 LLM/tools/code_notes_index.py` | a notes file not indexed |
| A shortened comment lost no fact | `python3 LLM/tools/fact_coverage.py BASE FILE...` | a backticked span, `file:line`, `#NNN`, quoted string, date or version that disappeared |

Run `LLM/tools/gate.sh` for the whole verification pass; the four document
checkers above are the part that fails on a docs-only change.

## The rules that are easy to break

- **`LLM/what-this-proves.md` is append-only.** Never rewrite a prior slice's
  paragraph; a stale claim is corrected by a later entry. Its text up to
  2026-11-07 is archived (`LLM/ARCHIVE.md`); a citation
  `what-this-proves.md:L` written before 2026-11-08 means line `L` of the
  archived `what-this-proves.until-2026-11-07.md`. Append at the end; never
  insert.
- **Backlog numbers are global across tiers.** Scan every `backlog-tasks*.md`
  before filing; plan filenames are `<tier>.<item>-<slug>.opus.md` and the
  numeric prefix must match the tier the entry actually sits in.
- **A plan's `Status:` header flips in the same commit as its last slice**,
  together with the backlog entry. A plan left saying `proposed` after it
  shipped is the most misleading staleness this repo produces.
- **A code note carries the context a short comment cannot**: `## <kind>
  <symbol>` sections under `LLM/code-notes/<crate>/<path>.md`, where the path
  mirrors the source without `src/` and `.rs`. Code comments stay short —
  `///` summary, `# Errors`/`# Panics`, a one-line *why* at most.
- **`renames.tsv` maps pre-2026-10-10 names.** Documents older than that use
  paths and identifiers that no longer exist; cite the mapping rather than
  silently following the old name.

## Writing a plan

- File name `LLM/<tier>.<item>-<slug>.opus.md` while the plan is live (finished
  plans are archived, see `LLM/ARCHIVE.md`). Title `<tier>.<item> — <what
  changes>`, then a `Status:` line.
- §0 tells the next agent what to read first and what would make the plan
  wrong.
- Every step names one test or one file, the command to run, and the expected
  output.
- `Global Constraints` lists the non-negotiables: where expected behaviour comes from, determinism,
  branch pairing, gates.
- `Review Focus` says what must still be true elsewhere after the change.
- A slice that claims a Portage behaviour with no capture is a gap; a slice
  that claims a behaviour is *right* needs the PMS, the Devmanual or a charter
  property behind it, not only a Portage capture.

## Traps

- **Editing `what-this-proves.md` in place** rewrites history and, for a
  deferred-cut marker, can orphan its `scope-cuts.md` acknowledgment, which the
  checker reports.
- **A new backlog number taken from the top of one file** collides with the
  other file. Numbers are allocated globally.
- **A link to a path you are about to archive** is a broken link; archive first,
  then fix the citations to point at `LLM/ARCHIVE.md`.
- **Quoting a document as authority when the charter contradicts it.** The
  charter wins, and the older text stays as history rather than being edited.
- **`fact_coverage.py` is not a formality**: it is how a comment rewrite is
  proven lossless, and it catches the backticked fact you thought was prose.

## Where to look

| Question | Source |
|---|---|
| What portuale is and what "correct" means | `docs/charter.md` |
| The operating rhythm and hard rules | `AGENTS.md` |
| What is settled | `LLM/agent-context.md` |
| Human-facing architecture and contribution | `docs/architecture.md`, `docs/CONTRIBUTING.md` |
| Old → new paths and identifiers | `LLM/renames.tsv` |
| Where everything went after the distillation | `LLM/ARCHIVE.md` |

## History

The plans, reviews and captures this rule came from are archived: see
[`LLM/ARCHIVE.md`](../../ARCHIVE.md); `git log` and the backlog entries in
`LLM/backlog-tasks.md` hold the per-item story.
