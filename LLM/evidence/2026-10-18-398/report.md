# #398 flip-path disjunction deferral — oracle + work report (2026-10-18)

Branch `backlog/398-flip-disjunction` in both repos (pre-existing, not
switched). **No commits, no pushes** (per brief). **A divergence WAS shown**
(variant v3 below) and is fixed on this branch.

## Oracle recipe (host Portage 3.0.82.2, say "Portage")

- Staged with the host copy of the S0 stager:
  `bash LLM/evidence/2026-10-15-plan-359/stage-host.sh /var/tmp/pt398/st`
  (fixtures from `../pmtest/fixtures`), plus one staged-copy-only delta:
  the `source /etc/make.local` line of the staged `make.conf` commented
  out (hermetic config; equivalent to S2's empty-the-host-file, no host
  file touched). `/var/db/pkg` and `/var/lib/portage/world` were NOT
  copied (host safety; running root is the stage, and the fixture has no
  BDEPENDs).
- Host-profile quirk (S2 precedent): `/etc/portage/make.profile` was
  temporarily symlinked at the staged fixture default profile for the
  duration of each Portage run and restored immediately after (one
  command: save target, `sudo ln -sfn`, run, restore, verify). Saved
  target `/.gentoo/repos/buildovl/profiles/vivo/desktop`, restored each
  time.
- Env (both managers, same staged `$FX`):
  `PYTHONHASHSEED=0 LC_ALL=C.UTF-8 TZ=UTC CLEAN_DELAY=0
  PORTAGE_CONFIGROOT=$FX ROOT=$FX PORTAGE_RUNNING_ROOT=$FX
  DISTDIR=$FX/distfiles` (+ `PORTAGE_REPOSITORIES` from the staged
  repos.conf for Portage; `env -i PATH=/usr/bin:/bin HOME=…` for portuale).
- Portage argv: `/usr/sbin/emerge -pv --autounmask-use=y dev-libs/pg398-t
  --color=n`. portuale argv: `portuale emerge -pv --autounmask-use=y
  dev-libs/pg398-t --color=n` (release binary, rebuilt after every product
  change; see the stale-binary note under Gates).
- Scratch `/var/tmp/pt398*` removed at the end (`sudo rm -rf`); nothing in
  `/dev/shm`; no container bed (coordinator's); no pkill/pgrep.

## Fixture (pmtest, untracked, `pg398-` prefix verified free)

`fixtures/repo/dev-libs/pg398-{t,c,a,b,x,y}/pg398-*-1.ebuild` + one
`fixtures/repo/metadata/md5-cache/dev-libs/pg398-*-1` entry each
(`_md5_` = md5sum of the ebuild; field order matches existing entries).
Untracked `pg359*` fixtures untouched. Final shape (v3):

- `pg398-x`, `pg398-y`: leaves.
- `pg398-a`: `IUSE="flip"`, `RDEPEND="flip? ( || ( dev-libs/pg398-x dev-libs/pg398-y ) )"`.
- `pg398-b`: `RDEPEND="dev-libs/pg398-y"` (plain Y puller, walks late).
- `pg398-c`: `RDEPEND="dev-libs/pg398-b dev-libs/pg398-a[flip]"` (flip trigger).
- `pg398-t`: `RDEPEND="dev-libs/pg398-a dev-libs/pg398-c"` (top).

## Variants tried (3 shapes + 1 control)

- **v1** (`t: "c a"`, `c: "a[flip] b"`): Portage and portuale byte-identical
  (`a y b c t`, no `x`, rc 1). The flip lands on a fresh pick in both
  (no existing-slot path). Capture `v1-t-portage.txt` (Portage only;
  portuale output observed identical on the terminal, not saved).
- **v2** (`t: "a c"`, `c: "a[flip] b"`): merge SET identical (`a y b c t`),
  but row ORDER differs (Portage `y`-first, portuale `a`-first) and the
  portuale rc was mis-captured through a pipe (`tail`'s status). No `x` on
  either side — the flip fires with `y` already queued/entered, so both
  pick `y`. Captures `v2-t-portage.txt`, `v2-t-portuale.txt`.
- **v3** (`t: "a c"`, `c: "b a[flip]"`) — **the divergence**: Portage prints
  5 rows `y a b c t`, no `x`, rc 1 (`v3-t-portage.txt`); portuale printed
  6 rows `y x a b c t`, rc 1 (`v3-t-portuale.txt`). The flip fires with
  neither alternative in graph or queue, so the flip path's
  `disjunction_preference`/`promote_tied_alternative` tie-break picks
  first-listed `x` immediately instead of deferring (proven with a
  temporary `eprintln` probe, since removed: `flat=["dev-libs/pg398-x"]`
  with `entries=[t,c,a]`, `queued=[b]`).
- **no-`y` control** (temporary `zz398nb-{c,t}` fixtures, since deleted):
  Portage and fixed portuale agree exactly (`x a c' t'`, rc 1) — with no
  other puller the deferred group falls back to first-listed `x` on both
  sides. No residual divergence; no new backlog number filed.

Walk-order note (diagnostic, out of scope): in v3 Portage walks `c`
before `a` (strict LIFO stack), so its `a[flip]` is a *fresh* flip whose
`||` defers through the normal main-walk path; portuale queues the top's
leftover `a` ahead of `c`'s children (global FIFO across levels), so the
same argv hits the *existing-slot* flip path. The deferred-choice fix
makes the observable sets, edges, parents and order agree anyway; the
walk-queue discipline itself is untouched.

## Fix (portuale repo, uncommitted)

- `crates/portage-repo/src/resolve/slot_expand.rs`:
  `expand_resolved_slot_with_flipped_use` splits tokens with
  `split_off_disjunctions` on the flipped USE; plains go through the
  unchanged flatten/filter/enqueue pipeline, groups defer. Malformed input
  falls back to today's path (splitter returns it unsplit).
- `crates/portage-repo/src/resolve/disjunction_stack.rs`: new
  `DeferredSite::Flip` (frozen flipped USE, buildtime/runtime/bdepend
  sets, `or_universe`, running tags, root-deps sets, flipped `delta_meta`,
  narrow flag, flipped slot); shared `resolve_target_scope_group` core for
  the Main and Flip sites (Main keeps its virtual-pullers join); Flip
  pop-time enqueue mirrors the flipped pipeline (pop-time genuinely-new
  filter, clean-reuse for fresh atoms only, pullers into `slot_pullers`
  plus the parent edge into `required_by_map` even for already-seen
  targets — Portage adds the parent atom for previously added packages —
  merge-order edges for kept and already-seen targets via a slot-key
  lookup with a version check, since entry indices shift mid-pass).
- `or_group_universe`/reverse-order tricks untouched. No bed run (per
  brief).

## Tests added

- Rust unit test
  `tests::autounmask::flip_revealed_disjunction_defers_to_the_drained_graph`
  (fails before the fix with `x` in the graph, passes after; asserts no
  `x`, the `A->Y` merge edge, `y`'s `{b, a}` parents, the recorded flip,
  in default and `--autounmask-backtrack=y` modes).
- pmtest CASES rc entry (`--pretend -v --autounmask-use=y dev-libs/pg398-t`,
  rc 1) + pinned-output test
  `test_autounmask_flip_revealed_disjunction_defers_to_the_drained_graph`
  (exact Portage rows `y a b c t`, change block, "terminated early"
  notice; backtrack=y variant with no notice). Docstrings name the host
  Portage 3.0.82.2 captures in this directory.

## Gates (in order, never concurrently)

- `cargo fmt --check`: clean (after `cargo fmt`).
- `cargo clippy --release --all-targets`: 0 warnings (two
  `needless_borrow` in new code fixed; one blanket-sed revert verified
  byte-identical for the pre-existing Main-arm line).
- `cargo build --release`: fresh (see stale-binary note).
- `cargo test --release -- --test-threads=2` (whole workspace): **2803
  passed, 0 failed**. Mid-task scare: 5 failures in `ebuild_merge`
  backend-equivalence + `broken_pipe` tests were a stale release binary
  (`built_portuale_bin` panics when the binary predates the edited
  sources — `cargo test`/`clippy` do not relink `target/release/portuale`);
  green after rebuilding first. Lesson recorded here for the next slice.
- pmtest full suite
  (`python3 -m pytest pytests-contract-suite -q
  --basetemp=/var/tmp/pt398 -p no:cacheprovider`): **4045 passed,
  37 skipped, 2 xfailed**, zero failures.
- `test_harvested_corpus.py`: 1 passed — **no corpus drift, nothing
  blessed** (per brief, drift would be reported, not blessed).
- Moved pins: none. Unexpected outputs: the v2 order-only tie (superseded
  by v3; resolved by the fix — final rows match Portage exactly and are
  stable across runs).

## Docs

- `LLM/what-this-proves.md`: paragraph appended (history untouched).
- `LLM/code-notes/portage-repo/resolve/disjunction_stack.md`: stale #398
  bullet corrected + Flip-site section; `slot_expand.md`: flip-split
  section. `python3 LLM/tools/code_notes_index.py` clean (exit 0).
- Backlog entry #398 itself is NOT flipped to DONE (no commit per brief;
  the coordinator closes it).

## Files changed (all uncommitted)

- portuale: `crates/portage-repo/src/resolve/disjunction_stack.rs`,
  `crates/portage-repo/src/resolve/slot_expand.rs`,
  `crates/portage-repo/src/tests/autounmask.rs`,
  `crates/portage-repo/src/tests/mod.rs` (probe registration added then
  removed — net zero), `LLM/what-this-proves.md`,
  `LLM/code-notes/portage-repo/resolve/{disjunction_stack,slot_expand}.md`,
  `LLM/evidence/2026-10-18-398/` (9 captures + this report).
- pmtest (untracked): 6 ebuilds + 6 md5-cache entries under
  `fixtures/repo/...pg398...`; `pytests-contract-suite/test_emerge_pretend_contract.py`
  (1 CASES row + 1 pinned test).
