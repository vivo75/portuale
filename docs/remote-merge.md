# Remote binary-package merge (`mrg` only): plan

Standalone planning document (deliberately **not** an edit to
`agent-context.md` / `scope-backlog.md` / `what-this-proves.md`, to avoid
rebase conflicts with `main`). Nothing here is implemented yet.

## 1. Goal and non-goals

Install portuale-built binary packages (`*.gpkg.tar`, and later `.tbz2`)
on a **client** machine over SSH from the **server** machine running
portuale, with **no Portage (and no portuale binary) on the client** and
**no internet on the client** — its only remote contact is the server.

- Resolution always happens on the **server** (repositories live only
  there), binary-only: `--getbinpkgonly` is mandatory, never a source
  build on the client.
- The four ebuild phases that a local binpkg merge runs --
  `pkg_pretend`, `pkg_setup`, `pkg_preinst`, `pkg_postinst` -- run **on
  the client**, against the client's own filesystem.
- The tarball reaches the client **through the SSH connection**
  (stdin stream), never via a client-side download.
- `mrg` only. `emerge` never learns these options (same split as
  `--solver`'s Rust-only status and `mrg`'s own portuale-only standing:
  no Python reference implementation).
- New options are all prefixed `--remote-` (e.g. `--remote-hostname`).

Non-goals for v1: source builds on the client (forever out -- the
client has no toolchain by design), multi-client fan-out in one
invocation (one `--remote-hostname` per run; orchestration loops in
shell), privilege escalation on the client beyond the SSH login user
(no sudo/become layer -- the remote user must own the target ROOT),
gpkg `.sig` verification on the client (server verifies at download
time; the SSH channel is the integrity boundary -- see §9).

## 2. Terminology

- **server**: runs `portuale mrg`; holds repos, `$PKGDIR`, config,
  the SSH client binary. Builds nothing new during a remote run when
  the binpkgs already exist (plain `--getbinpkgonly` semantics).
- **client**: receives and merges. Has **bash ≥ 5.3**, POSIX `/bin`
  (`tar`, `mkdir`, `rm`, `cat`, `chmod`, `ln` -- see §6, deliberately
  no `bzip2`), an SSH server, and
  nothing else. Never Python (the hard difference from Ansible).
- **unit**: one resolved `Binary` `GraphEntry` = one tarball + one
  client-side merge, executed in the resolver's merge order.

## 3. Grounding in real code

Local binpkg merge today (`rust/portuale/src/emerge_getbinpkg.rs`,
`rust/portuale/src/ebuild_merge.rs::merge_binpkg`):

1. `merge_one_binary_entry`: `AlreadyInstalled` is a no-op; else locate
   the binpkg (local `$PKGDIR` or `download_and_verify` remote:
   `Packages` `SIZE` + `MD5`) and call `merge_binpkg`.
2. `merge_binpkg` peeks embedded metadata (`CATEGORY`/`PF`/`SLOT`/
   `repository`), extracts image + `build-info/` into
   `${PORTAGE_TMPDIR}/portage/<cat>/<pf>` (`image/` = `${D}`,
   `build-info/` carries `CONTENTS`, the ebuild, `environment.bz2`,
   `DEFINED_PHASES`, per-class `*DEPEND`), then in real
   `_emerge/Binpkg` + `treewalk()` order: `pkg_setup` →
   collision/protect-owned check → `pkg_preinst` → copy image→`${ROOT}`
   (`merge_tree`, CONFIG_PROTECT-aware) → vdb write
   (`write_vdb_entry_from_dir`) → same-slot replace (old version's
   `pkg_prerm`/`pkg_postrm` from its vdb env) → `pkg_postinst`
   (non-fatal) → `env_update`.
3. Hooks run via `ebuild_phases::run_phase_from_saved_env`
   (`ebuild_phases.rs:2650`, `pub(crate)`): ebuild file +
   decompressed `environment.bz2` + local dirs + a shell backend
   (`bash` default) + `EMERGE_FROM=binary`. Gated on `DEFINED_PHASES`;
   a binpkg defining no hooks spawns no shell at all.
4. `pkg_pretend` does **not** run in a local binpkg merge today (it runs
   at resolve time on the server). Requirement §1 adds it on the client,
   so the remote flow is pretend → setup → preinst → copy+vdb →
   postinst per unit.

Option plumbing precedent: `mrg.rs`'s `OPTIONS` table + `Kind::Value`
with `choices` + `emerge_handles()` + `to_emerge_argv` forwarding
(`--solver=` is the template: clap validates, the emerge codepath
executes). `--getbinpkgonly` already forces binary-only resolve
(`usepkgonly`), so the remote run reuses the whole resolve path
unchanged and only swaps the *execution* of `run_merge_plan`'s per-entry
closure.

State today: no SSH/remote code exists anywhere (`ssh://` appears only
as a binhost `sync-uri` scheme); no repo-position history DB exists
(`mtimedb.rs` is resume-list only) -- both are new (§7, §10).

## 4. Ansible: what to steal, what to reject

References (read 2026-09-07): `ansible.builtin.ssh` connection plugin
docs, the shell-plugin index, and the `executor/` + `play_context.py`
layout named in the request.

Steal:

- **Transport = the system `ssh` binary, not a library.**
  `ansible.builtin.ssh` is explicitly "mostly a wrapper to the `ssh`
  CLI"; behavior follows the tool. Same for us: zero new Rust
  dependencies (no libssh -- the musl-static story and the
  near-zero-deps discipline both survive), server needs OpenSSH client,
  multiplexing via `-o ControlMaster=auto -o ControlPersist=60s`
  (ansible's own default `ssh_args`) so N units share one TCP
  connection + one authentication.
- **Pipelining over file transfer where possible.** Ansible's
  `pipelining=true` executes modules without file transfers; our
  analogue: the per-unit driver script goes over `ssh bash -s`
  (stdin), and the tarball streams through the *same* stdin channel
  (`tar` reading a heredoc-style concatenated stream) instead of an
  scp/sftp round-trip. `sftp` stays the fallback transfer method only
  if stdin streaming proves unworkable (ansible's `smart` order is
  sftp > scp > piped; ours is piped > sftp, because the client must
  not need an sftp server beyond sshd's internal-sftp -- usually
  present, but not guaranteed).
- **`play_context`-style separation.** Ansible splits *connection
  parameters* (play_context: host/user/port/key/become) from *task
  execution* (executor: queue, per-host state). Mirror it: one
  `RemoteContext` struct (all `--remote-*` values, validated once)
  threaded through, and one `run_remote_plan` executor consuming the
  already-resolved `Vec<GraphEntry>` -- resolution never knows the
  target is remote.
- **Return-code discipline.** Ansible treats ssh rc 255 as *transport
  error* (vs. remote command failure). Adopt verbatim: 255 (and ssh
  fatal stderr signatures) → "client unreachable", abort the plan;
  any other non-zero → the unit failed (keep-going drops dependents,
  same `run_merge_loop` policy as local).
- **Shell quoting model.** `ansible.builtin.sh` = POSIX `/bin/sh`
  quoting. Our client guarantees bash ≥ 5.3, so generate
  `bash -s` payloads with `printf %q`-style single-quote escaping done
  server-side in Rust (one `sh_quote` helper, unit-tested against
  adversarial filenames) -- never string-interpolate client paths.

Reject:

- **Python on the client.** Ansible ships modules + `AnsiballZ`
  self-extracting zips executed by remote Python. Our client agent is
  **generated bash** (one script, `set -u`, no arrays-of-doom, no
  process substitution -- only constructs POSIX sh *plus* the few
  bashisms we gate on the §6 preflight). No interpreter bootstrap,
  ever.
- **Become/privilege escalation.** Out for v1 (§1): the SSH user must
  already own the target ROOT (typically root via key). A `--remote-
  become` is a named future slice, not a silent addition.
- **Facts gathering.** Ansible's `setup` module harvests the client.
  We harvest exactly one fact set in preflight (§6) and take the rest
  (USE, keywords, acceptance) from the *server-side* config targeting
  the client (see §7).

## 5. Architecture

```
server (portuale mrg)                          client (bash 5.3+, no net)
─────────────────────                          ─────────────────────────
resolve --getbinpkgonly (local, unchanged)
        │
for each Binary GraphEntry, in merge order:
  locate binpkg ($PKGDIR / binhost, SIZE+MD5)         ┌─ one multiplexed
  build unit bundle: image.tar + build-info/ ──stdin─▶│  ssh connection
        + remote-manifest (§6)                        │  per invocation
  ssh bash -s < driver.sh  ──status/log──▶ stdout/err │
  record vdb-shadow + repo ledger (§7)                └─ workdir per unit
report merged/failed/skipped (keep-going policy)
```

`run_remote_plan` replaces `run_merge_plan`'s per-entry closure when
`--remote-hostname` is present (refusing `--pretend` combinations that
make no sense is unnecessary -- `--pretend` stays purely local and
ignores `--remote-*`, printing the same plan it always did; the remote
flags only take effect on a real run, mirroring how `--shell` is inert
under `--pretend`).

## 6. The client-side driver (generated bash)

One self-contained script, rendered server-side per unit with all values
pre-quoted. Stages, each emitting `portuale-remote: <stage> <rc>` status
lines on a dedicated fd (stderr stays the human log):

0. **Preflight** (once per invocation, not per unit): `bash --version`
   ≥ 5.3 (`BASH_VERSINFO`, same gate style as `bin/ebuild.sh`'s own
   `__check_bash_version`), `tar`/`mkdir`/`rm`/`cat`/`chmod`/`ln`
   present, workdir creatable, target ROOT writable, `${ROOT}/var/db/pkg`
   reachable per §7 placement. **Clock check**: `date +%s` on both
   sides; `|server - client| > 900s` aborts the run (see §5.5 for why
   time matters; `--remote-max-clock-skew` overrides). Any failure →
   transport-ok/unit-fatal before touching anything.
1. **Receive**: `tar -x` reads the bundle off stdin into
   `<workdir>/<cat>/<pf>/` (`image/`, `build-info/`,
   `remote-manifest`, driver already resident). No network, no second
   connection.
2. **Hooks from the binpkg's own files** (same gating as local:
   `DEFINED_PHASES` + ebuild + `environment.bz2` present): `pkg_pretend`
   (new vs. local -- a non-zero pretend aborts *this unit before any
   mutation*), `pkg_setup`, `pkg_preinst` with `${D}=…/image`,
   `${T}`, `${ROOT}`, `${EROOT}` exported exactly as
   `run_phase_from_saved_env` sets them (`EMERGE_FROM=binary` too).
3. **Copy + vdb**: collision/protect-owned check against the client's
   live vdb (or the server-kept shadow when the vdb lives server-side
   -- §7), CONFIG_PROTECT-aware copy per the manifest, `CONTENTS`
   recording, vdb entry write, same-slot replace with the replaced
   version's own `pkg_prerm`/`pkg_postrm` (from *its* vdb env, which
   must therefore also be on the client -- forces vdb placement for
   replaced versions, see §7), `pkg_postinst` (non-fatal, local rule
   kept), `env_update` equivalent (`ldconfig -r` + `env-update`
   if present; best-effort, logged).
4. **Report**: per-unit `STATUS=merged|failed|skipped:<reason>` trailer
   + exit code (0 merged-or-skipped, 1 unit-failed, 255 transport --
   never produced client-side, reserved for ssh itself).

Client tool floor (preflight-enforced): `bash` ≥ 5.3, GNU-or-BusyBox
`tar` (no `--selinux`/`--xattrs` flags used -- portable subset only),
`mkdir`/`rm`/`cat`/`chmod`/`ln` from POSIX `/bin`. Deliberately **no
`bzip2`**: the server pre-decompresses `environment.bz2` (the exact
`bzip2 -dc` step `run_phase_from_saved_env` already runs locally) and
ships a plain `environment` file in the bundle -- start uncompressed,
revisit wire compression (e.g. zstd) only if bundle sizes ever justify
a new client tool. Everything else (digests, merge math) is computed
server-side and shipped in the manifest.

## 7. Configuration placement (server vs. client)

Every path below is settable, and each states *where it lives*. Prefix
rule: `--remote-etc-portage=<server:/path|client:/path>`,
`--remote-vdb=<...>`, `--remote-edb=<...>` (default: everything
client-side at its real location -- the least surprising mapping).
The server always reads the *effective* config for resolution from the
designated side before resolving (client-side files are pulled once
over the multiplexed connection at plan start, cached server-side for
the run; server-side files are read directly).

| Data | Default | Notes |
|---|---|---|
| `/etc/portage` (profiles, `package.*`, `make.conf`) | client | Resolution models the *client*. Pull-once-per-run keeps N units to one transfer. |
| `/var/db/pkg` (vdb) | client | Required client-side whenever a same-slot replace can run another version's `pkg_prerm/postrm` (its env lives in the vdb). Server keeps a **shadow copy** (pulled at plan start) purely for the collision/ownership pre-check, so a doomed unit fails before shipping its tarball. |
| `/var/cache/edb` (binhost `Packages` cache) | server | The client has no binhosts; the server resolves `--getbinpkgonly` against its own cache. Never shipped. |
| target `${ROOT}` | client `/` | `--remote-root` overrides (default `/`). The driver prefixes every mutation; nothing ever touches the server's `/`. |
| `${PORTAGE_TMPDIR}` work area | client `/var/tmp/portage-remote` | `--remote-workdir`; removed per unit unless `--remote-keep-workdir`. |
| repo position ledger | **both** | §8. |

Mixed placements (e.g. vdb on server for a stateless client image) are
allowed by the option matrix but degrade honestly: no client vdb ⇒ no
same-slot `prerm/postrm`, no collision-vs-owner check (fail-closed on
any collision instead), recorded in the unit report.

## 8. Repo position ledger (both sides remember)

Repositories stay on the server, always. At install time both sides
record *which* repos the resolved binpkgs were built from:

- Per unit installed: `(repo_name, commit_hash, commit_timestamp)`
  appended to a ledger; each side keeps the **last 10** ledger entries
  (append + trim -- same rotation shape as `mtimedb["resume"]`).
- Server ledger: alongside `$PKGDIR` (or `--remote-ledger-dir`);
  client ledger: under the vdb side (`<vdb>/../remote-repos` or the
  server-shadow when the vdb lives server-side -- §7).
- Format: one line per install,
  `<timestamp> <repo> <commit> <cpv>…` (human-greppable, no new
  serialization format to version).
- The `Packages` index the server resolved against already pins the
  binpkg bytes; the ledger pins the *provenance story* ("this client
  state came from server repos at these commits"). No enforcement on
  mismatch in v1 (a `--remote-require-ledger-match` strict mode is a
  named future slice), but the preflight prints the client's last
  ledger line for the operator.

Commit hash source: `git -C <repo> rev-parse HEAD` + `git show -s
--format=%ct` at bundle-build time (repos are git checkouts in
practice; a non-git repo records `unknown` + mtime -- same honesty
rule as elsewhere).

## 9. Security model

- **Client trusts the server completely** (it executes server-rendered
  bash as the login user, typically root). No sandboxing on the
  client beyond "only the target ROOT is writable and only unit files
  are written" -- stated, not enforced, in v1.
- **Server trusts the client's status lines partially**: exit codes +
  `STATUS=` trailers drive the report, but the *merge record*
  (ledger append, world-file updates if any) is keyed off them --
  a lying client can desync the ledgers. Acceptable: the threat model
  is accidents and drift, not a malicious client (documented).
- SSH: key auth only (no passwords, no `sshpass` -- cf. Ansible's
  password-mechanism matrix, which we collapse to one row);
  first contact uses **trust-on-first-use**: `StrictHostKeyChecking=
  accept-new` (new hosts are added automatically *and their fingerprint
  is printed*, changed keys still abort) -- a plain `yes` would fail on
  every new machine, which is the common case for provisioning;
  full `yes` remains one flag away
  (`--remote-strict-host-key-checking=yes`); multiplexed socket in
  `~/.portuale/cp` with `0700` (`ControlPersist=60s`).
- Digests: server verifies `SIZE`+`MD5` at download (existing
  `download_and_verify`) and the gpkg internal `Manifest` before
  bundling; the SSH channel (not the client) is the last integrity
  boundary -- the client re-checks the streamed byte count against
  the manifest (cheap, catches truncation, not malice).

## 10. Option surface (`mrg` only)

All `Kind::Value` (or `Flag`) entries in `mrg.rs::OPTIONS`, forwarded
through `to_emerge_argv` to a new `remote.rs` executor (not through
`pretend.rs`'s resolver -- resolution is shared, execution branches).
Required values are clap-required; unknown values exit 2 via clap.

| Option | Kind | Default | Meaning |
|---|---|---|---|
| `--remote-hostname` | Value (required) | — | client host (ansible `host`). Presence selects remote execution. |
| `--remote-user` | Value | current user | SSH login user (ansible `remote_user`). Must own target ROOT. |
| `--remote-port` | Value | 22 | SSH port (ansible `port`). |
| `--remote-key-file` | Value | agent/defaults | private key (ansible `private_key_file`); `ssh-agent` recommended, same as Ansible's note. |
| `--remote-root` | Value | `/` | client `${ROOT}`. |
| `--remote-workdir` | Value | `/var/tmp/portage-remote` | client per-unit work area. |
| `--remote-keep-workdir` | Flag | off | keep failed units' workdirs for forensics. |
| `--remote-etc-portage` | Value | `client:/etc/portage` | `server:<path>` pulls from server instead (§7). |
| `--remote-vdb` | Value | `client:/var/db/pkg` | `server:<path>` = stateless-client degrade (§7). |
| `--remote-edb` | Value | `server:<edb-cache>` | informational; always server. |
| `--remote-timeout` | Value | `10` | ssh connect timeout (ansible `timeout`). |
| `--remote-max-clock-skew` | Value | `900` | abort if `|server - client|` clock differs by more seconds (`0` disables; §5.5). |
| `--remote-ssh-args` | Value | `-o ControlMaster=auto -o ControlPersist=60s` | passthrough (ansible `ssh_args`). |
| `--remote-strict-host-key-checking` | Value `accept-new\|yes\|no` | `accept-new` | TOFU by default (new key added + fingerprint printed, changed key aborts); `yes` for locked-down fleets, `no` only for throwaway labs (§9). |
| `--remote-jobs` | Value | `1` | units in flight (v1: sequential only; the knob exists so the future parallel slice needs no CLI change). |

`--getbinpkgonly` is **forced**: a remote run without it (or with a
plan containing any non-`Binary` entry) is a usage error, exit 2 --
"No source build on client" is enforced at the CLI layer, not
discovered mid-merge. `--pretend` stays local and ignores `--remote-*`
(prints the shared plan; documents what *would* ship).

### 5.5. Fail-early ordering (no partial mutation on predictable errors)

Gates run in this order; each stage passes only if every check in it
passes, and **nothing on the client changes before stage 4**:

1. **Server config sanity** (local, before any connection): remote
   options validate (hostname present, key file readable, `server:`/
   `client:` placements well-formed), `--getbinpkgonly` in effect.
2. **Client config pull + parse**: `/etc/portage` (per §7 placement)
   is pulled and must parse as a usable resolver config; the client
   preflight (§6.0, incl. the clock check) passes. A dead host
   (rc 255), an old bash, an unwritable ROOT, or >900s clock skew
   aborts here -- zero client writes so far.
3. **Resolve + verify, still read-only**: `--getbinpkgonly` resolve
   against the pulled config; every `Binary` entry's tarball located
   (`$PKGDIR` or binhost) with `SIZE`+`MD5` verified; ledgers readable
   both sides. The first failure aborts the whole plan (or, under a
   future keep-going mode, drops just that unit -- still pre-mutation).
4. **Per unit, mutation order**: stream bundle → byte-count check →
   `pkg_pretend` (aborts *this unit* pre-copy) → setup → preinst →
   collision check → copy+vdb → postinst. A failure anywhere stops
   that unit; earlier units in merge order are already live (same
   atomicity story as a local `run_merge_loop`, no more).

Why the clock check gates stage 2: CONTENTS records mtimes (unmerge's
`!mtime` staleness logic), the vdb compares `BUILD_TIME`s, and both
ledgers timestamp installs -- a >15min skew silently corrupts all
three. The 900s bound is `--remote-max-clock-skew` (seconds, `0`
disables -- only for labs with no NTP).

## 11. Bidirectional communication

- **Server→client**: multiplexed stdin (driver + bundle stream) and
  `ssh` argv (one `bash -s` per stage-group; preflight separate so a
  failed gate never consumes a bundle).
- **Client→server**: stdout = the merge log lines (`>>>`/`!!!`
  family, prefixed per unit so interleaving stays readable when
  `--remote-jobs` grows); stderr = status channel
  (`portuale-remote: <stage> <rc|bytes>` + final
  `STATUS=merged|failed|skipped:<reason>`); exit code = unit fate
  (0/1; 255 reserved for ssh itself, §4). No structured RPC, no
  second connection, no client-initiated traffic -- "bidirectional"
  means full-duplex over the one channel, and the server never blocks
  waiting on a client prompt (all questions are CLI flags).

## 12. Testing strategy (no Python reference -- `mrg` rules)

- **Client fixture: a podman container.** Specialize
  `TEST/create-container.bash` (today builds `localhost/test-portuale`
  from a Gentoo stage3 via buildah) into a client variant: stage3 +
  `openssh` + `bash` 5.3 check + a seeded test key + `sshd` on a
  loopback port = a disposable, reproducible "new machine" for
  first-contact (TOFU), preflight-reject (old bash / missing tar /
  unwritable ROOT / skewed clock via `date -s` in the container), and
  full-merge tests. Lean: script specialization in-tree (same
  reproducibility story as the existing image); a hand-built image
  from the operator stays a fallback, not the primary path.
- **Transport tests**: against that container (skip-gated when podman
  is absent): preflight accept/reject matrix, rc-255 mapping
  (unroutable host / killed sshd), byte-exact stdin streaming, TOFU
  fingerprint print + changed-key abort.
- **Driver tests**: a `--remote-transport=local` mode executing the
  *same generated driver* against a local directory ROOT -- every
  phase-order/CONTENTS/vdb/CONFIG_PROTECT assertion runs without SSH
  (this mode is also the offline debugging story). Fixture binpkgs
  (`packagepkg`, `binpkgphasepkg`, `binpkgrmpkg` -- the hooks already
  assert `setup→preinst→postinst` order locally) get remote twins.
- **Placement matrix**: `server:`/`client:` combos for vdb +
  etc-portage over the local transport, including the stateless-client
  degrade (fail-closed collision, skipped prerm/postrm, report notes).
- **Ledger tests**: last-10 rotation on both sides, `unknown` commit
  honesty for non-git repos.
- Rust unit tests for `sh_quote`, status-line parsing, option
  validation; contract-suite untouched (`--pretend` output identical
  with or without the flags present).

## 13. Slice plan (each: fixtures + tests + docs + full verify pass)

1. **Surface + transport**: `--remote-*` options in `mrg.rs`
   (choices/required/exit-2), `RemoteContext`, multiplexed `ssh`
   spawn, preflight-only run (`bash ≥ 5.3`, tool checks, clock skew,
   TOFU host-key path), rc-255 mapping, server-side option/config
   sanity gates (§5.5 stages 1-2). No payload yet.
2. **Streaming**: bundle build (image.tar + build-info/ +
   remote-manifest) + stdin transfer + client unpack + byte-count
   check, `--remote-transport=local` for tests.
3. **Phases on client**: pretend → setup → preinst from the bundle's
   own files, `DEFINED_PHASES`-gated, log streaming.
4. **Merge + vdb on client**: collision check, CONFIG_PROTECT copy,
   CONTENTS/vdb write, same-slot replace with old hooks, postinst,
   env_update equivalent.
5. **Placement + ledger**: `server:`/`client:` matrix, vdb shadow,
   stateless degrade, last-10 ledger both sides, `--getbinpkgonly`
   enforcement.
6. **Report + keep-going**: per-unit trailers, merged/failed/skipped
   summary, `--remote-jobs` still sequential (knob reserved).

## 14. Open questions (re-open, don't silently default)

1. ~~`environment.bz2` needs `bzip2` on the client for hook envs --
   keep the requirement, or have the server pre-decompress (bigger
   bundle, fewer client tools)?~~ **Decided**: server pre-decompresses
   (the `bzip2 -dc` step it already runs), bundle ships a plain
   `environment` file; no `bzip2` on the client (§6). Wire compression
   (zstd) only if bundle sizes ever justify it.
2. gpkg-only v1, or `.tbz2` (xpak) from the start? The merge code
   handles both locally; remote manifest should too -- but each format
   doubles driver test paths. Lean: gpkg first, xpak second slice.
3. Same-slot replace when the old version's vdb env is missing
   client-side: fail the unit, or merge without old hooks (local code
   degrades)? Lean: fail-closed (matches collision stance).
4. Ledger strict mode (`--remote-require-ledger-match`) now or later?
   Lean: later (named, §8).
5. `env_update`/`ldconfig` on exotic clients (no `ldconfig`)? Lean:
   best-effort + logged, never fatal (matches postinst stance).
