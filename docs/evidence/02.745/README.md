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

## S3 (verification)

- Host `mrg --plan-world --sort depend --json`, frozen binary before vs
  after: 349 rows both ways; checker findings 177 -> 62 (175 -> 60
  `build-order`); 4 second builds (`libxcrypt`, `nghttp2`, `openldap`,
  `python:3.15`). The 12 re-blamed findings sit on packages that already
  failed, via the pre-existing broken `openssl:0/3=` transition (installed
  `python-3.14.10` binds a gone sub-slot). Byte-identity on the acyclic
  fixture world (text, stderr and `--json` all identical old vs new).
- Bed `l0-health-20261114T035922Z` (final code): 122 plans, 113 checked,
  108 clean (was 107), 0 with failures; `FAST_world` 0 findings (was the one
  automake `build-order`); `Locale-gettext` #24 before `automake` #25,
  `perl-5.44` still #3; 15 second builds, no slot holds two versions, no
  `built_twice` under `--sort alpha`/`resolver`.
- `mrg --plan-world --sort alpha/resolver` on the all-build-time fixture:
  each slot once, no `built_twice` (resolver sort runs the resolver as a
  child process and succeeds on the ad-hoc tree).
