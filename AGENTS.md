# AGENTS.md

Entrypoint for LLM/agent work on this repo. **Read
[`docs/agent-context.md`](docs/agent-context.md) first** — it holds the
settled goals, hard constraints, architecture decisions, the bash-backend
investigation, the current state, and the open backlog. This file is just
the operating rhythm and the rules.

## The "next slice" workflow

The user drives portuale forward by saying **"next slice"** (or "scope
the next slice") and expects the same rhythm every time:

1. **Ground candidates in real code, not guesses.** Grep for the actual
   scope-cut / TODO / "deferred" doc comment, or read the corresponding
   real `lib/portage` / `lib/_emerge` source. Many slices come from
   noticing a doc comment's own "still out of scope" wording is now
   stale — or the backlog claiming something is open that `git log`
   shows shipped.
2. **Present 2–4 concrete candidate slices via `AskUserQuestion`**, one
   marked "Recommended", each with a short source-grounded rationale.
   Let the user pick.
3. **Re-open judgment calls that surface during implementation** rather
   than silently picking a default. If a slice conflicts with a hard
   constraint (e.g. contract-suite determinism), stop and surface it.
4. **Ground expected output in real Portage, not in a second copy.**
   There is no Python mirror any more
   ([`docs/second_python_copy_removal.md`](docs/second_python_copy_removal.md)).
   A slice that changes `emerge` output takes its expected value from real
   Portage — the container test bed (`TEST/`), this host's real `emerge`,
   or an upstream `lib/portage/tests/resolver/` case — and says which in
   the test's docstring. Real-execution features
   (merge/unmerge/package/fetch/phases) are checked the same way (L1–L3).
5. **Add fixtures by hand** under `fixtures/repo/…` (+ `metadata/md5-cache/…`).
   Check for name collisions with existing fixtures first. A fixture that
   "passes" without isolating the new behaviour is worse than none.
6. **Add tests**: a `CASES` entry (Rust exit code; the output invariants
   in `tests/test_output_invariants.py` run over it automatically) *and*
   a pinned-output test function in `tests/test_emerge_pretend_contract.py`,
   plus a Rust unit test in the relevant crate. Real-execution features
   get Rust fixture-driven end-to-end tests instead. If a change moves an
   output recorded in the harvested corpus (`tests/corpus/`), the run
   reports `corpus drift`: review it, then accept it with
   `PORTUALE_CORPUS_BLESS=1` in the same commit.
7. **Update the docs**: append a paragraph to
   [`docs/what-this-proves.md`](docs/what-this-proves.md) (never rewrite
   prior slices' paragraphs — they are history; fix one only to correct a
   now-stale claim) with a runnable, live-verified example, and update
   [`docs/scope-backlog.md`](docs/scope-backlog.md) if the slice closes or
   changes an open entry. Update other docs only if the slice makes them
   stale.
8. **Run the full verification pass** before a slice is done:
   `cargo fmt --check`, `cargo clippy --release --all-targets` (zero
   warnings), `cargo test --release` (whole workspace),
   `python3 -m pytest tests -q` (whole suite). **Periodically — and
   always after a big merge from another branch, or a change to the
   resolver / merge-order / ebuild-phase / merge code — also run the
   container differential test bed** (`TEST/run/l0-resolver.sh`, plus
   `TEST/run/l1-merge-from-binpkg.sh` if merge behaviour changed). It
   compares portuale against the real `emerge` on a real Gentoo tree and
   is the only thing that catches regressions the fixture suite can't;
   it's heavier, so it's not part of every slice. See
   [`TEST/README.md`](TEST/README.md).
9. **Only `git commit` / `git push` when explicitly asked** — separate,
   later requests each time, never implied by finishing a slice. Commit
   title `<what changed>: <short description>`; wrapped body explaining
   the *why* and the real-source grounding; trailer
   `Co-Authored-By: …` replace `…` with actual model name
10. **Track slices as tasks** — one per shipped slice, `completed` only
    once step 8 is green and the docs are updated.
11. **Close out a multi-slice plan in one commit.** When the last slice
    of a `docs/<tier>.<nnn>-*.opus.md` plan lands, the same commit that
    flips its `backlog-tasks.md` entry to `DONE <date>` must also flip
    the **plan file's own `Status:` header** from `proposed` to
    `done <date>` (with the slice range, branch and commits). A plan's
    §0 tells the next agent to read the plan first, so a plan left
    saying "proposed" after it shipped is the single most misleading
    staleness this repo produces — it happened to four of six plans in
    the 2026-09-15/16 batch. Same commit: correct any commit hash the
    entry cites if the branch was rebased before merging (cite hashes
    reachable from `main`, not pre-rebase ones), and give every residue
    you defer a real backlog number, checking the number is free first.

### Numbering new backlog items

Backlog item numbers are global across tiers and **never reused**. Before
filing, take the next free number by scanning *all* tiers
(`grep -nE '^[0-9]+\. \*\*' docs/backlog-tasks.md`), not just the tier
you are writing in — two items filed 13 minutes apart into different
tiers both claimed #58 on 2026-09-15. Plan filenames are
`<tier>.<item>-<slug>.opus.md`, so the numeric prefix must match the tier
the item actually sits in.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->
