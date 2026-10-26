# crates/portuale/src/pretend/tests/remote.rs
Remote execution prints no post-merge news notice.

## fn remote_success_prints_no_post_merge_notice_like_portage
- Backlog #231 (b): the remote-execution early return (`take_remote_exec` dispatch) skips `post_emerge`'s tail -- correctly so. The remote plan merges onto the *remote* host; the local `${ROOT}/var/db/pkg` is never touched, so Portage's `_pkgs_changed` gate (`post_emerge.py:112-117`) evaluates false and -- the run being non-`--pretend` -- the tail would print nothing. Local-transport end to end (`mrg --remote-transport local`, the same shape the contract suite's `test_mrg_remote_resolve_merges_a_binhost_binary` uses): the pre-resolution notice still prints once, before `Calculating...`, and no second notice follows the merge.
- Client ROOT: a copy of the fixtures' `var` (the hermetic 5-count vdb), so the pre notice has something to print. Tmp `file://` binhost serving the hook-ordering fixture binary with a minimal `Packages` index. Client `/etc/portage`: the fixture config repointed at the tmp binhost (absolute repo locations, like the contract suite's `_write_tmp_clientetc`).
- Exactly one notice: the pre-resolution one, standing before `Calculating...` -- no post-merge notice follows the merge.

## hermetic running root (Task 11)
- `PORTAGE_RUNNING_ROOT` points at the test's scratch root, not the shared fixtures: the run resolves through `pretend::run`, whose updates pass would otherwise record (and move under) the fixture tree. The copied `var` tree keeps installed-only resolution identical.
