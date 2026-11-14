# Phase 2 evidence — closing #745 with a usability closure (2026-11-14)

Plan `LLM/02.745-746-property-2a.opus.md` §8. The S0–S4 instruments left
#745 open: the cut still sacrifices the cuttable `(prov, user)` edge, so
the order stays `[prov, virt, user]` and the checker still fails `user`
(the need being the consumer itself at the same order).

## Fixture (`--plan-world --sort depend` on the w745 tree)

Tree (ad-hoc, `plan_world.rs` test): `w745prov-1` RDEPENDs `w745user`,
`w745user-1` BDEPENDs `w745virt`, `w745virt-1` RDEPENDs `w745prov`, all new.
Rows, text and `--json`:

```
[ebuild  N     ] dev-libs/w745prov-1:0
[ebuild  N     ] dev-libs/w745virt-1:0
[ebuild  N     ] dev-libs/w745user-1:0
[ebuild  N     ] dev-libs/w745user-1:0
```

Only the second `w745user` entry carries `"built_twice": 2` (no
`from_version` — `N` has none anyway). Checker (`--vdb` of the scratch
tree): one `cycle-noted` on `dev-libs/w745user-1`
("`BDEPEND dev-libs/w745virt is not usable yet: RDEPEND of
dev-libs/w745prov-1 is not matched by anything installed or merged before
it`"), zero failures. The duplicate-free plan (`dev-libs/w745prov-1`,
`dev-libs/w745virt-1`, `dev-libs/w745user-1` literal dicts) still reports
one `build-order` on `dev-libs/w745user-1` (`test_745_duplicate_free_mixed_cycle_still_fails`),
so the fix is provably planner-side.

## Host `@world` (final binary, VDB `/var/db/pkg`)

349 rows, 8 second builds, all §2.2 breakers — §8 adds none here
(verified by running the host plan with the closure stubbed to identity:
identical 8 slots): `net-libs/nghttp2` (25), `sys-libs/pam` (26),
`sys-fs/cryptsetup` (28), `app-crypt/tpm2-tss` (29), `net-nds/openldap`
(30), `dev-lang/python` (31), `sys-apps/systemd` (33), `media-video/pipewire`
(41). Checker: 60 `build-order` + 1 pre-existing `runtime-deps`
(`cargo-c`), i.e. unchanged by §8; 0 `cycle-noted` (no first build is
excused because no unmet need gets matched later on this transition
mid-flight host). STOP rule (§8: "more than a handful of second builds"):
not triggered — §8 itself duplicates nothing on this host.

## Beds (final code)

- `l0-health-20261114T051249Z`: 122 plans, 113 checked, 108 clean, 0 with
  failures; `FAST_world` 0 findings (187 entries, 172 unique; `perl-5.44`
  #3, `Locale-gettext` #24, `automake` #25; 15 second builds, capped at
  two per slot, no triplets). All 15 are §2.2 breakers — §8 adds none on
  the bed either: `FAST_world.json` from the pre-§8 run
  (`l0-health-20261114T035922Z`) and this run carry the identical 187
  entries and the identical 15 `built_twice` slots.
- `l0-20261114T053052Z` (default L0 resolver, final code): row-identical
  to the `l0-20261114T040319Z` baseline — 120 probes, 91 clean, parity
  0.758, 103 explained, 0 UNEXPLAINED. The resolver never calls the
  changed code (`depend_order`/`close_usability_gaps` are only reached
  from `World::ordered` for `--plan-world --sort depend` and its unit
  tests), so the ordering change cannot move the resolver bed either way.
