# AGENTS.md

When a repository's documentation and its existing code disagree, follow the conventions used in the existing code, and in your final reply point out each inconsistency you found between the documentation and the code.

Entrypoint for LLM/agent work on this repo. **Read
[`docs/charter.md`](docs/charter.md) first: it states what portuale is (an
independent Gentoo package manager, aiming at a healthy installation) and
how it relates to Portage (a teacher, not the oracle and not the goal).
The charter wins over any older passage that says "mimic Portage",
"drop-in replacement" or "Portage is the oracle".** Then read
[`LLM/agent-context.md`](LLM/agent-context.md): it holds the
settled goals, hard constraints, architecture decisions, the bash-backend
investigation, the current state, and the open backlog. This file is just
the operating rhythm and the rules. The durable how-to knowledge — delivery,
oracle probing, the resolver, ebuild and merge, the test harness, code
conventions, the output contract, the Portage source map, the document rules —
is in [`LLM/skills/`](LLM/skills/), one skill per kind of work, loaded on
demand. What is not there is record: the per-item plans, and the capture
corpus that [`LLM/ARCHIVE.md`](LLM/ARCHIVE.md) maps to its archive.
Humans new to the tree start with
[`docs/architecture.md`](docs/architecture.md) (what it is, how one
`emerge` flows through the code) and
[`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) (build, test, comments,
commits).

Paths under `LLM/` that older text cites but the tree no longer has (finished
plans, reviews, `LLM/evidence/…`) are in the archive: see
[`LLM/ARCHIVE.md`](LLM/ARCHIVE.md) and `LLM/ARCHIVE.manifest.tsv`. Do not
repoint those citations; they are history.

## The "next slice" workflow

The user drives portuale forward by saying **"next slice"** (or "scope
the next slice") and expects the same rhythm every time:

1. **Ground candidates in real code, not guesses.** Grep for the actual
   scope-cut / TODO / "deferred" doc comment; for a user-visible health
   problem, start from the charter's properties (a failing check, a wrong
   plan, a misleading message). Portage's `lib/portage` / `lib/_emerge`
   source is a place to learn how a problem was solved, not a spec. Many
   slices come from noticing a doc comment's own "still out of scope"
   wording is now stale — or the backlog claiming something is open that
   `git log` shows shipped. A difference from Portage is not a candidate by
   itself (step 4).
2. **Present 2–4 concrete candidate slices via `AskUserQuestion`**, one
   marked "Recommended", each with a short source-grounded rationale.
   Let the user pick.
3. **Re-open judgment calls that surface during implementation** rather
   than silently picking a default. If a slice conflicts with a hard
   constraint (e.g. contract-suite determinism), stop and surface it.
4. **Ground expected behaviour in the charter, not in Portage's output.**
   Portage is no longer the oracle (`docs/charter.md`). A slice takes its
   expected behaviour from the PMS, the EAPIs and the Devmanual, and from the
   health properties in the charter; where those are silent the owner
   decides. Portage's output is still useful as a comparison and as a hint
   (the container bed, this host's `emerge`, upstream
   `lib/portage/tests/resolver/`), and a test's docstring says which source
   it uses. Do not file or fix a difference from Portage as such: it matters
   only if the user is worse off (wrong packages, an order that breaks a
   build, a misleading message). Do not copy confirmed Portage bugs
   (`LLM/portage_bug_list_2026-11-07.html`); record suspicious Portage
   behaviour in [`LLM/portage-quirks.md`](LLM/portage-quirks.md). Prefer
   changes that remove code.
5. **Add fixtures by hand** under `fixtures/repo/…` (+ `metadata/md5-cache/…`).
   There is one fixture tree and it lives in pmtest: `fixtures/` here is
   a symlink to `../pmtest/fixtures`, so the file you add is pmtest's
   and belongs in pmtest's commit, while the Rust tests read it through
   the symlink. Check for name collisions with existing fixtures first.
   A fixture that "passes" without isolating the new behaviour is worse
   than none.
6. **Add tests**. The black-box suite lives in the sibling `pmtest`
   repo (see `LLM/agent-context.md`, "Where the tests live"): a `CASES`
   entry there (Rust exit code; the output invariants in
   `pytests-contract-suite/test_output_invariants.py` run over it
   automatically) *and* a pinned-output test function in
   `pytests-contract-suite/test_emerge_pretend_contract.py`, plus a Rust
   unit test in the relevant crate **here**. Real-execution features get
   Rust fixture-driven end-to-end tests instead. If a change moves an
   output recorded in the harvested corpus
   (`pytests-contract-suite/corpus/`), the run reports `corpus drift`:
   review it, then accept it with `PORTUALE_CORPUS_BLESS=1` — in pmtest's
   own commit, which stays separate from this repo's.
7. **Update the docs**: append a paragraph to
   [`LLM/what-this-proves.md`](LLM/what-this-proves.md) (never rewrite
   prior slices' paragraphs — they are history; fix one only to correct a
   now-stale claim) with a runnable, live-verified example, and update
   [`LLM/scope-backlog.md`](LLM/scope-backlog.md) if the slice closes or
   changes an open entry. Update other docs only if the slice makes them
   stale.
8. **Run the full verification pass** before a slice is done. Here:
   `cargo fmt --check`, `cargo clippy --release --all-targets` (zero
   warnings), `cargo test --release` (whole workspace). Then in
   `../pmtest`: `scripts/pytest-parallel.sh` (whole suite on 8 workers,
   about 90 s; it rebuilds this tree's binary itself through its registry;
   `JOBS=n` changes the workers, extra arguments go to pytest, and a plain
   `python3 -m pytest pytests-contract-suite -q` is the serial, ~8 min
   fallback).
   **Periodically — and always after a big merge from another branch, or
   a change to the resolver / merge-order / ebuild-phase / merge code —
   also run the container differential test bed**
   (`differential-test-bed/run/l0-resolver.sh`, plus
   `differential-test-bed/run/l1-merge-from-binpkg.sh` if merge behaviour
   changed, both from pmtest). It compares portuale with Portage's
   `emerge` on a real Gentoo tree and is the only thing that catches
   regressions the fixture suite can't (a difference is a hint to judge
   against the charter's properties, not a failure by itself); it's heavier, so it's not part of
   every slice. See pmtest's `USAGE.AGENTS.md` and
   `differential-test-bed/README.md`. **Merge-path safety gate:** any
   change to the merge path (per `agent-context.md` "Merge-path safety
   gate") must additionally be verified by a **successful test merge of
   `sys-libs/glibc` and `app-shells/bash`** — the two packages whose
   running files every process maps — before the slice is done.
 9. **Only `git commit` / `git push` when explicitly asked** — separate,
    later requests each time, never implied by finishing a slice. Commit
    title `<what changed>: <short description>`; wrapped body explaining
    the *why* and what it is grounded in (the charter property, the PMS/EAPI/Devmanual text, or the Portage source it learned from); trailer
    `Co-Authored-By: …` replace `…` with actual model name. Commits and
    pushes are paired across the two repos — see "Two repos, one
    project" below; one ask covers both repos unless stated otherwise.
10. **Track slices as tasks** — one per shipped slice, `completed` only
    once step 8 is green and the docs are updated.
11. **Close out a multi-slice plan in one commit.** When the last slice
    of a `LLM/<tier>.<nnn>-*.opus.md` plan lands, the same commit that
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

## Long jobs, processes and disk space

A long job — the contract suite, an L0/L1 bed run, a corpus run, a batch of
captures — is started **through a named systemd scope**, so its liveness is a
read-only question and stopping it is reliable:

```
sh LLM/tools/scope/scoperun.sh <slug> <log> <cmd> [args...]   # run it
sh LLM/tools/scope/ocgrep.sh   <slug> [pattern]               # RUNNING / STOPPED
sh LLM/tools/scope/ocstatus.sh <slug> <log> [hung_secs]       # STOPPED / HUNG / RUNNING
sh LLM/tools/scope/scopewait.sh <slug> [secs]                 # wait for it to end
sh LLM/tools/scope/ocstop.sh   <slug>                         # TERM then KILL the scope
```

Never use `ps`, `ps aux` or `pgrep`/`pkill -f` to decide whether a job is
running or to kill it: name matching catches the wrong process and misses the
detached container a test started. See `LLM/tools/scope/README.md` for why the
cgroup, not the process tree, is the boundary.

Before any run that writes a lot (parallel suite, a bed, a corpus run), check
the space:

```
sh LLM/tools/check-work-space.sh          # /tmp and /var/tmp each >= 50% free, exit 1 otherwise
```

One full contract-suite run leaves ~11 GB in its `PMTEST_BASETEMP` under
`/var/tmp` (a 48 GB mount), so two or three stale runs fill it and the next
suite fails with ENOSPC — which reads like a real test failure. `gate.sh` and
`pytest-parallel.sh` run the check themselves and stop with a message instead.
When cleaning, delete **finished** scratch dirs only; never the one a live run
is using (ask `ocgrep.sh`/`ocstatus.sh`, not `ps`).

## Two repos, one project

`portuale` (this repo: product code, unit tests, human docs in `docs/`,
agent material in `LLM/`) and `pmtest`
(the sibling checkout at `../pmtest`: the contract suite, fixtures, the
test bed, the harvested corpus) are **one project in two repositories**.
Every rule above that says "this repo" applies to the pair, split by
content, never duplicated:

- **Branches open in both repos at the same time, under the same name.**
  Before starting work, `git checkout -b <name>` here *and* in
  `../pmtest` — even if the slice looks like it will touch only one
  side (fixtures added mid-slice are the norm, not the exception). A
  branch that exists in only one repo is already drifted; if you find
  one, say so instead of working around it.
- **Commits are paired.** A slice that touches both sides lands as two
  commits, one per repo, on the same-named branch: product change here,
  fixtures / bed cells / contract pins / corpus bless there (step 5's
  split). The two messages quote each other so the pair stays traceable
  with nothing else linking them: the pmtest commit names the portuale
  branch and slice, and the portuale commit quotes the pmtest short sha
  (so commit pmtest first). A corpus bless (`PORTUALE_CORPUS_BLESS=1`)
  always rides in the pmtest commit, reviewed drift first, never in this
  repo's.
- **Standalone commits stay single.** Work that genuinely touches one
  repo only (e.g. a pmtest-only fixture correction) commits in that repo
  alone, on `main` or its own branch, with no counterpart — but the
  message says so explicitly, so the next agent doesn't go hunting for
  the missing half.
- **Pushes go together.** Rule 9's ask covers both repos at once unless
  stated otherwise; push portuale first, then pmtest, and report both
  shas. Never push one half of a pair and leave the other local.

### Numbering new backlog items

Backlog item numbers are global across tiers and **never reused**. Before
filing, take the next free number by scanning *all* tiers
(`grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md`), not just the tier
you are writing in — two items filed 13 minutes apart into different
tiers both claimed #58 on 2026-09-15. Plan filenames are
`<tier>.<item>-<slug>.opus.md`, so the numeric prefix must match the tier
the item actually sits in.

## Code comments and code notes

Comments stay short: `///` summaries plus `# Errors`/`# Panics`, `//` only
for a non-obvious why, one `// Portage: <file>, <function>` reference where
needed. Context, Portage file:line citations, backlog numbers and history go
to `LLM/code-notes/<crate>/<path without src/ and .rs>.md` under
`## <kind> <symbol>`; run `python3 LLM/tools/code_notes_index.py` after adding
a notes file. `LLM/tools/fact_coverage.py BASE FILE...` proves a comment edit
lost no fact.

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
