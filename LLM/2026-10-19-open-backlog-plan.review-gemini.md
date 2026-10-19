# Review of the 2026-10-19 plan by gemini-argon-ro (opencode)

Captured from the agent log by the executor: the agent is read-only and could not write this file.


**Strengths**

- Oracle-before-code stated up front (`LLM/2026-10-19-open-backlog-plan.md:3`) and per-slice capture→pin→red→green→gate (`:95`) matches `AGENTS.md` + `LLM/2026-10-18-batch-plan.md:23-31`.
- Branch/clean-tree/commit-split rule (`:12,:16`) correctly mirrors two-repo rule.
- G-R/G-M/G-P (`:71-73`) correctly map to 10-18 G10/G8/G9; G-M keeps 3× L1 + glibc+bash + `mrg_remote` 29-test note; G-P keeps pty-test requirement.
- Read-only sub-agent rule is stated twice (`:49,:65`): “you may delegate only to read-only agents (`gemini-argon-ro`, `codebase-memory*`)” + Claude `portage-oracle`/`plan-reviewer` only.
- `ROOT≠/` danger for #352 is explicit (`:40,:151` “never run against host”, “bed container only”).

**Issues**

**Critical**

- `LLM/2026-10-19-open-backlog-plan.md:7` — incomplete scope. Open `≥300` tail of #405 is omitted. `LLM/backlog-tasks-2026-10.md:216` is still `**(filed…)` (not DONE/CLOSED) with `open: emerge --load-average --pretend still exits 2 … where Portage prints help and exits 1`. Plan lists #410 #414 #415 #412 #413 #416 #417 #418 #406 | #411 #376 #407 | #390 #394 #363 #364 #365 #368 #420 | #419 #352 but not #405-tail. Either add #405-tail as explicit residue or record owner exclusion. Next-free `#421` check (`:13`) is correct — `grep -nE '^[0-9]+\. \*\*' LLM/backlog-tasks*.md` max is #420 — but must be re-run before filing.
- `LLM/2026-10-19-open-backlog-plan.md:84,86` — same-wave same-file writers violate `§2:46` “Two writer agents never edit the same file or crate”. W2 `C2:110` (`portage-profile,fetch/portage_lock,emerge_build`) and `C3:115` (`ebuild_package,fetch`) both touch `fetch`/`fetch.rs`. “starts after C2 finishes `fetch.rs`, else serial” is not a lock, no `flock`/file ownership. Fix: serialize C2→C3 or split `fetch.rs` ownership by file.
- `LLM/2026-10-19-open-backlog-plan.md:88` W6 — bed contention. `C9:149` (G-M 3× L1) + `C10:151` (bed only, G-M) in same wave contradicts `:86` “One bed run at a time” + “L1 gate is heavy: no other bed”. Fix: W6 serial: C9 + gate, then C10 + gate.
- `LLM/2026-10-19-open-backlog-plan.md:85,86` W3/W4 — gate races frozen copy. `B1:85`/`B2:86` run G-R in same wave as the `C4/C5`/`C6/C7` they gate (“after C4/C5 stages land (frozen copy refreshed first)”, “after each sub-slice”). No barrier; bed agent edits allowlist is forbidden (`:129` “allowlist edited by executor, never by bed agent”) but nothing prevents reading half-landed tree. Fix: B-waves start only after executor tags C-waves green + frozen-copy refresh.

**Important**

- `LLM/2026-10-19-open-backlog-plan.md:30,127` — #406 reversal risk. Backlog `LLM/backlog-tasks-2026-10.md:218` + owner Q6 (leave `-p` empty, `top_level_abort_notice_shown` in `crates/portuale/src/pretend/gates.rs:116`) is deliberately inconsistent. Plan includes #406 because “owner asked for all” and “reversal is deliberate”. Requires explicit owner sign-off in commit body + `LLM/code-notes/portuale/pretend/gates.md` update; otherwise keep closed per 10-18 Q6. Current wording invites silent spec break.
- `LLM/2026-10-19-open-backlog-plan.md:22,112-113` — #410/#412/#414 citations inexact. `portage-profile/src/make_conf.rs:8 logical_lines / :81 shlex_token` exist (verified), `LLM/evidence/2026-10-18-409-s0/oracle.txt` exists (verified `LLM/evidence/2026-10-18-409-s0:3`), but `resolve/entry.rs:86` is `pub interactive: bool` (`crates/portage-repo/src/resolve/entry.rs:86` verified) — plan says `scheduler ignores interactive`, correct file is `emerge_build/status.rs:121 scheduler_status_mode` + `GraphEntry::interactive`, not `status.rs` alone. Clarify.
- `LLM/2026-10-19-open-backlog-plan.md:39,149` — #419 citation stale. File `crates/portuale/src/remote/merge_flow.sh:24` is now `awk '$1=="obj"||$1=="sym"{print $2}'`, not literal `awk '{print $2}'`. Still cuts at spaces (verified `$2`), so bug stands, but citation must be updated to `:24` verbatim.
- `LLM/2026-10-19-open-backlog-plan.md:34,129` — #390 citation missing. No `stays cut` in `solver_bridge.rs` (only `crates/portage-repo/src/resolve/or_preference.rs:317` “`parent.onlydeps` half stays cut”). Claimed `solver_bridge.rs:4663 "stays cut"` does not exist. Must re-grep Portage `create_depgraph_params.py:97-103` + bridge location before briefing.
- `LLM/2026-10-19-open-backlog-plan.md:69,71-73` — gate baselines uncited. `4102 passed,37 skipped,2 xfailed`, `l0-resolver 120/101/31 UNEXPLAINED`, `l0-fixture-oracle 48/0` have no branch-point evidence path. 10-18 plan used live baselines per wave; here W0 promises baseline “by name” but V1 diffs against it without storing path. Fix: W0 writes `LLM/evidence/2026-10-19-baseline/{suite9.txt,l0.txt,fx.txt}` and V1/R2 diff by test name.
- `LLM/2026-10-19-open-backlog-plan.md:46-48` — `flock /var/tmp/cargo-portuale.lock` + `PMTEST_PM=portuale-bed PMTEST_NO_BUILD=1` + `cp -p target/release/portuale /var/tmp/bedbin/portuale` is correct for cargo/bed contention, but `timeout 7200000`, 60s stagger, `oc.sh/ocstop.sh`, “never `pkill`”, “do not edit running script”, `/var/tmp` tmpfs clean (`:48,:51`) assume tooling in `LLM/tools/scope/` that is not cited as existing. Verify `oc.sh`, `ocstop.sh`, `self_exe.rs` stale-binary guard exist or drop from plan.

**Minor**

- `LLM/2026-10-19-open-backlog-plan.md:8,16,65` — agent names (`gemini-argon`, `gemini-probe-capture`, `gemini-fixture-smith`, `gemini-bed-runner`, `gemini-verify-pass`, `gemini-argon-ro`, `gemini-closeout-scribe`, `portage-oracle`, `plan-reviewer`) not grounded in repo; 10-18 plan had same gap. List as assumption or link registry.
- `LLM/2026-10-19-open-backlog-plan.md:74` — “Every new `CASES` row goes at end… every new fixture ebuild gets real-md5 cache” correct, but W1 `F1:106` promises 5 cells (`#390 #363-365 #420`) on current binary before acceptance — must also check `metadata/md5-cache/<cat>/<pkg>-<ver>` hash + `_eclasses_` format per checklist; not stated.
- `LLM/2026-10-19-open-backlog-plan.md:143` — `LLM/evidence/2026-10-19-394/report.md` + `cells.txt` + `run-cells.sh` exist (verified `LLM/evidence/2026-10-19-394:1-7`), `LLM/evidence/2026-10-15-34-s5/triage.md:1,299,329,351` exists — good. Keep those paths verbatim in C5/C8 briefs; do not paraphrase as “13 probes” without `triage.md` line numbers.
- `LLM/2026-10-19-open-backlog-plan.md:157` — R2 writes `LLM/2026-10-19-open-backlog-plan.final-review.md`, S1 flips `Status:` to `done` + backlog DONE with shas. Matches `AGENTS.md:10-11` close-out, but must also run `python3 LLM/tools/code_notes_index.py` if notes added and `LLM/tools/fact_coverage.py` if comments trimmed — missing here, required by checklist §4.

**Task-Quality Verdict: CHANGES_REQUIRED** — fix scope (#405-tail), serialize W2-fetch and W6-bed, add gate barriers/baseline paths, correct `merge_flow.sh:24` and `solver_bridge` citations, gate #406 on explicit owner reversal, cite `oc.sh`/agents or remove.
