# crates/portuale/src/elog.rs
Elog message collection and dispatch (Portage's `lib/portage/elog/`). Context moved out of the code comments (#336 Phase 7); the code keeps only the short why.

## module
- After every package merge, Portage's `elog_process(cpv, settings)` reads the per-phase message files `bin/isolated-functions.sh::__elog_base` wrote under `${T}/logging/`, filters them by `PORTAGE_ELOG_CLASSES` (default `"log warn error"`), and hands them to each module named in `PORTAGE_ELOG_SYSTEM` (default `"save_summary:log,warn,error,qa echo"`).
- This module ports: `mod_echo` -- `collect` reads one package's `${T}/logging/`, `echo_summary` prints the accumulated `* Messages for package <cpv>:` blocks (Portage's `finalize()`, an atexit handler). `mod_save` -- one `<logdir>/elog/[<cat>/]<pf>:<ts>.log` file per package (`save_process`). `mod_save_summary` -- append every package's messages to a single `<logdir>/elog/summary.log` (`save_summary_process`). This one is ON by default (it's in `make.globals`'s `PORTAGE_ELOG_SYSTEM`). `mod_syslog` -- one `syslog(3)` call per elog line, tagged `"portage"`, facility `LOG_LOCAL5` (`syslog_process`). `mod_custom` -- always saves the file first (unconditional `mod_save.process` call), then runs the unmodified `$PORTAGE_ELOG_COMMAND` with `${LOGFILE}`/`${PACKAGE}` substituted (`custom_process`). `mod_mail` -- one MIME mail per package, sent immediately via a sendmail binary (`PORTAGE_ELOG_MAILURI` naming an absolute path) or hand-rolled plain SMTP (AUTH LOGIN/PLAIN, dot-stuffing), `mail_process`. `mod_mail_summary` -- accumulates every package's messages and sends ONE multipart mail at process exit (atexit `finalize`, armed on first use), `mail_summary_accumulate` / `mail_summary_finalize`.
- Portuale never deletes the builddir, so the driver (`pretend.rs`) re-scans each entry's `${T}/logging/` after the merge loop (and after the `emerge -C`/`--depclean`/`--prune` removal loop, filtered to the `prerm`/`postrm` phases -- Portage's `dblink.unmerge` `_elog_process(phasefilter=...)`) via `process_batch` -- no message buffer threaded through the (un)merge machinery.
- `PORTAGE_ELOG_CLASSES` / `PORTAGE_ELOG_SYSTEM` / the three `PORTAGE_ELOG_MAIL*` settings are read from the env only (no `make.conf`), defaulting to `make.globals`. The `logdir` is `$PORTAGE_LOGDIR` else `<root>/var/log/portage` -- root-relative, a deliberate divergence from Portage's `mod_save` `<BROOT>/var/log/portage` (`BROOT` is `/`, needs privileges), matching portuale's other `<root>`-relative path choices for a relocatable tree. The uid/gid/mode chmod dance on the log dir/files is a documented cut, like every other privilege-preserving `chown` in portuale.

## const EBUILD_PHASES
- Ports `portage.const.EBUILD_PHASES`, in order -- `mod_echo._finalize` walks this so messages print in phase order regardless of file mtime.

## struct ElogMessage (fields phase, level)
- `phase`: which `${T}/logging/<phase>` file this came from -- Portage's `collect_ebuild_messages` keys on it, and `_combine_logentries` emits a `<LEVEL>: <phase>` header on every phase/level change. `level`: `LOG` / `INFO` / `WARN` / `ERROR` / `QA` (QA is a distinct level; see `syslog_priority`).

## struct ElogPackage (fields cpv, root, logfile, messages)
- `cpv` is `cat/pkg-ver` (Portage's `mod_echo` `key`); `logfile` is `PORTAGE_LOG_FILE` when `PORTAGE_LOGDIR` is set (`mod_echo.process`, backlog #435).

## fn echo_logfile_for
- Resolves `PORTAGE_LOG_FILE` for `mod_echo.process` (backlog #435): when `PORTAGE_LOGDIR` (or `PORT_LOGDIR`) is set and `${T}/build.log` (or `.gz`) is a symlink whose target lies inside that log directory, returns the target path.

## fn module_enabled
- Whether the module is listed in `PORTAGE_ELOG_SYSTEM` -- its token is either the bare name or `name:levels`. `-` is accepted for `_` (Portage's `elog_process`: `s = s.replace("-", "_")`), so `save-summary` == `save_summary`.

## fn echo_enabled
- Whether the `echo` module is enabled (Portage default: yes).

## fn portage_elog_classes
- `PORTAGE_ELOG_CLASSES` (Portage's `make.globals` default `"log warn error"`), uppercased.

## fn module_classes
- The uppercased message classes module `name` shows -- its own `name:levels` override in `PORTAGE_ELOG_SYSTEM` if present (Portage's `elog_process` per-module `filter_loglevels(..., levels)`), else `PORTAGE_ELOG_CLASSES`.

## fn collect_all
- Reads `${t_dir}/logging/*` for one package -- every `LOG`/`INFO`/`WARN`/`ERROR`/`QA` line, in `EBUILD_PHASES` order, unfiltered (Portage's `collect_ebuild_messages`). Empty when there is nothing there.

## fn collect_all_phases
- `collect_all`, restricted to the named phases -- Portage's `_elog_process(phasefilter=...)`, which `dblink.unmerge()` calls with `("prerm", "postrm")` so a package's stale install-time `${T}/logging` files don't resurface on removal. The builddir is cleaned after every merge since backlog #42, but `FEATURES=noclean`/`keepwork` can still keep one, so the filter stays load-bearing.

## fn filter_by_classes
- `collect_all` filtered to `classes` (Portage's `filter_loglevels`).

## fn collect
- `collect_all` filtered by the `echo` module's classes -- the shape `echo_summary` consumes. `phases` optionally restricts which `${T}/logging/<phase>` files are read (Portage's `phasefilter`).

## fn combine_logentries
- Ports Portage's `_combine_logentries`: one flat string, phases in `EBUILD_PHASES` order, a `<LEVEL>: <phase>` header emitted whenever the (phase, level) pair changes, a trailing blank line when anything was written. `msgs` must already be in `collect_all`'s phase order.

## fn logdir
- `$PORTAGE_LOGDIR` if set, else `<root>/var/log/portage` -- see the module notes on the `<BROOT>` divergence.

## fn utc_stamp
- UTC `%Y%m%d-%H%M%S` of the current time (Portage's `mod_save` `time.strftime(..., time.gmtime())`).

## fn utc_stamp_at
- UTC `%Y%m%d-%H%M%S` of an arbitrary `SystemTime` -- `utc_stamp`'s formatting, generalized so `emerge_build`'s `PORTAGE_LOGDIR` build-log naming can reuse it against a `.logid` marker file's mtime (Portage's `prepare_build_dirs.py` `time.gmtime(os.stat(logid_path).st_mtime)`) instead of "now". Civil-from-days (Howard Hinnant's algorithm) -- no chrono dep.

## fn save_process
- Ports `mod_save.process`: write one package's `fulltext` to `<logdir>/elog/<pf>:<utc-stamp>.log` (or, with `FEATURES=split-elog`, `<logdir>/elog/<cat>/<pf>:<stamp>.log`; otherwise the `<cat>:` is prefixed onto the filename). `key` is `cat/pkg-ver`. Returns the path written. Skipped entirely by the caller when the package has no class-filtered messages.

## fn save_summary_process
- Ports `mod_save_summary.process`: append one package's block to `<logdir>/elog/summary.log` -- a `>>> Messages generated by process <pid> on <local-time> for package <key>:\n\n` header, then `fulltext`, then `\n`. Portuale uses the same UTC stamp `mod_save` does (with a `UTC` suffix) rather than Portage's `time.localtime()` + `%Z`, for a deterministic, timezone-independent line.

## fn syslog_priority
- Ports `mod_syslog.py`'s `_pri` map: `INFO`/`ERROR`/`LOG` each get their own syslog level; `WARN` and `QA` (a distinct level -- see `ElogMessage::level`) both fall to `LOG_WARNING`, matching `_pri["QA"] = syslog.LOG_WARNING`. Split out from `syslog_process` so the mapping is directly unit-testable without any `syslog(3)` call involved.

## fn syslog_line
- Ports `mod_syslog.process`'s per-line format: `"{key}: {phase}: {line}"`, trailing newline stripped (Portage `line.rstrip("\n")`). Split out from `syslog_process` for the same direct-unit-test reason `syslog_priority` is.

## fn syslog_process
- Ports `mod_syslog.process` (elog/mod_syslog.py): one `syslog(3)` message per elog line, tagged `"portage"`, facility `LOG_LOCAL5`, priority mapped from that line's own level (`syslog_priority`). `key` is `cat/pkg-ver`. The `openlog()` "logopt" argument is literally `LOG_ERR | LOG_WARNING | LOG_INFO | LOG_NOTICE` in Portage's own source -- those are priority-level constants, not `LOG_PID`/`LOG_CONS`-style option flags, almost certainly a copy-paste mistake upstream -- but this ports Portage behavior bug-for-bug rather than the (more sensible) `0` an author fixing it today would likely use.
- SAFETY: `ident` outlives the openlog/closelog pair (glibc keeps only the pointer, not a copy); the priority/facility arguments are plain ints. A fixed `"%s"` format string -- `c_line` (which may contain `%` from arbitrary ebuild output) is never itself interpreted as a format string, matching Python's own `syslog.syslog()` (which always does the same internally). `closelog` takes no arguments; it is always paired with the `openlog` above, so `ident` is never accessed after close.

## fn custom_process
- Ports `mod_custom.process` (elog/mod_custom.py): always calls `mod_save.process` first (unconditionally, even when the `save` module itself isn't separately enabled -- `${LOGFILE}` needs a path to substitute), then runs `$PORTAGE_ELOG_COMMAND` (unmodified `bash -c`, matching portuale's own "run the external process" precedent) with `${LOGFILE}`/`${PACKAGE}` substituted. Errors if `PORTAGE_ELOG_COMMAND` is unset/empty (Portage's `MissingParameter`) or the command exits non-zero (Portage's `PortageException`). Returns the saved log file's own path -- the same file `${LOGFILE}` pointed the command at.

## section: mail (elog_mailuri and following)
- Ports `lib/portage/elog/mod_mail.py` + `mod_mail_summary.py` + `lib/portage/mail.py`, in one place.
- `mod_mail.process` sends one mail per package right away: `PORTAGE_ELOG_MAILURI` picks the recipient (default `root@localhost`, Portage's `make.globals`) and optionally the route -- `"address [[user:passwd@]mailserver[:port]]"`, where a `mailserver` that is an absolute path means "pipe the message to that sendmail binary" (`sendmail -f <from> <recipient>` on stdin) instead of SMTP. `PORTAGE_ELOG_MAILFROM` (default `portage@localhost`) and `PORTAGE_ELOG_MAILSUBJECT` (default `"[portage] ebuild log for ${PACKAGE} on ${HOST}"`) take the same `${HOST}`/`${PACKAGE}`/`${ACTION}` substitutions Portage does; `${ACTION}` is `merged`, or `unmerged` when only `prerm`/`postrm` (or `other`) phases appear, else `unknown`.
- `mod_mail_summary.process` only accumulates (`header + fulltext` per package, with a snapshot of the three settings -- Portage's `_items[config_root]`); its `finalize` (an atexit handler in Portage, registered from `elog/__init__.py`) sends ONE mail for the whole run -- subject's `${PACKAGE}` is `one package` / `multiple packages`, the body lists every package, each package's messages ride as a MIME attachment.
- MIME shape mirrors Portage's `portage.mail.create_message` (single `text/plain` UTF-8 part, `multipart/mixed` with one text part plus one part per attachment, `From `/`To`/`From`/`Subject`/`Date` headers, bodies base64-wrapped at 76 columns exactly like Python's `email` package with an explicit UTF-8 charset). `Date` is a local RFC-2822 stamp (`formatdate(localtime=True)`); the `From ` mbox separator carries the same stamp in asctime shape, as Python's `set_unixfrom` flattening does.
- Delivery without new dependencies: the sendmail-binary route spawns the binary directly (Portage's `os.popen`s a shell word-split command line -- argv is the same for sane addresses and safer for odd ones); the SMTP route is a small hand-rolled conversation over `std::net::TcpStream` (EHLO extensions, `AUTH LOGIN` then `AUTH PLAIN`, `MAIL`/`RCPT`/`DATA` with dot-stuffing, `QUIT`, 60 s I/O timeouts matching Portage's finalize alarm). Deliberate cut: STARTTLS (`:port` above 100000) needs a TLS stack portuale doesn't carry -- attempting it prints a one-line cut notice and skips, the same "message still logged elsewhere, action skipped" shape the old `mail*`-unsupported notice had. `socket.getfqdn()` for `${HOST}` is approximated with `gethostname()` (no DNS lookup); on a system without DNS-backed names these already agree.
- Errors follow Portage's own `PortageException` text: SMTP protocol failures print `!!! An error occurred while trying to send logmail:`, unreachable hosts `!!! A network error occurred ... Sure you configured PORTAGE_ELOG_MAILURI correctly?`, a failing sendmail binary its own `!!! <path> returned with a non-zero exit code` line -- all to stderr, delivery of the other modules unaffected. Portage's `make.globals` defaults for the three mail settings.

## fn mail_host
- `${HOST}` for subject/from substitution: Portage's `socket.getfqdn()` (a DNS-backed canonical name); portuale reads the local hostname via `gethostname(2)` with no DNS lookup, which agrees on every system whose hostname already is the FQDN. SAFETY: `buf` is a valid 256-byte stack array; `gethostname` writes at most its length including the NUL. NUL-terminated by `gethostname` on success; hostname bytes are never interior-NUL. Invalid UTF-8 is lossy-mapped (a subject line, never a protocol field).

## fn force_ascii
- Ports `portage.mail._force_ascii_if_necessary` (gentoo bug #291331): smtplib is ASCII-only, so non-ASCII becomes `\xNN`/`\uNNNN` backslash escapes. Subjects go through this; bodies travel base64'd instead.

## fn base64_encode
- Minimal base64 (AUTH credentials, MIME bodies) -- no new dependency for ~20 lines; standard alphabet, `=` padding.

## fn base64_wrapped
- Wrap base64 at 76 columns, the way Python's `email` package emits a UTF-8-charset payload.

## fn rfc2822_now
- Ports `email.utils.formatdate(localtime=True)`: local RFC-2822 stamp (e.g. `Thu, 10 Sep 2026 07:45:00 +0200`). `%a`/`%b` follow the process locale like any C `strftime` (Python always renders English -- portuale runs under the C locale in practice, same result).
- `libc::time_t` is deprecated in the libc crate's musl definitions ("changed to 64-bit in musl 1.2.0, we'll follow that change in a future release", libc#1848) while `localtime_r` still takes it. Keep the ABI-correct alias for this one call; the value is epoch seconds. SAFETY: `localtime_r` writes exactly one `struct tm` through a valid pointer; the input is a live `time_t`. `buf` fits any `strftime` expansion of this fixed format; `tm` was just filled by `localtime_r`. `strftime` wrote `len` bytes of (locale-)text with no interior NUL; lossy-mapped like the hostname.

## fn mime_text_part
- One `text/plain` MIME part, Portage's `MIMEText(body)` + `set_charset("UTF-8")` (which is why the payload is base64, even when it happens to be pure ASCII).

## fn create_message
- Ports `portage.mail.create_message`: `From ` mbox separator, then `To`/`From`/`Subject`/`Date`, then either one text part or `multipart/mixed` (text body first, then one part per attachment). `subject` must already be `${...}`-substituted; it is ASCII-forced here, like Portage's `Header(...)` assignment. Boundary minted from pid + time: Portage's boundary is `email`'s random token (same uniqueness role, and -- unlike a fixed string -- it cannot collide with a logged payload). Tests match the structure, never the token.

## enum MailRoute
- Where one message goes: Portage's `PORTAGE_ELOG_MAILURI` `"address [[user:passwd@]mailserver[:port]]"` -- an absolute-path `mailserver` pipes to that sendmail binary, anything else is SMTP (`localhost:25` when bare). A `:port` above 100000 means STARTTLS on `port - 100000` (Portage's `mail.py`); portuale cannot do TLS and reports that as a cut at send time.

## fn parse_mailuri
- Split `PORTAGE_ELOG_MAILURI` into its recipient and route. Returns the recipient alone (direct-SMTP-default shape) when there is no second word, exactly like Portage's `else` branch. Test arms: bare recipient (direct SMTP to localhost:25), absolute-path server (sendmail route), SMTP host with default port and no auth, AUTH + explicit port, port above 100000 (STARTTLS on port-100000).

## fn send_mail
- Ports `portage.mail.send_mail` over the parsed route: the sendmail binary gets `-f <from> <recipient>` on argv and the message on stdin; SMTP runs the hand-rolled conversation below. `Ok(())` means accepted; `Err` text already follows Portage's own `PortageException` shapes (the caller just prints it).
- No second MAILURI word: Portage still SMTPs to localhost:25, addressed to the whole URI word. Missing sendmail path: Portage's `os.path.exists` gate falls through to SMTP against the path as a host (which then fails as a network error); portuale reports the missing binary directly -- same failure, less confusing detour.

## fn smtp_read_response
- One SMTP response: `(code, lines)` -- `250-...` continues, `250 ...` ends (smtplib parses the same way).

## fn smtp_send
- Hand-rolled `smtplib` subset: greeting, EHLO (extensions parsed for the AUTH choice), optional `AUTH LOGIN` (then `AUTH PLAIN`), `MAIL`/`RCPT`/`DATA` with dot-stuffing, `QUIT`. Response codes follow Portage's `SMTPException`-on-error shape; transport failures follow its `OSError` shape. Empty authorize-id, like `smtplib`'s own PLAIN initial response (`\0user\0pass`). Dot-stuffing (Portage's `smtplib.sendmail` does the same). `BufRead::lines` drops a trailing empty line; the message must still end `<CRLF>.<CRLF>` (and an empty body still terminates).

## fn mail_action
- Ports `mod_mail.process`'s `${ACTION}` call: `merged` normally; `unmerged` when only `*rm` (or `other`) phases appear; `unknown` when an `*rm` phase is mixed with anything else (to "avoid misinformation").

## fn mail_process
- Ports `mod_mail.process`: one mail for package `key` right away. `phases` are the phases present in this package's logentries (for the merged/unmerged/unknown `${ACTION}` call); `fulltext` is the class-filtered combined text. Delivery failures print Portage's own `PortageException` line; nothing is returned (Portage's `process` returns `None`).

## struct MailSummaryState
- Accumulated `mod_mail_summary` state: Portage's `_items[config_root]` (first-seen settings snapshot + per-package header+fulltext), narrowed to portuale's single config per process.

## static MAIL_SUMMARY
- Process-lifetime `mail_summary` accumulation (Portage module-global `_items`); finalized once at process exit (atexit `finalize`).

## fn mail_summary_accumulate
- Ports `mod_mail_summary.process`: stash `header + fulltext` for `key`, snapshotting the three settings on first use (Portage's `setdefault` keeps the first config for the whole run). First accumulation arms the exit-time finalize (atexit registration); `Once` keeps one registration per process. SAFETY: plain function pointer, no captures; the handler only locks `MAIL_SUMMARY` and does I/O, and process exit (not `_exit`/abort) is the only path that runs it -- the same contract Portage's `atexit_register` relies on.

## fn mail_summary_finalize
- Ports `mod_mail_summary.finalize` (via `_finalize`): one multipart mail for every accumulated package (or nothing when no package produced messages -- Portage returns early on zero items). Always clears the accumulation, sent or not. Attaches one `TextMessage` per package (its own `header + fulltext`), each base64'd like the lead part.

## fn mail_summary_atexit
- `extern "C"` entry for `libc::atexit` -- see `mail_summary_accumulate`. Must not unwind (that would abort); every fallible step is already `Option`/`Result`-guarded inside `mail_summary_finalize` (lock failure returns silently, delivery errors print).

## fn save_modules_process
- Ports `mod_save` / `mod_save_summary` / `mod_syslog` / `mod_custom` / `mod_mail` / `mod_mail_summary` for one merged package: build the per-module `fulltext` from `all_msgs` (unfiltered `collect_all`) and hand it to whichever modules are enabled. Each module's own `filter_loglevels` runs here (`save_summary`'s default token carries `:log,warn,error,qa`). A module is skipped for this package when the filter leaves nothing (Portage's `if len(mod_logentries) == 0: continue`). Returns the paths written (`save`/`save_summary`/`custom`; `syslog`/`mail`/`mail_summary` write no file); the caller prints nothing (Portage's `elog_process` discards the modules' return value, #427). `mail_summary` only accumulates here -- its single run-wide mail goes out from `mail_summary_finalize` at process exit (atexit `finalize`). Hands the mail module its filtered logentries; only the phases present matter (`${ACTION}` detection).

## fn process_batch
- Ports `elog_process` over a batch of packages that just merged or unmerged. Each item is `(cpv, t_dir)` where `t_dir` is that package's `${T}` (`${PORTAGE_BUILDDIR}/temp`). `phases` restricts which `${T}/logging/<phase>` files are read (Portage's `_elog_process(phasefilter=...)`): `None` after a merge (every phase), `Some(&["prerm", "postrm"])` for `dblink.unmerge()`. `root_display` is the `ROOT` string for the `echo` header (`/` gives the short form).
- The `save` / `save_summary` modules run immediately per package (Portage's `mod_save.process`), printing nothing (#427: the old `Elog messages ... written to ...` line existed in no Portage module); `echo` is accumulated for the call and printed by `echo_summary` at its end (Portage accumulates across the whole run and prints once at exit via `mod_echo._finalize`; portuale flushes per package before the post-merge clean, #42, so each merged package gets its own block). `mail` sends immediately per package (Portage's `mod_mail.process`); `mail_summary` accumulates and sends once at process exit (atexit `finalize`, armed on first use). A no-op when no module is enabled or nothing has messages. The caller hands in each package's `${T}/logging/` at Portage's own position -- for a merge that is inside `dblink.merge()` before the post-merge clean (backlog #42), see `ebuild_merge::process_merge_elog` -- rather than threading a message buffer through the (un)merge machinery. `split_elog` is Portage's `settings.features` `split-elog` token: the `emerge` callers pass the resolved list (#37 S3), the standalone unmerge path the raw env fallback.
- "save_any" gates the one `collect_all`/`save_modules_process` call below, so it needs every module that call also dispatches to -- `syslog`/`custom`/`mail`/`mail_summary` included, not just the `save`/`save_summary` pair its own name still refers to.

## fn format_echo_summary
- Pure formatter behind `echo_summary` (`mod_echo._finalize`), including the `* Log file: <path>` line when `pkg.logfile` is present (backlog #435).

## fn echo_summary
- Ports `mod_echo._finalize`: the `* Messages for package <cpv>:` block for every accumulated package, all message types on stdout, each line `<colour> * </colour><msg>` (`EOutput.e{info,log,warn,error,qawarn}`).

## tests
- `collect_reads_phase_files_in_order_and_filters_by_class`: default classes (`"log warn error"`) -- INFO/QA are dropped, and phase order (install before postinst) is honoured.
- Phasefilter test: a builddir portuale never cleaned -- install-time logs still sit next to the removal-time ones; `dblink.unmerge`'s `phasefilter=("prerm","postrm")` must ignore the install log.
- Echo-filter test: the echo-filtered variant drops the WARN only if classes say so; default classes keep it, so both survive here.
- Echo test: no colour -> plain `" * "` prefixes; capture is via a run in test_portuale.py, this just exercises the non-panicking path.
- Module-classes test: `save_summary:log,warn,error,qa` (the make.globals default token) overrides the bare `PORTAGE_ELOG_CLASSES` for that module only; `echo` has no override -> the bare default {LOG,WARN,ERROR}.
- Save tests: `save`: one `<cat>:<pf>:<stamp>.log`, `fulltext` only has the LOG line (QA filtered out by `PORTAGE_ELOG_CLASSES`); `save_summary`: its `:log,warn,error,qa` override keeps the QA line.
- Serial-test scoping: minimal process-env scoping for these serial tests -- `env::var` reads are process-global, so run one closure at a time (`temp_env` serializes the env side; every env write is `LOCK`-scoped).
- Mail test: `mail_process` + `mail_summary` accumulation/finalize through a fake sendmail binary: argv (`-f <from> <recipient>`) and the MIME bytes on stdin. One test function (not three) because `MAIL_SUMMARY` is process-global -- internal steps stay sequential. Drain any accumulation a parallel test left behind (finalize always clears, sent or not). One immediate `mail` for a merged package. Two packages into `mail_summary`: nothing is sent yet (one run-wide mail at finalize). Portage's summary substitutes only `${PACKAGE}` (the one/multiple-packages count) and `${HOST}` -- a `${ACTION}` in the template stays literal, exactly as Portage's `mod_mail_summary._finalize` leaves it. The lead part is base64 (like Portage), so the package list is asserted in its encoded form. Lead part + one MIME part per attachment (Portage attaches a `TextMessage` per package), on top of the first mail's own single part. Full MIME round-trip (decode each part) is covered end to end in test_portuale.py with Python's own `email` parser. Finalize with nothing accumulated sends nothing. Bodies are base64 even when pure ASCII (Python's `email` base64-codes even pure-ASCII UTF-8 bodies).
