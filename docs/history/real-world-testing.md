# Real-world testing: Portuale vs Portage under `podman`

> **Historical planning doc.** L0 + L1 are **shipped and run live** —
> `TEST/README.md` is the current operating guide, `TEST/findings/` the
> results. The execution-useful content (determinism controls, triage
> guidance, archive-comparison and L2–L5 designs, risks, metrics) now
> lives in [`../real-world-testing.md`](../real-world-testing.md);
> what remains here — the §1 methodology critique, image specs, and
> slice history — is reference only. L2–L5 status is tracked in
> `scope-backlog.md`.

Goal: a standing, repeatable, container-based differential test bed that
exercises the *execution* side (build, package, merge, unmerge, binhost
serve, remote merge) against real ebuilds — the `emerge --pretend`
contract suite covers the resolver. Slice plan in §14.

---

## 1. Honest critique of the proposed plan

The request sketches: a "builder" container that produces `.gpkg.tar` and
serves them over HTTP; a "consumer" container that installs them; a
Portage and a Portuale variant of each; an `mrg` remote-install
container; and "a binary comparison of differences" after every
operation, mined for bugs.

The intent is right. Several details will not survive contact with
reality and should change before we build anything:

### 1.1 Byte-identical binary packages are an unattainable bar

A `.gpkg.tar` (and a `.tbz2`/xpak) embeds `BUILD_TIME`, `BUILD_ID`,
per-file mtimes, tar member ordering, compression-level and
compressor-version headers, and xattrs. **Two runs of Portage itself do
not produce byte-identical archives**, let alone Portage vs Portuale. A
raw `cmp` of the tarballs will always differ and tells us nothing.

What actually carries signal, in descending order of value:

1. The **installed image** — the `$ROOT` filesystem after merge
   (normalised: see §4).
2. The **VDB entry** — `/var/db/pkg/<cat>/<pf>/` (`CONTENTS`, `NEEDED`,
   `environment`, the consolidated `metadata` file, `*DEPEND`, `USE`,
   `IUSE_EFFECTIVE`, `INSTALL_MASK`, …). The memory log shows this
   directory has *already* produced ~5 distinct Portuale bugs
   (`gpkg-vdb-entry-fix`), so it is the single highest-yield artefact.
3. The **resolver decision** (`emerge -p` output) at real-tree scale.
4. The **`Packages` binhost index** entries.
5. The **archive structure** — member list, paths, modes, the
   `metadata`/`build-info` contents — compared *structurally*, never by
   `cmp`.

So: reframe "binary comparison" as **"normalised filesystem + VDB +
metadata comparison against a documented normalisation ruleset, plus a
checked-in allowlist of justified divergences."**

A raw `cmp` is noise, but a *normalised* archive comparison is still
worth having as a distinct, cheap check — see §4.7: a small
`TEST/compare/gpkg-diff.sh` that unpacks two `.gpkg.tar` (or `.tbz2`)
into scratch dirs, drops the volatile fields (`BUILD_TIME`, `BUILD_ID`,
`COUNTER`, mtimes, `Manifest` hashes), and diffs the trees + `build-info/`.
For deep structural insight into *why* two archives differ, use
[`diffoscope`](https://diffoscope.org/) — it already recurses into tar,
gzip/zstd, ELF, and ar members and renders a readable nested diff; run it
builder-side (it is Python, so not on the `mrg` client) as the
investigation tool once `gpkg-diff.sh` flags a mismatch.

### 1.2 Compiler nondeterminism will swamp a build-vs-build diff

If the Portage builder and the Portuale builder each *compile* the same
package, the resulting binaries differ for reasons that are almost never
a package-manager bug: `-frandom-seed`, absolute build paths baked into
debug info, `__DATE__`/`__TIME__`, Go/Rust build IDs, `.pyc` header
timestamps, ar/linker ordering, parallel-make race in generated files.
Chasing those is a sink.

**Sharper design: build each package exactly once (with Portage, the
reference), publish it to the binhost, then have *both* a Portage
consumer and a Portuale consumer merge that identical prebuilt
archive.** Now the only variable is the merge path, and every `$ROOT` or
VDB diff is a real Portuale bug. This is L1 below and should be the
workhorse.

Portuale-as-builder is still worth testing (L2/L3) — but its yardstick
is "does it build and package at all, is the archive structurally valid,
can Portage install what Portuale packaged" — not a byte diff against a
Portage-built archive.

### 1.3 The `mrg` client *can* be compared directly

The request says a direct comparison "may not be possible" for the
remote-install container. It is: don't compare the `mrg` client against
Portage — compare it against a **reference local Portuale binpkg merge**
of the same atom, on a twin container built from the same stage3. Diff
the two `$ROOT`s + VDB exactly as in L1. The client stays Portage-free
and Python-free (that is the whole point of `mrg` — see
[`remote-merge.md`](remote-merge.md) §12), and we still get a
byte-level yardstick.

### 1.4 "One image with both" is right for most roles, wrong for the `mrg` client

A shared `test-portuale` image (pinned Portage + mounted Portuale
binaries) is the correct base for the builder and consumer roles. But
the `mrg` client's central claim is "no Portage, no Python, nothing but
bash ≥ 5.3 + POSIX `/bin` + sshd". If the shared image has Python in it,
the client test proves nothing. The `mrg` client needs its **own minimal
image**. Two images total (§3).

### 1.5 Determinism has to be engineered, not hoped for

Even L1 (merge from identical archive) needs both consumers to agree on
profile, `USE`, `ACCEPT_KEYWORDS`, `CONFIG_PROTECT`, `INSTALL_MASK`,
`FEATURES`, locale, umask, and the ebuild-repo commit — and must never
`emerge --sync`. L3 additionally needs `SOURCE_DATE_EPOCH`, `--jobs=1`
for generated-file races, fixed `MAKEOPTS`, and a frozen system clock
where practical. See §8.

### 1.6 Version pinning is load-bearing

Portuale mirrors `=sys-apps/portage-3.0.82.2` exactly. The differential
is only meaningful against **that** Portage build. `00-install-portage.sh`
staying lexicographically first is not a nicety — it is a correctness
precondition for every downstream comparison. Likewise the `gentoo` and
`buildovl` repo commits are pinned in `create-container.bash` and must
stay frozen.

### 1.7 Scale vs feedback loop

A full `@world` source build is hours-to-days and tens of GB. "Resources
are secondary" is fine for a soak run, but if the *only* test is a
multi-hour build nobody runs it per change and regressions rot in. Tier
it: L0/L1 finish in minutes and gate every change; L3 is a weekly soak.
A persistent binpkg-cache volume makes re-runs cheap.

### 1.8 Rootless podman + Portage sandbox interaction

Nested user namespaces + `FEATURES="sandbox usersandbox pid-sandbox
network-sandbox userpriv"` is fragile. `network-sandbox` wants
`CAP_NET_ADMIN` / a private netns that rootless podman may not grant;
`userpriv` needs the `portage` uid/gid mapped inside the container's
sub-uid range. Expect to need `--security-opt seccomp=unconfined`
(already in the example), possibly `FEATURES="-network-sandbox"` in the
rootless variant, and an explicit `RESTRICT=network-sandbox` fallback.
This is a known risk, not a blocker (memory:
`sandbox-build-isolation-complete`). **Decision (§1.10): use root
containers wherever they give a cleaner test** — the builder and
source-parity roles run rootless only if it turns out to be
frictionless, else `sudo podman` with the full `FEATURES` set intact.

### 1.9 Minor

- The applet is **`mrg`** (module `mrg.rs`); the request and one origin
  branch say `mgr`. Typo — use `mrg`.
- Portuale is a multicall binary: `emerge` / `ebuild` / `mrg` are
  symlinks next to `portuale` (README). The images mount
  `rust/target/release` on `/usr/local/bin` and rely on those symlinks
  existing.

### 1.10 Decisions taken (2026-09-08 review)

The user reviewed §1.1–§1.9 and settled:

1. **§1.1** — accepted. *Additionally*: build small purpose utilities for
   comparing like resources (the `gpkg-diff.sh` "unpack both, strip
   `BUILD_TIME`/`BUILD_ID`, diff trees" tool), and lean on the existing
   [`diffoscope`](https://diffoscope.org/) for deep structural
   investigation. Both fold into §4 (the normalised `$ROOT`+VDB
   comparison stays the primary gate; `gpkg-diff.sh` + `diffoscope` are
   the archive-level check and the drill-down tool).
2. **§1.2 / §1.3 / §1.4** — accepted as written. One image was always a
   possibility, never a requirement, so the two-image split is fine.
3. **§1.5 / §1.8** — **root containers are acceptable** if they produce a
   better test. No need to bend the design around rootless podman;
   prefer the configuration that keeps `FEATURES` / sandboxing / uid
   mapping realistic and comparable.
4. **§1.9** — `mrg` typo confirmed.

---

## 2. What we are actually testing for

| Class | Artefact | Detects |
|---|---|---|
| Resolution | `emerge -p …` stdout/stderr, exit code | wrong pkg/slot/USE choice, wrong merge order, missing/extra graph nodes, bad REQUIRED_USE handling |
| Merge | `$ROOT` filesystem, `env_update` output, `ldconfig` cache | wrong modes/owners/xattrs, CONFIG_PROTECT mistakes, collision handling, symlink/hardlink handling, INSTALL_MASK, docompress |
| VDB | `/var/db/pkg/<cat>/<pf>/` | CONTENTS format/paths, NEEDED/soname data, environment capture, metadata consolidation, COUNTER, *DEPEND recording |
| Packaging | `.gpkg.tar` / `.tbz2` structure, `build-info/`, embedded Manifest | archive layout, metadata segment, multi-instance BUILD_ID layout, MD5 in index |
| Binhost | `Packages` index, HTTP serve, client fetch/verify | index field parity, SIZE/MD5, `--binpkg-changed-deps`, signature fields |
| Lifecycle | unmerge, depclean, preserved-libs, `@preserved-rebuild`, `--resume`, news | orphan cleanup, preserve/rebuild logic, resume replay, news `.unread` write-back |
| Remote (`mrg`) | client `$ROOT` + VDB, ledgers, phase order | everything in `remote-merge.md` §12 |

---

## 3. Images and container roles

### 3.1 Image A — `localhost/test-portuale` (shared)

Extends today's `TEST/create-container.bash` output:

- stage3-amd64-systemd, frozen date `2026-08-23`.
- `gentoo` + `buildovl` repos at pinned commits, **plus** a new
  `porttest` overlay (§7) baked in.
- `make.conf` as today (FEATURES line, `USE`, `PYTHON_SINGLE_TARGET`,
  repos.conf).
- **Stable** Portage in the image; `00-install-portage.sh` upgrades to
  `=sys-apps/portage-3.0.82.2` on first boot.
- Portuale binaries **not** baked — mounted read-only on
  `/usr/local/bin` from `rust/target/release` (so a rebuild on the host
  is picked up without an image rebuild), with `emerge`/`ebuild`/`mrg`
  symlinks pre-created by the build step.
- `/init` (the existing PID-1 script runner) drives
  `/TEST/scripts/NN-*.sh` in lexicographic order.
- Adds: `app-misc/jq` or a static `busybox` for the snapshot tooling;
  `dev-vcs/git` already present; a small static `sha256`/`tar` is
  guaranteed by coreutils. Also `dev-util/diffoscope` (and its optional
  deps that matter here: `binutils`, `zstd`, `gzip`) for archive
  drill-down (§4.7) — builder/analysis roles only, irrelevant to the
  `mrg` client image.

Used for roles: **builder-portage**, **builder-portuale**,
**consumer-portage**, **consumer-portuale**, **mrg-server**,
**lifecycle** — differentiated by which `NN-*.sh` scripts and which env
(`env:PM=portage|portuale`) `/init` is handed.

### 3.2 Image B — `localhost/test-mrg-client` (minimal)

- Same stage3 tar, but stripped: **remove `dev-lang/python`, `sys-apps/portage`,
  `app-portage/*`** after unpack (or build from a `stage3` that never
  had them — simpler to `rm -rf` and prove absence in a preflight
  assertion).
- `net-misc/openssh` (sshd), a seeded authorized test key, `bash` ≥ 5.3,
  POSIX `/bin` (`tar mkdir rm cat chmod ln find sha256sum`),
  **deliberately no `bzip2`** (server pre-decompresses `environment`).
- `sshd` on a fixed port, host key baked so TOFU is stable across runs.
- No `/init` script loop — it just runs `sshd -D`.
- A preflight script (run from the server side or a probe container)
  asserts `! command -v python3 && ! command -v emerge`.

### 3.3 Networking

- A user-defined podman network `porttest-net` so containers resolve
  each other by name (`mrg-server`, `mrg-client`, `binhost`).
- The binhost HTTP server: run `app-admin/webapp` — no; just
  `python3 -m http.server` is banned on the client but fine on the
  **builder** (it has Python). Simpler and dependency-free: **`busybox
  httpd`** or `app-misc/thttpd`, serving `$PKGDIR` read-only on
  port 80. Document the exact server; it must send correct
  `Content-Length` and support `HEAD` (Portage's fetcher uses it).
- Offline enforcement: the consumer and `mrg-client` containers get
  `--network porttest-net` **only** (no default route to the internet)
  so an accidental `--sync` or distfile fetch fails loudly instead of
  silently working.

### 3.4 Volumes

| Volume | Mounted by | Purpose |
|---|---|---|
| `porttest-pkgdir` | builder (rw), binhost (ro), consumers (ro via HTTP only) | the shared `$PKGDIR` / `.gpkg.tar` store |
| `porttest-distfiles` | builder (rw) | distfile cache, so re-runs don't re-download |
| `porttest-bincache` | builder (rw) | ccache / persistent binpkg cache for fast soak re-runs |
| `$repo/TEST/scripts` | all | probe scripts (ro) |
| `$repo/TEST/logs` | all | log + snapshot output (rw, git-ignored) |
| `$repo/rust/target/release` | all Portuale roles | the binaries (ro) |
| `porttest-snapshots` | all | normalised `$ROOT`/VDB snapshots for cross-container diff |

---

## 4. Comparison methodology

The heart of the test bed. Lives in `TEST/compare/`.

### 4.1 `snapshot.sh <root> <out>`

Walks `<root>` and emits a deterministic manifest — one line per path,
sorted by path:

```
<path>\t<type>\t<mode>\t<uid>\t<gid>\t<size>\t<sha256|->\t<symlink-target|->\t<xattr:name=b64,...>
```

- `type` ∈ `f d l b c p s`.
- Directories: no size/sha.
- Symlinks: target recorded, no sha.
- mtimes **excluded** from the primary manifest; written to a parallel
  `mtimes.tsv` for a separate lenient check (§4.4).
- A second section dumps every `/var/db/pkg/*/*/` file verbatim except
  the normalised set (§4.3).

### 4.2 Snapshot scope

Primary: everything Portage's `CONTENTS` would track, plus `/etc`
(CONFIG_PROTECT targets), `/usr/lib*/**` (soname/preserve-libs),
`/var/db/pkg`. Excluded entirely: `/proc /sys /dev /run /tmp
/var/tmp /var/cache /var/log /var/lib/portage/home
/var/db/repos /usr/src /root/.cache`.

### 4.3 Normalisation ruleset (checked-in as `TEST/compare/normalize.md` + code)

Fields/files that legitimately differ and are blanked before diff:

- VDB: `BUILD_TIME`, `BUILD_ID`, `COUNTER` (allocation order differs),
  `INSTALL_TIME`, `NEEDED.ELF.2` ordering (sort), `environment`
  (decompress, then strip `SRANDOM`, `BUILD_TIME`, `EPOCHREALTIME`,
  `SECONDS`, PID-bearing paths `/var/tmp/portage/*/`, `T=`, `WORKDIR=`,
  hostname), `repository` whitespace.
- `Packages` index: `MTIME`, `BUILD_TIME`, `BUILD_ID`, entry ordering
  (sort by `CPV`).
- Filesystem: `ld.so.cache` (regenerated — compare presence + that
  `ldconfig -p` lists the same sonames, not bytes), `/etc/.pwd.lock`,
  `.keep*` files (presence only), `/usr/share/info/dir`,
  font/mime/gtk-icon caches, `.pyc`/`.pyo` (compare presence, not
  bytes — timestamp/hash embedded), `/var/lib/portage/world` (sort),
  `/var/lib/portage/config` (the CONFIG_PROTECT hash db — compare
  key set).
- Ownership: if the container runs the two PMs under different effective
  uid mappings, normalise `uid/gid` through a documented map (better:
  run both under the same mapping — §8).

Everything **not** on this list is a hard diff.

### 4.4 `diff.py <snap-a> <snap-b>`

Structured, categorised output — never a raw unified diff:

- `MISSING` — path in A, not in B (and vice-versa)
- `MODE` / `OWNER` / `XATTR` / `SIZE` / `CONTENT` — path in both, attr
  differs
- `SYMLINK` — target differs
- `VDB:<field>` — VDB metadata field mismatch
- `CONTENTS` — the `CONTENTS` file itself differs (line-level, typed by
  `obj`/`sym`/`dir`)
- `MTIME` — reported separately, non-fatal by default

Exit non-zero if any hard category is non-empty **and** not covered by
the allowlist.

### 4.5 Allowlist — `TEST/compare/known-divergences.yaml`

```yaml
- id: go-buildid
  packages: ["dev-lang/go", "app-*/*-bin"]
  category: CONTENT
  path_glob: "/usr/**"
  reason: "Go embeds a content hash / build id; not reproducible, not a PM bug"
  owner: unknown
  ticket: null
```

Every entry needs a reason and ideally a ticket. CI is green iff all
diffs match an allowlist entry. New unexplained diffs fail the run and
must be triaged into: **Portuale bug** (file it), **Portage bug** (file
upstream, allowlist with ticket), or **environmental nondeterminism**
(allowlist, reason). This triage is the actual deliverable of the whole
exercise.

### 4.6 Triage guidance

- Diff in `$ROOT` only, VDB agrees → merge-path bug (copy, mode,
  CONFIG_PROTECT, INSTALL_MASK, docompress, strip).
- Diff in VDB only → metadata-capture bug (the `gpkg-vdb-entry` class).
- Diff in both, same package, same input archive → serious; usually
  preinst/postinst phase behaviour or collision handling.
- Diff only after a *second* operation (upgrade/unmerge) → lifecycle
  bug (COUNTER, preserve-libs, same-slot replace).
- Symmetric diff that flips when you swap which PM goes first → order
  dependence / global state leak.

### 4.7 Archive-level comparison — `gpkg-diff.sh` + `diffoscope`

Complements the `$ROOT`+VDB diff; used in L2 (Portuale-built archives)
and any time two `.gpkg.tar` / `.tbz2` need comparing.

`TEST/compare/gpkg-diff.sh <a> <b>`:

1. Unpack both archives into scratch dirs (handles gpkg's nested
   `image.tar` + `metadata.tar` and xpak's trailing-segment layout).
2. Strip the volatile set: `build-info/BUILD_TIME`, `build-info/BUILD_ID`,
   `build-info/COUNTER`, `build-info/environment` (through §4.3
   normalisation), the embedded `Manifest` hash lines, and all mtimes.
3. `diff -r` the normalised trees; categorise like §4.4
   (`build-info/<file>` mismatches are their own bucket).
4. Exit non-zero on any hard diff not in `known-divergences.yaml`.

When `gpkg-diff.sh` flags something, run
[`diffoscope`](https://diffoscope.org/) `<a> <b>` for the readable
nested diff — it already recurses tar → gzip/zstd → ELF/ar and shows
exactly which member and which bytes differ. `diffoscope` is Python, so
it runs builder-side only, never on the `mrg` client; it is an
investigation aid, not a gate (its output is not stable enough to
allowlist against).

---

## 5. Test layers

Ordered by cost and inverse yield-per-second. Each layer is a set of
`TEST/scripts/NN-*.sh` probes + a `compare/` invocation.

### L0 — Resolver parity at real-tree scale (minutes)

No building. Extends today's `20-real-compare.sh`.

- `emerge -p --color=n <atom>` for a large curated atom list (200–500
  packages spanning python-r1, cmake, meson, kde, systemd units,
  multilib, go, rust, perl, texlive, virtual/*, `||` deps, slot-op
  chains) — diff the `[ebuild/binary …]` block + `emerge:` errors +
  exit code.
- `emerge -peuD --color=n @world` — diff the full merge list *and
  order* (memory: `merge-list-order-serialize-tasks-port` — order is
  all-or-nothing and already ported; this is its scale regression
  gate).
- `emerge -pc @world` (depclean) — diff the removal list.
- `emerge -p --autounmask …` on a set known to need unmasking.
- Both PMs read the *same* frozen repo; run twice with PM order swapped
  to catch state leaks.

Gate: zero unexplained `[…]`-line or order diffs.

### L1 — Merge parity from an identical prebuilt archive (tens of minutes)

The workhorse (§1.2).

1. `builder-portage` builds a curated ~80-package set from source with
   `FEATURES=buildpkg`, populating `porttest-pkgdir`. **Portage is the
   only builder here.**
2. `binhost` serves `$PKGDIR` over HTTP.
3. `consumer-portage`: `emerge -K --getbinpkg` the set into a clean
   `$ROOT`. Snapshot → `A`.
4. `consumer-portuale`: identical command into an identical clean
   `$ROOT`. Snapshot → `B`.
5. `diff.py A B`.

Package set chosen to hit merge edge cases: setuid/setgid binaries,
file capabilities (xattr), hardlinks (e.g. `busybox`, `coreutils`),
many symlinks (`ncurses`, `openssl`), CONFIG_PROTECT-heavy
(`shadow`, `openssh`, `nano`), `dodoc` compression, `keepdir`/empty
dirs, splitdebug, multilib (`glibc`, `zlib[abi_x86_32]`),
`.la` files, systemd units, udev rules, info files, bash-completion,
Python namespace packages, pkg-config files, large single file
(`texlive-*` or a `-bin`).

Also: install the **same archive twice** (reinstall), and install
version N then upgrade to N+1 (same-slot replace path), diffing after
each step.

Gate: `diff.py` clean modulo allowlist.

### L2 — Portuale as builder, structural + cross-install (tens of minutes)

1. `builder-portuale`: `emerge -b` the L1 set from source. For a subset
   also have `builder-portage` build the *same* atoms with
   `SOURCE_DATE_EPOCH` + `-j1` pinned (§8) so the two archives are as
   close to comparable as the toolchain allows.
2. Structural check of each `.gpkg.tar` Portuale produced:
   `TEST/compare/gpkg-structure.sh` — member list, path prefixes
   (`image/` handling — memory: `gpkg-vdb-entry-fix`), `build-info/`
   file set, `metadata` consolidation, embedded `Manifest`, MD5 in the
   `Packages` index, multi-instance `<cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar`
   layout (memory: `binpkg-multi-instance-both-formats`).
   Then `TEST/compare/gpkg-diff.sh` (§4.7) against the paired
   Portage-built archive where one exists; `diffoscope` on any
   `build-info/` or metadata-segment mismatch it reports.
3. **Cross-install**: `consumer-portage` merges the Portuale-built
   archive. It must merge cleanly and produce a VDB/`$ROOT` that
   matches a Portage-built-then-Portage-installed reference (modulo
   compiler nondeterminism — so compare CONTENTS *paths/types/modes*
   and VDB *metadata*, tolerate `CONTENT` sha diffs on compiled
   objects).
4. Reverse: `consumer-portuale` merges a Portage-built archive (this is
   L1 step 5, listed here for symmetry).

Gate: every Portuale archive installs under Portage; structural checks
pass; CONTENTS path/mode/type parity.

### L3 — Full source-build parity (hours; weekly soak)

`builder-portage` and `builder-portuale` each build the **same** set
from source into separate roots with `SOURCE_DATE_EPOCH` + `--jobs=1` +
identical everything (§8). Diff VDB metadata + CONTENTS structure +
`$ROOT` structure. Tolerate compiled-artefact `CONTENT` sha diffs;
**do not** tolerate mode/owner/path/symlink/missing/VDB diffs.

Target sets, in order of ambition: `@system`, then a desktop
`@world` (~1000 pkgs), then musl / no-multilib / hardened profile
variants (separate images, separate soak).

Gate: zero structural / metadata diff; `CONTENT`-only diffs all
allowlisted.

### L4 — `mrg` remote merge (tens of minutes)

See §6.

### L5 — Lifecycle & failure injection (minutes–tens of minutes)

On a container that has installed the L1 set:

- `emerge -C <atom>` under each PM → diff `$ROOT` + VDB (orphan
  cleanup, CONTENTS removal, prerm/postrm, directory-not-empty
  handling).
- `emerge --depclean` → diff.
- soname bump in `porttest` overlay → rebuild consumer → diff
  preserved-libs state + `@preserved-rebuild` set + the
  `!!! existing preserved libs` advisory (memory:
  `preserve-libs-rebuild-half-complete`).
- CONFIG_PROTECT: install a pkg that ships `/etc/foo`, edit it, reinstall
  → diff `._cfg0000_foo` creation + `config` hash db.
- `emerge --resume` after `SIGKILL` mid-merge-list → diff final state
  vs an uninterrupted run.
- `eselect news` / `emerge --check-news` → diff `.unread`/`.skip`
  write-back (memory: `check-news-versioned-display-if-installed`).
- Failure injection: disk full during merge (loopback fs), corrupt
  `.gpkg.tar` (truncate), binhost 500s mid-fetch, killed `env_update`.
  Assert **both** PMs fail safely and leave a recoverable state; diff
  the wreckage.

---

## 6. `mrg` remote-merge testing

Builds directly on [`remote-merge.md`](remote-merge.md) §12 — do not
reinvent it here.

Topology on `porttest-net`:

- `mrg-server` (Image A): has the repos, `$PKGDIR` (populated by L1's
  Portage builder or by `mrg --remote-binpkg` trials), the Portuale
  binary, the SSH client, and a private key.
- `mrg-client` (Image B): minimal, `sshd`, authorized key, no
  Portage/Python.
- `ref-client` (Image A or a twin of B **with** Portuale): receives a
  plain local `portuale emerge -K --getbinpkgonly` merge of the same
  atoms — the reference (§1.3).

Flow per atom set:

1. `mrg-server`: `portuale mrg --getbinpkgonly --remote-hostname
   mrg-client <atoms>`.
2. Snapshot `mrg-client:$ROOT` + VDB → `M`.
3. `ref-client`: `portuale emerge -K --getbinpkgonly <atoms>` from the
   same `$PKGDIR` (over HTTP or a mounted copy). Snapshot → `R`.
4. `diff.py M R` — expect clean modulo: the `mrg` ledger files
   (`--remote-*` ledgers, §8 of that doc), and any documented
   client-degradation notes (no preserve-libs on the client, etc.).

Additional `mrg`-specific probes:

- `--remote-transport=local` driver test (no SSH) — phase order,
  CONTENTS, vdb, CONFIG_PROTECT assertions (that doc §12).
- Preflight failures: bash < 5.3 (shim an old bash), missing `tar`,
  clock skew (`date -s` in the client), unwritable `$ROOT`,
  non-root-owned `$ROOT`.
- Ledger: last-10 rotation both sides; `unknown` commit for a non-git
  repo; strict-mode mismatch behaviour if `--remote-require-ledger-match`
  ships.
- Same-slot replace on the client with the old vdb `environment`
  present and absent (fail-closed per that doc's open question 3).
- Keep-going: one unit fails (corrupt bundle), rest proceed, summary
  correct.
- Sanity (not diff): `ldd` on installed binaries resolves; `equery`-ish
  CONTENTS spot check via the server; systemd `systemd-analyze verify`
  on any installed units.

---

## 7. `porttest` test overlay

A dedicated overlay baked into Image A (alongside `buildovl`), holding
tiny fast-building ebuilds engineered to isolate one merge/packaging
behaviour each. These build in seconds, so they run in L1/L5, not L3.

Candidates (each an `EAPI=8` ebuild with a trivial `src_install`):

| pkg | exercises |
|---|---|
| `porttest/cfgprotect` | ships `/etc/porttest/a.conf`; reinstall → `._cfg` |
| `porttest/setuid` | a `4755` binary + a file capability (`fcaps`) |
| `porttest/hardlinks` | two hardlinked regular files + `CONTENTS` `obj` dedup |
| `porttest/symfarm` | 50 relative + absolute symlinks, dangling and live |
| `porttest/emptydirs` | `keepdir` + a genuinely empty dir + `.keep` naming |
| `porttest/docs` | `dodoc` tree → docompress `.gz`, `newdoc`, `doinfo` |
| `porttest/unicode` | filenames with UTF-8, spaces, `$`, newline |
| `porttest/bigfile` | one 200 MB sparse file (merge perf + copy correctness) |
| `porttest/installmask` | installs files an `INSTALL_MASK` should drop |
| `porttest/splitdebug` | C source → `FEATURES=splitdebug` `.debug` split |
| `porttest/soname-1` / `-2` | shared lib `libpt.so.1` → `.so.2` for preserve-libs |
| `porttest/slotdep-*` | the slot-op rebuild chain (mirror `40-slotop-cascade.sh`) |
| `porttest/phases` | defines every `pkg_*` phase, each writing a marker → phase-order + env capture |
| `porttest/collision` | two pkgs shipping the same path → collision-protect |
| `porttest/config-script` | non-trivial `pkg_config` |
| `porttest/preserve-fail` | postinst that exits 1 → non-fatal handling |

These overlap the `fixtures/` tree conceptually but run against the
**real** merge/package/phase code with a **real** bash backend inside a
**real** filesystem — which `fixtures/` (subprocess-free, `--pretend`
only for the Python mirror) cannot.

---

## 8. Determinism controls

Baked into both PMs' `make.conf` / env for L1–L3:

- Same **profile** (`default/linux/amd64/23.0/systemd`), same
  `ACCEPT_KEYWORDS`, `USE`, `USE_EXPAND`, `CONFIG_PROTECT`,
  `INSTALL_MASK`, `FEATURES` (minus the known-flaky ones under
  rootless — §1.8), `PORTAGE_NICENESS`, `PORTAGE_COMPRESS`,
  `BINPKG_COMPRESS` + level, `BINPKG_FORMAT=gpkg`.
- `LC_ALL=C.UTF-8`, `TZ=UTC`, `umask 022`.
- `SOURCE_DATE_EPOCH=1740000000` (fixed) for L3.
- `MAKEOPTS="-j1"` for L3 generated-file races; `-j$(nproc)` fine for
  L0–L2 (merge from prebuilt archive is race-free).
- `EMERGE_DEFAULT_OPTS=""` (no hidden flags — memory:
  `emerge-info-config-layer-complete` normalises this for `--info`).
- `PORTAGE_RSYNC_EXTRA_OPTS` irrelevant — **never sync**; repos are
  frozen git checkouts and any `--sync`/`--regen` in a probe is a bug
  in the probe.
- **Same uid mapping** for both consumers so `uid/gid` in snapshots are
  directly comparable (run both containers with the same
  `--userns`/`--uidmap`, or both as in-container root mapped to the
  same host sub-uid).
- Frozen clock where the runtime allows (`--tz`, and for failure tests
  `date -s` is fine); otherwise rely on §4.3 mtime exclusion.
- `PORTAGE_TMPDIR` on a `tmpfs` of fixed size (catches disk-full paths
  and speeds builds).
- Record `portuale --version` + git SHA + `portage` version + repo SHAs
  into every snapshot's header so a stale comparison is obvious.

---

## 9. In-tree layout

Keep everything under `TEST/` so it rebases as one unit and the doc
(`docs/real-world-testing.md`) is the only `docs/` touch.

```
TEST/
  create-container.bash        # extended: builds Image A + Image B + porttest overlay
  images/
    Containerfile.test-portuale
    Containerfile.mrg-client
    overlay/porttest/…          # the §7 ebuilds (source of truth; copied into the image)
  net/
    up.sh / down.sh            # podman network + volume lifecycle
  run/
    l0-resolver.sh
    l1-merge-from-binpkg.sh
    l2-portuale-builder.sh
    l3-source-parity.sh
    l4-mrg-remote.sh
    l5-lifecycle.sh
    all.sh                     # orchestrator: brings up net, runs a tier, tears down
  scripts/                     # the /init NN-*.sh probes (as today), grouped by layer
    00-install-portage.sh      # unchanged, stays first
    L0-10-resolver-atomlist.sh
    L1-20-merge-consumer.sh
    …
  compare/
    snapshot.sh
    diff.py
    normalize.md               # the §4.3 ruleset, prose
    normalize.py               # its implementation
    gpkg-structure.sh          # §4.7 archive structural validation
    gpkg-diff.sh               # §4.7 normalised archive-vs-archive diff
    known-divergences.yaml
  atomlists/
    l0-resolve.txt
    l1-merge.txt
    l3-world-desktop.txt
  logs/                        # git-ignored (as today)
  snapshots/                   # git-ignored
  README.md                    # extended
```

The `porttest` overlay's ebuilds are tracked in-tree
(`TEST/images/overlay/porttest/`) and copied into the image at build
time — same pattern as `helpers/gentoo/repos/` today but human-readable.

---

## 10. Runbook

```sh
# one-time: build images (root or `buildah unshare`, as today)
sudo TEST/create-container.bash            # Image A + B + overlay

# bring up shared infra
TEST/net/up.sh                             # network + volumes + binhost

# fast gate (run per change to portuale)
cargo build --release -p portuale && TEST/run/l0-resolver.sh
TEST/run/l1-merge-from-binpkg.sh
# -> reads TEST/logs/*.log, TEST/snapshots/*, prints the triage report

# occasional
TEST/run/l2-portuale-builder.sh
TEST/run/l4-mrg-remote.sh
TEST/run/l5-lifecycle.sh

# weekly soak
TEST/run/l3-source-parity.sh --set l3-world-desktop

TEST/net/down.sh                           # optional; volumes persist by default
```

Each `l*.sh` exits non-zero if `diff.py` reports an unexplained hard
diff. The report (categorised, per-package) lands in
`TEST/logs/<layer>-report.txt` and a machine-readable
`TEST/logs/<layer>-report.json`.

---

## 11. Metrics & regression tracking

Emit per run into `TEST/logs/metrics/<date>-<layer>.json`:

- `parity_rate` = packages with zero unexplained diff / total.
- `divergences` grouped by category (§4.4) and by package.
- `allowlist_hits` (and any allowlist entry that matched nothing —
  candidate for removal).
- wall time per operation, Portuale vs Portage (perf regression signal —
  memory: `perf-investigation-2026-09-07` has Portuale ~3.5× faster on
  `-puD`; keep it that way).
- resolver: node-set size, merge-list length, order-diff count.

A tiny `TEST/compare/trend.py` appends to a CSV so drift is visible over
weeks. "Ready to pilot on real machines" = `parity_rate == 1.0` on
L0–L2 for N consecutive weeks with a stable allowlist, plus a clean L3
`@system` and a green L4/L5.

---

## 12. Risks & open questions (re-open, don't silently default)

1. **Sandbox** (§1.8) — **resolved**: root containers are acceptable.
   Run whichever roles need it (builder, source-parity) under
   `sudo podman` with the full `FEATURES` set; use rootless only where
   it is frictionless. Still verify `network-sandbox` / `userpriv`
   actually engage inside the container and note where they do not.
2. **Which HTTP server** for the binhost — `busybox httpd`, `thttpd`,
   or (builder-side only) `python3 -m http.server`? Must do `HEAD` +
   correct `Content-Length` + range requests (Portage resume).
   *Lean: `thttpd` from the tree, documented config.*
3. **uid mapping parity** (§8): run both consumers with an identical
   userns so snapshot `uid/gid` compare directly — or accept a
   normalisation map? *Lean: identical userns.*
4. **How big is L3's default set?** `@system` only for the gate, and a
   ~1000-pkg desktop `@world` as the opt-in soak? Or go straight for
   `@world`? *Lean: `@system` gate + opt-in `@world`.*
5. **`.tbz2`/xpak coverage**: L1/L2 with `BINPKG_FORMAT=xpak` too, or
   gpkg only for v1? Merge code handles both (memory). *Lean: gpkg
   gate, xpak as a second matrix axis once gpkg is green.*
6. **Non-amd64**: at least an arm64 L0 (resolver-only, via qemu-user
   binfmt) before "thousands of machines"? *Lean: yes, follow-on.*
7. **Allowlist ownership**: who adjudicates a new divergence as
   "Portage bug" vs "Portuale bug"? Needs a human in the loop — the
   test bed flags, it does not decide.
8. **Clock**: can podman freeze the container clock (`--tz` only sets
   zone), or do we rely entirely on mtime exclusion + a
   `libfaketime` `LD_PRELOAD` for the build phase? *Lean: mtime
   exclusion is enough for parity; `libfaketime` only if L3 needs it.*

---

## 13. Follow-on for "thousands of machines" confidence

Beyond this test bed:

- **Profile matrix**: desktop / server / musl / hardened / no-multilib /
  split-usr-vs-merged — separate images, L0 for all, L3 soak for two.
- **Upgrade-path testing**: install from an old repo snapshot, fast-
  forward the frozen repo to a newer pinned commit, `emerge -uDU
  @world`, diff against Portage doing the same. This is the real
  production scenario and the current frozen-single-commit design does
  not cover it.
- **Property/fuzz testing** the resolver: generate random consistent
  ebuild graphs, assert Portuale ≡ Portage on `-p`.
- **Real `@world` corpus**: collect anonymised `world` + `make.conf` +
  `installed` sets from volunteer production boxes, replay `-puD` in
  the container.
- **Chaos**: OOM-killer during merge, power-cut simulation (kill -9 the
  whole container mid-op, restart, `--resume`), NFS `$ROOT`, read-only
  `/usr` with `/etc` writable.
- **`mrg` at fan-out scale**: 50 clients from one server, partial
  failures, network partitions.
- **Signature path**: once `.gpkg.tar` `.sig` / binhost GPG lands, a
  key-management + verification-failure matrix.
- **Observability parity**: the `observability` FEATURE's output vs
  Portage's.

---

## 14. Slice plan

Each slice: scripts + `compare/` code + docs paragraph here + a green
run. Committed only when asked.

1. **Infra + L0.** — *shipped 2026-09-08.* `create-container.bash` bakes
   the `porttest` overlay skeleton (`TEST/images/overlay/porttest/` +
   `repos.conf/porttest.conf`); `TEST/net/{up,down}.sh` (network +
   `porttest-{pkgdir,distfiles,bincache,snapshots}` volumes);
   `TEST/compare/` — `snapshot.sh` (filesystem+VDB manifest, complete),
   `normalize.md` (the §4.3/§4.7 ruleset, complete), `normalize.py` +
   `diff.py` (slice-2 skeletons), `resolve-compare.py` (the L0 engine —
   typed findings, allowlist, JSON+text report), empty
   `known-divergences.yaml`; `TEST/atomlists/l0-resolve.txt` (~150
   atoms + `@system`/`@world`); `TEST/layers/l0/in-container.sh` +
   `TEST/run/{lib.sh,l0-resolver.sh}`.
   The binhost container is deferred to slice 2 — L0 is single-container
   and has no use for it.
   Repo-root note: portuale's `ebuild_phases::repo_root()` is a
   compile-time `CARGO_MANIFEST_DIR/../..` path; the orchestrator
   bind-mounts the host checkout at that same absolute path so it
   resolves inside the container (needed from L1 on; L0 never runs
   phases).
   First baseline run (120 probes, 69 clean, parity 0.575): 11 distinct
   divergence clusters, triaged in
   [`TEST/findings/l0.md`](../TEST/findings/l0.md)
   — none allowlisted (all are portuale bugs / backlog, to be fixed on
   `main`). Highlights: autounmask-required resolves exit 0 + disclose
   the full merge list where real exits 1 + withholds it (cluster A,
   ~15 probes); spurious `use.force` `( )` parens on `dev-libs/glib`
   `sysprof` etc. (B, ~13); `app-crypt/gcr[gtk]` mask not enforced (C);
   `@system` set expansion is short (H); merge order still diverges at
   real-tree scale (I, 25 — the known `_serialize_tasks` problem, now
   with a regression surface).
2. **L1 merge parity.** — *shipped 2026-09-08.*
   `TEST/run/l1-merge-from-binpkg.sh`: Portage builds
   `atomlists/l1-merge.txt` (10 all-stable packages + deps) from source
   once with `FEATURES=buildpkg` into a persistent
   `TEST/logs/_l1-pkgcache/` `$PKGDIR` (`layers/l1/build.sh`); then
   Portage and portuale each `-k --getbinpkg --oneshot` that identical
   `$PKGDIR` into their own fresh container's `/` (`layers/l1/consume.sh`
   — both first upgrade portage to 3.0.82.2 with `-1` so the base `/` is
   identical). `compare/snapshot.sh` gained `--paths <file>` (trailing
   `/` recurses, else stat-only) and `--vdb-list <file>`;
   `compare/normalize.py` + `compare/diff.py` (skeletons → implemented)
   do the normalised comparison per `normalize.md`; typed findings
   (`MISSING`/`MODE`/`OWNER`/`XATTR`/`SIZE`/`CONTENT`/`SYMLINK`/
   `VDB:<file>`/`CONTENTS`, non-fatal `MTIME` count),
   `known-divergences.yaml`-gated.
   First run (5-package smoke set): after the methodology +
   normalisation fixes, **5 findings, one root cause, allowlisted; 0
   unexplained** — portuale's binpkg merge produces a byte-identical
   `$ROOT` and a VDB differing only in the tracked `environment`
   refilter and mtimes. Triaged in
   [`TEST/findings/l1.md`](../TEST/findings/l1.md): **L1-a**
   `--usepkgonly` (`-K`) doesn't treat an installed dep with no binpkg
   as satisfied (blocks `-K`; real does — workaround `-k --getbinpkg`);
   **L1-b** portuale needs `--getbinpkg` to *execute* a local-`$PKGDIR`
   binary merge where real merges on `-k` alone; **L1-c** the vdb
   `environment` isn't re-filtered on a binpkg merge (`declare -- x=""`,
   missing `SKIP_KERNEL_BINPKG_ENV_RESET`) — the one allowlisted entry,
   `owner: portuale-bug`, to be deleted when fixed on `main`.
   The reinstall + upgrade sub-cases and the full 10-package set are a
   slice-2 follow-up.
3. **`porttest` overlay — real ebuilds.** — *shipped 2026-09-08.* Nine
   `EAPI=8` fixtures under
   `TEST/images/overlay/porttest/porttest/`: `setuid` (4711/2755/1750 +
   0600 modes), `hardlinks`, `symfarm` (rel/abs/dangling/chain +
   20 uniform), `emptydirs` (`keepdir` + `.keep` + bare owned empty
   dir), `docs` (`dodoc -r`/`newdoc`/`doman`/`doinfo`/`docinto`),
   `installmask` (`INSTALL_MASK`/`*.la`/`*.log` drop), `phases` (every
   `pkg_*` appends to `/var/lib/porttest/phase.log`), `splitdebug`
   (`.debug` + `.build-id` for a binary + a soname lib), `unicode`
   (spaces/tabs/UTF-8/metachars). Live-mounted into the L1 containers
   (`-v …:/porttest-overlay:ro`), staged by `layers/l1/{build,consume}.sh`
   when the atom list has `porttest/` atoms — no image rebuild.
   `atomlists/l1-porttest.txt`; `consume.sh` gained an `INSTALL_MASK`
   line in `make.conf` (not an env export — L1-d) and `/var/lib/porttest/`
   in the snapshot path list. First run (2026-09-08, both PMs at portage
   3.0.82.2, identical fresh containers, same Portage-built `$PKGDIR`):
   **all 9 fixtures byte-identical at the file level** (108/108 paths —
   modes incl. `4711/2755/1750/0600`, ownership, xattrs/filecaps, sha256,
   symlink targets incl. the dangling/chained ones, unicode names, the
   `splitdebug` `.debug`/`.build-id`, the `INSTALL_MASK`/`.la` drops, the
   `keepdir` `.keep` files); VDB `CONTENTS`/`metadata`/`NEEDED` match for
   all 9. First run: **10 hard findings** — L1-c (vdb `environment` not
   refiltered, ×9) and **L1-e (new) — `pkg_pretend` not run on a
   `-k`/`--getbinpkg` binary merge** (the `phases` fixture's phase.log
   had `setup preinst postinst` where portage's has `pretend setup
   preinst postinst`). **All of L1-a…L1-f then fixed on `main`** (see
   `TEST/findings/l1.md`); the re-run is **0 hard findings, 0
   unexplained**, `known-divergences.yaml` empty. L1-c: `PORTAGE_UPDATE_
   ENV` vdb-env regeneration + a `FEATURES`-is-incremental fix in
   `portage-profile` + a brush save-env-wrapper local-leak fix. L1-f
   (found while fixing L1-c): the binpkg phase env inherited the whole
   process env — now filtered to real's `environ_whitelist`.
   Also fixed here: `snapshot.sh` aborted the whole walk (silently, under
   `set -o pipefail`) when `getfattr` dereferenced a dangling symlink —
   now `getfattr -h` + `set -e`-safe `stat`/`readlink`/`getfattr`.
   Deferred (need the reinstall sub-case / L5): `cfgprotect`,
   `soname-{1,2}`, `slotdep-*`, `collision`, `config-script`,
   `preserve-fail`.
4. **L2 Portuale-as-builder + `gpkg-structure.sh` + `gpkg-diff.sh`.**
   Structural validation, normalised archive-vs-archive diff (§4.7),
   `diffoscope` wired as the drill-down aid, cross-install both
   directions. Deliverable: L2 report.
5. **L5 lifecycle.** unmerge / depclean / preserve-libs (needs
   `porttest/soname-{1,2}`) / CONFIG_PROTECT / `--resume` / news.
   Deliverable: L5 report.
6. **Image B + L4 `mrg`.** Minimal client image, `net` wiring,
   `ref-client`, the §6 flow + `mrg`-specific probes. Deliverable: L4
   report; `mrg` client ≡ reference local merge modulo documented
   degradations.
7. **L3 soak + metrics + trend.** `SOURCE_DATE_EPOCH` plumbing, the
   `@system` gate set, `metrics/*.json`, `trend.py`. Deliverable: a
   clean L3 `@system` run and a first trend data point.
8. **Failure injection** (L5 tail) and the **xpak matrix axis**.

Later / follow-on (§13): profile matrix, upgrade-path, fuzzing, arm64.
