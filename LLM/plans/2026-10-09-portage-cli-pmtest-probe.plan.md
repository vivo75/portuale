# portage-cli × pmtest probe — findings + follow-up plan

Date: 2026-10-09. Probe revs: portage-cli `5316d36` (/tmp clone),
portuale `backlog/330-332` (dirty; reference binary approximate).
Nothing in either repo was modified; all artifacts live in
`/tmp/opencode/pmtest-shims/` and `/tmp/probe-fix/`.

## Goal

Test https://github.com/lu-zero/portage-cli (`em` binary) with pmtest —
first as a registry entry with adapter shims, surfacing the gap list.

## Key facts (verified live)

- `em` is subcommand-dispatched (`em emerge`, `em ebuild`, …,
  `default_subcommand = "emerge"`), NOT `argv[0]`-multicall like portuale.
  (`portage-cli/src/cli.rs:175-189`, `dispatch.rs:32-44`).
- `ROOT` env is honored (`cli/topology.rs:111`); `PORTAGE_CONFIGROOT`
  has NO env fallback (config defaults to host `/`). Shims inject
  `--config-root`/`--root` from env.
- Relative `repos.conf` `location`s resolve against CWD, not config root
  (portuale joins to `config_root`: `rust/portage-repo/src/lib.rs:1268-1272`).
  Probe workaround: run with CWD = fixture root.
- pmtest fixture isolation is env-based (`ROOT`, `PORTAGE_CONFIGROOT`,
  `DISTDIR`: `pytests-contract-suite/conftest.py:377-397`); applets are
  symlinked expecting `argv[0]` dispatch (`conftest.py:262-269`).

## Blockers (layered, live-verified on a /tmp fixture copy)

| # | Gap | Evidence |
|---|---|---|
| B1 | Relative `location` → CWD, not config root | `!!! repo not found at repo` unless CWD=fixtures |
| B2 | Missing `metadata/layout.conf` → repo skipped | `repository.rs:371`; kills `overlay`, `independentoverlay` |
| B3 | Master names resolved as `repos_dir.join(name)` (`repository.rs:437`, `repos_dir = main.path().parent()` in `repo_open.rs:55`) instead of via repos.conf locations | `skipping repo 'overlay' …: not a valid repository: testrepo` |
| B5 | `repo:profile` parent treated as literal path | `I/O error at …/default/overlay:crossrepo-parent`; zero suite coverage until fixed |
| B4 | layout.conf `repo-name`/`aliases` ignored; section≠name repos skipped | `resolve_repo_name` reads only `profiles/repo_name` (`util.rs:213`) |
| CLI | `emerge --regen` → usage error rc 2; `ebuild … unmerge` → `!!! unknown phase` rc 1; no `argv[0]` dispatch | verbatim runs |

Note: `layoutmasteroverlay` (which HAS layout.conf) misreported
`invalid layout.conf: … not found` — real failure was downstream
master/name resolution (B3/B4 cascade). Misleading error worth fixing.

## Positive findings (probe-simple profile + scratch layout.conf + testrepo symlink)

- `emerge --pretend dev-libs/newpkg` → rc 0,
  `[ebuild  N     ] dev-libs/newpkg-1.0 to $ROOT`.
- Remaining byte-level diffs vs portuale pin: header block
  (`These are the packages…` / `Dependency resolution took…`) and a
  trailing slash on ROOT (`to $ROOT/` vs pinned `to {ROOT}`).
- `dev-libs/diamond` closure order sane; `dev-libs/maskneedpkg` → rc 1
  with mask disclosure (`=dev-libs/maskeddep-1.0:0`).

## Repro recipe

```sh
git clone --depth 1 https://github.com/lu-zero/portage-cli /tmp/opencode/portage-cli
cargo build --profile quick -p portage-cli   # in the clone (~2 min)
cp -r pmtest/fixtures /tmp/probe-fix
printf 'masters = testrepo\n' > /tmp/probe-fix/overlay/metadata/layout.conf
printf 'masters = testrepo\n' > /tmp/probe-fix/independentoverlay/metadata/layout.conf
ln -sfn repo /tmp/probe-fix/testrepo
cd /tmp/probe-fix
export PORTAGE_CONFIGROOT=/tmp/probe-fix   # one var per export statement!
export ROOT=/tmp/probe-fix
export DISTDIR=/tmp/probe-fix/distfiles
/tmp/opencode/pmtest-shims/emerge --pretend dev-libs/newpkg
```

Shims: `/tmp/opencode/pmtest-shims/{emerge,ebuild,mrg}` —
`em emerge` / `em ebuild` + root injection + `--version` passthrough.
Shell gotcha: `export A=… B=$A` in ONE statement expands `$A` against the
OLD env (empty on a fresh shell) — use separate statements.

## Slices

- **S1 (pmtest, standalone commit):** `portage-cli` entry in
  `managers/managers.yaml` (`repo` + `rust_dir` = checkout, `package:
  portage-cli`) + committed shim scripts + doc note. No test edits (F1);
  suite stays red for it until S2 lands. Note: registry `_provide` runs
  `cargo build --package portage-cli` in `rust_dir`; bed needs all applets
  in one bin dir (shims + `em`); `em` needs no `bin/`/`3rdparty` runtime.
- **S2 (portage-cli upstream, in order):** B2+B3+B5 (unlocks the suite) →
  B1 (drops CWD hack) → renderer (header/trailing slash) → B4 → CLI
  surface (`--regen`/`--sync` spelling, ebuild vocab).
- **S3 (later):** L0 bed wiring after S2.
