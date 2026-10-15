# G6a — `NS app-text/docbook-xml-dtd-4.3-r2:4.3` on `-pv kde-apps/kdecore-meta`

Verdict: **engine** (downstream of G5's `||`-choice). No bridge patch, no
fixture/test/fix — allowlist + crate-gap text at the bottom.

## The row (bed `-pv l0-20261015T091741Z`)

- real `kde-apps_kdecore-meta.txt` (269 lines): no docbook row.
- portuale `kde-apps_kdecore-meta.txt` (267 lines), line 22:
  `[ebuild NS] app-text/docbook-xml-dtd-4.3-r2:4.3::gentoo
  [4.1.2-r7:4.1.2/::, 4.2-r3:4.2/::, 4.4-r3:4.4/::, 4.5-r2:4.5/::]`
- VDB holds docbook slots 4.1.2, 4.2, 4.4, 4.5 — no 4.3.
- Same row recurs in `-pvu`, `-pvuD`, `-pvuDN` (S5c modes-c.md) and as 1 of
  the 9 `-pve` kdecore extras.

## Chain (established from the `PORTUALE_PG_TRACE=1` run, `../cmd.txt`)

1. Root `kde-apps/kdecore-meta` → … → `virtual/secret-service-0-r1`
   (in BOTH plans: `N virtual/secret-service-0-r1`).
   Container-tree RDEPEND (`virtual/secret-service-0-r1.ebuild:10-14`):
   `|| ( gnome-base/gnome-keyring
   >=kde-frameworks/kwallet-runtime-6.18.0[keyring] app-admin/keepassxc )`.
2. Both plans contain in-graph `N kde-frameworks/kwallet-runtime-6.27.0`
   (`USE="X (keyring) …"`). Portage resolves the virtual to the in-graph
   kwallet-runtime (no gnome-keyring row). The engine takes the
   first-listed branch — portuale-only
   `[ebuild N] gnome-base/gnome-keyring-48.0-r1` (the G5 pick; triage §G5).
3. Trace (`pg-pv-trace.txt`):
   `pg-edge gnome-base/gnome-keyring-48.0-r1 -> app-text/docbook-xml-dtd (BDEPEND)`
   Container-tree atom (`gnome-keyring-48.0-r1.ebuild`, unconditional in
   `BDEPEND`): `app-text/docbook-xml-dtd:4.3`.
4. Slot 4.3 has exactly one version (4.3-r2), none installed → the engine
   MUST select it → `NS` row. The installed slots (4.1.2/4.2/4.5) the
   solution also contains map to hidden `AlreadyInstalled` (trace
   `pg-plan app-text/docbook-xml-dtd-4.5-r2` etc. with no display row).

The engine picking exactly slot 4.3 (neither max 4.5 nor min) proves the
`:4.3` slot constraint reached it intact — no bridge slot-constraint loss.

## Why Portage has no row

Nothing in Portage's solution requires slot 4.3: its only requirer in
either closure is gnome-keyring's BDEPEND, and Portage never selects
gnome-keyring (in-graph kwallet-runtime wins the `||`).

## Control: Portage agrees downstream of the parent

Real `emerge -pv gnome-base/gnome-keyring` (control probe, `../cmd.txt`)
prints `[ebuild NS] app-text/docbook-xml-dtd-4.3-r2:4.3` itself (list of
115 packages; rc 1 is the G4a freetype/harfbuzz autounmask gate AFTER the
list). So given gnome-keyring in-graph, both PMs merge docbook:4.3 — the
`:4.3` handling agrees; the sole divergence is the `||` parent pick.

## Engine mechanism (cited, not patched)

- `portage-atom-pubgrub-0.8.0/src/provider/solve.rs:139-258` (`choose_version`):
  virtual `||` (`Choice`, n-i numbering = first-listed highest) consults
  ONLY installed state (`direct_installed`, `newest_installed_choice_branch`,
  `deps_reach_installed`); the final fallback (`:258`) is
  `candidates.into_iter().max()` = the first-listed alternative. No
  in-graph input exists.
- `provider/mod.rs:1168` doc: `newest_installed_choice_branch` returns
  `None` "when no candidate exposes an installed version, so the caller
  falls back to the default `max()` (= first-listed) pick". Here NO
  `||` branch is installed (gnome-keyring, kwallet-runtime, keepassxc all
  uninstalled… kwallet-runtime is in-graph-but-uninstalled, which the
  heuristic cannot see) → blind first-listed pick.
- Portage: `_emerge/depgraph.py:778-793` comment — atoms for
  already-in-graph packages are preferred ("packages that have been added
  to the graph … over other choices"); the bridge's `DepEntry::AnyOf`
  facts cannot express dynamic graph membership (triage §G5 — a static
  bridge-side child order over the closure superset would misfire where
  Portage picks a merely-reachable non-first alternative).
- A bridge re-solve loop constraining `||` branches to the previous
  solution would be patching around the engine (oscillation risk, no
  convergence argument) — per the task stop rule, NOT done.

## Disposition

`owner: engine`, riding on the G5 crate gap (proposed backlog #366,
triage §G5). Proposed allowlist text (extends the G5 entry — same match,
same slug; S5-step-2 files it):

`id: l0-pubgrub-or-ingroup-choice / layer: l0-pubgrub / categories:
[extra] / slugs: [kde-apps_kdecore-meta] / match: "present for portuale,
absent for real" / reason: engine-inherent ||-choice difference: the crate
falls back to the first-listed provider when no branch is installed
(portage-atom-pubgrub-0.8.0 provider/solve.rs choose_version, fallback
:258; provider/mod.rs newest_installed_choice_branch), while Portage
prefers the already-in-graph alternative (depgraph.py:778-793). Covers the
gnome-keyring/gcr/p11-kit cluster AND its downstream rows: NS
docbook-xml-dtd-4.3 (this file: gnome-keyring BDEPEND :4.3 unconditional;
Portage merges it too when gnome-keyring is selected — control probe
real-gnome-keyring-pv.txt) and the -pve openssh/shadow chain (sibling file
b/). Filed as #366; see the crate-gap note. Evidence:
LLM/evidence/2026-10-15-34-s5/triage.md (G5) + g6/a/ (+g6/b/, g6/c/ for the
sibling rows).`

Crate-gap note text (for `LLM/evidence/2026-10-15-34-crate-gap.md`, §0.4):
input = texinfo/virtual-secret-service-style `||` with an in-graph but
uninstalled alternative; crate lines = solve.rs `choose_version`
(`:140-259`) + mod.rs `newest_installed_choice_branch` (`:1171-1204`);
needed = an in-solution preference input (e.g. a solve-time callback or a
post-solve `||`-recheck the bridge could drive) so an already-selected
package wins its `||` group the way Portage's graph_db does
(depgraph.py:778-793).
