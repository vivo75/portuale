# crates/portage-repo/src/tests/installed_meta.rs
Live-metadata overlay for installed packages (`FakeVartree` port) tests.

## fn live_metadata_for_installed_prefers_the_recorded_repo
- Backlog #86 S1: `live_metadata_for_installed` reads the vdb-recorded repo's copy of the exact installed cpv, never the priority winner's -- Portage `FakeVartree._aux_get_wrapper` (`FakeVartree.py:136`, `myrepo=pkg.repo`).
- Two scratch repos, same cpv, different RDEPEND.
- No `repository` file: the pre-#86 priority search is the fallback, so minimal fixture vdbs keep their live view.
- Recorded repo no longer configured: same fallback.
- Gone from every repo: `None` (the caller's Raw fallback).

## fn installed_dep_string_effective_is_the_live_ebuild_value
- Backlog #25 S1b: the built-`:=` append is EAPI-gated exactly like Portage (`FakeVartree._apply_dynamic_deps`). These two fixtures' vdbs carry no `EAPI` file, so Portage reads the installed EAPI as `"0"` (`vartree.py:1054-1055`) -- no `slot_operator` support -- and the live value stands even though the vdb carries a built binding. (Without the gate this append broke both #24 pins, Phase 5 S1 S0.)
- --dynamic-deps=n and Raw collapse to the vdb record; the no-live-metadata arm is Portage's `_DynamicDepsNotApplicable` fallback (raw + global moves, identity in fixtures).
- Every live map below carries `EAPI`, like Portage's `portdb` `aux_get` always does -- a missing live `EAPI` is unsupported and takes the raw fallback (unreachable in Portage).
- slotopdepspkg's vdb RDEPEND is `dev-libs/slotoptarget:2=` -- the live value stands.
- revdepslotconsumer's vdb RDEPEND is `dev-libs/revdepslottarget:0/1=` -- built, but its vdb has no `EAPI` file: Effective is the live value (empty here), never the vdb binding.

## fn installed_dep_string_effective_appends_built_atoms_when_eapi_gated
- Backlog #25 S1b: Portage `FakeVartree._apply_dynamic_deps` (`lib/_emerge/FakeVartree.py:146-191`) on the hermetic r25 shape (findings/l0.md "## #25 S0 (2026-09-27)"): both vdb rows carry `EAPI` files (`8`), so the recorded bound atoms append to the live depstring -- `Depstring: dev-libs/r25lib:= >=dev-libs/r25lib-1.0:0/1=` (`realC-debug.log:35-47`).
- r25mid: live `dev-libs/r25lib:=`, vdb `>=dev-libs/r25lib-1.0:0/1=` (Portage's recorded bound form).
- r25consumer: live `<dev-libs/r25lib-2.0:=`, vdb `<dev-libs/r25lib-2.0:0/1=`.
- `--ignore-built-slot-operator-deps`: append suppressed, live stands (Portage `FakeVartree.py:166-168`).
- Live EAPI without `:=` support voids the whole overlay, not just the append (Portage `FakeVartree.py:176-178`): the raw record stands.
- Unsupported live EAPI: `_DynamicDepsNotApplicable`, raw stands (Portage `FakeVartree.py:158-162`).
