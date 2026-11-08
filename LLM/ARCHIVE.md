# LLM/ archive and skill map

`LLM/` used to be 133 MB and 12 845 files, of which 86 MB was raw probe
capture. On **2026-11-08** the capture corpus and six prose documents were
distilled into nine skills and moved out of the tree.

| | before | after |
|---|---|---|
| size | 133 MB | 11 MB |
| files | 12 845 | 911 |
| prose | 214 documents | 199 kept, 6 distilled and archived |

## What left the tree

**11 938 files, 89.4 MB**, in one verified archive:

```
/mnt/D500G/space/portuale-distill/archive/llm-archive-2026-11-08.tar.zst   5.5 MB
sha256 1686e8d2725ea1ed10352ec0da30147a445d303e30825e23ef5f1846475ec6f6
```

It is a plain `tar --zstd` of paths relative to the repo root, so restoring a
single file is:

```
tar --zstd -xf llm-archive-2026-11-08.tar.zst LLM/evidence/2026-10-25-491/captures/p491-debug-v1.stdout
```

Everything is also still in git — the archive is a convenience for offline
reading, not a second source of truth:

```
git show <commit>^:LLM/evidence/2026-10-25-491/captures/p491-debug-v1.stdout
```

The commit that performed the removal is the one whose message starts with
`LLM: distil the prose into nine skills and archive the capture corpus`.

## Why the captures were not distilled

A capture is a `stdout`, a `.cast` or a `counts-*.txt`. It has no prose to
compress: its value is that it is the exact bytes a probe produced, cited by
path from a backlog entry or a plan. Summarising one destroys the only thing it
is for. 603 distinct evidence paths are cited from 255 documents, so the paths
must keep resolving — hence an archive plus this manifest rather than a
rewrite.

All 11 938 archived paths are listed, one per line, in the manifest that
produced the archive (`work/manifest.tsv`, column `destination`). Every file in
`LLM/` has exactly one row there: `keep`, `keep+index`, or `archive:<skill>`.

## Second archive: plans, reviews, evidence (2026-11-08)

A second, smaller archive took what the first left: 364 files, 3.8 MB, in
`llm-archive-2026-11-08b.tar.zst` (sha256 `90d5bc59abebd9bb7c6dc3e56a5ab789446ebae862cb9bb863aee8765d092376`,
next to the first one). It holds every finished plan and review
(`LLM/0N.*`, `LLM/2026-10-*`, `plan-residui-514-555.md`), `LLM/plans/`,
`LLM/superpowers/plans/`, all of `LLM/evidence/` and the frozen
`backlog-tasks-2026-10.md`. Kept: `portage-quirks.md`, the Portage bug list,
the unfinished plan `06.447`, and the errata `02.442`.

**`LLM/ARCHIVE.manifest.tsv` lists every archived path** (both archives; the
second also carries size and sha256). A path under `LLM/` cited in older text
and not found in the tree is in the archive when it appears there;
`check_links.py` and `portuale-distill/f4_check.py` read it for that reason.
Restore one file with `tar --zstd -xf <archive> <path>`.

`LLM/evidence/` stays as an empty, wanted directory (`.keep`): new
`merge-gate-<branch>.md` files go there (`pair_check.sh`). Test data that code
reads lives next to the code, not here: the Portage capture the
`global_updates` tests compare against is
`crates/portuale/src/global_updates/testdata/portage-capture/`, and
`bt_compare.py` reads `LLM/tools/data/portage-debug-masks.txt`.

## Third archive: the old `what-this-proves.md` (2026-11-08)

`LLM/what-this-proves.md` held 21 118 lines written while Portage was the
oracle, and an agent grepping it found "mimic Portage" wording that the charter
has withdrawn. Lines 1–21 068 (everything up to the 2026-11-07 charter) moved
verbatim to `what-this-proves.until-2026-11-07.md` in
`llm-archive-2026-11-08c.tar.zst` (sha256
`e5ec0472064e2dd2c1efe10b053dc4956ba225e7a0e2425786a0d3285f49a238`, listed in
`LLM/ARCHIVE.manifest.tsv`). The live file keeps a new header and the 50 lines
written after the charter, and is append-only again.

**Line numbers:** a citation `what-this-proves.md:L` written before 2026-11-08
names the same line `L` of the archived file; the nine that existed
(`scope-cuts.md`, `recurring-issues.md`, the resolver skill) are already
repointed. Prose such as "see `what-this-proves.md`'s X section" means the
archived file too. Restore with
`tar --zstd -xf <archive> LLM/what-this-proves.until-2026-11-07.md`, or read
`git show <this commit>^:LLM/what-this-proves.md`.

## Skill provenance tables

Each skill used to carry `references/sources.md`, a table of the documents its
facts came from. They cited mostly archived files, so an agent could not follow
them; they were removed on 2026-11-08, and the skills no longer cite archived
plans, reviews or evidence inline. The tables are reproducible with
`LLM/tools/portuale-distill/f3_sources.py` from the distillation work directory
(`3rdparty/portuale-distill/work`), and remain in git history.

## The nine skills

Sources are `LLM/skills/<name>/SKILL.md`; `.claude/skills/<name>` is a symlink,
so both harnesses see one copy.

| Skill | Answers |
|---|---|
| `portuale-slice-delivery` | turning a backlog entry into a closed slice: plan, blocking decisions, single-writer rule, numbering, two-repo pairing, gates, close-out |
| `portuale-oracle-probing` | establishing truth before coding: probes, playground, determinism, capture discipline, bed read-only rule, quirks not to copy |
| `portuale-resolver` | merge order, backtracking, slot operators, virtuals, demoters, seeds, post-skip re-resolution, the pubgrub bridge |
| `portuale-ebuild-merge` | EAPI phases, config chain, unmerge reinstall, preserved libs, VDB write order, binpkgs, sandbox/namespace/umask, the glibc+bash gate |
| `portuale-tests-harness` | pmtest layout, fixtures, CASES, contract pins, corpus drift, L0/L1 beds, benchmark modes |
| `portuale-code-conventions` | function and module size, pure-move rules, closures and context structs, comments versus code notes, the refactor tools |
| `portuale-output-contract` | display rows, streams, colours, counters, failure summary, news notices, helper exit codes |
| `portage-source-map` | where a behaviour lives in Portage's Python source, and the 1 468 citations already recorded |
| `portuale-docs-and-charter` | authority order, the four document checkers, what is append-only, how a plan is written |

## The six distilled documents

These were read, summarised into the skills, then archived:

| Document | Distilled into |
|---|---|
| `lessons-of-backlog-ops-2026-10-03.md` | `portuale-slice-delivery`, `portuale-oracle-probing` |
| `recap-of-backlog-ops-2026-10-03.md` | `portuale-slice-delivery` |
| `ideas.md` | `portuale-slice-delivery` |
| `Paragone_solver_portage.md` | `portuale-resolver` |
| `glep-compliance-review.md` | `portuale-ebuild-merge` |
| `feat-157-authoritative-vdb-database.md` | `portuale-ebuild-merge` |

## What was kept, and why

Not because it is precious as prose, but because something reads it by exact
position:

- `what-this-proves.md` — append-only and **line-addressed**: `scope-cuts.md`
  cites `what-this-proves.md:<L>`, and `cut_registry_check.py` enforces it.
  Inserting a line re-points every entry below.
- `backlog-tasks.md`, `backlog-tasks-2026-10.md` — item numbers are global and
  never reused; `doc_status_check.py` cross-checks plan headers against them.
- `code-notes/` — 483 files indexed by `code_notes_index.py`; an agent looks a
  symbol up by name, which no summary can replace.
- `plans/*.opus.md`, `*.opus.md` — the per-item record the backlog links to.
  Removing them broke 36 links from `backlog-tasks.md` and
  `what-this-proves.md`.
- `scope-backlog.md`, `scope-cuts.md`, `renames.tsv`, `agent-context.md`,
  `recurring-issues.md`, `portage-quirks.md` — live state or checked format.
- `LLM/tools/` — the checkers themselves.

## Maintaining this

`python3 LLM/tools/portuale-distill/f4_check.py` re-runs the
acceptance checks: every inventoried file has a destination, every
`archive:<skill>` names a skill that exists, no markdown link in the skill tree
dangles, and no skill cites a path that exists neither in the tree nor in the
archive list. [`LLM/tools/portuale-distill/`](tools/portuale-distill/README.md)
holds the whole pipeline, with the session's measurements and decisions.
