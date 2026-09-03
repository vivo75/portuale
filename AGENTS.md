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
4. **Implement both language sides in lockstep** — `rust/…` and
   `python/emerge_pretend_reference.py` must stay behaviourally
   identical, verified *empirically* (run both against `fixtures/` and
   diff), not just via pytest. Real-execution-only features
   (merge/unmerge/package/fetch/phases) have no Python mirror — only
   their CLI-recognition surface is mirrored.
5. **Add fixtures by hand** under `fixtures/repo/…` (+ `metadata/md5-cache/…`).
   Check for name collisions with existing fixtures first. A fixture that
   "passes" without isolating the new behaviour is worse than none.
6. **Add tests**: a parametrized `CASES` entry *and* a pinned-output test
   function in `tests/test_emerge_pretend_contract.py`, plus a Rust unit
   test in the relevant crate. Real-execution features get Rust
   fixture-driven end-to-end tests instead.
7. **Update the docs**: append a paragraph to
   [`docs/what-this-proves.md`](docs/what-this-proves.md) (never rewrite
   prior slices' paragraphs — they are history; fix one only to correct a
   now-stale claim), add a live-verified example to
   [`docs/running-it.md`](docs/running-it.md), and update
   [`docs/agent-context.md`](docs/agent-context.md)'s "Current state" /
   "Open backlog" and [`docs/scope-backlog.md`](docs/scope-backlog.md).
8. **Run the full verification pass** before a slice is done:
   `cargo fmt --check`, `cargo clippy --release --all-targets` (zero
   warnings), `cargo test --release` (whole workspace),
   `python3 -m pytest tests -q` (whole suite).
9. **Only `git commit` / `git push` when explicitly asked** — separate,
   later requests each time, never implied by finishing a slice. Commit
   title `<what changed>: <short description>`; wrapped body explaining
   the *why* and the real-source grounding; trailer
   `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>`.
10. **Track slices as tasks** — one per shipped slice, `completed` only
    once step 8 is green and the docs are updated.
