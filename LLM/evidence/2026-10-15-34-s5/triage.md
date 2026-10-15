# #34 S5 step 1 — corpus residue triage (S4 runs, 2026-10-15)

Input runs (pmtest `differential-test-bed/logs/`): `-pv l0-20261015T022535Z`,
`-pvu l0-20261015T023148Z`, `-pvuD l0-20261015T023755Z`, `-pvuDN l0-20261015T024616Z`,
`-pve l0-20261015T025613Z` (117 probes each; Portage 3.0.82.2 oracle, tree
`11c58b7a`, VDB 320 pkgs). Remaining unexplained counted findings per
`LLM/evidence/2026-10-15-34-s4/modes.md`: `-pv` 24 missing / 5 extra / 30 exit;
`-pvu` same; `-pvuD` 98 / 4 / 30; `-pvuDN` 81 / 4 / 30;
`-pve` 785 missing / 10 extra / 88 version / 88 action / 30 exit.

Read-only analysis (no builds, no podman, no host-binary trace runs — the
`PORTUALE_PG_TRACE=1` scratch reproduction is left to each fix task).
Portage citations below are against the installed oracle itself,
`/usr/lib/python3.14/site-packages/{_emerge/depgraph.py,...}` (Portage
3.0.82.2, same version as the container oracle), plus ebuilds from
`/var/db/repos/gentoo` (host tree, newer than the container's `11c58b7a`;
used only for dep-string structure, flagged where it matters) and one
container overlay copy under
`/home/vivo/.local/share/containers/storage/overlay/`.

## Summary table

| Group | Modes (missing / extra / version / action / exit) | Cause | Disposition |
|---|---|---|---|
| G1 installed-newer-than-visible downgrade (`sys-apps/portage` UD) | -pv 1/0/0/0/0; -pvu 1/0/0/0/0; -pvuD 75/0/0/0/0; -pvuDN 81/0/0/0/0; -pve 785/0/88/88/0 | VDB portage-3.0.82.2 is `~amd64`-masked on the stable profile; Portage's downgrade probe re-selects best-visible 3.0.81.3 (UD + subtree under `-e`); pubgrub keeps the installed version (omit, or R 3.0.82.2 with stub deps under `-e`) | `fix` (bridge downgrade-probe rule; fixture idea in §G1) |
| G2 parent USE-atom forces reinstall of installed child (R + subtree) | -pv 23/0/0/0/0; -pvu 23/0/0/0/0; -pvuD 23/0/0/0/0; -pvuDN 0; -pve 0 | `curl[http2]`, `xmlto[text]`, `systemd[policykit]`, `zlib[minizip]` required by a parent but off in the installed instance → Portage R + subtree; pubgrub keeps the selective leaf, drops R and subtree | `fix` (parent-driven USE reinstall; fixture idea in §G2) |
| G3 go self-cycle false positive | all modes: exit, 2 probes (`dev-lang_go`, `app-containers_runc`) | go BDEPEND `\|\| ( go go-bootstrap )`; pubgrub picks go-itself → self buildtime edge → "circular dependencies", rc 1, drops go-bootstrap; Portage avoids the direct self-cycle → go-bootstrap, rc 0 | `fix` (fixture idea in §G3); escalate to §0.4 crate-gap if the engine cannot express self-avoidance |
| G4a autounmask USE-change gate (real rc 1 / portuale rc 0) | all modes: exit, 9 probes (gtk_4, gtk__3, firefox, thunderbird, libreoffice, gimp, wireshark, i3, nginx) | Portage prints the same list, then exits 1 on "USE changes are necessary"; pubgrub applies the USE silently, rc 0 | `allowlist owner: portuale-bug` (proposed #363; text in §G4) |
| G4b masked-dependency abort (real rc 1 / portuale rc 0) | all modes: exit, 2 probes (`app-editors_gedit`, `gnome-base_nautilus`) + `gnome-base_gnome-shell` | `>=app-crypt/gcr-3.27.90:0=[gtk]` masked (gcr-3.41.2-r2 masked by empty entry); gnome-shell adds `samba[client]`/masked bundle | `allowlist owner: portuale-bug` (proposed #364; text in §G4) |
| G4c REQUIRED_USE abort (real rc 1 / portuale rc 0) | all modes: exit, 1 probe (`app-emulation_wine-vanilla`) | libsdl2 `wayland? ( gles2 )` unsatisfied; Portage aborts, pubgrub plans 143 rows rc 0 (no REQUIRED_USE enforcement) | `allowlist owner: portuale-bug` (proposed #365; text in §G4) |
| G5 `\|\|` first-listed fallback extras (gnome-keyring cluster) | 3 of kdecore-meta's 4 extras in -pv/-pvu/-pvuD/-pvuDN; 3 of 9 in -pve kdecore | `virtual/secret-service` `\|\|` picks first-listed gnome-keyring (nothing installed → crate fallback); Portage prefers the in-graph kwallet-runtime | `allowlist owner: engine` + §0.4 crate-gap note (proposed #366) |
| G6 installed-satisfiable deps merged anyway | docbook-xml-dtd-4.3 NS: -pv/-pvu/-pvuD/-pvuDN 1 extra each (kdecore), -pve 1; openssh/shadow chain: -pve kdecore 5 extra; info-manual: -pve emacs 1 extra | Mechanism NOT established (see §G6): `:slot` atoms satisfied in-vdb get merged (wrong slot for docbook); `-e`-only over-reach rows | `fix` (fixture + trace left to the fix task) |
| G7 soft-blocker auto-unmerge row | -pv 1 extra, -pvu 1 extra (`sys-apps_systemd-utils` only) | portuale prints `[uninstall]` gentoo-systemd-integration-9-r2; Portage prints only `[blocks B]` rows | `fix` (blocker rendering; fixture idea in §G7) |

Arithmetic check: -pv missing 1 (G1) + 23 (G2) = 24 ✓; extras 3 (G5) + 1 (G6 docbook) + 1 (G7) = 5 ✓;
-pvuD missing 75 (G1) + 23 (G2) = 98 ✓, extras 3 + 1 = 4 ✓; -pvuDN missing 81 (G1) ✓, extras 4 ✓;
-pve missing 785 (all G1: 88+88+88+87+87+87+87+87+86) ✓, version/action 88/88 (all G1 portage UD-vs-R) ✓,
extras 3 (G5) + 1 + 5 + 1 (G6) = 10 ✓. Exit 30 in every mode = 15 probes × 2 findings
(G3: 2 probes; G4a: 9; G4b: 3 incl. gnome-shell; G4c: 1) ✓.

Note on exit probes: the comparator suppresses merge-list comparison once the
exit differs, so suppressed set differences can hide inside G3/G4 probes.
Observed case: `-pv www-servers_nginx` — real prints a 4th row
`[ebuild R] app-misc/mime-types-2.1.54 USE="nginx*"` that portuale omits
(G2 cause riding along with the G4a exit). Any fix task for G3/G4 must
re-diff the merge lists after the exit matches.

---

## G1 — installed-newer-than-visible downgrade (`sys-apps/portage` UD)

Counts per mode: -pv missing 1 · -pvu missing 1 · -pvuD missing 75 ·
-pvuDN missing 81 · -pve missing 785 + version 88 + action 88.

Aggregates (every row verified by `grep … | sort | uniq -c` over each
`counts.txt`): all 81 `-pvuDN` missing rows, all 75 non-chain `-pvuD`
missing rows, the single `-pv`/`-pvu` non-chain missing row, and all 785
`-pve` missing rows / 88 version rows / 88 action rows are one package:

- Example 1 — `-pv sys-apps_portage` (also `-pvu`, `-pvuD` identical shape):
  real `Total: 1 package (1 downgrade)`:
  `[ebuild     UD ] sys-apps/portage-3.0.81.3::gentoo [3.0.82.2::gentoo] USE="(ipc) native-extensions rsync-verify xattr ..." 1212 KiB`
  portuale: `Total: 0 packages`.
- Example 2 — `-pvuDN app-admin_sudo` (representative of all 81; every
  affected `-pvuDN` probe is `missing=1` except kdecore-meta at
  `missing=1 extra=4`): real contains
  `[ebuild     UD ] sys-apps/portage-3.0.81.3::gentoo [3.0.82.2::gentoo] ...`
  among 62 rows (`Total: 62 packages (1 downgrade, 49 new, 12 reinstalls)`);
  portuale's 61 rows are otherwise identical (modulo order-only `order`
  context), the UD row alone absent (`Total: 61 packages (49 new, 12 reinstalls)`).
- Example 3 — `-pve app-admin_sudo` (representative of all 88):
  real `UD sys-apps/portage-3.0.81.3 [3.0.82.2]` + 9 dep rows
  (`sys-apps/sandbox-2.49`, `sys-apps/install-xattr-0.13`,
  `app-portage/gemato-20.12`, `dev-python/urllib3-2.7.0`,
  `dev-python/charset-normalizer-3.4.9`, `dev-python/requests-2.34.2`,
  `dev-python/pysocks-1.7.1-r2`, `dev-python/idna-3.18`,
  `dev-python/certifi-3024.7.22` — 86–88 probes each; `dev-python_pip`
  shows only 3 because pip's own closure already contains the python
  half; `dev-python_certifi` shows 8, lacking itself);
  portuale: `[ebuild   R    ] sys-apps/portage-3.0.82.2::gentoo ...`
  (installed version, no dep subtree).

Cause (established from run files + oracle source, no trace run needed).
The bed pins Portage itself to 3.0.82.2 with testing keywords
(`in-container.sh`: `ACCEPT_KEYWORDS=~amd64 … "=sys-apps/portage-$PIN"`);
VDB holds `sys-apps/portage-3.0.82.2` while the stable profile's best
visible is 3.0.81.3 — confirmed at the ebuild level: in a container tree
copy, `portage-3.0.81.3.ebuild` carries `KEYWORDS="~alpha amd64 arm …"`
(stable) vs `portage-3.0.82.2.ebuild` `KEYWORDS="~alpha ~amd64 ~arm …"`
(testing). Portage: (a) a named target is always update-wanted —
`dep.want_update = (not complete_mode) and (arg_atoms or update) and not
_too_deep(depth)` (`_emerge/depgraph.py:3805-3809`); (b) under `-u`/`-D`
the deep walk reaches installed portage and `_downgrade_probe`
(`depgraph.py:2946-2957`, "a downgrade … is considered desirable due to
the current version being masked or unavailable") fires because no
visible version is `>=` installed 3.0.82.2 → UD row; (c) under `-e` the
re-selected 3.0.81.3 expands its full RDEPEND (the 9 missing rows).
Pubgrub: installed portage is kept (Favor, never re-selected to a lower
visible version — the bridge has no downgrade-probe rule), hence omitted
(`-pv`/`-pvu`/`-pvuD`/`-pvuDN`); under `-e` the crate keeps the installed
3.0.82.2 as R while its facts come from the empty-deps stub for a
non-offered version (`provider/mod.rs:742-753`; S3's installed-only
path), hence R-at-wrong-version plus the empty subtree.

Disposition: **`fix`** (no backlog number; fix tasks don't take one).
Minimal fixture: repo with stable `pg34-downer-1` + `~arch`-masked
`pg34-downer-2`, VDB holding `-2`, container captures
(`-pvuDN` → `UD -1`; `-pve` → `UD -1` + dep subtree). Portage rule for
the fix task: `_downgrade_probe` + `want_update` as cited above; bridge
work is to detect "installed cpv with no visible version `>=` it" under
update/empty and offer best-visible as the downgrade pick (render UD).

---

## G2 — parent USE-atom forces reinstall of installed child

Counts per mode: -pv missing 23 · -pvu missing 23 · -pvuD missing 23
(non-portage part) · -pvuDN 0 · -pve 0 (beyond G1).

Per-probe (identical in `-pv`, `-pvu`, and the non-portage part of
`-pvuD`): `dev-lang_rust` 4, `kde-apps_kdecore-meta` 7,
`kde-frameworks_kcoreaddons` 5, `media-video_vlc` 5,
`net-misc_networkmanager` 1, `sys-auth_polkit` 1.

- Example 1 — `-pv dev-lang_rust` (real 15 rows / portuale 11; the only
  delta is this chain):
  ```
  [ebuild  N     ] net-libs/nghttp3-1.17.0::gentoo  USE="-verify-sig" ...
  [ebuild  N     ] net-libs/ngtcp2-1.24.0::gentoo  USE="openssl ssl -gnutls -verify-sig" ...
  [ebuild  N     ] net-libs/nghttp2-1.69.0:0/1.14::gentoo  USE="systemd -debug ..." ...
  [ebuild   R    ] net-misc/curl-8.21.0-r1::gentoo  USE="adns alt-svc ftp hsts http2* http3* httpsrr ..." ...
  ```
  Portuale has the other 11 rows but no curl row and none of its three
  deps. Mechanism anchor: `dev-lang/rust` DEPEND contains
  `net-misc/curl[http2,ssl]` (host-tree `rust-1.96.1.ebuild:139`; dep
  shape, not version, is what matters); the installed curl (VDB
  `net-misc/curl-8.21.0-r1`) does not satisfy the USE atom, so Portage
  re-selects the repo ebuild → R. The `*` marks flags differing from the
  installed instance; no "USE changes are necessary" block and rc 0
  because the ebuild's own defaults satisfy the requirement (contrast
  G4a/nginx where the flag needs a config change). The reinstalled curl
  then expands `http2 → nghttp2`, `http3 → nghttp3+ngtcp2`.
- Example 2 — `-pv kde-apps_kdecore-meta` (real 257 / portuale 254):
  ```
  [ebuild  N     ] dev-libs/libatomic_ops-7.10.0::gentoo ...
  [ebuild  N     ] dev-libs/boehm-gc-8.2.12::gentoo  USE="large threads ..." ...
  [ebuild   R    ] sys-libs/zlib-1.3.2-r1:0/1::gentoo  USE="minizip* -static-libs -verify-sig" ...
  [ebuild   R    ] sys-apps/systemd-260.1-r2:0/2::gentoo  USE="... policykit* ..." ...
  [ebuild  N     ] www-client/w3m-0.5.6::gentoo  USE="X nls ssl unicode ..." ...
  [ebuild  N     ] virtual/w3m-1::gentoo  0 KiB
  [ebuild   R    ] app-text/xmlto-0.0.28-r11::gentoo  USE="text* -latex" ...
  ```
  Chain: some parent requires `xmlto[text]` → installed xmlto (VDB
  `app-text/xmlto-0.0.28-r11`) reinstalls with `text` → `text? ( || (
  virtual/w3m … ) )` (`app-text/xmlto` RDEPEND, host tree) → w3m →
  `dev-libs/boehm-gc:=` (unconditional in `www-client/w3m` DEPEND, host
  tree) → libatomic_ops. `kcoreaddons`/`vlc` show the same 5 (minus the
  systemd/zlib R rows, which are per-plan parents: `systemd[policykit]`,
  `zlib[minizip]`).
- Example 3 — `-pv net-misc_networkmanager` / `sys-auth_polkit`
  (`missing=1` each; `-pvuD` adds the same single row on top of G1):
  `[ebuild   R    ] sys-apps/systemd-260.1-r2:0/2::gentoo  USE="... policykit* ..." ...`
  present for real, absent for portuale.

Cause (established). Portage re-evaluates a parent's USE-qualified dep
atom against the installed instance even without `--deep`/`--newuse`:
the atom (`curl[http2]`, `xmlto[text]`, …) does not match installed →
the repo ebuild is selected → R row with adjusted USE → the reinstalled
node's deps expand (dep-matching in `_select_atoms`/`dep_check` with the
vartree + `mytrees["parent"]`, `depgraph.py:6030-6110` region).
Pubgrub: S2's kept rule deliberately makes every installed non-target a
leaf under selective non-empty (`!kept.deep && installed && !rebuild →
empty deps`), and `rebuild` never fires on parent USE pressure — so the
R row and its subtree both vanish. (S2's "kept named argument is a leaf
too" reasoning covers the *named* case; this group is the *parent-driven*
case it did not cover.) Under `-pvuDN`/`-pve` the rows already match
(S2–S4 rebuild machinery), which corroborates the diagnosis.

Disposition: **`fix`**. Minimal fixture (distinct from S2's): installed
`pg34-clib-1` (USE `-f`), repo ebuild with `IUSE="f"` and
`f? ( dev-libs/pg34-cdep )`, app with `RDEPEND="dev-libs/pg34-clib[f]"`
where `f` is default-on in the ebuild → `-pv` capture shows
`R pg34-clib` + `N pg34-cdep`, rc 0. The fix task must also cover the
config-change variant (flag default-off → G4a-shaped autounmask block +
rc 1; `www-servers_nginx`/`mime-types[nginx]` is the live example).

---

## G3 — go self-cycle false positive (pubgrub rc 1, Portage rc 0)

Counts per mode: exit findings in all 5 modes, 2 probes
(`dev-lang_go`, `app-containers_runc`).

- Example 1 — `-pv dev-lang_go`: real rc 0,
  `Total: 2 packages` (`go-bootstrap-1.24.6`, `go-1.26.4`), backtrack 1/20.
  Portuale rc 1, `Total: 1 package`, then:
  ```
   * Error: circular dependencies:

  (dev-lang/go-1.26.4:0/1.26.4::gentoo, ebuild scheduled for merge) depends on
   (dev-lang/go-1.26.4:0/1.26.4::gentoo, ebuild scheduled for merge) (buildtime)
  ```
- Example 2 — `-pv app-containers_runc`: real rc 0, 4 rows (same two +
  `go-md2man` + `runc`); portuale rc 1 with the identical self-cycle
  error (3 rows + error block).

Cause (established). `dev-lang/go` BDEPEND is
`|| ( >=dev-lang/go-$MIN >=dev-lang/go-bootstrap-$MIN )` (host-tree
`go-1.26.4.ebuild`; container behaviour matches: real selects
go-bootstrap). Pubgrub resolves the `||` to go-itself (highest/newest;
nothing installed so the crate's installed-preference in
`provider/mod.rs:1164-1194` falls back to first-listed/max) → self
buildtime edge → hard circular error. Portage avoids "unresolvable
direct circular dependencies" at selection time: `_select_atoms` threads
`parent`/`atom_graph`/`circular_dependency` into `dep_check`
(`depgraph.py:6030-6110` region), with the rationale spelled out in the
comment at `depgraph.py:778-793` (graph-added packages preferred as
choices; the `gwydion-dylan`-depends-on-itself-or-`-bin` precedent —
the same shape as go/go-bootstrap).

Disposition: **`fix`** first. Minimal fixture: `pg34-self-1` with
`BDEPEND="|| ( dev-libs/pg34-self dev-libs/pg34-selfboot )"` + a
`-pv` container capture showing Portage picking the bootstrap sibling.
Honest caveat: the `||` pick happens inside `portage-solver`; if the
engine cannot express "avoid the self alternative", the fix task must
stop and file the §0.4 crate-gap note instead (in-graph/self-avoidance
is the same engine limitation as G5's).

---

## G4 — Portage aborts after printing the list (real rc 1, portuale rc 0)

Counts per mode: exit findings in all 5 modes, 13 probes (9 + 3 + 1).
In every case the merge lists are near-identical (line counts equal,
e.g. gtk_4 120/120) and only the post-list gate differs.

### G4a — pure autounmask USE-change gate (9 probes)

`gui-libs_gtk_4`, `x11-libs_gtk__3`, `www-client_firefox`,
`mail-client_thunderbird`, `app-office_libreoffice`, `media-gfx_gimp`,
`net-analyzer_wireshark`, `x11-wm_i3`, `www-servers_nginx`.
None has any `!!!`/`emerge:` error line; the sole failure signal is:

- Example 1 — `-pv gui-libs_gtk_4`, tail of real (120 rows, then):
  ```
  Total: 120 packages (119 new, 1 in new slot), Size of downloads: 797294 KiB

  The following USE changes are necessary to proceed:
   (see "package.use" in the portage(5) man page for more details)
  # required by x11-libs/pango-1.57.1::gentoo
  # required by gnome-base/librsvg-2.62.2-r1::gentoo
  ...
  >=media-libs/freetype-2.14.3 harfbuzz

   * In order to avoid wasting time, backtracking has terminated early
   ...
  ```
  portuale: same 120 rows (`Total: 120 packages (119 new, 1 in new slot)`),
  no USE block, rc 0.
- Example 2 — `-pv www-servers_nginx`, tail of real (4 rows):
  ```
  [ebuild   R    ] app-misc/mime-types-2.1.54::gentoo  USE="nginx*" 29 KiB
  ...
  Total: 4 packages (3 new, 1 reinstall), Size of downloads: 1342 KiB

  The following USE changes are necessary to proceed:
  ...
  # required by www-servers/nginx-1.31.3-r1::gentoo
  # required by www-servers/nginx (argument)
  >=app-misc/mime-types-2.1.54 nginx
  ```
  portuale: the 3 N rows, no mime-types row (G2 cause, suppressed), rc 0.
  Per-probe advice per plan: firefox/thunderbird add `libvpx postproc`;
  libreoffice `xmlsec nss`; gimp `poppler cairo`, `babl lcms`, `gegl …`;
  nginx `mime-types nginx`; the rest are the shared `freetype harfbuzz`.

Portage rule: the message is rendered by `_show_autounmask`'s `_writemsg("USE
changes", "package.use")` (`_emerge/depgraph.py:10956-10976`); the early
stop by the `_autounmask_backtrack_disabled` block (`:11102-11110`); the
failure itself by `need_config_change` (`:11708-11730`, true when
`_success_without_autounmask` is unset). Autounmask notices are
explicitly out of #34's scope (plan owner decisions), and exit-code
parity for a *config-change gate* is a display/policy behaviour, not a
set/action difference.

Disposition: **`allowlist owner: portuale-bug`** (needs a number —
propose **#363**; text below). One entry can cover all 9 probes
(`categories: [exit]`, `match: "real rc=1 portuale rc=0"` + the 9 slugs).
Proposed text:
`id: l0-pubgrub-autounmask-use-exit / layer: l0-pubgrub /
categories: [exit] / slugs: [gui-libs_gtk_4, x11-libs_gtk__3,
www-client_firefox, mail-client_thunderbird, app-office_libreoffice,
media-gfx_gimp, net-analyzer_wireshark, x11-wm_i3, www-servers_nginx] /
match: "real rc=1 portuale rc=0" / reason: Portage prints the same merge
list and then exits 1 demanding a package.use change ("The following USE
changes are necessary to proceed", depgraph.py need_config_change);
pubgrub applies the USE silently and exits 0. The package set and action
letters agree; only the config-change gate differs, and autounmask
notices are out of #34's scope. Evidence: LLM/evidence/2026-10-15-34-s5/triage.md (G4a).`

### G4b — masked-dependency abort (3 probes)

`app-editors_gedit`, `gnome-base_nautilus`
(`>=app-crypt/gcr-3.27.90:0=[gtk]` unsatisfiable — every candidate
masked), `gnome-base_gnome-shell` (additionally
`>=net-fs/samba-4.0.0[client]` + masked notices).

- Example — `-pv app-editors_gedit`, tail of real:
  ```
  # required by gnome-base/gvfs-1.58.4::gentoo[gcr]
  # required by app-editors/gedit-49.0-r1::gentoo
  # required by app-editors/gedit (argument)
  >=app-crypt/gcr-3.41.2-r2:0 gtk
  ...
  !!! All ebuilds that could satisfy ">=app-crypt/gcr-3.27.90:0=[gtk]" have been masked.
  !!! One of the following masked packages is required to complete your request:
  - app-crypt/gcr-3.41.2-r2::gentoo (masked by: )
  ```
  portuale plans through (143-ish rows incl. gcr stack) rc 0. Masked-package
  disclosure is out of #34's scope (plan owner decisions: slot-conflict /
  autounmask / masked notices stay out).

Disposition: **`allowlist owner: portuale-bug`** (propose **#364**).
Proposed text: `id: l0-pubgrub-masked-dep-exit / layer: l0-pubgrub /
categories: [exit] / slugs: [app-editors_gedit, gnome-base_nautilus,
gnome-base_gnome-shell] / match: "real rc=1 portuale rc=0" / reason:
Portage aborts on masked/unsatisfiable deps (gcr USE-combination masked;
gnome-shell also samba[client]); pubgrub has no masked-package
disclosure and exits 0. Evidence: …/triage.md (G4b).`

### G4c — REQUIRED_USE abort (1 probe)

`app-emulation_wine-vanilla`: real rc 1 with no merge list —
```
!!! The ebuild selected to satisfy "media-libs/libsdl2[haptic,joystick,abi_x86_32(-)?,abi_x86_64(-)?]" has unmet requirements.
- media-libs/libsdl2-2.32.8::gentoo USE="X alsa dbus haptic joystick ... wayland ..."
  The following REQUIRED_USE flag constraints are unsatisfied:
    wayland? ( gles2 )
```
(`depgraph.py:6932-6945` "has unmet requirements" display).
Portuale: 143 rows incl. libsdl2, rc 0 — the bridge/engine never
enforces REQUIRED_USE.

Disposition: **`allowlist owner: portuale-bug`** (propose **#365**).
Proposed text: `id: l0-pubgrub-required-use-exit / layer: l0-pubgrub /
categories: [exit] / slugs: [app-emulation_wine-vanilla] / match:
"real rc=1 portuale rc=0" / reason: Portage enforces REQUIRED_USE
(libsdl2 wayland→gles2) and aborts with no list; pubgrub does not
evaluate REQUIRED_USE at all (crate required_use.rs exists but the
bridge path lets the plan through) and prints 143 rows rc 0. Evidence:
…/triage.md (G4c).`

Owner alternative for all of G4 (flagged, not decided here): if the
owner judges rc parity for config-change aborts outside #34's
"same set + same actions" goal, these three entries may instead be
filed `owner: accepted-variance` (precedent exists in
`known-divergences.yaml`) — but S5's stop condition counts them either way.

---

## G5 — `||` first-listed fallback extras (gnome-keyring cluster)

Counts per mode: 3 extras in `-pv`/`-pvu`/`-pvuD`/`-pvuDN`
(`kde-apps_kdecore-meta` only), 3 of the 9 `-pve` kdecore extras.

Rows (verbatim, `-pv kde-apps_kdecore-meta`; identical in `-pvu`,
`-pvuD`, `-pvuDN` modulo the surrounding G1/G2 rows):
```
[ebuild  N     ] app-crypt/p11-kit-0.26.2::gentoo  USE="libffi nls systemd -gtk-doc -test -verify-sig" ...
[ebuild  N     ] app-crypt/gcr-3.41.2-r2:0/1::gentoo  USE="X introspection systemd vala wayland -gtk -gtk-doc -test"
[ebuild  N     ] gnome-base/gnome-keyring-48.0-r1::gentoo  USE="pam ssh-agent systemd -caps (-selinux) -test"
```
present for portuale, absent for real. Both plans contain
`virtual/secret-service-0-r1` AND `kwallet-runtime-6.27.0` (real:
`USE="X (keyring) (legacy-kwallet) man …"`); real satisfies the virtual
with kwallet-runtime, portuale additionally merges gnome-keyring.

Cause (established). `virtual/secret-service` RDEPEND is
`|| ( gnome-base/gnome-keyring >=kde-frameworks/kwallet-runtime-6.18.0[keyring]
app-admin/keepassxc app-admin/chipass )` (host-tree virtual; shape
matches). Neither alternative is installed, so the crate's
installed-preference (`provider/mod.rs:1164-1194`,
`newest_installed_choice_branch`) finds no branch and falls back to
`max()` = the first-listed alternative → gnome-keyring → gcr → p11-kit.
Portage prefers "atoms for packages that have been added to the graph …
over other choices" (`depgraph.py:778-793` comment) — kwallet-runtime is
already selected (the KDE target needs it), so the `||` resolves to it.
The choice happens inside `portage-solver` from the bridge's
`DepEntry::AnyOf` facts; static bridge-side child ordering cannot
express dynamic graph membership (the closure is a superset of the
solution, so closure-ordering would misfire where Portage picks a
non-first alternative that merely happens to be reachable).

Disposition: **`allowlist owner: engine`** + §0.4 crate-gap note
`LLM/evidence/2026-10-15-34-crate-gap.md` (propose backlog **#366** for
the engine filing). Proposed entry text:
`id: l0-pubgrub-or-ingroup-choice / layer: l0-pubgrub / categories:
[extra] / slugs: [kde-apps_kdecore-meta] / match: "present for portuale,
absent for real" / reason: engine-inherent ||-choice difference: the
crate falls back to the first-listed provider when no branch is
installed (provider/mod.rs newest_installed_choice_branch), while
Portage prefers the already-in-graph alternative (depgraph.py:778-793).
Filed as #366; see the crate-gap note. Evidence: …/triage.md (G5).`
Second example probe for the S5 record: `-pve kde-apps_kdecore-meta`
carries the same 3 rows among its 9 extras.

---

## G6 — installed-satisfiable deps merged anyway (mechanism open)

Counts per mode: docbook-xml-dtd-4.3 `NS` — 1 extra in `-pv`/`-pvu`/
`-pvuD`/`-pvuDN` (kdecore-meta) + 1 of 9 in `-pve` kdecore;
`-pve`-only over-reach — openssh chain (5: `acct-group/sshd`,
`acct-user/sshd`, `net-misc/openssh-10.4_p1-r1`, `virtual/openssh-0-r1`,
`sys-apps/shadow-4.19.4:0/5`) in kdecore + `app-doc/info-manual-30.2`
in `app-editors_emacs`.

- Example 1 — `-pv kde-apps_kdecore-meta`:
  `[ebuild  NS    ] app-text/docbook-xml-dtd-4.3-r2:4.3::gentoo [4.1.2-r7:4.1.2/::, 4.2-r3:4.2/::, 4.4-r3:4.4/::, 4.5-r2:4.5/::]`
  present for portuale, absent for real. VDB holds docbook slots 4.1.2,
  4.2, 4.4, 4.5 (no 4.3); known requirers pin installed slots
  (`:4.2` in xmlto RDEPEND / libsecret BDEPEND; `:4.5` in kdoctools —
  host-tree dep shapes); real shows no docbook row. Why the engine
  lands on the one slot that is neither installed nor (visibly)
  required is **not established** — slot-constraint loss in the
  bridge's facts vs engine `SlotChoice` needs the trace (left to the
  fix task; `PORTUALE_PG_TRACE=1` host-scratch reproduction per the
  task brief).
- Example 2 — `-pve kde-apps_kdecore-meta` (real 535 / portuale 535 —
  same total, different composition): portuale-only
  `[ebuild   R    ] net-misc/openssh-10.4_p1-r1 …`,
  `[ebuild   R    ] virtual/openssh-0-r1  USE="ssl"`,
  `[ebuild   R    ] sys-apps/shadow-4.19.2…:0/5 …` (+ both sshd acct
  rows), although openssh/shadow/virtual-openssh are all installed and
  real has no rows for them. Which installed parent's expanded dep pulls
  them under `-e` in pubgrub but not Portage is **not established**.
- Example 3 — `-pve app-editors_emacs` (real 341 / portuale 332):
  `[ebuild   R    ] app-doc/info-manual-30.2::gentoo` present for
  portuale, absent for real, although VDB holds info-manual-30.2 and
  both sides show `[ebuild R] sys-apps/texinfo-7.3` with identical USE.
  Conditional-evaluation divergence on the rebuild path — **not
  established** which conditional or which USE source.

Disposition: **`fix`** (one task with three fixtures: multi-slot
installed + `:slot` dep → expect no row; `-e` openssh/shape reproduction
from the trace; texinfo/info-manual conditional). If the trace shows the
docbook pick is engine-internal, that sub-row escalates to a §0.4
crate-gap like G5.

---

## G7 — soft-blocker auto-unmerge row

Counts per mode: `-pv` extra 1 · `-pvu` extra 1 · others 0
(`sys-apps_systemd-utils` only; under `-pvuD`/`-pvuDN` the extra is gone
and only G1's missing remains — noted but unexplained).

- Example — `-pv sys-apps_systemd-utils`: real prints two soft-block
  rows and no uninstall —
  `[blocks B      ] sys-apps/systemd ("sys-apps/systemd" is soft blocking …)`,
  `[blocks B      ] sys-apps/gentoo-systemd-integration ("sys-apps/gentoo-systemd-integration" is soft blocking …)`,
  `Conflict: 2 blocks (2 unsatisfied)`, `Total: 2 packages (2 new)`.
  Portuale prints the same two N rows plus
  `[uninstall     ] sys-apps/gentoo-systemd-integration-9-r2::gentoo`
  (and downgrades one block to lowercase `[blocks b]`;
  `Total: 2 packages (2 new, 1 uninstall)`).

Cause (partially established): the installed
`gentoo-systemd-integration-9-r2` soft-blocks the new `systemd-utils`
(its own RDEPEND needs `>=sys-apps/systemd-207`, absent — real's
"required by" dump shows the installed instance's unsatisfiable edge);
Portage keeps the block displayed and unresolved, while portuale
additionally schedules the blocker for unmerge. Which layer (bridge
blocker facts vs display) invents the uninstall row needs code reading
in the fix task.

Disposition: **`fix`**. Minimal fixture: installed soft-blocker of a
fresh target, `-pv` capture showing Portage's blocks-only rendering.

---

## What was NOT established (honest residue)

1. G6 mechanisms entirely (docbook slot pick; `-e` openssh/shadow and
   info-manual parents) — needs the `PORTUALE_PG_TRACE=1` scratch-trace
   runs the fix tasks will do (no cargo build was allowed in this step).
2. Host-tree ebuild citations (rust `curl[http2]`, xmlto `text?→w3m`,
   w3m `boehm-gc:=`, secret-service `||`, libsecret/docbook slots) are
   dep-*shapes* from a newer tree than the container's `11c58b7a`; the
   run-file rows corroborate each shape, but a fix task should confirm
   the exact dep strings from the container tree (or its own fixtures'
   captures, which supersede this reasoning anyway).
3. Whether `-pvuDN`/`-pve` hide further G2-shaped rows inside the
   now-matching plans — no: `-pvuDN` non-portage missing is 0 and `-pve`
   non-portage missing is 0 by aggregate, so nothing in the counted
   categories remains hidden there. Suppressed rows inside G3/G4 exit
   probes (e.g. nginx mime-types) are the known exception, flagged above.
4. Backlog numbers #363–#366 are *proposed* (max in use is #362 across
   `LLM/backlog-tasks.md` + `LLM/backlog-tasks-2026-10.md`); nothing was
   filed, no allowlist or crate-gap file was written — that is S5 step 2.
