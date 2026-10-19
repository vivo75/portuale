# #394 oracle: in-graph installed parents are never rebuilt (2026-10-19)

Backlog #394 (`LLM/backlog-tasks-2026-10.md:189`, filed 2026-10-16 from
02.359 S2): for `--rebuild-if-*` / `--changed-deps`, Portage restarts the
graph walk after `_rebuild_config` records a parent that is already in the
graph (`depgraph.py:424-442`, `:531-573`); portuale keeps the walked
outcome for reached parents and only stops rebuilding unreached ones
(S2, `crates/portage-repo/src/slot_operator/eliminate.rs`). The
`s2-probe-changeddeps.txt` probe (`LLM/evidence/2026-10-16-359-exec/`)
grounds the `--changed-deps` hide/drop split.

## Staging

`stage-host.sh` = container `stage.sh`
(`../pmtest/differential-test-bed/layers/l0-fixture-oracle/stage.sh`)
adapted for the host: copy `SRC` (pmtest `fixtures/`) to `$STAGE`,
absolute `repos.conf` locations, `${PORTAGE_CONFIGROOT}` interpolation in
`binrepos.conf`, `etc/portage/categories` + per-repo `profiles/categories`,
per-package `Manifest` regeneration, news dropped, `common-1.0` staled,
`layout.conf` masters, updates-comment drop, `*/<pkg>` rewrite, `repo_name`
fill. Deltas vs the container script: no `touch /etc/make.local` (the
host file is instead emptied with `sudo truncate -s 0` around each Portage
cell and restored after; md5 `3f29e0…` verified intact), and `$SCRATCH`
(`scratch/repo/`) staged-only ebuilds are injected before Manifest
generation. The two scratch ebuilds never touch the committed tree:

- `dev-libs/r394top-1`: `RDEPEND="dev-libs/rebuildconsumer"` (reaches the
  installed consumer as a dep).
- `dev-libs/r394cdtop-1`: `RDEPEND="dev-libs/changeddepspkg"` (reaches the
  installed changed-deps package as a dep).

Fresh stage per cell per PM (global updates mutate the staged vdb),
`run-cells.sh` drives both sides from `cells.txt`. No
`PORTAGE_REPOSITORIES` inlined (391 README). Host Portage 3.0.82.2 env:
`PORTAGE_CONFIGROOT=ROOT=$FX`, `DISTDIR=$FX/distfiles`,
`SYSROOT=$FX` (without it, EAPI-8 `DEPEND` resolves against the host
`ESYSROOT=/` — see §"Topology trap" — while portuale resolves it in the
target; the two PMs would solve different roots), `PYTHONHASHSEED=0
LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0`, inherited config vars stripped.
portuale env identical plus `HOME=/var/tmp/pt-394`,
`PORTAGE_RUNNING_ROOT=$FX` (`ROOT==ESYSROOT==RUNNING`, single root both
sides). portuale binary `target/release/emerge`, md5 recorded per capture
(`5905012f…`, built 2026-10-19 from a clean `50de74f2` tree).

Host-Portage noise, recorded verbatim, not part of any verdict: the host
gentoo repo's global-updates passes and `app-admin/awscli-bin` mask notice
(host `repos.conf` is visible because `PORTAGE_RUNNING_ROOT` is a
portuale-only hook — Portage's running root stays `/`; cf. #395), and the
fixture updates `2Q-2024` moves applied to each Portage-side stage copy.

## Topology trap (read before reusing these cells)

`DEPEND` of a `$FX`-targeted package resolves against `ESYSROOT`, which is
`/` unless `SYSROOT` is exported (`LocationsManager`: `sysroot ==
target_root` ⇒ `esysroot = target`). The first harness revision omitted
`SYSROOT`: Portage merged `rebuildtrigger-2.0` to `/` (row without the
` to <root>` suffix — `resolver/output.py:841-858` prints it only when
`ROOT != "/"`) and planned it `N` (host vdb has no trigger). `SYSROOT=$FX`
co-locates both sides; the co-location is stated in every capture's env
line only implicitly (see `run-cells.sh`).

## Verdict table (rows only; `to <root>` paths differ by stage dir)

| cell | argv | Portage 3.0.82.2 | portuale | verdict |
|---|---|---|---|---|
| a-noflag | `--pretend --update --deep dev-libs/r394top` | `U rebuildtrigger-2.0 [1.0]`, `N r394top-1` (backtrack 0/20) | same rows | AGREE |
| a-unbuilt-nodeep | `--pretend --update --rebuild-if-unbuilt dev-libs/r394top` | `N r394top-1` (0/20) | same row | AGREE |
| a-unbuilt-deep | `--pretend --update --deep --rebuild-if-unbuilt dev-libs/r394top` | `U rebuildtrigger-2.0 [1.0]`, **`rR` rebuildconsumer-1.0**, `N r394top-1` (backtrack **1/20** = the restart) | `N r394top-1` only | **DIVERGE** |
| a-newrev | `--pretend --update --deep --rebuild-if-new-rev dev-libs/r394top` | same as a-unbuilt-deep (1/20) | `N r394top-1` only | **DIVERGE** |
| a-newver | `--pretend --update --deep --rebuild-if-new-ver dev-libs/r394top` | same as a-unbuilt-deep (1/20) | `N r394top-1` only | **DIVERGE** |
| b-cddeep | `--pretend --changed-deps --deep dev-libs/r394cdtop` | `N newpkg-1.0`, `R changeddepspkg-1.0`, `N r394cdtop-1` (0/20) | same rows | AGREE |
| b-cdnodeep | `--pretend --changed-deps dev-libs/r394cdtop` | `N r394cdtop-1` (0/20) | same row | AGREE |

So the divergence is exactly the titled one, for all three
`--rebuild-if-*` flags uniformly: Portage's post-walk restart rebuilds the
reached installed parent (`rR` = forced-reinstall tag) while keeping the
trigger upgrade; portuale prints the walked outcome minus the upgrade.
`--changed-deps` needs no work: the reinstall fires at selection time
(`depgraph.py:8290-8307`, no restart involved) and both sides already agree
on reached parents (deep) and on the no-deep drop (the s2 hide/drop split).

## Portage mechanism (3.0.82.2, `lib/_emerge/depgraph.py`)

- `_rebuild_config.add` (`:424-442`): records a dep→parent edge only for a
  built (`parent.built`, i.e. vardb/bintree instance) parent with buildtime
  priority, minus `--rebuild-exclude`/`--rebuild-ignore`.
- `_needs_rebuild` (`:444-471`): dep not built + `--rebuild-if-unbuilt` ⇒
  True; `--rebuild-if-new-rev` ⇒ dep cpv not installed; `--rebuild-if-new-ver`
  ⇒ dep norev not installed.
- `trigger_rebuilds` (`:531-576`): bottom-up over the rebuild graph; a hit
  adds the parent's `(root, slot_atom)` to `rebuild_list`, `need_restart`.
- `_resolve` (`:5692-5698`): on a hit, stashes `rebuild/reinstall_list` in
  `backtrack_infos["config"]`, sets `_need_restart`, returns False.
- Outer loop (`:12235`): `need_restart` ⇒ next try with the lists as
  `backtrack_parameters`.
- Restart consumption: `_gen_reinstall_sets` (`:5460-5480`) yields an
  internal `__auto_rebuild__` `SetArg` (`force_reinstall=True`,
  `reset_depth=False`); the re-walk skips installed instances in the lists
  (`:7883-7885`) and masks built ones with "need to rebuild from source"
  (`:6649`). The re-walk re-resolves the parent's own deps (a restart, not
  a patch-up).
- Complete mode is NOT on during the first walk (`_complete_mode = False`,
  `:726`); `_complete_graph` (`:8661`) flips it only afterwards and extends
  the graph via `_select_pkg_from_graph` — it never demotes a merge. The
  `myparams["complete"]` marker that `--rebuild-if-*` sets only enables
  that later phase.

## portuale mechanism (the two halves of the divergence)

- `slot_operator/eliminate.rs::rebuild_if_entries` (`:388-557`): the scan
  exists but always returns empty — the `!in_graph ⇒ continue` arm (`:435`)
  stops unreached rebuilds (S2, pinned), and the `in_graph ⇒ continue` arm
  (`:442`) keeps every reached parent's walked outcome with the comment
  that the re-walk "is a separate, unfiled gap" (now #394).
- `pretend/resolve_plan.rs:257-270`: any `--rebuild-if-*` sets request
  `complete_graph`. `pretend/resolve_phases.rs:255-320`: phase 1 runs
  non-complete (it would print the `U`, cf. a-noflag), but
  `want_complete` is then true, so the phase-2 complete walk **replaces**
  the phase-1 result; the complete walk re-resolves unlocked deps from
  installed (`pass/initial_outcome.rs` `complete_locked`) and drops the
  non-top-level `U` (the lock only gates cps phase 1 never merged). Hence
  portuale prints `N`-only, losing both the `U` and the `rR`.

## Captures

`out/portage-<cell>.txt`, `out/portuale-<cell>.txt`: full argv, env-shaped
runs, `rc=` trailers. Scratch ebuilds under `scratch/`; drivers under
`cells.txt`, `run-cells.sh`, `stage-host.sh` (copies of the
`/var/tmp/pt-394/` originals).
