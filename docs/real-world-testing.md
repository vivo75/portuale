# Real-world testing: controls, triage, and the forward layers

Live companion to [`../TEST/README.md`](../TEST/README.md) (the L0/L1
operating guide) and [`scope-backlog.md`](scope-backlog.md) §I (the
L2–L5 one-liners). L0 (resolver parity) + L1 (merge parity) are shipped
and run live; what follows is the information needed to *run* the bed
correctly and to *build* the forward layers — extracted from the
retired planning doc [`history/real-world-testing.md`](history/real-world-testing.md),
which stays as the historical record (methodology critique, image
specs, slice history). Nothing here duplicates `TEST/README.md`'s
runbook, `TEST/compare/normalize.md`'s ruleset, or
[`remote-merge.md`](remote-merge.md)'s `mrg` design (cited, not copied).

## 1. What each layer checks

| Class | Artefact | Detects |
|---|---|---|
| Resolution | `emerge -p …` stdout/stderr, exit code | wrong pkg/slot/USE choice, wrong merge order, missing/extra graph nodes, bad REQUIRED_USE handling |
| Merge | `$ROOT` filesystem, `env_update` output, `ldconfig` cache | wrong modes/owners/xattrs, CONFIG_PROTECT mistakes, collision handling, symlink/hardlink handling, INSTALL_MASK, docompress |
| VDB | `/var/db/pkg/<cat>/<pf>/` | CONTENTS format/paths, NEEDED/soname data, environment capture, metadata consolidation, COUNTER, *DEPEND recording |
| Packaging | `.gpkg.tar` / `.tbz2` structure, `build-info/`, embedded Manifest | archive layout, metadata segment, multi-instance BUILD_ID layout, MD5 in index |
| Binhost | `Packages` index, HTTP serve, client fetch/verify | index field parity, SIZE/MD5, `--binpkg-changed-deps`, signature fields |
| Lifecycle | unmerge, depclean, preserved-libs, `@preserved-rebuild`, `--resume`, news | orphan cleanup, preserve/rebuild logic, resume replay, news `.unread` write-back |
| Remote (`mrg`) | client `$ROOT` + VDB, ledgers, phase order | everything in `remote-merge.md` §12 |

## 2. Determinism controls

Baked into both PMs' `make.conf` / env for L1–L3 (the L0/L1 scripts
carry the applicable subset; a forward layer must carry all of them —
a run that skips one is not comparable):

- Same **profile** (`default/linux/amd64/23.0/systemd`), same
  `ACCEPT_KEYWORDS`, `USE`, `USE_EXPAND`, `CONFIG_PROTECT`,
  `INSTALL_MASK`, `FEATURES`, `PORTAGE_NICENESS`, `PORTAGE_COMPRESS`,
  `BINPKG_COMPRESS` + level, `BINPKG_FORMAT=gpkg`.
- `LC_ALL=C.UTF-8`, `TZ=UTC`, `umask 022`.
- `SOURCE_DATE_EPOCH=1740000000` (fixed) for L3.
- `MAKEOPTS="-j1"` for L3 generated-file races; `-j$(nproc)` is fine
  for L0–L2 (merge from a prebuilt archive is race-free).
- `EMERGE_DEFAULT_OPTS=""` (no hidden flags).
- Never sync; repos are frozen git checkouts. Any `--sync`/`--regen`
  in a probe is a bug in the probe.
- Same uid mapping for both consumers so snapshot `uid/gid` compare
  directly (same `--userns`/`--uidmap`, or both as in-container root
  mapped to the same host sub-uid).
- Frozen clock where the runtime allows, otherwise §4.3-style mtime
  exclusion (`normalize.md`).
- `PORTAGE_TMPDIR` on a `tmpfs` of fixed size (catches disk-full paths
  and speeds builds).
- Record `portuale --version` + git SHA + `portage` version + repo SHAs
  into every snapshot's header so a stale comparison is obvious.

## 3. Reading a report: triage guidance

`diff.py` categorises, it does not decide. Map the shape to the cause:

- Diff in `$ROOT` only, VDB agrees → merge-path bug (copy, mode,
  CONFIG_PROTECT, INSTALL_MASK, docompress, strip).
- Diff in VDB only → metadata-capture bug (the `gpkg-vdb-entry` class).
- Diff in both, same package, same input archive → serious; usually
  preinst/postinst phase behaviour or collision handling.
- Diff only after a *second* operation (upgrade/unmerge) → lifecycle
  bug (COUNTER, preserve-libs, same-slot replace).
- Symmetric diff that flips when you swap which PM goes first → order
  dependence / global state leak.

Every unexplained hard diff is triaged into exactly one of: **Portuale
bug** (fix it, never allowlist), **Portage bug** (report upstream,
allowlist with ticket), **environmental** (allowlist with `reason:`).
That triage — recorded in `TEST/compare/known-divergences.yaml` and
`TEST/findings/` — is the deliverable, not the green run.

## 4. Archive-level comparison (L2 tooling spec)

Complements the `$ROOT`+VDB diff; the L0/L1 bed does not need it.

- `TEST/compare/gpkg-structure.sh`: per-`.gpkg.tar` structural
  validation — member list, path prefixes, `build-info/` file set,
  `metadata` consolidation, embedded `Manifest`, MD5 in the `Packages`
  index, multi-instance `<cat>/<pn>/<pf>-<BUILD_ID>.gpkg.tar` layout.
- `TEST/compare/gpkg-diff.sh <a> <b>`: unpack both archives
  (gpkg's nested `image.tar` + `metadata.tar`, xpak's trailing-segment
  layout), strip the volatile set (`build-info/BUILD_TIME`,
  `build-info/BUILD_ID`, `build-info/COUNTER`,
  `build-info/environment` through the §3 normalisation, embedded
  `Manifest` hash lines, all mtimes), `diff -r` the normalised trees;
  `build-info/<file>` mismatches are their own bucket; exit non-zero
  on any hard diff outside the allowlist. Never a raw `cmp`: two runs
  of Portage itself do not produce byte-identical archives
  (`BUILD_TIME`, mtimes, tar ordering, compressor headers).
- [`diffoscope`](https://diffoscope.org/) is the drill-down tool when
  `gpkg-diff.sh` flags a mismatch (recurses tar → gzip/zstd → ELF/ar).
  Builder-side only (it is Python — never on the `mrg` client) and an
  investigation aid, not a gate.

## 5. Forward layers (not built; build order: L2, L5, L4, L3)

### L2 — Portuale as builder, structural + cross-install

1. `builder-portuale`: `emerge -b` the L1 set from source. For a subset,
   `builder-portage` builds the same atoms with `SOURCE_DATE_EPOCH` +
   `-j1` pinned (§2) so the pair is as comparable as the toolchain
   allows (yardstick is *structural validity*, never a byte diff —
   compiler nondeterminism would swamp it).
2. `gpkg-structure.sh` over every Portuale archive; `gpkg-diff.sh`
   against the paired Portage-built archive where one exists.
3. **Cross-install both directions**: Portage merges the
   Portuale-built archive (must merge cleanly; CONTENTS paths/types/
   modes + VDB metadata must match a Portage-built-then-installed
   reference, tolerating `CONTENT` sha diffs on compiled objects);
   Portuale merges the Portage-built archive (symmetric case).
4. Gate: every Portuale archive installs under Portage; structural
   checks pass; CONTENTS path/mode/type parity.

### L3 — Full source-build parity (hours; weekly soak)

Both PMs build the same set from source into separate roots with
`SOURCE_DATE_EPOCH` + `--jobs=1` + identical everything (§2). Diff VDB
metadata + CONTENTS structure + `$ROOT` structure. Tolerate
compiled-artefact `CONTENT` sha diffs; **do not** tolerate
mode/owner/path/symlink/missing/VDB diffs. Sets in order of ambition:
`@system`, then a desktop `@world`, then musl / no-multilib /
hardened profile variants. Gate: zero structural/metadata diff.

### L4 — `mrg` remote merge

Differential, not absolute: compare the `mrg` client against a
reference local Portuale binpkg merge of the same atoms (the client
stays Portage-free and Python-free — that is the point). Topology on
`porttest-net`: `mrg-server` (repos, `$PKGDIR`, Portuale binary, SSH
client + key), minimal `mrg-client` (`sshd`, authorized key, no
Portage/Python), `ref-client` (plain local `portuale emerge -K
--getbinpkgonly` of the same atoms). Snapshot client `$ROOT` + VDB vs
reference; expect clean modulo the `mrg` ledger files and documented
client degradations. Probes: `--remote-transport=local` driver test,
preflight failures (old bash, missing tar, clock skew, unwritable
`$ROOT`), ledger rotation, same-slot replace with/without the old vdb
`environment`, keep-going with one corrupt bundle. `remote-merge.md`
§12 owns the `mrg`-side test strategy; this layer owns the
differential harness around it.

### L5 — Lifecycle & failure injection

On a container holding the L1 set: `emerge -C` diffs, `--depclean`
diffs, soname bump → rebuild consumer (preserved-libs state +
`@preserved-rebuild` + advisory), CONFIG_PROTECT reinstall
(`._cfg0000_` + config hash db), `--resume` after `SIGKILL` vs an
uninterrupted run, news write-back, and fault injection (disk-full via
loopback fs, truncated `.gpkg.tar`, binhost 500s mid-fetch, killed
`env_update`): assert **both** PMs fail safely and recoverably, and
diff the wreckage. Needs the deferred fixtures below
(`soname-{1,2}`, `collision`, `config-script`, `preserve-fail`,
`cfgprotect`).

## 6. Deferred `porttest` fixtures

Shipped (see `TEST/images/overlay/porttest/README.md`): `setuid`,
`hardlinks`, `symfarm`, `emptydirs`, `docs`, `installmask`,
`phases`, `splitdebug`, `unicode`. Still to add (each an `EAPI=8`
ebuild with a trivial `src_install`), in the order L2/L5 need them:
`porttest/cfgprotect` (ships `/etc/porttest/a.conf`; reinstall →
`._cfg`), `porttest/soname-1` / `-2` (shared lib `.so.1` → `.so.2`
for preserve-libs), `porttest/slotdep-*` (slot-op rebuild chain),
`porttest/collision` (two pkgs, same path), `porttest/config-script`
(non-trivial `pkg_config`), `porttest/preserve-fail` (failing
postinst → non-fatal handling).

## 7. Open risks for the forward layers

- **HTTP binhost server**: `busybox httpd`, `thttpd`, or builder-side
  `python3 -m http.server`? Must do `HEAD` + correct `Content-Length`
  + range requests (Portage resume). Lean: `thttpd`, documented config.
- **L3 set size**: `@system` gate + opt-in `@world` soak (not `@world`
  from the start).
- **`.tbz2`/xpak axis**: gpkg gate first, xpak as a second matrix axis
  once gpkg is green.
- **Non-amd64**: at least an arm64 L0 (qemu-user binfmt) before any
  "thousands of machines" claim.
- **Allowlist ownership**: a human adjudicates every new divergence;
  the bed flags, it does not decide.
- **Clock**: mtime exclusion suffices for parity; `libfaketime`
  `LD_PRELOAD` only if L3 needs it.
- (Resolved: root containers are acceptable wherever they give the
  cleaner test; the `mrg`/`mgr` naming is `mrg`.)

## 8. Metrics (for the L3 soak and trend tracking)

Per run, `TEST/logs/metrics/<date>-<layer>.json`: `parity_rate`
(packages with zero unexplained diff / total), divergences grouped by
`diff.py` category and package, `allowlist_hits` (plus allowlist
entries that matched nothing — removal candidates), wall time per
operation per PM (perf regression signal), resolver node-set size /
merge-list length / order-diff count. "Ready to pilot" = `parity_rate
== 1.0` on L0–L2 for N consecutive weeks with a stable allowlist,
plus a clean L3 `@system` and a green L4/L5.
