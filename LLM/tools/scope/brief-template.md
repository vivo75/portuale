# Opencode brief template

Start every opencode brief with the block below, then add the batch's own
lines (branch, plan file, batch-specific notes) and the task. Fill in the
`<...>` fields. Launch briefs through `oc.sh` (see `README.md` here).

```markdown
## Rules for every worker (read first)
- **Do the work yourself. Do not start sub-agents or tasks.**
- Repos: `/home/vivo/repo/PORTUALE/portuale` (Rust product) and
  `/home/vivo/repo/PORTUALE/pmtest` (contract suite, fixtures;
  `portuale/fixtures` is a symlink to `pmtest/fixtures`). Both are on
  branch `<branch>`. **Do NOT commit, do NOT switch branches, do NOT
  push** unless this brief says you are the manager.
- Read `portuale/AGENTS.md` and the plan `<plan file>` sections
  "Global Constraints" and your task.
- Reference Portage is <version>, source in `portuale/3rdparty/portage`.
  Cite the Portage file:line in each new test's docstring or doc comment.
- Naming: call the reference package manager "Portage", never "real" or
  "real Portage"; avoid "real" in other senses in new text too.
- Comments: `///` summary + `# Errors`/`# Panics`; `//` only for a
  non-obvious why; at most one `// Portage: <file>, <function>`. No
  backlog numbers, plan or task names, evidence paths, SHAs or pmtest
  test names in code comments; those go to
  `portuale/LLM/code-notes/<crate>/<path without src/ and .rs>.md`
  (then run `python3 LLM/tools/code_notes_index.py`).
- Rust: new multi-argument functions take an `*Args` struct; no
  `let _ = x;` to silence values or Results; regexes compiled once
  (`OnceLock`); reuse existing parsers.
- Never use `pkill`/`pgrep -f`. Never write to `/dev/shm`. Remove your
  temp files under `/var/tmp` when done.
- Dates: write today's date as `date +%F` prints it, never a guessed or
  planned date, in backlog entries, `DONE`/`filed` stamps, plan headers
  and doc paragraphs.
- Before finishing run `cargo fmt --check`,
  `cargo clippy --release --all-targets` (0 warnings) and the tests your
  task names. Write the report to the file your task names: files
  changed, what each test pins (with Portage citation), commands and
  results, open doubts, and questions you cannot settle from Portage
  source under "OWNER QUESTION". End with `Status: DONE` /
  `DONE_WITH_CONCERNS` / `BLOCKED`.
```

When the brief makes the agent a manager that commits, add:

```markdown
## Commits (manager only)
- Title `<area>: <what changed> (#NNN)`, body in plain English: what was
  wrong, what changed, which Portage behaviour grounds it. No internal
  jargon (task labels, reviewer shorthand, gate codes) in the body.
- Commit pmtest first; the portuale message quotes the pmtest short sha,
  or says "Standalone, no pmtest half."
- End with one `Co-Authored-By:` line per model, always with an e-mail:
  `Co-Authored-By: <model name> <noreply@<vendor domain>>`, for example
  `Co-Authored-By: muse-spark-1.3-contributor <noreply@opencode.ai>`.
- Past commits are already pushed: never rewrite them to fix these points.
```
