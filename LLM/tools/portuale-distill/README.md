# portuale-distill

The tooling that turned `LLM/` from **133 MB / 12 845 files** into
**11 MB / 938 files**, by distilling the durable knowledge into nine skills and
archiving the capture corpus. The programs here are the ones that did it, kept
because the same pipeline can be re-run: new documents arrive, a skill drifts,
the corpus needs re-distilling.

The session that produced them is summarised in
[`LLM/ARCHIVE.md`](../../ARCHIVE.md) (what left the tree) and in the nine
`SKILL.md` files under [`LLM/skills/`](../../skills/).

## What was measured first

The plan was not "summarise 100 MB of documentation", because that is not what
the 100 MB was:

| Class | Files | Bytes | What it is |
|---|---|---|---|
| A | 540 | 6.6 MB | machine-referenced: `what-this-proves.md` (line-addressed), `backlog-tasks.md` (numbered), `code-notes/` (indexed), `LLM/tools/` |
| B | 214 | 7.5 MB | prose: plans, reviews, references |
| C | 12 094 | 86 MB | raw capture: `stdout`, `.cast`, `counts-*.txt` |

Class C is 86% of the bytes and has no prose to compress — a capture's value is
being those exact bytes, cited by path from 255 documents. Summarising it
destroys the only thing it is for. So C was archived, not distilled.

Of class B, 5.3 MB turned out to be baseline artefacts (`.tsv`, `.summary`), and
the plans are the target of 36 links from `backlog-tasks.md` and
`what-this-proves.md`. Only six documents were genuinely narrative prose.

**Conclusion: the corpus was 1.11 M words of prose, not 100 MB of it.** That is
what the local model could read end to end.

## The pipeline

```
F0  inventory      deterministic, no LLM      9 s
F1  cards          Qwen3.6-35B-A3B           ~2 h
F2  taxonomy       bge-m3 + k-means + LLM    ~10 min
F3  routing+synthesis  deterministic + frontier
F4  checks         deterministic             seconds
F5  archive+manifest  tar zstd + git rm      minutes
```

### F0 — `f0_inventory.py`

Classifies every file under `LLM/` into A/B/C, hashes it, counts how many
documents cite it, chunks the markdown by heading into ~600-word packs, and
finds near-duplicates with minhash.

Writes `inventory.jsonl`, `chunks.jsonl`, `dupes.md`, `summary.md`.

Result: 2 676 chunks, 1.89 M input tokens, **0 near-duplicate pairs** — nothing
to gain from deduplication, the documents are genuinely distinct.

### F1 — `f1_cards.py`

The map stage. One JSON card per chunk from the local model, with constrained
decoding (`response_format: json_schema`).

The rule that makes the output usable: **every claim carries a `quote` that must
appear verbatim in the chunk.** The validator checks it, repairs quotes joined
with an ellipsis, and **discards any statement it cannot ground**. On this
corpus that discarded 874 of 2 830 candidate statements (31%), leaving 1 956
verified facts. A fact that cannot be quoted is not a fact.

Two modes: full cards (`--cls B,C`) and index-only cards for the documents that
stay in the tree (`--cls A --thin`). Resumable; it appends to the output file
and skips chunk ids already present.

### F2 — `f2_taxonomy.py`, `f2b_taxonomy.py`

Embeds each document's card with bge-m3 and clusters it.

`f2_taxonomy.py` — agglomerative at k = 5/7/9/11, cosine, then the local model
names each cluster. **It produced a negative result worth keeping**: 363 of 376
documents landed in one cluster, because every document talks about portuale.

`f2b_taxonomy.py` — the same data with the shared-vocabulary component removed
(centring before clustering) and k-means. Balanced clusters of 26–97. This one
worked, and the four taxonomies are readable in `work/taxonomy/`.

### F3 — `f3_routing.py`, `f3_dump.py`, `f3_sources.py`

- `f3_routing.py` aggregates the per-chunk `skills` votes the cards already
  carried, so the document→skill assignment is reproducible and auditable with
  no second LLM pass.
- `f3_dump.py` writes one file per skill with its facts, deduplicated, each with
  its quote — the input the `SKILL.md` files were written from.
- `f3_sources.py` generates `references/sources.md` in each skill: every
  document that contributed a fact, and how many. That file is the traceability
  the skill prose does not carry — **752 of the 1 956 facts (38%) came from
  plans**, and without this table a reader could not tell.

### F4 — `f4_check.py`

The acceptance gate, and the only program here with a life after this job.
Exit 0 only when:

1. every inventoried file has a destination;
2. every destination is `keep`, `keep+index`, `archive:<skill>` or
   `archive:raw`, and names a skill that exists;
3. no markdown link inside `LLM/skills/` dangles;
4. no skill cites a path that exists neither in the tree nor in the
   archive list;
5. the size and file budgets hold.

Run it after touching any skill.

### F5 — `f5_manifest.py`, `f5_archive.sh`

`f5_manifest.py` writes `manifest.tsv`: one row per file with class, kind, word
count, destination and note. This is the proof that nothing was lost — there is
no file in `LLM/` without a row.

`f5_archive.sh` tars every `archive:` path with `zstd`, tests the archive,
compares the entry count with the manifest, and compares the digest of a random
extracted entry against the working tree. It refuses to finish if any of that
disagrees.

## Environment

All paths are derived from the script location or overridable:

| Variable | Default | Meaning |
|---|---|---|
| `PORTUALE_DISTILL_REPO` | three levels above this directory | the checkout to distil |
| `PORTUALE_DISTILL_WORK` | `~/.cache/portuale-distill/work` | artefacts (`inventory.jsonl`, `cards_*.jsonl`, `manifest.tsv`, …) |
| `PORTUALE_DISTILL_ARCHIVE` | `<work>/../archive` | where the tarball goes |
| `PORTUALE_DISTILL_MODEL` | the Qwen3.6-35B-A3B GGUF on `/mnt/D500G` | the worker model |
| `PORTUALE_DISTILL_EMB_MODEL` | `~/.cache/portuale-distill/models/bge-m3-q8_0.gguf` | the embedder |

The session used `PORTUALE_DISTILL_WORK=/mnt/D500G/space/portuale-distill/work`
and `PORTUALE_DISTILL_ARCHIVE=/mnt/D500G/space/portuale-distill/archive`.

## Re-running

```bash
export PORTUALE_DISTILL_WORK=/mnt/D500G/space/portuale-distill/work
d=LLM/tools/portuale-distill

python3 $d/f0_inventory.py                                    # 9 s, deterministic

llama-server -m "$PORTUALE_DISTILL_MODEL" -c 16384 -np 1 -ngl 99 -fa on \
    --jinja --no-reasoning-preserve --port 8097 &              # F2 naming needs it

python3 $d/f1_cards.py --start-server --n-parallel 3 --cls B,C --out cards_b.jsonl
python3 $d/f1_cards.py --start-server --n-parallel 3 --cls A --thin --out cards_a.jsonl
python3 $d/f3_routing.py
python3 $d/f3_dump.py
python3 $d/f2b_taxonomy.py                                    # starts the embed server itself

python3 $d/f5_manifest.py
python3 $d/f4_check.py
bash    $d/f5_archive.sh
git rm --pathspec-from-file="$PORTUALE_DISTILL_WORK/to-archive.txt"
```

`f1_cards.py` needs only the standard library plus a running `llama-server`;
`f2_*.py` need `numpy` and scikit-learn (`hdbscan` does not build on Python
3.15, which is why k-means and agglomerative are used instead), and
`f3_sources.py` writes `references/sources.md` under `LLM/skills/`; those files
were removed on 2026-11-08 (see `LLM/ARCHIVE.md`), so run it only to look at
provenance and do not commit its output.

## Decisions

1. **Three classes, not one corpus.** A stays, B is distilled, C is archived. A
   single "summarise everything" pass would have broken `what-this-proves.md`'s
   line citations and `code-notes`' symbol index.
2. **Archive, never delete.** 89.4 MB left the tree as a 5.5 MB
   `tar.zst` (`sha256 1686e8d2…`) plus a manifest row per file; everything is
   also recoverable from git.
3. **The plans stay.** They are the target of 36 links from the live backlog and
   the proof file. Their *knowledge* is in the skills; their *record* is theirs.
4. **Verifiable quotes or nothing.** 31% of candidate statements were discarded
   rather than trusted.
5. **Local model for volume, frontier for judgement.** 1.11 M words read
   locally; only the taxonomy decision and the nine `SKILL.md` files were
   written by the frontier model.
6. **Nine skills, one per kind of work**, with a `description` that states when
   to load the skill. Boundaries were checked against the corpus, not invented:
   `resolver` and `tests-harness` absorb the bulk; `slice-delivery` and
   `docs-and-charter` are thin in documents but procedural, and their sources
   are `AGENTS.md` and the checkers rather than the per-item record.
7. **Clustering is evidence, not the answer.** The taxonomy came from the
   card-level intent votes; the clusterings are the counter-check.
8. **HIP build deferred** — `rocm-hip-devel` was missing, Vulkan worked at
   55 tok/s per slot. Not a blocker, only a speed-up.

## Limits, and what to distrust

- **The cards are lossy by design.** They are an index into a document, not a
  substitute; the skills were written from the dumps plus the sources I opened,
  and a fact absent from both is absent from the skill.
- **31% of candidate statements were dropped.** Some were true but paraphrased.
  The skills therefore under-report rather than over-claim, but a gap in a skill
  does not prove the corpus has nothing on it.
- **The 26–97-document clusters are stable only at this corpus size.** A
  re-distillation after a hundred new documents may need a different k.
- **`f5_archive.sh` cannot be re-run after the `git rm`** — `to-archive.txt`
  lists paths that no longer exist. Rebuild the manifest (and commit) first.
- **No tests.** These tools operate on a one-off artefact set; a test would pin
  today's corpus. `f4_check.py` is the assertion that matters, and it runs on
  the real tree.
