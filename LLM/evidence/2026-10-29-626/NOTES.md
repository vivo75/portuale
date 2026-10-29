# #626 — hyperfine-1.21.0 unpack failure: S0/S1 record

## Verbatim first error (portuale, `portuale-build.log`)

`cargo_crate_unpack` → `tar -x -C …/work/cargo_home/gentoo -f <crate>`:

- `tar: difflib-0.4.0/…: Cannot change ownership to uid 1000, gid 1000: Invalid argument`
  (same for `normalize-line-endings` 1000:1000, `number_prefix` 501:20, …)
- `tar: Exiting with failure status due to previous errors`
- eclass `die` → `ERROR: app-benchmarks/hyperfine-1.21.0::gentoo failed (unpack phase)`,
  call stack `src_unpack → cargo_src_unpack → cargo_crate_unpack → die`.
- Also at unpack start: `chgrp: changing group of '…/temp/eclass-debug.log': Invalid argument`
  (same EINVAL signature).

Fetch was clean (all 130 crates `[ ok ]`, digests verified). Resolver agrees
(`portage-pretend.txt` vs `portuale-pretend.txt`: identical
`[ebuild R ] … USE="-debug"`, one reinstall). Class: **(b) `src_unpack`/cargo
environment — more precisely, phase-execution uid vs namespace isolation**,
not fetch, not sandbox-QA, not merge-time. The `FEATURES=-test` control is
N/A: the failure precedes `src_test`.

## Mechanism (all steps reproduced live on this host)

1. Host `FEATURES` carries `sandbox usersandbox userpriv network-sandbox
   ipc-sandbox pid-sandbox` (`emerge-info.txt`). `unpack` is a sandboxed
   `src_*` phase (`sandboxed_src_phases.txt`), not network-exempt
   (non-live package), so portuale wraps it in
   `unshare --map-root-user [--net --ipc --pid …]`
   (`ebuild_phases/sandbox.rs`: `phase_isolation`, `unshare_flags` —
   `--map-root-user` first "so it works non-root").
2. `--map-root-user` builds a single-uid/gid-map user namespace. Any
   `chown(2)` to a uid/gid outside the map fails with **EINVAL**.
3. `cargo_crate_unpack` runs `tar -x` as (namespace-)root. GNU tar as root
   restores the recorded owners; the `.crate` files record foreign uids
   (`difflib` 1000:1000, `number_prefix` 501:20). Every such `chown` →
   EINVAL → tar exits nonzero → eclass `die`.
4. Direct repro (byte-identical error, no portuale involved):
   `sudo unshare --map-root-user --net tar -x -C <777 dir> -f difflib-0.4.0.crate`
   → `Cannot change ownership to uid 1000, gid 1000: Invalid argument`.
   Controls: same tar as real root without userns → rc 0; as `portage`
   user → rc 0 with no chown attempted (non-root tar never preserves owners).
5. Portage avoids it with `FEATURES=userpriv`: `_spawn_actionmap`
   (`3rdparty/portage/lib/portage/package/ebuild/doebuild.py:1795`)
   sets `droppriv` when `userpriv ∈ FEATURES` and `secpass >= 2` (root),
   so `src_unpack` runs as `portage:portage` and tar never chowns.
   Live proof: `sudo ebuild …/hyperfine-1.21.0.ebuild clean unpack` → rc 0,
   and `…/work/` plus `…/work/cargo_home/gentoo/` are owned `250:250`
   (`phase-env-portage.txt` cam from that run).
6. Portuale never demotes: no `droppriv`/`setuid` path exists in
   `crates/portuale/src/ebuild_phases/` (only `PORTAGE_USERNAME` in
   `environ_whitelist.txt`, i.e. env passthrough); `sandbox.rs:133`
   documents `sandbox`/`usersandbox` as "equivalent here: no `userpriv`".
   Standing rule: `LLM/scope-backlog.md:513` lists
   **`userpriv`/`fakeroot` as non-goals (Part 3)**.

## Phase-env comparison

`phase-env-portuale.txt` (failed run) vs `phase-env-portage.txt`
(successful unpack): `RUSTFLAGS`, `PORTAGE_TMPDIR`, `DISTDIR`, `FEATURES`,
`EBUILD_PHASE` all match. `CARGO_HOME`/`ECARGO_*` appear in Portage's saved
`environment` but not in portuale's — yet portuale's live log unpacks to
the correct `…/work/cargo_home/gentoo` path, so the vars were set at
execution time and the saved-file gap is dump timing, **not causal**.
Recorded as an observation only (needs a check before any filing).

## Stop-or-scope gate → STOP (owner decision required)

The only Portage-faithful fix is implementing `userpriv` phase demotion
(`droppriv` + the `_post_phase_userpriv_perms` tail,
`doebuild.py:2485`), which contradicts the Part 3 non-goal and is a
multi-slice feature (spawn uid/gid/groups, per-phase RESTRICT gates,
`usersandbox` interplay, root-gated tests), not a one-slice fix.
No narrower fix is faithful: Portage-as-root-without-userpriv fails
identically (same tar, same userns), and the eclass command line cannot
be altered (it is Portage's file). S2 stays unscopped until the owner
either lifts the non-goal or directs a documented cut
(e.g. `--no-same-owner` injection — a deliberate divergence needing an
owner sign-off and a `portage-quirks.md` entry, since Portage would
still fail that way as root).
