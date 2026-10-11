# crates/portuale/src/ebuild_merge/tests/mod.rs
Shared test helpers and the test-module list for the ebuild-merge suite.

## fn raw_chown
- Test setup, not a call into production code: `chown()`s to an arbitrary uid/gid independent of `lchown_or_chown` itself (the function under test).

## fn tempdir
- No moved context.

## fn collision_fixture
- No moved context.

## fn versioned_fixture
- No moved context.

## fn seed_old_owned_consumer_root
- Seeds a synthetic root: soname `libkeep.so.1`, indexed by `provider-1.0`'s hand-written `NEEDED.ELF.2`, registered under `dev-libs/provider:0`; the consumer `/usr/bin/cprog` is owned by `oldapp-1.0`'s `CONTENTS`.

## fn compile_linked_pair
- Compiles a genuine `libA` (soname `libA.so.1`) and a genuine `libB` (`DT_NEEDED libA.so.1`, soname `libB.so.1`) with the host `cc`, installed under the synthetic root's `/usr/lib` -- the same `gcc -shared -fPIC -Wl,-soname` shape the `sonamebumplib` fixtures use, so `scanelf` (hence the orphan branch under test) sees genuine ELF headers.

## fn env_update_fixture
- No moved context.

## fn fixtures_root
- No moved context.

## mod PUBLISH_HOOK
- Test hook (feat#157 S4.1, S4.2): runs on a database backend, after every call of the publishing / retiring transaction and right before its `commit`.

## fn publish_hook
- No moved context.

## mod db_merge
- feat#157 S4.1: the merge on a database backend (sqlite).
