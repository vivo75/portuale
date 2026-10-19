# Batch 2026-10-19 — open backlog ≥ #300, grouped by verification tool

> **Oracle before code.** Every expected value comes from Portage 3.0.82.2 (host `emerge`, container bed `../pmtest/differential-test-bed/`, or `lib/portage/tests/resolver/`), never from portuale's own output. Call the reference "Portage", never "real". Reports to the owner are in Italian.

Status: proposed (2026-10-19), revised after two reviews (`2026-10-19-open-backlog-plan.review.md` by `plan-reviewer`, `…review-gemini.md` by `gemini-argon-ro`), updated 2026-10-19 for the state of the tree. Step 0 is done; no slice has started (W0 next). Backlog text of all 21 items re-checked: every one is still OPEN, none shipped since the plan was written; next free number is still #421.

Scope: every open backlog item ≥ #300 as listed to the owner on 2026-10-19: #410 #414 #415 #412 #413 #416 #417 #418 #406 #405(tail) | #411 #376 #407 | #390 #394 #363 #364 #365 #368 #420 | #419 #352.
Owner instructions: implement all of them; at most **three operations active at once**; use the `gemini-*` opencode agents; sub-agents only if read-only; commits at the executor's discretion when the tree is clean and the diff against the previous commit has value.

Backlog layout note: #410–#418 live in `backlog-tasks.md`; #363–#368, #376, #419, #420 sit in `backlog-tasks-2026-10.md` under the "Deliberate cuts — do NOT pick these" heading. The owner's request overrides that heading for this batch; the close-out moves or annotates each entry so the heading is not left contradicting a DONE.

## 0. Branch rule and step 0

- **Done (2026-10-19):** `batch/2026-10-19` exists in both repos (from `batch/2026-10-18`, portuale `b76ee705`, pmtest `19a2d8e`); both trees are clean. The plan and its two reviews are committed in portuale (`0f4c7735`, plan only, no pmtest counterpart), and the baseline logs were saved by `21240314` into `LLM/evidence/2026-10-19-baseline/`. Nothing else to do for step 0.
- Next free backlog number is **#421**; re-check with `grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md` before filing a residue.
- Evidence root: `LLM/evidence/2026-10-19-b<item>-*/` (captures verbatim with argv and binary mtime). Baselines are stored under `LLM/evidence/2026-10-19-baseline/`, not only on tmpfs: the three logs of 2026-10-18 are already there (suite log has dots only, no test names).
- Bed findings go to `../pmtest/differential-test-bed/findings/*.md`.
- Commits: pmtest first, the portuale commit quotes the pmtest short sha; title `<what>: <why>`; English, plain words; trailer `Co-Authored-By: <actual model name of whoever commits> <noreply@anthropic.com>`; **no push**; no squash. The commit grant is the owner's message of 2026-10-19 ("commit when revisions arrive" and "i commit saranno a discrezione di chi esegue il piano"); no push permission exists.

## 1. Items, tool, and corrected ground truth

| # | Item | Tool | Ground truth (checked by the reviews, 2026-10-19) |
|---|---|---|---|
| 410 | `KEY=a\<nl>b` unquoted → `a b` | captures exist | `LLM/evidence/2026-10-18-409-s0/oracle.txt` rows A–J, case C; fix `portage-profile/src/make_conf.rs` `logical_lines` (`:8`)/`shlex_token` (`:81`). No new probe needed. |
| 414 | fetch leaves `.<file>.portage_lockfile` | host oracle | `portage_lock.rs` already has the lock shape that removes the file (`acquire_new_lockfile`); the fix is the call site `fetch.rs:507`. |
| 415 | fetch-only gaps a–d | host oracle per sub-item | `Scheduler.py:192-193`, `fetch.py` listonly, `BinpkgFetcher`; sub-item closed as "no divergence" if Portage agrees. |
| 412 | `interactive` ignored | host oracle | `Scheduler.py:483-519`; text has **three parts**: the package list, `>>> Setting --jobs=1`, the `--accept-properties=-interactive` hint, plus "Sending package output to stdio…". Code: `emerge_build/status.rs`, `resolve/entry.rs:86`. |
| 413 | `package.bashrc` not read | host oracle + hand-built profile | `config.py:921-947`, `:2008-2022`. Work is in **`portage-profile`** (`scalar_layers.rs:229`, `phase_environ.rs:396-403`, `config.rs:159`) plus `portuale/src/emerge_build/entry_env.rs`, **not** `ebuild_package.rs`. |
| 416 | excluded binary-only package wording | captures exist | `out373/c373-exclude.txt`; the pin `test_usepkg_exclude_drops_the_only_binary_candidate` asserts wording Portage does not print. May touch resolver code ⇒ **G-R**. |
| 417 | `searching for similar names` | captures exist | Two halves: the block itself, and the candidate order. Portage's condition checks **every active package database**, not only the ebuild one (`c396-pv-upo-n-new`). |
| 418 | `usepkg-exclude` warnings ×3 | **probe first** | Portage prints three times only because the config is reloaded after each "Performing Global Updates" pass (`actions.py:3669-3685`). Capture with `--package-moves=n` and on a second run over the same root; expected: once ⇒ **close as no divergence** (portuale never does global updates, cut 351-UPD). |
| 406 | `-p` all-masked prints no notice | host oracle | Owner Q6 was decided in **plan 02.346** ("leave as is"). Included only because the owner asked for "all": the executor asks the owner (AskUserQuestion) before starting it; no answer ⇒ skip and report. Expect many pins and corpus keys to move: plan the drift review. |
| 405 tail | `emerge --load-average --pretend` exits 2, Portage prints help, rc 1 (`backlog-tasks-2026-10.md:216`) | host oracle | Added to scope (review: it is an open ≥300-era residue omitted from the first draft). |
| 411 | no signal forwarding | **pty capture of Portage first** | Portage's emerge handles SIGINT/SIGTERM only: prints `Exiting on signal N`, terminates tasks, exits 128+N (`Scheduler.py:1227-1239`). The `pid-ns-init:27-33` forwarding is inside the sandbox. No SIGHUP/SIGTSTP handler in emerge unless the capture shows one. The new handler must coexist with the existing `--ask` prompt SIGINT handling. Code: `ebuild_phases/scheduler_registry.rs` `kill_registered_children`. |
| 376 | `cargo test` hang | none | backlog says fixed (`catch_unwind`, watchdog): verify and close. |
| 407 | 14 tests failed once | none | not reproduced: run only when the tree is quiet (W0/W7 style), never beside active edits. |
| 390 | pubgrub bdeps auto | bed + existing fixtures | The "stays cut" comment was already rewritten (`9a2b266f`). The real target is the test `pg34_blib_pubgrub_pvud_plans_nothing` (`solver_bridge.rs:4665-4689`), which pins the divergence; fixtures and Portage captures already exist (`LLM/evidence/2026-10-15-plan-359/`). No new cell. |
| 363–365 | pubgrub abort cases | five pubgrub bed modes | evidence `LLM/evidence/2026-10-15-34-s5/triage.md`. |
| 368 | pubgrub: binaries, notices, `--deep=N`, `_greedy_slots` | bed; **(a) needs a new bed mode with binaries** (does not exist yet) | Needs its own sub-plan (see S-368). |
| 420 | dependency binaries rejected | bed, own cell design | `_show_unsatisfied_dep`; built on the pair of binaries used by #402 (`test_s3_bid_package_use_respect_use_…`). |
| 394 | in-graph installed parents | bed L0 | Design in `LLM/evidence/2026-10-19-394/` (`report.md`, `cells.txt`, `run-cells.sh`); needs its own sub-plan (see S-394). |
| 419 | remote merge collision gate | pmtest `mrg_remote` + **`l31-remote-merge.sh` with `L31_MODE=candidate`, and `l31b-two-container.sh`** | `remote/merge_flow.sh:24` is `awk '$1=="obj"||$1=="sym"{print $2}'` (cuts at spaces). The L1 gate never runs `merge_flow.sh`: do not rely on it alone. |
| 352 | merge-time IDEPEND into target root | bed only | portuale **already** routes IDEPEND to the running root (`resolve/entry.rs:146-159`); the `d-idep8-merge` divergence may come from the staging. Diagnose first (S-352); code only if a divergence survives. Never `ROOT≠/` on the host. |

## 2. Concurrency model (≤ 3 operations, one writer)

An operation is: a running opencode agent, a bed run, or a long local run (full `cargo test`, full pmtest suite).

**Rule: exactly one agent edits the portuale tree at a time** (the "writer slot"). The first draft ran writers in parallel in one worktree; the review showed their gates would build each other's half-done edits and the tree would never be clean enough to commit. Parallelism comes from the other two slots, which never edit portuale source:

- **Slot W (writer):** `gemini-argon` on portuale source, tests, notes. One slice at a time, ending clean (fmt, clippy, tests green) so the executor can commit.
- **Slot O (oracle/fixtures):** `gemini-probe-capture`, `gemini-fixture-smith` (pmtest fixture files only), `gemini-argon-ro` reviews. Probes use the **frozen binary copy**, never `target/release`.
- **Slot B (bed):** `gemini-bed-runner` on the frozen copy, one run at a time; or a full local suite run (`gemini-verify-pass`), never both a suite and a bed in the same moment with a writer building (pmtest's suite rebuilds the binary through its registry, outside any lock: it runs only in a gate window, below).

**Gate window:** the full pmtest suite and `cargo test` run when slot W is idle (writer has stopped, tree is the commit candidate). Slots O/B may continue on the frozen copy during a gate window; slot W waits.

- **Frozen binary:** `cp -p target/release/portuale /var/tmp/bedbin/portuale` after the writer's slice is green and before the bed/probe runs that gate it; the registry entry is `portuale-bed` (`PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1`). A bed run starts only after the copy for that slice exists (this is the barrier between a slice and its gate).
- **Cargo:** every cargo command goes through `flock /var/tmp/cargo-portuale.lock`. A spurious failure while another agent builds is re-run once before it is believed.
- **Launch:** `LLM/tools/scope/oc.sh <slug> <agent> <workdir> <brief.md> <log>`, 60 s stagger, `timeout 7200000`, one persistent Monitor per agent, stop with `ocstop.sh <slug>`; never `pkill`/`pgrep -f`; never edit a script that is running.
- **Sub-agents:** every brief says "you may delegate only to read-only agents (`gemini-argon-ro`, `codebase-memory*`); never to an agent that can write." They outlive the scope: wait for files to go quiet.
- **Read-only agents cannot write files** (the review showed `gemini-argon-ro` returned its findings in the log): the executor captures them into the evidence file.
- **Disk:** `/var/tmp` is tmpfs (~40 G shared with RAM); clean `/var/tmp/bedbin`, staging dirs at the end of each wave; never `/dev/shm`.
- **Commits stay with the executor**, per verified slice (pmtest first); agents do not commit.

| Agent | Work |
|---|---|
| `gemini-argon` | Rust code, tests, notes, pmtest pins (slot W) |
| `gemini-probe-capture` | host and bed captures, mechanical |
| `gemini-fixture-smith` | pmtest fixtures with real md5-cache (also checks `_eclasses_` format) |
| `gemini-bed-runner` | bed runs and per-cell delta |
| `gemini-verify-pass` | fmt/clippy/test/suite diffed by test **name** |
| `gemini-argon-ro` | read-only review of diffs and sub-plans |
| `gemini-closeout-scribe` | backlog, plan status, `what-this-proves`, number check |

Claude sub-agents only `portage-oracle` and `plan-reviewer`.

## 3. Gates

Per slice (AGENTS.md step 8): `cargo fmt --check`; `cargo clippy --release --all-targets` (0 warnings); `cargo build --release`, `cargo test --release`; then in `../pmtest` `python3 -m pytest pytests-contract-suite -q`, failures compared by test **name** against `LLM/evidence/2026-10-19-baseline/` (W0 re-run by name; reference of 2026-10-18: 4102 passed, 37 skipped, 2 xfailed). Corpus drift: review, bless only in the pmtest commit and only when the new output equals a Portage capture. When comments are trimmed run `LLM/tools/fact_coverage.py`; when a code note is added run `python3 LLM/tools/code_notes_index.py`.

- **G-R resolver:** `l0-resolver.sh` (reference 120 probes / 101 clean / 31 UNEXPLAINED) and `l0-fixture-oracle.sh` (48 probes, 0 UNEXPLAINED with `FX_HOST_RUNNING_ROOT=1`), no new UNEXPLAINED row. Applies to: #390 #363–365 #368 #394 #420 #416 and any slice touching `portage-repo/src/resolve*`, `masked_deps.rs`.
- **G-PB pubgrub:** the five pubgrub bed modes for #363–365/#390; #368(a) first builds the "with binaries" mode.
- **G-M merge path:** `l1-merge-from-binpkg.sh atomlists/l1-merge-gate.txt`, then `L1_SKIP_BUILD=1 L1_CONSUME_REINSTALL=1`, then with `L1_CONSUME_INSTALL_MASK="/usr/share/doc /usr/share/info /usr/share/man"`; glibc and bash merged_count 2, 0 divergences. For #419/#352 additionally `l31-remote-merge.sh` with `L31_MODE=candidate` and `l31b-two-container.sh`, plus the 29 `mrg_remote` tests.
- **G-P phase spawn:** `l2-portuale-builder.sh` + `l1-porttest.txt` or `l3-source-parity.sh` for #411/#412/#413/#415/#414 (they touch what a build or fetch phase sees); no tty on the bed, so #411 also has a pty test with a Portage pty capture.
- New `CASES` rows at the **end**; new fixture ebuilds get a real-md5 cache entry; check fixture-name collisions first.

## 4. Waves

| Wave | Slot W (writer) | Slot O (oracle/fixtures/review) | Slot B (bed/suite) |
|---|---|---|---|
| W0 | — | — | by-name baseline: re-run the suite with `-rA` or `--junitxml` (the saved log has no names) and store it next to the three saved logs; L0 and fixture-oracle logs already exist, re-run only if the binary changed; **S-376/#407** verification here, tree quiet |
| W1 | **S-1** #410 | **P1** captures: #414 #415a–d #412 #413 #405tail #418(`--package-moves=n`) #406; **P1b** pty capture of Portage for #411 | — (gate window for S-1) |
| W2 | **S-2** #414, #412 | **P2** review of S-1 diff (argon-ro); fixtures for #420 | bed G-P on S-2 frozen copy |
| W3 | **S-3** #413, **S-4** #415 (serial) | P3 captures for #368 sub-plan, #394 re-capture | bed G-P |
| W4 | **S-5** #411, then **S-6** #405 tail | review (argon-ro) of W3 diffs | bed G-P (l2/l3) |
| W5 | **S-7** #416, #417 (a then b), #418 (if divergent) | captures for #420 | bed G-R after each |
| W6 | **S-8** #390, **S-9** #363–365 | sub-plan review: S-368, S-394 (argon-ro) | bed G-PB then G-R |
| W7 | **S-10** #420 | — | bed G-R |
| W8 | **S-11** #368 a–d (one at a time, per the sub-plan) | oracle cells per sub-slice | bed G-PB/G-R each |
| W9 | **S-12** #394 (per the sub-plan) | — | bed G-R full |
| W10 | **S-13** #419 | — | bed: G-M (3 L1) then `l31` candidate and `l31b` — serial, never beside another bed |
| W11 | **S-14** #352 (diagnose first) | — | bed G-M |
| W12 | **S-15** #406 only if the owner confirms | — | gate window |
| W13 | — | final review R2 (argon-ro); closeout-scribe | full suite + L0 + fixture-oracle + L1 vs baseline |

A review of each group's diff (argon-ro, slot O) happens **before** that group is committed; the executor fixes findings first. Commits are per verified slice, not per wave.

## 5. Slices

Each: capture → pin expected output → red test → code → green → gates → executor reads the diff → commit when clean. Briefs name files, Portage citation, exact captures, and forbid touching oracles and allowlists. If a capture shows Portage agrees, the item is closed "no divergence" with the capture (as #385).

- **S-1 #410:** join `\`+newline in unquoted values; tests = oracle rows A–J.
- **S-376/#407 (W0):** #376: re-read the `catch_unwind`/`test_watchdog.rs` commit, run `cargo test --release` three times at default threads; no hang ⇒ DONE. #407: two full pmtest runs, compare names; not reproduced ⇒ close as "not reproduced, two clean runs". No code.
- **S-2:** #414 remove the dotfile at the `fetch.rs:507` call site; DISTDIR holds only the two files named in the backlog line. #412 the scheduler reads `GraphEntry::interactive`, forces foreground and `--jobs=1`, prints the exact Portage three-part text.
- **S-3 #413:** per-package `package.bashrc` in `portage-profile` + `entry_env.rs`, atoms ordered by specificity after that node's `profile.bashrc`, only nodes with `profile-bashrcs`.
- **S-4 #415:** sub-items (a) binary fetch under `-f -k`, (b) `FEATURES=mirror` ⇒ `--fetch-all-uri`, (c) `has fetch restriction turned on`, (d) resume/keep-going; each separately gated by its capture.
- **S-5 #411:** SIGINT/SIGTERM handler for builds (also single-job) running `kill_registered_children`, printing `Exiting on signal N`, exit 128+N as the pty capture shows; coexists with the `--ask` handler; pty test; G-P.
- **S-6 #405 tail:** `emerge --load-average --pretend` ⇒ help text, rc 1.
- **S-7 #416/#417/#418:** #416 `!!! All ebuilds that could satisfy "…" for <root>/ have been masked.` + `(masked by: )` row and fix the wrong pin; note the known ` for <root>` suffix difference on the bed. #417 both halves, condition over every active database. #418 only if the `--package-moves=n` probe still shows three.
- **S-8/S-9 pubgrub:** #390 flips `pg34_blib_pubgrub_pvud_plans_nothing` to the Portage output (`U pg34-btool-2`, rule `create_depgraph_params.py:97-103`); #363/#364/#365 make the bridge abort rc 1 with the Portage notice. Allowlist rows are flipped by the executor only, after the five modes show the group clean.
- **S-10 #420:** design in `evidence/2026-10-19-b420-*/design.md` first (cell from the #402 binaries), then masked disclosure for dependencies, top-level `changed_deps` included.
- **S-368 sub-plan (W6 review):** written first as `LLM/plans/…-368-pubgrub-binaries.md`-style note listing (a) binaries (new bed mode first), (b) slot-conflict and autounmask notices, (c) `--deep=N`, (d) `_greedy_slots` lookahead, each with its oracle cell and gate; reviewed by argon-ro, then executed one sub-slice at a time. A sub-slice larger than a sitting gets a number (#421+), and #368 lists what remains.
- **S-394 sub-plan (W6 review):** from `evidence/2026-10-19-394/report.md`: restart of the phase-1 walk after `_rebuild_config` records an in-graph parent (`depgraph.py:424-442`, `:531-573`); phase 2 extends and never replaces. Gate cell `r394top`: `-u --deep --rebuild-if-{unbuilt,new-rev,new-ver}` ⇒ `rR consumer`, backtrack 1/20; `--changed-deps` and no-flag/no-deep controls unchanged. Reviewed before execution.
- **S-13 #419:** parse CONTENTS keeping spaces (or call the shipped Rust ownership logic as #382 does); pin spaced-path collision; gates as §3 G-M.
- **S-352 #352:** diagnose the `d-idep8-merge` cell (is it staging?). If portuale already matches Portage, close as no divergence with the diagnosis. Only if a divergence survives: fix, bed only, `ROOT≠/` never on the host; if the bed cannot isolate the running root stop and report.
- **S-15 #406:** ask the owner first. If confirmed: `-p` all-masked prints `Calculating dependencies ... done!` and timing; expect pins/corpus keys to move; review drift key by key, bless in the pmtest commit; update `LLM/code-notes/portuale/pretend/gates.md`; the commit body records the Q6 reversal and the owner's answer.

## 6. Close-out

- **R2:** `gemini-argon-ro` reads `git diff batch/2026-10-18..HEAD` in both repos (every item has a pin that fails without the change; no agent edited an allowlist; no debug leftovers; short comments; real backlog numbers); the executor records findings in `LLM/2026-10-19-open-backlog-plan.final-review.md` and fixes them.
- **Full verification** against the W0 baseline by name: suite, L0, fixture-oracle, L1.
- **closeout-scribe:** backlog entries DONE with real shas (or OPEN with a residue number), move/annotate the "do NOT pick" entries, `what-this-proves.md` paragraphs (append only), this plan's `Status:` to `done <date>`, next-free-number check across all tiers, stale-claim scan.
- Memory: update the batch note and next free number; clean `/var/tmp`.

## 7. Limits

No push. No work outside §1 without the owner. A residue gets a real number (#421+) before the slice is called done.
