# crates/portuale/src/remote/config_place.rs
Placing the client's config root on the server for resolve runs.

## module
- Resolve-path support (slice 5).

## struct ConfigRootOverride
- Guards `PORTAGE_CONFIGROOT` for the whole resolve; every env read inside observes one constant value.

## struct PlacedConfig
- The pulled temp dir is removed best-effort on drop once the resolve is done (backlog #171c review: both the resolve site and `--remote-binpkg` leaked one `/tmp/portuale-remote-etc-<pid>-<nanos>` per run).
- The pulled tree carries the server's own `make.globals` seeded under `<tmp>/usr/share/portage/config/` (backlog #171 follow-up 3): the pull only carries the client's `/etc/portage` contents, so without the seed the resolve would stack no base layer at all.
- One placement match, no duplicated pull logic. A dangling pulled `make.profile` symlink is remapped onto the server's repos (`remap_client_make_profile`, backlog #171 follow-up 4) in the same helper, so both paths resolve the client profile the same way.

## fn lexical_normalize
- `canonicalize` would resolve the wrong machine's paths, so the fold is purely lexical.

## fn remap_client_make_profile
- Backlog #171 follow-up 4, owner Q10 = b: the client may hold no repo at all, so a symlink into the client's repo path dangles here and the profile chain would silently resolve to `[]`.
- Only dangling symlinks are rewritten: to the same-named server repo's `<location>/profiles/<rel>` when the pulled client `repos.conf` (if any) has a repo whose `location` is the link prefix, else to the first server repo in `find_repos` priority order (main repo first) providing `<location>/profiles/<rel>`.
- A link that already resolves inside the pulled tree (e.g. a relative link to a custom profile shipped under `/etc/portage`, or a loopback absolute link), a directory `make.profile` (its `parent` entries ride the normal profile machinery), a missing one, and a target naming no repo profile are all left alone.
- Portage grounding (`3rdparty/portage` = portage-3.0.82.2): the profile root is `<config_root>/etc/make.profile` followed as a link (`package/ebuild/_config/LocationsManager.py:119-149`, `realpath`ed at `:154-157` for repo matching), and Portage warns it "should point into a profile within $PORTDIR/profiles/" (`package/ebuild/config.py:1444-1450`). The rewrite keeps exactly that shape, server-side, then reuses the existing resolver.
- The pull is tar-only, so the target text is never followed on the client; relative targets resolve against the client's own `/etc/portage` dir, lexically.
- A step-2 hit (same-named server repo) is tried first; `find_repos` already sorts main-first.

## fn place_config_root
- Backlog #171 follow-up 3 (owner Q9 = b): the server's own `make.globals` is located exactly the way the local path locates it (`resolve_config` reads `<config_root>/usr/share/portage/config/make.globals`, which is the live `/usr/share/portage/config/make.globals` for a `/` run; Portage sources it from `global_config_path` regardless of `config_root`, `config.py:446-499,569` + `_config/LocationsManager.py:413-415`). Read before the `ConfigRootOverride` repoints `PORTAGE_CONFIGROOT` at the placed dir, so this still names the server file. Under the client profile chain and the pulled `make.conf`: Portage's `config` stacking order.
- Backlog #171 follow-up 4 (owner Q10 = b): `Server` placements are untouched by the remap.
- Backlog #171 follow-up 5 (l171f; review of l171d/l171e): owner Q10, the client operates without the gentoo repository. `find_repos` on the placed root fails `NoReposConf` without the user `etc/portage/repos.conf` path, so both resolving paths hard-errored after a successful remap. Portage layers the global file before the user's in one parser, so the user wins per key (`repository/config.py:1488-1508`), and the user slot is never clobbered here. Absent on the server (or a directory rather than a file): contributes nothing. Same file-copy mechanism (with tmp cleanup) as the `make.globals` seed.
