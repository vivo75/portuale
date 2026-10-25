# crates/portuale/src/pretend/tests/mod.rs
Shared fixtures and pty helpers for the pretend tests.

## fn test_news_context
- A [`NewsContext`] for the unit tests: the fixture tree's own `default` profile and `amd64`, owned by this process's own uid/gid (Portage's unprivileged mode) so no chown is ever needed.

## fn news_notice_test_tree
- An isolated ROOT for the fix-round-1 notice tests: a temp dir sharing the fixtures' own vdb read-only (the same isolation `unread_news_counts_reuses_the_check_news_evaluation` and the contract suite's `_check_news_isolated_root` use) plus a config with `news` in the resolved `FEATURES`. Returns the scratch base (for cleanup), the ROOT, the two fixture repos, and the config. Prints from the notice helpers land in the test capture; assertions observe the state dir (evaluation ran) or its absence (gated off before any evaluation).

## fn single_resume_root
- A scratch `ROOT` with a single-item stale resume list, so `--resume --skipfirst` empties it and takes the "nothing to do" exit without merging anything (fast, hermetic).

## fn copy_tree
- Plain recursive copy (files + dirs + symlinks) for fixture trees (`fixtures/var` has no symlinks; `fixtures/etc/portage` carries `make.profile`, repointed absolutely after the copy).

## fn news_resolve_env
- `fixture_resolve_env` plus `FEATURES="news"` opted in -- the same layering the contract suite's `_news_env` uses.
- Since #564 also pins `PORTAGE_REPOSITORIES` to the fixture repos.conf text (`fixture_repos_conf_text`, walked like production): the spawned child would otherwise merge the host's installed system repos.conf and these exact-output pins would depend on the host. Same sections and config-root-relative locations as the on-disk file; on-disk parsing itself stays pinned by the pmtest suite and portage-repo's file-based tests.

## fn eselect_stub
- A stub `eselect` on `PATH`: an executable shell script logging its argv to a marker file. Returns the stub dir (to prepend to `PATH`) and the marker path.

## fn ask_read_news_declined
- Runs `emerge <ask_flags> --oneshot dev-libs/schedok` on a pty and answers "No" to both the news prompt and the merge prompt. Asserts the shared outcome (exit 130, one news notice, no eselect spawn) and returns stdout for the caller's own checks.

## fn pty_foreground_pre_exec
- `pre_exec` for the backlog #240 SIGINT tests: the child leaves the test's session and process group (`setsid`), takes the pty slave as its controlling terminal (`TIOCSCTTY`), and makes itself the foreground group (`tcsetpgrp`). Only then does the slave line discipline turn a `^C` byte written to the master into a real SIGINT for the child (a plain spawned child has no foreground group, so the byte is swallowed and the test would hang). Only async-signal-safe syscalls run here.

## fn read_stdout_until_marker
- Drain the child's piped stdout until `marker` appears, with a hard deadline so a prompt regression fails loudly instead of hanging the suite. Returns every byte read so far; the caller keeps draining the pipe afterwards. EOF before the marker panics with the output so far (the child died before prompting).

## fn sigint_prompt_child
- Spawn `emerge --ask ...` for a backlog #240 SIGINT test: pty stdin in the child's foreground group (so `^C` raises SIGINT), both outputs piped, stderr drained on a thread from the start (so a chatty resolve can never block on a full pipe while the test polls stdout for the prompt). Returns the child, the master, the stdout pipe, and the stderr-drain handle.

## fn ctrl_c_at_prompt
- Spawns `args` on a pty with the env `make_env` builds, waits for `prompt` on stdout, then sends `^C` through the pty. Asserts what every interrupted prompt shares with Portage's `UserQuery.query` (`_emerge/UserQuery.py:74-76`): exit 130 by exit, not by signal, one `Interrupted.` and no `Quitting.`. Returns stdout. The `^C` goes out only after the prompt text is seen, so it cannot strike during startup or the resolve, where SIGINT must still kill the process. Delivery needs the foreground group from `pty_foreground_pre_exec`.

## fn ask_pretend_unowned_root
- ROOT the test caller cannot own, so `privileges::is_privileged` is false for a non-root caller: `/usr` reliably exists, is root-owned and not world-writable, so its non-existent child is an unowned target (the same trick as `privileges::tests::a_dir_the_caller_cannot_write_is_not_privileged` and pmtest's `..._refuse_to_merge_into_a_root_owned_tree`). Every #246 pin returns early for a root caller -- the privilege gate only fires for a non-root caller, like those pins. Returns `None` for a root caller (the test returns early).

## fn true_spelling_matches_bare
- Run the built binary twice under `--pretend` -- once with `--X=True`, once with the bare `--X` -- against the committed fixture tree on a shared scratch `ROOT`, and assert the two runs are byte-identical (exit code, stdout, stderr). Backlog #241: Portage's `true_y_or_n` choices (`_emerge/main.py:320-322,365-371,405-415,446-449,612-623,697,732-747`; normalized `in true_y` per option) admit `=True` for eleven options portuale parsed narrowly (`=y`/`=n` only, rc 2 `unrecognized option` for nine of them and a wrong `(choose from "y", "n")` rejection for the two autounmask-keep options); `=True` must behave exactly like the bare flag. `expected_rc` pins the shared exit code (0 for the resolving shapes, 1 for the two autounmask-keep shapes whose fixtures fail like Portage). `--pretend` never writes, so sharing one `ROOT` across both runs is hermetic.

## fn built_portuale_bin
- The `portuale` binary next to the current test executable (same layout `ask_without_a_tty_...` already relies on).

## fn stale_resume_root
- A scratch `ROOT` with nothing but a stale multi-item resume list (multi-item so the `resume_backup` rotation would *also* rewrite the file if it ran -- the byte comparison below pins the rotation stays out too). Returns the root plus the file's bytes before the run under test.

## fn fixture_resolve_env
- Fixture-config env for a spawned `portuale emerge` that resolves (never merges): the committed fixture tree as config, the scratch dir as `ROOT`, everything else absolute so the test holds from any working directory.

## fn pty_slave_path
- The slave path of `master`, through the reentrant `ptsname_r`: plain `ptsname` returns a pointer into one static buffer, so two tests creating ptys on parallel threads could open each other's slave -- crossed answers, `--ask should only be used in a terminal`, EIO at `TIOCSCTTY`, hung prompts (#321).

## fn pty_pair
- A pty pair without any external helper: the child answers `--ask` through the slave (its stdin), satisfying the up-front isatty gate, while the test writes the answer into the master. `libc` is already a dependency (`stdin_is_tty` uses it).
- Backlog #234 / #321: close-on-exec on both ends, set at open time (`O_CLOEXEC`), not by a later `fcntl`: a sibling test forking in that window keeps a private copy of the fd past its exec (a child's fd table is copied at fork), which under load holds this pty open. Without it the spawned child inherits the master, so dropping the test's `master` never delivers EOF to the child's slave read (the EOF test hung on exactly this). The stdio dup onto fd 0 clears the flag on the dup, so the child's stdin survives.

## fn pty_watchdog_bound
- Backlog #259: bound every wait on a pty-driven child. Plain `wait_with_output`/`wait` has no timeout, so a child stuck in `read_prompt_line`'s blocking `libc::read` (the #259 hang: the slave sat in `wait_woken` and the test binary waited 20 hours) takes the whole `cargo test` run with it. Polls `try_wait` against a bound and kills the child when it expires. The bound is 60 s by default -- orders of magnitude past these prompt tests' real runtime (sub-second) and still short enough to fail a CI job instead of hanging it. `PORTUALE_TEST_PTY_WATCHDOG_SECS` overrides it: set it low to reproduce a hang, or high to give a loaded machine room. The `watchdog:` prefix on the timeout message is the race-chase hook -- the first timeout's captured output is the evidence for why the child never woke.

## fn wait_pty_output
- Bounded wait collecting the child's piped stdout/stderr. The pty-driven `--ask`/`--read-news` tests answer through the master and read the merged result back; on timeout the child is killed and the panic carries whatever it had printed, which is what the #259 race chase reads.

## fn wait_pty_status
- Bounded wait for the `sigint_prompt_child` shape, whose stdout and stderr are already owned by the caller's drain threads -- there is nothing left for `wait_with_output` to collect, so this returns the exit status alone. Same bound and `watchdog:` panic; the pre-wait marker text the caller already holds is the chase evidence for these tests.

## static INFO_SCALAR_ENV_LOCK
- Serialize the one test that mutates the process environment, in the `PLACED_CONFIG_ENV_LOCK` (`remote.rs`) style.
