# #352: IDEPEND merges into the running root — design

Oracle: Portage 3.0.82.2 (`/usr/lib/python3.14/site-packages/`).
Cell: `LLM/evidence/2026-10-12-348-s0/d-lifecycle.md` (`d-idep8-pret`, `d-idep8-merge`).

## 1. What Portage does

- `_running_root = trees[trees._running_eroot]["root_config"]`
  (`_emerge/depgraph.py:311`; same in `Scheduler.py:246`). `_running_eroot`
  is the process EROOT (`portage/__init__.py:498,528`); `broot = EPREFIX`
  (`portage/package/ebuild/_config/LocationsManager.py:99`) — fixed, no
  `--broot` option, not redirectable via env/make.conf.
- `_add_pkg_deps` (`depgraph.py:4256-4288`) assigns one root per dep key:
  RDEPEND/PDEPEND → `myroot`; **IDEPEND → `_running_root.root`**;
  DEPEND → ESYSROOT (else running root); BDEPEND → running root. The
  IDEPEND child's own RDEPEND then stays in the running root.
- Display (`_emerge/resolver/output.py:841-867`): `if
  pkg.root_config.settings["ROOT"] != "/": … + darkgreen("to " + pkg.root)`,
  else bare. Cell: `good-1` (root `/`) bare, `idep-8` (root=cfg)
  `to /var/tmp/t348-d/cfg/` (trailing slash: `pkg.root` is EROOT-slashed).
- VDB: per root, `<eroot>/var/db/pkg` (`vartree.py:2029`,
  `const.py: VDB_PATH="var/db/pkg"`). Merge order lists the IDEPEND dep
  first (`good-1`, then `idep-8`); each `Package` merges under its own
  `pkg.root` (`EbuildBuild.py:265`, `Scheduler.py:2359`, `EbuildMerge.py:81`
  `… to {pkg.root}`, `MergeListItem.py:78` `for {pkg.root}`).
- Cell `d-idep8-merge`: Portage aborts `!!! Read-only file system:
  /var/db/pkg` (tried the host running root); portuale merged both into
  ROOT, rc 0.

## 2. Where portuale decides the merge root

- Resolution already routes per-entry: `dep_child_root` (`crates/
  portage-repo/src/resolve/entry.rs:150`) maps IDEPEND (key 1) → running
  root; `GraphEntry.targets_running_root` marks it. Display `root_suffix`
  (`portuale/src/pretend/display/attrs.rs:351`) already renders per-entry,
  and `d-idep8-pret` matches Portage except the trailing slash (§1).
  `conflicts.rs:383` (`skipped_node_suffix`) is per-entry too.
- The merge ignores it: `run_merge_loop(entries, keep_going, root,
  merge_one)` (`emerge_build/source_merge.rs:164`) takes one `root` for all
  entries; `MergeOneSourceEntryArgs.root`, `MergeOneBinaryEntryArgs.root`
  (`emerge_getbinpkg.rs:754`), `SourceEngine.root` / `BinaryEngine`
  (`merge_engines.rs`), `StatusDisplay` `for/to {root}` lines
  (`emerge_build/status.rs:187,461,498,534`), `MergeOptions.root`
  (`ebuild_merge/mod.rs:136`), `mtimedb::remove_merged_entry(root, …)`.
  All must take a per-entry root resolved as
  `if entry.targets_running_root { running_root } else { root }`.

## 3. Safe implementation plan

- Thread one helper `entry_root(entry, root, running_root)` through the
  sites in §2 (merge args, engines, status lines, mtimedb, VDB/MergeOptions
  selection). No other behaviour changes.
- Guard: refuse any write whose per-entry root is the running root and
  differs from target root **unless** an explicit opt-in env is set
  (proposed `PORTUALE_ALLOW_RUNNING_ROOT_WRITES=1`, set only by the bed
  harness) — else abort with a one-line error before any mutation. Never
  sniff `/proc`; on the host (`ROOT=/cfg`, running root `/`) the default
  path must keep today's behaviour (merge into ROOT) or abort, never touch
  host `/`. Note `PORTAGE_RUNNING_ROOT` is portuale-only (Portage ignores
  it); the guard keys off the real `/`, not that var.
- `--pretend` display: host-safe. Only change is the trailing slash
  (`to <root>/` when the root is non-`/`, matching `pkg.root`), pinnable
  by pytest against captured `d-idep8-pret` output.

## 4. Bed verification cell

- No existing script covers it: `l1-merge-from-binpkg.sh` pins
  `ROOT=/ PORTAGE_RUNNING_ROOT=/` (`layers/l1/consume.sh:29`), so both PMs
  are single-root; `l32-lifecycle.sh` has no IDEPEND cell.
- Needed: a new cell (extend L1 consume or L32) that, inside one container
  (running root = container `/`, safe to write), runs
  `emerge --oneshot --root /stage <idep-pkg>` on both PMs with the opt-in
  env for portuale, then diffs `/stage` + container `/var/db/pkg`
  snapshots. Portage's side prints `Read-only file system` only if `/`
  is protected; in a writable container it merges the dep into `/`.
- Not captured here: running the container bed needs a podman build+run
  cycle (heavy); the runner shape above is confirmed, the cell itself is
  missing.

## 5. Size, risk, slicing

- S1 (display-only: trailing slash + pytest pin vs `d-idep8-pret`): tiny,
  host-safe, no guard needed. S2 (real per-entry-root merge + guard + bed
  cell): medium; touches every merge path, gated by the glibc/bash merge
  gate plus the new bed cell.
- Recommendation: ship S1 now; do S2 only with the bed cell green.
