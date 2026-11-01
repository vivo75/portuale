# #638 — Audible demoted-loopback setup failure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** done 2026-11-01 (backlog #638; branch `slice/638-loopback-warn` in both repos — portuale `b962d453`, `cb61ad50`, `de71ebcb` + this docs-only close-out; pmtest clean, no commit, no pretend-output change; residues #639/#640)

**Goal:** A failed demoted loopback setup stops being silent — the first failed netlink step aborts the rest (Portage parity) and the phase log carries Portage's `Unable to configure loopback interface: …` warning, while the phase still runs best-effort.

**Architecture:** The sender returns the first-failure `(step, errno)` instead of ignoring results; the pre_exec child reports exactly 2 bytes over a `CLOEXEC` pipe created by the parent (`pipe2`, moved write end, no parent copy); after the pump threads join, the parent reads the code and appends the Portage-identical warning to the phase log file (or `eprintln` for terminal spawns). A `PORTUALE_TEST_LOOPBACK_FAIL` parent-side hook forces per-step failure for the root test. The non-demoted `ip`-shim path is untouched (out of scope, stated).

**Tech Stack:** Rust 2024 (`crates/portuale`), Portage 3.0.83 (`3rdparty/portage/lib/portage/process.py:867-871`, `:1262`, `lib/portage/util/netlink.py:49-57`), pmtest contract suite (`../pmtest`).

**Spec:** `LLM/backlog-tasks.md` #638, argued from Portage's warn-and-abandon shape (`process.py:861-871`: `except OSError` → `writemsg` → remaining steps skipped) and the #636 sender (`commands.rs:687-733`, currently runs every request and ignores every error).

## Global Constraints

- **Expected behaviour comes from Portage, never a second portuale run.** Cite `3rdparty/portage` `file:line` in every code note (repo `// Portage:` convention: bare names).
- **Demote exactly Portage's phases under exactly Portage's gate** (unchanged — this slice touches neither).
- **Best-effort is preserved: no new `Err` path.** Loopback failure never fails the spawn; the only new failure modes are pipe-creation failure (parent-side, `Err` like any other spawn setup failure) — never a phase failure for loopback itself.
- **No printing, no allocation, no subprocess in `pre_exec`** (inherited #630 rule). The child writes 2 raw bytes to the pipe; all formatting happens in the parent. `// SAFETY:` on every unsafe block.
- **Linux-only.** Pipe, sender changes, and the `pre_exec` report are `#[cfg(target_os = "linux")]` alongside the existing block; non-Linux demoted spawns behave exactly as today.
- **Corpus untouched** (no pretend-output change). New backlog numbers start at **639**.
- **Root-gated tests run via `sudo <test-binary> <filter>`** (image rebuild is an owner step); skip cleanly when `euid != 0` or the `portage` user is unresolvable.

## Review Focus

- A loopback setup failure is no longer silent — the phase log contains Portage's warning line — pinned by Task 1's injected-failure spawn asserting the exact line in its `Capture` log.
- The first failure aborts the remaining requests (Portage parity: no `v4` after a failed link-up) — pinned by Task 1's fib_trie assertions (absent 10.0.0.1 pair under injected link-up failure, present in the clean control) plus code review of the early-return chain; the message itself stays Portage-identical (no step) by design.
- The phase still exits rc 0 when loopback setup fails (best-effort preserved) — pinned by the same test asserting rc 0 alongside the warning.
- No spurious warning on the success path — pinned by the same test's second, un-injected spawn asserting the log lacks the line and `connect` still works.
- The non-demoted shim path keeps its silent `ip … 2>/dev/null` fallback (out of scope) — existing tests re-run in Task 1, not modified.

---

### Task 0: Baseline (no product code)

**Files:** none (read-only checks). S0 is one shell probe (fib_trie pattern, Task 0 Step 3): the mechanism (pipe protocol + parent append) and the oracles (Portage text, entry text) are otherwise fully determined by #638 + Portage source; the raw-`write(2)`-destination question is designed away by never writing from the child except into the pipe.

- [ ] **Step 1: Cut the branch pair and verify clean trees**

```bash
export PORTUALE_DIR=/home/vivo/repo/PORTUALE/portuale
export PMTEST_DIR=$(cd "$PORTUALE_DIR/../pmtest" && pwd)
git -C "$PORTUALE_DIR" checkout -b slice/638-loopback-warn
git -C "$PMTEST_DIR" checkout -b slice/638-loopback-warn
git -C "$PORTUALE_DIR" status --short; git -C "$PMTEST_DIR" status --short
```

Expected: both empty (this plan file stays untracked until Task 3 commits it).

- [ ] **Step 2: Rebuild and record the binary**

```bash
cargo build --release -p portuale && ls -l --time-style=full-iso target/release/portuale
```

Expected: success; note the mtime.

- [ ] **Step 3: S0 fib_trie pattern probe (makes abort-rest testable)**

```bash
sudo unshare --net -- sh -c 'grep -c "10.0.0.1" /proc/net/fib_trie; python3 -c "import socket,struct; s=socket.socket(socket.AF_NETLINK, socket.SOCK_DGRAM, 0); print(\"has-netlink\")"' 2>&1 | tee LLM/evidence/2026-11-01-638/s0-fib-trie-probe.log
```

Expected: count `0` in the fresh netns (no 10.0.0.1 anywhere), python netlink socket creatable. Then confirm the positive shape on the host (which has addresses assigned): `grep -A1 '|-- 10\.48\.' /proc/net/fib_trie | head` — record whatever `|-- <addr>` + `/32 host LOCAL` pair looks like. Default pattern for Task 1 (override from this log if it differs): a `10.0.0.1` local route appears as a line matching `^ *\|-- 10\.0\.0\.1$` immediately followed by a line containing `host LOCAL`.

```bash
cargo build --release -p portuale && ls -l --time-style=full-iso target/release/portuale
```

Expected: success; note the mtime.

---

### Task 1: Report loopback failures to the phase log (the fix)

**Files:**
- Modify: `crates/portuale/src/ebuild_phases/sandbox.rs` (sender returns first-failure `(step, errno)`; abort rest)
- Modify: `crates/portuale/src/ebuild_phases/commands.rs` (pipe creation + write-end move + post-wait read/append; env-hook read; message text)
- Test: `crates/portuale/src/ebuild_phases/tests/sandbox.rs` (injected-failure root test + normal-path absence leg)

**Interfaces:**
- Consumes: `configure_loopback_netlink` (`commands.rs:687`), demoted `pre_exec` block (`commands.rs:869-883`), `SpawnBashScriptArgs{droppriv, iso, log_file}` (destructured at `:745-756`), `PhaseLog::Capture/Tee(&Path)` (`commands.rs:8-23`, path precedent at `:182`), `host_has_ipv6` parent pattern.
- Produces: `pub(crate) fn loopback_warn_msg(errno: i32) -> String` (parent-side, Portage-identical text); pipe protocol `[step: u8, errno: u8]` (errno fits — real errnos < 134; note the bound in a comment).

**Step codes:** `0` ok, `1` GETLINK, `2` link-up, `3` v4, `4` v6. Env hook `PORTUALE_TEST_LOOPBACK_FAIL` (parent-read, `PORTUALE_TEST_*` precedent): unset/empty/garbage → `0` (normal); `"getlink"|"linkup"|"v4"|"v6"` → force that step's failure path (skip the syscall, use the failure code). Moved into the closure as `u8` alongside `has_ipv6`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn demoted_loopback_failure_warns_and_continues() {
    // SKIP unless euid == 0 and portage_phase_ids(None).is_some().
    // Set PORTUALE_TEST_LOOPBACK_FAIL=linkup (Drop guard restores prior
    // value; serial root runs per the demoted_ convention).
    // Spawn demoted+net via spawn_bash_script, PhaseLog::Capture:
    // script asserts lo has NO default route (setup failed).
    // Assert rc == 0 (best-effort: phase still runs) AND the log file
    // contains exactly "Unable to configure loopback interface: " prefix.
  // Assert abort-rest observably: /proc/net/fib_trie must NOT contain the
  // S0-recorded 10.0.0.1 local-route pair (default: a line matching
  // `^ *\|-- 10\.0\.0\.1$` followed by `host LOCAL`; S0 log overrides) —
  // proving the v4 request never ran after the link-up failure.
    // Then clear the hook, spawn again, assert rc 0, NO warning line,
    // connect works, AND fib_trie DOES contain the 10.0.0.1 pair (earlier
    // steps ran when nothing failed — the control that makes the absence
    // above meaningful rather than vacuous).
}
#[test]
fn loopback_warn_msg_matches_portage_text() {
    // Pure unit test: prefix is exactly
    // "Unable to configure loopback interface: ", one trailing newline,
    // strerror body from from_raw_os_error (C-locale wording assumed;
    // assert only the stable prefix + newline, not the errno text).
}
```

- [ ] **Step 2: Run them to see red**

Run: `cargo test --release -p portuale loopback_` (compile fail: new fns/fields missing) and `sudo <test-binary> demoted_loopback` (no warning line — old code is silent)
Expected: both red for the stated reasons; record which.

- [ ] **Step 3: Implement**

In `sandbox.rs`: builders unchanged (already pinned by #636). In `commands.rs`:
0. Signature first (the implementer cannot guess it): change the sender to `#[cfg(target_os = "linux")] fn configure_loopback_netlink(has_ipv6: bool, fail_at: u8) -> (u8, u8)` returning `(failed_step, errno_trunc)` with `0,0` on full success; parent maps `PORTUALE_TEST_LOOPBACK_FAIL` (`""|"getlink"|"linkup"|"v4"|"v6"`, garbage → `0`) to the `fail_at` code and moves it into the closure alongside `has_ipv6`.
1. Sender: check each netlink step in order; on first failure capture `(code, errno)` and return immediately (abort rest). GETLINK failure (short reply / wrong type / no ifindex): code 1, errno = real errno or `EPROTO` for a malformed reply (comment why: unexpected reply bytes are a protocol error). `recv` EINTR → treat as failure (benign: degrades to warning + no-lo, the safe direction).
2. Pipe: in `spawn_bash_script`, `#[cfg(target_os = "linux")]`, only when `droppriv.is_some() && iso.net`: `libc::pipe2(fds, O_CLOEXEC)` (`Err` on failure — spawn setup, allowed); move the write end into the `pre_exec` closure; parent keeps only the read end. Rationale comment: CLOEXEC closes the grandchild's copy at exec and the direct child's on exit, so EOF is guaranteed once the waited-on child is gone (daemons can't pin it); the parent holds no write copy.
3. Child: after the loopback sequence, always write `[step, errno_trunc]` (2 bytes, best-effort `let _`). Pre-existing behavior for the write itself: never fails the spawn.
4. Middle (pid dance): no change needed (inherits copies, exits after reap; EOF unaffected) — but assert this in a comment at the fork site so a future edit doesn't hold the fd.
5. Parent: after `spawn_trackable` returns AND the pump/tee guards drop (log file complete), blocking-read up to 2 bytes (EOF-terminated, guaranteed above); if `step != 0`, append `loopback_warn_msg(errno)` to the `Capture`/`Tee` path (`OpenOptions::append`, ignore open/write errors — nothing left to report to), or `eprintln!` for `Inherit`/`Foreground`.
6. Replace the stale `#636` best-effort comment with the new shape (message text + abort-rest + pipe).

- [ ] **Step 4: Re-run root-gated plus the full crate suite**

Run: `sudo <test-binary> demoted_loopback --test-threads=1` (serial: process-global env hook) then `cargo test --release -p portuale`
Expected: PASS (warning present + rc 0 + lo down on injected run; absent + connect OK on clean run; SKIP only when not root; suite green modulo known host-state reds).

- [ ] **Step 5: Commit**

```bash
git add crates/portuale/src/ebuild_phases/sandbox.rs crates/portuale/src/ebuild_phases/commands.rs crates/portuale/src/ebuild_phases/tests/sandbox.rs
git commit -m "phases: report demoted loopback failure to the phase log (#638)"
```

Body: cite `process.py:861-871`, `netlink.py:49-57`, the pipe protocol, the Portage-identical message choice (grep parity), and the fib_trie abort-rest pin (S0-recorded pattern).

---

### Task 2: Regression gate (no new oracle on this host)

**Files:**
- Test: none expected (amend only if the gate exposes a gap)
- Evidence: `LLM/evidence/2026-11-01-638/` (acceptance log)

**Interfaces:**
- Consumes: Task 1 product + tests.

- [ ] **Step 1: Re-run the Task 1 root tests on the final binary (rebuild first — stale-binary trap)**

Run: `cargo build --release -p portuale && cargo test --release --no-run -p portuale && sudo <fresh-binary> demoted_loopback --test-threads=1`
Expected: green (warning + rc 0 injected; absent + connect OK clean).

- [ ] **Step 2: Host acceptance — hyperfine once, as regression gate**

```bash
sudo ./target/release/portuale emerge -1 app-benchmarks/hyperfine 2>&1 | tee LLM/evidence/2026-11-01-638/hyperfine-acceptance.log; echo "rc=${PIPESTATUS[0]}"
```

Expected: rc=0, `completed emerge (1 of 1) app-benchmarks/hyperfine-1.21.0`. Rationale (say so in the report): the netlink-failure path cannot trigger on this host, so this run only proves the always-write/read/append plumbing didn't break spawns — the spawn tests are the pin, this is the gate. If red for unrelated host reasons: record, do not chase; a second red attributable to the new code → BLOCKED with the log.

- [ ] **Step 3: Full verification pass**

```bash
cargo fmt --check
cargo clippy --release --all-targets   # zero warnings
cargo test --release
cd "$PMTEST_DIR" && scripts/pytest-parallel.sh
```

Expected: green modulo the known host-state reds (1 cargo `embedded_runtime` + 7 pytest `mrg_remote`, root-only `gnupg.getuto.tmp` tar exit 2; #632/#633 unexplained-once; #634 basetemp workaround) — disclose with evidence, do not fix here. Zero corpus drift — any drift is a red flag, never a bless.

- [ ] **Step 4: Commit (evidence only if tests untouched)**

```bash
git add LLM/evidence/2026-11-01-638
git commit -m "phases: pin demoted loopback warning end to end (#638)"
```

Body: test name, acceptance rc + version, pmtest untouched (no pretend change). If Task 2 had to amend a test, include the file and say why.

---

### Task 3: Close-out

**Files:**
- Modify: `LLM/backlog-tasks.md` (#638 → DONE; file any new residue with a fresh number)
- Modify: `LLM/what-this-proves.md` (append one paragraph, never rewrite)
- Modify: this plan's `Status:` header → `done <date>`
- Modify: `LLM/code-notes/portuale/ebuild_phases/{commands,sandbox}.md` (terse entries; run `code_notes_index.py` + `fact_coverage.py` over comment edits)

**Interfaces:**
- Consumes: Task 1 commit + Task 2 evidence and verification.

- [ ] **Step 1: File residues (check numbers free first)**

```bash
grep -hoE '^[0-9]+\. \*\*' LLM/backlog-tasks.md LLM/backlog-tasks-*.md | grep -oE '^[0-9]+' | sort -n | tail -1
```

File only what the evidence supports.

- [ ] **Step 2: Append the `what-this-proves.md` paragraph**

Cause (one line + #638 pointer), fix shape (pipe-reported first failure, parent-appended Portage-identical warning, best-effort preserved), runnable proofs (`sudo <test-binary> demoted_loopback`, hyperfine acceptance rc), honest limits (abort-rest reviewer-pinned; message identical by design; residue numbers).

- [ ] **Step 3: Re-run the close-out verification**

Same gate as Task 2 Step 3 (docs-only delta expected — product identical; record counts, disclose reds, zero drift).

- [ ] **Step 4: Flip the entries + this plan's Status, commit the close-out**

```bash
git add LLM/backlog-tasks.md LLM/what-this-proves.md LLM/superpowers/plans/2026-11-01-638-loopback-warn.md LLM/code-notes/
git commit -m "docs: close #638"
```

Do **not** push unless asked (separate request). pmtest stays commit-free.
