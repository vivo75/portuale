# crates/portuale/src/remote/tests/config_place.rs
Config-placement resolve tests: both placements, profile remap, repos.conf seeding.

## tests: placed_config_root_covers_both_placements
- Backlog #171c task 4: the `--remote-binpkg` trial path places `/etc/portage` through the shared `place_config_root` helper. The same `load_repos_and_config` + `config_install_mask` / `config_features_string` / `config_bzip2_command` getters the plan path uses see the placed tree.
- A synthetic client tree pins both placements (`server:` takes a config root in root layout, `client:` the `/etc/portage` dir itself; the contract suite's `test_mrg_remote_resolve_merges_a_binhost_binary` uses `server:{root}` the same way). Task 3: the pulled temp dir is gone after the `Client` resolve.

## tests: remote_client_make_profile_resolves_against_same_named_server_repo
- Backlog #171 follow-up 4, step 2: a dangling absolute `make.profile` plus a client `repos.conf` naming that repo. A pure main-first scan would pick the wrong profile when the main repo provides the same relative profile with different content.
- Portage's stacking order holds: server `make.globals` + profile + client `make.conf` (`config.py:446-499`, `const.INCREMENTALS`).
- Since #564 the client's repo path lives under the test tmpdir, created: discovery keeps a repo only when its location exists (Portage drops a missing directory), while the remap still matches on the path text alone.

## tests: remote_client_make_profile_resolves_without_client_repos_conf
- Backlog #171 follow-up 4, step 3: the owner Q10 client holds no repo at all.

## tests: remote_client_make_profile_unresolvable_fails_loudly
- Backlog #171 follow-up 4, step 5: failure is a `!!!` message naming the target.

## tests: remote_client_make_profile_dir_with_cross_repo_parent_resolves
- Backlog #171 follow-up 4, step 4. The doc named the `parent` entry `gentoo:base`; the test itself writes `testrepo:base`.

## tests: remote_client_make_profile_prefers_main_repo_and_relative_targets
- Backlog #171 follow-up 4, step 3 order: the link text climbs out of the client's `/etc/portage`.

## tests: remote_binpkg_path_resolves_the_placed_client_config
- Backlog #171b task 1: the SAME shared helper `pretend::run` uses (`load_repos_and_config`: one resolver call, no duplicate logic). A synthetic client config root pins the wiring end to end, including the `no{man,info,doc}` fold.

## tests: remote_client_without_repos_conf_resolves_on_the_production_path
- Backlog #171 follow-up 5 (l171f; review of l171e): the owner Q10 client holds no `repos.conf` at all. The shared chain `run_remote_resolve` / `pretend::run` and `run_bundle_stage` both resolve through; not just the test-only `l171e_resolve_with_server_repos` helper.
- The server root carries its repos *only* in the global `usr/share/portage/config/repos.conf` slot (Portage's `repository/config.py:1488-1508` reads the global file before the user's). Like a genuine server, which always has `/etc/portage/repos.conf`, `find_repos` on the server root itself keeps working. The resolved `FEATURES` stack server globals + profile + client `make.conf` in Portage's order (`config.py:446-499`, `const.INCREMENTALS`).
- Since #564 the server user slot also repeats the global `[gentoo]`: discovery merges the host's installed global file too, with a user key winning over a global one, so this keeps the section pinned at the server tree on any host. The seed-source pin is unaffected (the placed tree still receives global-slot content only, and the overlay still must not leak).
