# Backlog #453 oracle: what Portage 3.0.82.2 prints only on a tty (`emerge --pretend`)

Oracle run 2026-10-20 on this host. Portage source: `/usr/lib/python3.14/site-packages/{portage,_emerge}`
(paths below are relative to that). portuale: `target/release/portuale` (built 2026-10-20 19:09),
symlinked as `/var/tmp/b453/bin/emerge`.

## Method

- `probe.py` (here; it lived in the session scratchpad, so fix the `sys.path` line in `run*.py`
  before re-running) stages a fresh fixtures copy per scenario with
  `cast_cells.stage_fixtures` under `/var/tmp/b453/stage-*`, builds the env with
  `cast_cells.fixture_env` (NOCOLOR=true unless `color=True`), and runs the argv either
  under `cast_record.record` (asciinema 3.2.0 `rec --headless`, 120x40, a real pty: stdin, stdout
  and stderr are all the tty) or piped (`stdin=DEVNULL`, stdout and stderr captured
  separately). A "warm" stage first runs `emerge --ignore-default-opts --pretend dev-libs/newpkg`
  with output discarded.
- Every argv starts with `--ignore-default-opts`. The fixture `make.conf` and `/etc/make.local`
  set no `EMERGE_DEFAULT_OPTS`, so the effective options match what the contract suite uses.
  Inherited host env: `TERM=xterm-256color`, `LOGNAME=vivo`, no `HOSTNAME`, no `PROMPT_COMMAND`,
  hostname `portuale`.
- Casts and pipe captures are in `casts/` (`<probe>.<pm>.cast`, `<probe>.<pm>.pipe.std{out,err}`).
  The scripts are `run1.py` (tty vs pipe: -p, -pv, --tree, masked, --quiet), `run2.py`
  (--nospinner, TERM, FEATURES, split stdout/stderr, colour), `run3.py` (global updates, colour
  edge cases), `run4.py` (unsatisfied-dep abort) and `run5.py` (--nodeps, --debug).

## Verdict table

| # | item | Portage condition (source) | Portage bytes (tty) | Portage bytes (non-tty) | portuale bytes (tty) | verdict |
|---|---|---|---|---|---|---|
| 1a | spinner animation + cursor hide/show | Mode TWIRL by default (`_emerge/stdout_spinner.py:80`), or SCROLL with `FEATURES=candy` (`_emerge/actions.py:3812-3813`). Downgraded to STATIC when `--nospinner`, `TERM=dumb` or `not sys.stdout.isatty()`, all only if `--quiet` is absent (`_emerge/actions.py:3936-3941`). Set to QUIET by `--quiet` (`:3906-3907`, again `depgraph.py:12129-12130`), by `--nodeps` (`depgraph.py:12129-12130`) and by `PORTAGE_DEBUG=1`/`--debug` (`actions.py:3947-3948`). A QUIET notice prints nothing (`stdout_spinner.py:92-93,117`). | `Calculating dependencies \x1b[?25l` then frames `-\x08\\\x08|\x08/\x08...` (one `\b`+char every 0.08 s from a daemon thread, `stdout_spinner.py:84,90,187-195`), then `\rCalculating dependencies ... done!\x1b[K\n\x1b[?25h` (`:136-137`), plus one more `\x1b[?25h` at exit through atexit (`:197-203,205-207`). The cursor sequences only appear with an animated spinner (`:198`). | `Calculating dependencies ... done!\n` (STATIC, `:110-112,133-134`) | not printed on success. On the all-masked / unsatisfied-dep abort, the STATIC form `Calculating dependencies ... done!\r\n` on a tty as well (`crates/portuale/src/pretend/merge_list.rs:442`) | **Not observable, an intentional cut.** The only tty-only bytes are the animation frames, the `\r`/`\x1b[K` redraw and the cursor sequences, which a terminal renders as the same final line `Calculating dependencies ... done!`. On success the cause is the existing convention that portuale omits the preamble (see 2), not the tty. Optional cosmetic port: animate the twirl only when stdout is a tty and the notice is not QUIET. Side finding: portuale rejects `--nospinner` with rc 2 (`option "--nospinner" is a Portage emerge option, but is not yet implemented in portuale`), while Portage accepts it (`_emerge/main.py:37`). That is a missing option, not tty output. |
| 1b | `Calculating dependencies` notice itself | `_start_resolution_display` → `spinner.begin_notice` (`depgraph.py:12086-12132`), called from `backtrack_depgraph` (`:12164`) and `resume_depgraph` (`:12297`). Printed in every mode except QUIET. **Not a tty condition.** | see 1a | `Calculating dependencies ... done!` | see 1a | not tty-only. portuale prints it only on the abort arms (`crates/portuale/src/pretend/gates.rs:103-118`, `merge_list.rs:425-450`) |
| 2 | `These are the packages that would be merged, in order:` banner | `depgraph.py:12089-12127`: printed when `--quiet` is absent **and** at least one of `--pretend`/`--ask`/`--tree`/`--verbose` is given. The text changes to `would be fetched` (`--fetchonly`/`--fetch-all-uri`), `would be built` (`--buildpkgonly`), `, in reverse order:` (`--tree`) and `would be merged:` (`--tree --unordered-display`). Written with `writemsg_stdout` and `darkgreen` (`:12122-12127`), **with no tty test**. `--nodeps` and `--debug` do not suppress it (probe). | `\n\x1b[32mThese are the packages that would be merged, in order:\x1b[39;49;00m\n\n` (green only because colour is on, see 5) | `\nThese are the packages that would be merged, in order:\n\n` | not printed on success. On abort: `\nThese are ... in order:\n\n`, never coloured (also not with `--color=y`) | **Not a tty difference.** Portage prints it on pipes too. Its absence on a successful `--pretend` is portuale's existing recorded cut (contract suite `test_emerge_pretend_contract.py:3474-3481` "K2 keeps the static ... preamble a recorded cut", `:7786-7789`). #453 does not need to touch it. **Colour bug (any output, not tty-specific):** with colour on, Portage colours the banner `darkgreen`, the timing seconds `darkgreen`, and the abort text `BAD`/`INFORM`, while portuale prints them plain, even with `--color=y` piped (probe 6 below). |
| 3 | terminal title `\x1b]0;…\x07` | Start: `actions.py:3694-3696` `xtermTitle("emerge")` unless `FEATURES=notitles`. End: `_emerge/main.py:1377-1382` `xtermTitleReset()` unless `notitles`. `xtermTitle` (`portage/output.py:261-288`) writes to **stderr** only when `sys.__stderr__.isatty()` **and** `$TERM` matches `^(xterm\|xterm-color\|Eterm\|aterm\|rxvt\|screen\|kterm\|rxvt-unicode\|gnome\|interix\|tmux\|st-256color\|alacritty\|konsole\|foot)` (`:254-256,263-268`, cached after the first call) and `dotitles` (`:26`). The start title is prefixed with `socket.gethostname()` and truncated to 253 characters (`:271-277`). The reset (`:291-332`): with `PROMPT_COMMAND` unset it writes `\x1b]0;$LOGNAME@${HOSTNAME%%.*}:$PWD(~-abbreviated)\x07`. With `PROMPT_COMMAND` set it runs `$SHELL -c "$PROMPT_COMMAND"` on stderr instead (`:298-321`). With `PROMPT_COMMAND=""` it writes an empty string. Other callers, `emergelog` (`_emerge/emergelog.py:30`) and `JobStatusDisplay` (`_emerge/JobStatusDisplay.py:298`), are not reached under `--pretend` (`main.py:1378-1379`). `portage.output.notitles()` (`output.py:335-337`) is a no-op (it lacks `global`), and emerge never calls it. | `\x1b]0;portuale: emerge\x07` (after the global-updates block, before `WARNING: ... repo_name`), and at exit `\x1b]0;vivo@:/var/tmp/b453\x07` | nothing | nothing | **Genuine tty-only gap, low-severity and not output text.** It is invisible in the rendered screen (it sets the window title). If ported, the gate is: stderr is a tty, `TERM` matches the regex above, `notitles` is not in FEATURES; write to stderr. Probes: `TERM=vt100` → no title; `FEATURES=notitles` → no title; stderr redirected to a file → no title even though stdout is a tty; stdout redirected → title still written on the tty stderr. |
| 4a | `Dependency resolution took N s (backtrack: a/b).` | `_show_resolution_report` (`depgraph.py:12135-12145`), called from the `finally` of `backtrack_depgraph` (`:12170-12172`) and `resume_depgraph` (`:12300-12302`) **iff `spinner.end_notice()` returns True**, which means the notice was displayed and still pending (`stdout_spinner.py:126-139`). So it is suppressed by QUIET (`--quiet`, `--nodeps`, `--debug`) and after `cancel_notice` (`depgraph.py:6709-6712`, `:12044-12047`). The ` (backtrack: …)` suffix appears only when `backtracked >= 0` (`:12139-12140`), which is always the case on the `backtrack_depgraph` path; `resume_depgraph` passes no counts (`:12302`), so `--resume` prints no suffix. The seconds are `darkgreen` (`:12143`). **Not a tty condition**, and it also prints on the masked and unsatisfied-dep aborts. | `Dependency resolution took \x1b[32m1.72\x1b[39;49;00m s (backtrack: 0/20).\n\n` | `Dependency resolution took 1.70 s (backtrack: 0/20).\n\n` | not printed on success. On abort `Dependency resolution took (backtrack: 0/20).` (seconds cut) | not tty-only. The wall-clock cut is the existing determinism convention (contract suite `:3470-3481`). |
| 4b | `Performing Global Updates` pass | `run_action` (`actions.py:3672-3684`): runs unless the action is `help`/`info`/`sync`/`version` or `--package-moves=n`. Inside `_global_updates` (`portage/_global_updates.py:39-40`) it needs `secpass >= 2` (true here: unprivileged mode because the user owns the staged EROOT, `portage/data.py:105-160`; probe `secpass 2 True`; host ROOT=/ as vivo gives `secpass 1`, so no pass) and no `SANDBOX_ACTIVE`. The header is printed once if any update file is new by mtime (`:99-125`). `--quiet` drops the legend and the per-file dot lines but keeps the header and the `(Could take…)` line (`:105-131`). The mtime is recorded only when the file parsed with **zero errors** (`:133-137`), and mtimedb is committed only when there were valid updates (`actions.py:3682-3683`). **No tty condition** (probe: identical on the pipe). | the block, then the per-file lines | same | nothing. portuale applies `profiles/updates` at read time (`crates/portage-repo/src/updates.rs:1-6`) and prints nothing | **Not a portuale bug for #453.** It is Portage setup noise caused by the fixture tree: `repo/profiles/updates/2Q-2024` starts with a `# …` comment line, which `parse_updates` rejects (`portage/update.py:205-206`, `ERROR: Update type not recognized`). Its mtime is never stored, so the pass re-runs on **every** call (probe A). Also, `cast_cells.py`'s warm-up `--pretend --version` is a no-op, because the `version` action skips global updates (`actions.py:3674`). That is why the #443 casts show the full `/.gentoo/repos/gentoo` pass. Remedies for pmtest (owner's choice): give Portage `--package-moves=n` (probe B: block gone, but portuale must accept the option or only Portage's argv may carry it), or warm up with a non-`version` action and drop the comment line from the staged copy (probe C: the second run is clean). |
| 5 | colour depends on the tty | `main.py:1222` sets `havecolor = 0`. Then, for **every tree in `trees`, last one wins** (`actions.py:2713-2724`, `adjust_config` `:2817-2828`): `havecolor = 1` unless `no_color(settings)` (`NO_COLOR` set, or `NOCOLOR` in yes/true, `portage/util/__init__.py:1968-1974`). An explicit `--color y\|n` overrides that. Otherwise colour is off when `TERM=dumb` or stdout is not a tty. **Trap:** with `ROOT != /` the running-root `/` config is built from a filtered env without `NOCOLOR`/`NO_COLOR` (`portage/__init__.py:497-525`), and it is iterated last, so **`NOCOLOR=true` and `NO_COLOR=1` in the caller's env do not turn colour off on a tty in the fixture runs** (trace below). With ROOT=/ they do (host probe below). | colour on (the #443 casts are coloured despite `NOCOLOR=true`) | colour off unless `--color=y` | `crates/portuale/src/color.rs:273-301` reads `NO_COLOR`/`NOCOLOR` from the process env. With `NOCOLOR=true` on a tty: no colour. With `NOCOLOR` unset on a tty: colour on the merge list, matching Portage | **Mostly correct.** portuale follows the ROOT=/ semantics. The fixture-only divergence (NOCOLOR honoured with ROOT != /) is a Portage quirk of a multi-root tree order. Classify it as not a bug, or port it deliberately (owner's call). **pmtest consequence:** `cast_cells.fixture_env(color=False)` does not decolour Portage under a pty. To compare text, pass `--color=n` to both PMs (probe: Portage `--color=n` on a tty prints no colour). |

## Discriminating probe output (trimmed; the `!!! Repository ... masters` and `repo_name` WARNING lines are omitted)

All argvs below start with `--ignore-default-opts`; `$ROOT` is the stage root.

```
=== tty -p      argv: ['/usr/sbin/emerge','--ignore-default-opts','--pretend','dev-libs/newpkg']   rc=0 events=35
...\x1b]0;portuale: emerge\x07WARNING: ...\r\n\r\n\r\n\x1b[32mThese are the packages that would be merged, in order:\x1b[39;49;00m\r\n\r\n
Calculating dependencies \x1b[?25l-\x08\\\x08|\x08/\x08-...\rCalculating dependencies ... done!\x1b[K\r\n\x1b[?25h
Dependency resolution took \x1b[32m1.72\x1b[39;49;00m s (backtrack: 0/20).\r\n\r\n[\x1b[32mebuild\x1b[39;49;00m ...\r\n
\x1b]0;vivo@:/var/tmp/b453\x07\x1b[?25h
=== pipe -p     (same argv) rc=0
stdout: "\nPerforming Global Updates\n...\n\n\n\nThese are the packages that would be merged, in order:\n\nCalculating dependencies ... done!\nDependency resolution took 1.70 s (backtrack: 0/20).\n\n[ebuild  N     ] dev-libs/newpkg-1.0 to $ROOT/\n"
stderr: "ERROR: Update type not recognized '# Quarter 2 2024 package moves (fixture)'\nWARNING: ..."
=== portuale tty -p   argv: ['/var/tmp/b453/bin/emerge','--ignore-default-opts','--pretend','dev-libs/newpkg']  rc=0 events=1
'[ebuild  N     ] dev-libs/newpkg-1.0 to $ROOT\r\n'
=== tty --quiet  (Portage) rc=0 events=10
"\r\nPerforming Global Updates\r\n(Could take ...)\r\nERROR: ...\r\n\r\n\r\n\x1b]0;portuale: emerge\x07[\x1b[32mebuild...\r\n\x1b]0;vivo@:/var/tmp/b453\x07"     # no banner, no notice, no cursor seqs, titles kept
=== tty --nospinner (Portage) rc=0: "...Calculating dependencies ... done!\r\nDependency resolution took \x1b[32m1.68\x1b[39;49;00m s ..."   # STATIC, colour and titles kept
=== tty TERM=dumb (Portage): no title, no colour, "Calculating dependencies ... done!\r\n" (STATIC)
=== tty TERM=vt100 (Portage): no title, colour on, twirl animated, trailing \x1b[?25h
=== tty FEATURES=notitles (Portage): no \x1b]0; at all, everything else as tty -p
=== tty FEATURES=candy (Portage): "Calculating dependencies \x1b[?25l\rCalculating dependencies  \x1b[32;01mr\x1b[39;49;00m\x1b[K\r..." (SCROLL)
=== sh -c 'emerge ... 2>/var/tmp/b453/err.txt' (Portage): tty shows banner+twirl+cursor seqs, no \x1b]0; ; err.txt has no title
=== sh -c 'emerge ... >/var/tmp/b453/out.txt' (Portage): tty (stderr) shows "\x1b]0;portuale: emerge\x07"; out.txt is plain STATIC
=== tty masked (Portage) argv [...,'--pretend','dev-libs/maskedpkg'] rc=1: banner, twirl, "... done!", "Dependency resolution took \x1b[32m1.33\x1b[39;49;00m s (backtrack: 0/20)", then coloured "!!! \x1b[31;01mAll ebuilds that could satisfy ..."
=== portuale tty masked, colour on (NOCOLOR unset) rc=1:
'\r\nThese are the packages that would be merged, in order:\r\n\r\nCalculating dependencies ... done!\r\nDependency resolution took (backtrack: 0/20).\r\n\r\n\r\n!!! All ebuilds that could satisfy "dev-libs/maskedpkg" have been masked.\r\n...'   # no colour anywhere
=== piped --color=y masked: Portage "^[[32mThese are the packages ...^[[39;49;00m" ; portuale "These are the packages ..." (plain)
=== tty --nodeps (Portage) rc=0: title, banner (green), merge line, title reset; no notice, no timing line, no cursor seqs
=== tty --debug (Portage): banner True, 'Calculating dependencies' False, 'Dependency resolution took' False, cursor-hide False, title True
=== missingdep (Portage, --package-moves=n) tty: banner, twirl, "done!", "took 1.35 s (backtrack: 1/20)", then "emerge: there are no ebuilds to satisfy ..." (coloured)
```

Colour trace (in-process wrapper around `_emerge.actions.adjust_config`, run under `script -qec` for a pty,
`NOCOLOR=true`, ROOT = stage):
```
[trace] adjust_config root=/var/tmp/b453/stage-cond-portage/fixtures/ NOCOLOR='true' no_color=True before=0
[trace] after=0
[trace] adjust_config root=/ NOCOLOR=None no_color=False before=0
[trace] after=1
```
Host ROOT=/ control (`asciinema rec --headless -c "/usr/sbin/emerge --ignore-default-opts --pretend --nodeps sys-apps/portage"`):
`NOCOLOR=true` → `ebuild     U  ] sys-apps/portage-3.0.83-r1 [3.0.82.2]` (plain); `NOCOLOR=false` → coloured.

Global updates (`run3.py`):
```
A fresh stage, warm-up `emerge --pretend --version` (stdout "Portage 3.0.82.2 ..."), then tty -p: Performing Global Updates + 29 update lines (/.gentoo/repos/gentoo/profiles/updates/*)
A second run: Performing Global Updates / $ROOT/repo/profiles/updates/2Q-2024..      (repeats every run)
A third run, pipe: same (not tty-dependent)
A with --package-moves=n: no block.   B fresh stage + --package-moves=n: no block.
C fresh stage, '#' line stripped from the staged 2Q-2024: run 1 full pass, run 2 no block.
D portuale fresh stage: no block.
```

## Escapes and edge cases

- `interrupt_notice`/`cancel_notice` (`stdout_spinner.py:141-159`) emit `\r\x1b[K` + `\x1b[?25h` on a tty
  and `\n` when STATIC. `cancel_notice` also forces QUIET, which suppresses the timing line. They are
  reached from `ambiguous_package_name` (`depgraph.py:12043-12047`), `_show_unsatisfied_dep` with a pending
  notice (`:6709-6712`) and depclean's `interrupt/resume` (`actions.py:1016-1022`). In the masked and
  missingdep probes the diagnostics print after `end_notice`, so no visible cancel happened.
- `--ask` without `--pretend` on a non-tty stdin aborts with `!!! "--ask" should only be used in a terminal`
  (`actions.py:3922-3927`). `--pretend` removes `--ask` first (`:3917-3918`).
- The banner is printed by `backtrack_depgraph`/`resume_depgraph` before resolution, whether or not the run
  ends up printing a list (it shows on aborts too).
- `PORTAGE_SHOW_LRU_CACHE_INFO` adds output after the timing line (`portage/util/portage_lru_cache.py:9-11`).
  It is env-gated, not tty-gated.

## Side findings (not tty, outside #453's question)

- Leading newline: Portage's stdout starts the banner with `\n` (`depgraph.py:12122`). portuale's missingdep
  abort omits it (`'These are the packages...'`), while its masked abort keeps it.
- Fixture-root only: Portage prints the root with a trailing `/` (`to $ROOT/`) and adds ` for $ROOT/` to the
  all-masked and no-ebuilds messages when ROOT != `/`. portuale prints `to $ROOT` and omits ` for …`. These
  are already known to the contract suite (`:7786-7789`), and with ROOT=/ they do not differ.

## Not established

- Whether a real `cancel_notice` mid-resolution (and its tty `\r\x1b[K`) is reachable under `--pretend` for
  some fixture shape. No probe hit it, and I did not search for a triggering shape.
- The exact frame count of the twirl depends on wall-clock time (0.08 s cadence), so it is never byte-stable.
- `--ask` interactive output was not probed (needs stdin input in the pty).
