# G243 report — backlog #243: the `slot_operator_mask_built` probe honours ebuild visibility (2026-09-28)

Worktree `/home/vivo/repo/PORTUALE/wt-243-slotop-mask-visibility/{portuale,pmtest}`,
branch `backlog/243-slotop-mask-visibility`.
Real source: vendored portage 3.0.82.2 (`3rdparty/portage/lib/_emerge/depgraph.py`).

## Status: NEEDS_CONTEXT (S0 committed; no product commit — see §5)

## Commits

- pmtest `4e79ed8` — `fixtures: #243 S0 stale-slot binary under a
  keyword-masked ebuild + g212 atomlist cell` (S0 only: EAPI 8 pair
  `dev-libs/somaskvischild`/`somaskvisparent`, genuine child binpkg at
  SLOT 0/1 via host `ebuild package`, Packages stanza, `--usepkg` cell
  in `l0-fixture-oracle-g212.txt`). No CASES/pin (product unchanged).
- portuale: NO commit (deliberate STOP, §5).

No `READY-FOR-BEDS` written (no product sha exists). No `BEDS` reply awaited.

## §1. Real mechanism (cited before code, per brief)

- `_slot_change_probe` (`lib/_emerge/depgraph.py:2317-2359`): fires only
  for an **unbuilt parent + built child** (`:2322-2325`); looks up the
  same-version ebuild (`=cpv`, `:2328-2338`); skips `runtime_pkg_mask`
  (`:2341`) and `--exclude`d (`:2343`); **returns None unless the ebuild
  passes `_pkg_visibility_check`** (`:2347`); masks only on slot/sub-slot
  difference (`:2351-2356`). With `autounmask_level=None` (the probe
  passes none), `_pkg_visibility_check` (`:7562-7576`) returns False for
  any masked, non-graph ebuild — source-proven, no ambiguity.
- Pool side, `_wrapped_select_pkg_highest_available_imp`: the
  `_equiv_ebuild_visible` gate (`:8015-8052`) applies only `if
  (use_ebuild_visibility or matched_packages)` (`:8019`); under
  `--usepkgonly`/`--useoldpkg-atoms` match it is skipped (`:8027-8033`).
- Portuale counterparts: probe parent gate
  (`rust/portage-repo/src/lib.rs:15924-15928`, ebuild-only) and binary
  arm tree lookup (`:16058-16068`, raw `list_candidates`, no visibility
  filter) with the overclaiming comment at `:16012-16027`.

## §2. S0 shapes tried (all EAPI 8, RDEPEND-only — no #242 cross-root content)

Fixture pair (committed): child tree ebuild SLOT `0/2`, KEYWORDS `~amd64`
(keyword-masked); genuine child binpkg SLOT `0/1` (host `ebuild digest` +
`USE="-*" ... ebuild ... package`, layout.conf `repo-name = testrepo`,
BUILD_TIME 1790594659, SIZE 20480); parent ebuild-only, unbuilt
`RDEPEND="dev-libs/somaskvischild:="`; Packages stanza mirrors the g212
format; md5-cache entries computed with `md5sum`.

1. **Literal entry shape `--usepkgonly`**: NO divergence possible.
   Parent resolves built → real's `not dep.parent.built` (`:2322-2325`)
   and portuale's ebuild-parent gate both skip. Portuale run (worktree
   release binary): `[binary N] child + [binary N] parent`, rc 0 —
   the keep-the-binary outcome on both sides.
2. **V1 `--usepkg` (ebuild-only parent)**: BOTH fail alike, rc 1, masked
   by `~amd64`. Real probed verbatim (container
   `localhost/test-portuale:latest`, fixtures copied to /tmp/fx,
   binrepos.conf removed, categories generated — the g212 staging;
   full command + output in `/tmp/opencode/g243/real-probe.log`):
   `!!! All ebuilds that could satisfy "dev-libs/somaskvischild:=" ...
   (masked by: ~amd64 keyword)`, `backtrack: 1/20`. Portuale pre-fix:
   same message, `backtrack: 1/10` (probe mask + restart + dead-end).
   Per §1 the probe returns None in real, so real's binary never reached
   it — rejected at selection by an unidentified sub-gate (plausibly
   `binpkg-respect-use`/`_reinstall_for_flags` with `myeb=None`; see
   §5 Q2). Same output, different path.
3. **V2 `--usepkg --useoldpkg-atoms 'dev-libs/somaskvischild'`**: TRUE
   divergence. Portuale pre-fix (worktree binary): probe masks,
   restarts, dead-ends rc 1 (same masked text). Real, source-derived:
   UEB skipped (`:8027-8033`, atoms match), `:8235` respect_use block
   skipped (`not useoldpkg` false), binary selected; probe returns None
   (§1) → keeps binary → `[binary N] child + [ebuild N] parent`, rc 0.
   Supported by the #212 precedent (binary reached `dep.child.built`
   under `--usepkg`). **Real's V2 output is source-derived, UNPROBED**
   (probe budget spent on V1).

## §3. Why STOP (no product commit)

Porting only the probe gate (`is_visible` on the tree candidate before
`masked.insert`, real `:2347`) repairs V2 but **regresses V1**: post-fix
portuale on V1 keeps its selection (no mask, no restart) → rc 0
`[binary N] + [ebuild N]` vs real rc 1 (probed). The V1 parity gap is a
pool-side (selection) rule whose exact real sub-gate is unidentified —
porting a guess would violate "never invent what real does". One rule
per commit cannot cover probe+pool, and the pool half exceeds the entry's
prescribed scope ("add the visibility gate" to the probe). Per common
rule 4 and the brief: STOP, NEEDS_CONTEXT.

## §4. Verification

- Host `ebuild package` builds (2, incl. a discarded parent binpkg that
  would have short-circuited the probe): exit 0; gpkg metadata inspected
  (SLOT 0/1, testrepo, BUILD_TIME).
- Pre-fix portuale runs (release binary): `--usepkgonly` → rc 0 binaries
  kept; `--usepkg` (V1) → rc 1 mask+restart+dead-end; V2 → rc 1
  mask+restart+dead-end. All logged in §2.
- Real probe (container, once): V1 rc 1 masked error, verbatim in
  `/tmp/opencode/g243/real-probe.log`. (An earlier host-direct attempt
  failed on host config leakage — staging noise, not counted.)
- `test_fixture_caches.py`: 6 passed. #212 pin: passed. Focused
  `usepkg|usepkgonly|useoldpkg|binpkg|binary|slot_operator|slotop`
  subset: 143 passed, 3 skipped, 2 xfailed.
- Full pmtest suite (fresh `--basetemp=/var/tmp/pmtest-g243-full`,
  `-p no:cacheprovider`): **2116 passed / 37 skipped / 4 xfailed /
  3 failed** (log `/tmp/opencode/g243/pmtest-full.log`). The 3 failures
  (`test_profiles_updates_package_moves_pinned_output[args5,args7]`,
  `test_package_moves_n_disables_profiles_updates`) are caused by
  **foreign vdb dirt in this worktree** (deleted
  `fixtures/var/db/pkg/dev-libs/oldmovepkg-1.0/{CATEGORY,SLOT}`,
  flipped `slotmovepkg-1.0/SLOT` 0→1, untracked `newmovepkg-1.0/`,
  mtimes 11:18/11:33 — concurrent agents `g195b/g198/g238/n240` are
  active; not mine, not touched, left unstaged). Failing assertions
  reference exactly those packages; nothing references `somaskvis*`.
  Corpus drift lists only those 3 tests — NOT blessed (coordinator call).
- No product change → `cargo fmt`/`clippy`/`cargo test` N/A (nothing to
  gate); release binary built once for S0 runs (3m05s).
- Rule 17: no GnuPG home created (running `gpg-agent --homedir /tmp/`
  instances are all `/tmp/n194-oracle-gpg-*`, another item's). Nothing
  to kill.
- `pmtest/3rdparty/portage` present (3.0.82.2).

## §5. Questions for the owner/coordinator (NEEDS_CONTEXT)

- **Q1 (scope):** land the probe gate + V2 pin now (accepting a V1
  rc1→rc0 regression vs probed real rc 1), or hold the probe fix until
  the pool half is identified? I recommend the latter; the pool half
  needs its own backlog number (next free — check all tiers first) and
  probably a `--debug` real probe of V1 selection.
- **Q2 (grounding, pool half):** which selection sub-gate rejects the V1
  binary in real? Candidate: `binpkg-respect-use`/`_reinstall_for_flags`
  with `myeb=None` (`:8259-8284`, `now_use=PORTAGE_USE` vs the #212
  `myeb` path that keeps). Needs a `--debug` probe to confirm.
- **Q3 (probe budget):** V2's real output (rc 0, `[binary N] child +
  [ebuild N] parent`) is source-derived only. One coordinator-run
  container probe (`--usepkg --useoldpkg-atoms 'dev-libs/somaskvischild'
  dev-libs/somaskvisparent` on this branch's fixtures) would confirm it
  for the pin.

## BED-PENDING (coordinator only, from the pmtest worktree)

1. `differential-test-bed/run/l0-fixture-oracle-all.sh` — expect the new
   `--usepkg dev-libs/somaskvisparent` cell in `l0-fixture-oracle-g212.txt`
   green (agreement cell: both sides rc 1 masked-by-~amd64; watch for
   comparator noise on `backtrack: 1/10` vs `1/20` and the `for <ROOT>/`
   suffix) and no movement elsewhere.
2. `differential-test-bed/run/l0-resolver.sh` — expect identical-or-better
   vs the last green L0 row (Track G guard; S0 adds no product change, so
   any movement is the foreign vdb dirt's, not this item's).

## Concerns / disclosures

1. The entry's "real keeps the binary" does not hold on the shapes tried
   except source-derived V2; the literal `--usepkgonly` flag cannot reach
   the probe at all. The g212-review Important's `/useoldpkg` alternative
   is the live one.
2. `backtrack_missed_updates` reason-skip tie-in (entry's second half)
   untouched — no product change.
3. The pmtest worktree has foreign uncommitted vdb dirt (§4); my commit
   stages only the 7 S0 paths. The portuale worktree is untouched
   (no product edit, clean tree).
4. Did not flip backlog entry #243 (brief).

---

## Round 2 (2026-09-28, `g243b-brief.md`) — STATUS: NEEDS_CONTEXT (no product commit)

Merges (brief-ordered `merge main into both first`): portuale
`backlog/243-slotop-mask-visibility` had no local commits, so it
fast-forwarded `a046502f` → `7a8587a0` (then-current main); pmtest merged
as `4c102c3` (over S0 `4e79ed8`). `main` has since moved to `738acfa6`
(#216 DONE, 12:27 UTC, a concurrent agent) — not chased; every gate below
ran on `7a8587a0`. Common-rule-2 tension noted: the merge was explicitly
ordered by the brief (and contemplated by rule 17); no other
merge/rebase/push/branch operations were performed.

### Probe (a): V1 with `--debug` (verbatim)

Command (from the pmtest worktree; `localhost/test-portuale:latest`):

```
touch /tmp/opencode/g243/make.local && podman run --rm -v "/home/vivo/repo/PORTUALE/wt-243-slotop-mask-visibility/pmtest/fixtures:/fixtures:ro" -v "/tmp/opencode/g243/make.local:/etc/make.local:ro" -e PYTHONHASHSEED=0 --entrypoint /bin/bash localhost/test-portuale:latest -c 'cp -r /fixtures /tmp/fx && rm -f /tmp/fx/etc/portage/binrepos.conf && for r in repo overlay repnamerepo; do ls /tmp/fx/$r | grep -v profiles | grep -v metadata | grep -v eclass | grep -v licenses | tr "\n" " " > /tmp/fx/$r/profiles/categories; done; export PORTAGE_CONFIGROOT=/tmp/fx ROOT=/tmp/fx PORTAGE_RUNNING_ROOT=/tmp/fx DISTDIR=/tmp/fx/distfiles; export PORTAGE_REPOSITORIES="[DEFAULT]
main-repo = testrepo
[testrepo]
location = /tmp/fx/repo
[overlay]
location = /tmp/fx/overlay
priority = 10
[repnamesection]
location = /tmp/fx/repnamerepo
priority = 40"; echo =====PROBE-A-emerge-p---debug---usepkg-dev-libs-somaskvisparent=====; emerge -p --color=n --debug --usepkg dev-libs/somaskvisparent; echo "rc=$?"' > /tmp/opencode/g243b/real-probe-a-debug.log 2>&1
```

Output (`/tmp/opencode/g243b/real-probe-a-debug.log`, 214 lines):

```
=====PROBE-A-emerge-p---debug---usepkg-dev-libs-somaskvisparent=====
[DEBUG] Using selector: EpollSelector
!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability

Performing Global Updates
(Could take a couple of minutes if you have a lot of binary packages.)
  .='update pass'  *='binary update'  #='/var/db update'  @='/var/db move'
  s='/var/db SLOT move'  %='binary move'  S='binary SLOT move'
  p='update /etc/portage/package.*'
/tmp/fx/repo/profiles/updates/2Q-2024..
ERROR: Update type not recognized '# Quarter 2 2024 package moves (fixture)'


!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability

Performing Global Updates
(Could take a couple of minutes if you have a lot of binary packages.)
  .='update pass'  *='binary update'  #='/var/db update'  @='/var/db move'
  s='/var/db SLOT move'  %='binary move'  S='binary SLOT move'
  p='update /etc/portage/package.*'
/tmp/fx/repo/profiles/updates/2Q-2024..
ERROR: Update type not recognized '# Quarter 2 2024 package moves (fixture)'


!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
!!! Repository 'overlay' is missing masters attribute in '/tmp/fx/overlay/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
!!! Repository 'repnamefromfile' is missing masters attribute in '/tmp/fx/repnamerepo/metadata/layout.conf'
!!! Set 'masters = testrepo' in this file for future compatibility
--- Invalid atom in /tmp/fx/repo/profiles/package.use.force: */pkgusemaskforcepkg
Undefined license group 'FREE'
FEATURES variable contains unknown value(s): cgroup, observability
WARNING: One or more repositories have missing repo_name entries:

	/tmp/fx/overlay/profiles/repo_name

NOTE: Each repo_name entry should be a plain text file containing a
unique name for the repository on the first line.


myaction None
myopts {'--debug': True, '--pretend': True, '--color': 'n', '--regex-search-auto': 'y', '--usepkg': True}


myparams {'recurse': True, 'binpkg_respect_use': 'auto', 'autounmask': True, 'autounmask_keep_use': False, 'autounmask_keep_license': True, 'autounmask_keep_keywords': True, 'autounmask_keep_masks': True, 'ignore_soname_deps': 'y', 'dynamic_deps': True, 'implicit_system_deps': True, 'binpkg_changed_deps': 'auto'}


These are the packages that would be merged, in order:

Calculating dependencies

      Arg: dev-libs/somaskvisparent
     Atom: dev-libs/somaskvisparent
   ebuild: dev-libs/somaskvisparent-1.0::testrepo

Child:         (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/') USE="" ELIBC="glibc"
Parent Dep:    dev-libs/somaskvisparent

Parent:    (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/')
Depstring: dev-libs/somaskvischild:=
Priority:  runtime

Parent:    (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/')
Depstring: dev-libs/somaskvischild:=
Priority:  runtime
Candidates: ['dev-libs/somaskvischild:=']


backtracking due to unsatisfied dep:
    parent: (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/')
  priority: runtime_slot_op
      root: /tmp/fx/
      atom: dev-libs/somaskvischild:=



backtracking try 1

forced reinstall atoms:


slot operator dependencies:


forced rebuilds:



!!! All ebuilds that could satisfy "dev-libs/somaskvischild:=" for /tmp/fx/ have been masked.
!!! One of the following masked packages is required to complete your request:
- dev-libs/somaskvischild-1.0::testrepo (masked by: ~amd64 keyword)

(dependency required by "dev-libs/somaskvisparent-1.0::testrepo" [ebuild])
(dependency required by "dev-libs/somaskvisparent" [argument])
For more information, see the MASKED PACKAGES section in the emerge
man page or refer to the Gentoo Handbook.



runtime_pkg_mask: {<Package ('ebuild', '/tmp/fx/', 'dev-libs/somaskvisparent-1.0', 'merge', 'testrepo')>: {'missing dependency': {(<Package ('ebuild', '/tmp/fx/', 'dev-libs/somaskvisparent-1.0', 'merge', 'testrepo')>, '/tmp/fx/', 'dev-libs/somaskvischild:=')}}}



      Arg: dev-libs/somaskvisparent
     Atom: dev-libs/somaskvisparent


backtracking aborted after 1 tries

forced reinstall atoms:


slot operator dependencies:


forced rebuilds:



!!! All ebuilds that could satisfy "dev-libs/somaskvisparent" for /tmp/fx/ have been masked.
!!! One of the following masked packages is required to complete your request:
- dev-libs/somaskvisparent-1.0::testrepo (masked by: backtracking: missing dependency)

For more information, see the MASKED PACKAGES section in the emerge
man page or refer to the Gentoo Handbook.



      Arg: dev-libs/somaskvisparent
     Atom: dev-libs/somaskvisparent
   ebuild: dev-libs/somaskvisparent-1.0::testrepo

Child:         (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/') USE="" ELIBC="glibc"
Parent Dep:    dev-libs/somaskvisparent

Parent:    (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/')
Depstring: dev-libs/somaskvischild:=
Priority:  runtime

Parent:    (dev-libs/somaskvisparent-1.0:0/0::testrepo, ebuild scheduled for merge to '/tmp/fx/')
Depstring: dev-libs/somaskvischild:=
Priority:  runtime
Candidates: ['dev-libs/somaskvischild:=']
... done!
Dependency resolution took 0.24 s (backtrack: 1/20).

forced reinstall atoms:


slot operator dependencies:


forced rebuilds:



!!! All ebuilds that could satisfy "dev-libs/somaskvischild:=" for /tmp/fx/ have been masked.
!!! One of the following masked packages is required to complete your request:
- dev-libs/somaskvischild-1.0::testrepo (masked by: ~amd64 keyword)

(dependency required by "dev-libs/somaskvisparent-1.0::testrepo" [ebuild])
(dependency required by "dev-libs/somaskvisparent" [argument])
For more information, see the MASKED PACKAGES section in the emerge
man page or refer to the Gentoo Handbook.

!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-10-portuale-format1-useatom/2026-09-10-portuale-format1-useatom.en.txt
!!!   line 7: Display-If-Installed: dev-libs/infoinstpkg[alpha]
!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-09-portuale-format1-slotatom/2026-09-09-portuale-format1-slotatom.en.txt
!!!   line 7: Display-If-Installed: dev-libs/samepkg:1
!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-08-portuale-malformed/2026-09-08-portuale-malformed.en.txt
!!!   line 7: Display-If-Installed: dev-libs/infoinstpkg[[bad

 * IMPORTANT: 5 news items need reading for repository 'testrepo'.
 * Use eselect news read to view new items.

rc=1
```

What (a) establishes: the parent resolves to the ebuild; the child
`:=` dep is unsatisfied at *selection* — no `binary:` match line ever
appears, the dep backtracks immediately (`runtime_slot_op`), rc 1
masked-by-`~amd64`. Debug `myparams` confirm the defaults the audit
relies on: `binpkg_respect_use: auto`, `binpkg_changed_deps: auto`, no
`changed_slot`/`selective`/`rebuilt_binaries`; `myopts` has no
`--newrepo`/`--newuse`/`--reinstall`. --debug names no single rejecting
line (real prints matches only), so the gate is identified by
elimination (see Findings).

### Probe (b): V2 `--useoldpkg-atoms` (verbatim)

Command: same staging as (a), with `echo
=====PROBE-B-emerge-p---usepkg---useoldpkg-atoms-dev-libs-somaskvischild-dev-libs-somaskvisparent=====;
emerge -p --color=n --usepkg --useoldpkg-atoms dev-libs/somaskvischild
dev-libs/somaskvisparent; echo "rc=$?"` (`/tmp/opencode/g243b/real-probe-b-useoldpkg.log`).

Output (staging noise identical to (a), elided here to the result block —
full log on disk; the elided head is byte-identical Global
Updates/profile/news noise):

```
These are the packages that would be merged, in order:

Calculating dependencies  ... done!
Dependency resolution took 0.21 s (backtrack: 1/20).


!!! All ebuilds that could satisfy "dev-libs/somaskvischild:=" for /tmp/fx/ have been masked.
!!! One of the following masked packages is required to complete your request:
- dev-libs/somaskvischild-1.0::testrepo (masked by: ~amd64 keyword)

(dependency required by "dev-libs/somaskvisparent-1.0::testrepo" [ebuild])
(dependency required by "dev-libs/somaskvisparent" [argument])
For more information, see the MASKED PACKAGES section in the emerge
man page or refer to the Gentoo Handbook.

!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-08-portuale-malformed/2026-09-08-portuale-malformed.en.txt
!!!   line 7: Display-If-Installed: dev-libs/infoinstpkg[[bad
!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-09-portuale-format1-slotatom/2026-09-09-portuale-format1-slotatom.en.txt
!!!   line 7: Display-If-Installed: dev-libs/samepkg:1
!!! Invalid news item: /tmp/fx/repo/metadata/news/2026-09-10-portuale-format1-useatom/2026-09-10-portuale-format1-useatom.en.txt
!!!   line 7: Display-If-Installed: dev-libs/infoinstpkg[alpha]

 * IMPORTANT: 5 news items need reading for repository 'testrepo'.
 * Use eselect news read to view new items.

rc=1
```

### Findings (why this is a STOP, not a port)

- **F1 — S0's source-derived V2 is REFUTED.** Real under `--usepkg
  --useoldpkg-atoms 'dev-libs/somaskvischild'` fails alike (rc 1, same
  masked text, `backtrack: 1/20`), it does not keep the binary (no rc 0,
  no `[binary N]`). The g212 cell's V2 paragraph was therefore false and
  is corrected by standalone pmtest `32c4597` (comment-only, no pins
  touched). There is no `--useoldpkg-atoms` divergence on this shape.
- **F2 — the §5 Q2 candidate is RULED OUT.** The suspected gate,
  `reinstall_use or (not installed and respect_use)` with `myeb=None`
  (`depgraph.py:8259-8284`, `now_use=PORTAGE_USE`): (i) statically, our
  binary has empty IUSE/USE, so `_reinstall_for_flags` (`:3134-3161`,
  first branch) computes the empty set → returns None → no rejection;
  (ii) empirically, probe (b) skips the *entire* `built and not
  useoldpkg` block (`:8233-8288`) via `not useoldpkg`, yet the binary is
  still rejected. The rejector is outside `:8259-8284`. (The other block
  members are likewise inactive: no `--newrepo` in `myopts`, no
  `changed_slot` in the debug `myparams`, `_changed_deps` False — binary
  and ebuild RDEPEND both empty over `_runtime_keys`.)
- **F3 — elimination ledger for the V1/V2-common rejector** (all in
  vendored 3.0.82.2, all checked this round): UEB gate (`:8016-8033`,
  inactive — `matched_packages` empty, no `--use-ebuild-visibility`; V2
  additionally exempt); `dbs` order verified ebuild-first (`:809-826`);
  `dep_expand` is identity for non-virtual (`dbapi/dep_expand.py:48-53`);
  `match_from_list(:=, bare-cpv)` verified True on host python;
  `_will_replace_child` returns None (cp mismatch, `:7350`);
  `_minimize_children` len-1 path yields directly; excluded/usepkg_ex-
  clude/include empty; no invalid-USE list; atom carries no USE;
  vardb/reinstall_atoms empty; post-loop len-1 returns as-is
  (`:8492-8493`); autounmask levels (`_autounmask=True` per debug
  `myparams`) should return a visible binary, yet none does.
- **F4 — survivors (unobserved, opposite port directions):**
  `:7992 _pkg_visibility_check(binary)` (the binary's own `visible`
  flag — every checkable input is clean: `KEYWORDS=amd64` ACCEPTED as
  proven by the same-run parent selection, no `package.mask` hit, EAPI
  8, empty deps, missing CHOST explicitly accepted
  (`config.py:2684-2685`), but the flag itself is unobserved) vs the
  bintree match-yield (`_iter_match_pkgs_atom`, `:7117-7169`:
  `cp_list`/`self._pkg` construction/`findAtomForPackage` on the Package
  instance). Naming one without observing it would violate "never invent
  what real does" (common rule 4).
- **F5 — consequence for the entry.** Q1's hold is confirmed, not just
  recommended: post-merge portuale still probe-masks+restarts+dead-ends
  on V1 (rc 1, `backtrack: 1/10`, live-verified this round), so landing
  only the probe gate would flip V1 rc 1 → rc 0 (`[binary N]` kept) vs
  real rc 1. And with (b), the "selection rule" of the brief's first
  commit has no portable line. Untried hypothesis for the coordinator:
  `--usepkgonly` + the (now ebuild-only) parent — S0's shape 1 carried a
  parent binpkg, so the probe-gate shape under `--usepkgonly` was never
  actually probed on the committed fixtures.
- **F6 — `backtrack_missed_updates` reason-skip tie-in** (entry's second
  half) still untouched.

### Commits, guard, gates (round 2)

- portuale: NO product commit (deliberate STOP). Tree clean at
  `7a8587a0`. No `READY-FOR-BEDS` written — the guard triggers per
  *product* commit and none exists, so no `BEDS` reply is awaited and
  this report does not exit-pending on one.
- pmtest `32c4597` (standalone, 1 file — the g212-cell comment fix; the
  foreign vdb dirt stays unstaged/untouched).
- `cargo fmt --check` (workspace `rust/`): clean. `cargo clippy
  --release --all-targets`: zero warnings
  (`/tmp/opencode/g243b/clippy-release.log`). `cargo test --release`:
  **1850 passed / 0 failed** (`/tmp/opencode/g243b/cargo-test-release.log`).
  Release rebuild post-merge 2m24s; live post-merge portuale runs: V1 rc
  1 masked (`1/10`), V2 rc 1 masked (`1/10`) — output-agreement with real
  holds, path differs as in S0.
- Full pmtest suite (fresh `--basetemp=/var/tmp/pmtest-g243b-full`, `-p
  no:cacheprovider`): **2137 passed / 37 skipped / 4 xfailed / 3 failed**
  (`/tmp/opencode/g243b/pmtest-full.log`) — the 3 failures are the same
  foreign-dirt `package-moves` pins as round 1, drift lists only those 3,
  NOT blessed. Brief-deviation disclosure: I did **not** run the ordered
  `git checkout -- fixtures/var && git clean -fdq fixtures/` — the dirt
  is unchanged from round 1 (other items' live fixtures, not debris) and
  cleaning would delete a concurrent agent's untracked tree.
- Rule 17: no GnuPG home created. `pmtest/3rdparty/portage` present.

## BED-PENDING round 2 (coordinator only, from the pmtest worktree)

1. `differential-test-bed/run/l0-fixture-oracle-all.sh` — the V1
   `--usepkg dev-libs/somaskvisparent` cell should stay green post-merge
   and post-comment-edit (comment-only; same agreement: both rc 1
   masked-by-`~amd64`; known comparator noise `1/10` vs `1/20`, `for
   <ROOT>/` suffix).
2. `differential-test-bed/run/l0-resolver.sh` — identical-or-better vs
   the last green L0 row (Track G guard).

## Answers to §5 / brief Q1–Q3

- **Q1:** hold CONFIRMED (F5) — do not land the probe gate.
- **Q2:** §5 candidate REFUTED (F2); rejector narrowed to F4's two
  survivors. Needs a live-introspection probe (container depgraph REPL
  or instrumented `--debug` naming the dropping line) — beyond the two
  authorised probes, so not run.
- **Q3:** ANSWERED by probe (b) — V2 real output is rc 1 masked, not the
  S0-derived rc 0; no V2 pin, no second cell.
- Backlog entry #243 still OPEN, not flipped (brief).
