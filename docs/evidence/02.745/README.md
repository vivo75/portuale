# Evidence for `LLM/02.745-746-property-2a.opus.md`

Naming: `02.745/` after the plan's first backlog number (the two entries
travel together); see also the bed logs in
`../pmtest/differential-test-bed/logs/`.

## S0 (capture-only, 2026-11-14)

- `six-orders.txt`: the #745 synthetic shape (`prov -RDEPEND-> user`,
  `user -BDEPEND-> virt`, `virt -RDEPEND-> prov`, all merged, nothing kept)
  under all six merge orders through the current `health_checker.py`:
  four orders yield only the accepted `cycle-noted`, two (`prov,virt,user`
  and `virt,prov,user`) yield a `build-order` failure on `user-1`. Matches
  the #745 entry's table.
- The #746 host shape no longer reproduces on this host (the installed
  `Locale-gettext` now records `dev-lang/perl:0/5.44=`, matching installed
  `0/5.44` — the binding repaired itself after 2026-11-13), but reproduces
  in the fixed bed image: `l0-health-20261114T011717Z/plans/FAST_world.json`
  has exactly one failure, `dev-build/automake-1.19` merged (#8) before the
  `dev-perl/Locale-gettext` it needs at build time (#31). A temporary
  `runtime_cut` dump on the #746-shaped graph (since removed) confirmed the
  cut lands on the RDEPEND-only edge `help2man -> Locale-gettext`.
- `second-build.json`: the load-bearing charter-2a plan for the
  `drop_second_build` mutant (`health-mutants.py --vdb /var/db/pkg
  second-build.json` kills it with a `build-order` on `dev-libs/w745p-1`).

## S1 (why the BackEdge arm keeps scanning)

- A "silent violation" is a cycle edge the final walk cuts without any rule
  naming it: `unordered()` ignores an edge back into a node it is still
  walking, so the order silently drops a constraint no cut and no breaker
  chose. (A finding the checker reported before and re-reports with
  different blame is *not* silent — it is visible, just re-blamed; see the
  12 below.)
- Count and derivation: on the bed FAST world (fixed image, seeded
  `@world`), `runtime_cut`'s loop with the old stop-at-first-BackEdge rule
  removed 4 back edges and then reported the graph acyclic (1117 edges, 367
  nodes in the dumped working set). Simulating the exact `unordered`
  depth-first walk on that dumped set finds 51 dependent-before-dependency
  pairs — 51 cycles the walk cuts implicitly, including the
  perl/libcrypt/libxcrypt region that put `perl` at #47 with 10
  `build-order` findings on the modules rebuilt against it. With the loop
  fixed to cut the back edge and keep scanning until truly acyclic,
  `unordered` sees a DAG and the count is 0 by construction (post-order on
  a DAG is exact); the bed FAST plan corroborates it with 0 findings and
  `perl` back at #3.

## S3 (verification)

- Host `mrg --plan-world --sort depend --json`, frozen binary before vs
  after: 349 rows both ways; checker findings 177 -> 62 (175 -> 60
  `build-order`); 4 second builds. The four slots built twice, each a
  same-version duplicate (so the final set — and charter property 1 — is
  unchanged; the checker collapses identical cpvs before the slot check):
  - `sys-libs/libxcrypt-4.5.2-r1` (orders 3, 4): breaks the
    `perl -> virtual/libcrypt -> libxcrypt -> perl` cycle (back edge
    `libxcrypt -> perl` is `BDEPEND`). Its first build was already
    checker-clean, so no `cycle-noted` was needed.
  - `net-libs/nghttp2-1.71.0` (orders 9, 28), `net-nds/openldap-2.6.14`
    (orders 12, 30), `dev-lang/python-3.15.2_p1` (orders 15, 31):
    breakers in the same libcrypt-region overlap (each overlapping cycle
    keeps every runtime edge because its source is build-used). Their
    first builds carry the transition needs below; the second copies
    change no version in the final set.
- The 12 re-blamed findings (all on packages that already failed before,
  via the pre-existing broken `openssl:0/3=` transition — installed
  `python-3.14.10` binds a gone sub-slot): `app-misc/ca-certificates`,
  `dev-lang/python-3.14.10_p1`, `dev-lang/python-3.15.2_p1`,
  `dev-python/platformdirs`, `media-video/pipewire`, `net-libs/nghttp2`,
  `net-nds/openldap`, `sys-apps/systemd`, `sys-apps/util-linux`,
  (twice:) `sys-libs/libnvme`, `sys-libs/pam` — each "`X` is not usable
  yet: RDEPEND of ..." with the root moved from the old libnvme/cryptsetup
  chain to the python-3.14/openssl chain. Same packages failed before;
  only the blamed need moved.
- Byte-identity on the acyclic fixture world (text, stderr and `--json`
  all identical old binary vs new).
- Bed `l0-health-20261114T035922Z` (final code): 122 plans, 113 checked,
  108 clean (was 107), 0 with failures; `FAST_world` 0 findings (was the one
  automake `build-order`); `Locale-gettext` #24 before `automake` #25,
  `perl-5.44` still #3; 15 second builds, no slot holds two versions, no
  `built_twice` under `--sort alpha`/`resolver`.
- Bed `l0-20261114T040319Z` (default L0 resolver, final code):
  row-identical to the `l0-20261114T012111Z` baseline — 120 probes, 91
  clean, parity 0.758, 103 explained, 0 UNEXPLAINED.
- `mrg --plan-world --sort alpha/resolver` on the all-build-time fixture:
  each slot once, no `built_twice` (resolver sort runs the resolver as a
  child process and succeeds on the ad-hoc tree).
