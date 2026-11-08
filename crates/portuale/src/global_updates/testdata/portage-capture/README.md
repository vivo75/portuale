# Task 5 oracle: what Portage 3.0.83 does for the global-updates fixture

Reference: Portage 3.0.83 (`3rdparty/portage`, tag `portage-3.0.83`;
host `emerge --version` agrees — see `portage-version.txt`).
Reproduce: `./capture.sh` (writes only under `mktemp -d /var/tmp/…`,
removed on exit; never touches the host roots or the fixture).

## How the run works

`capture.sh` copies `pmtest/fixtures/global-updates/{root,repo}` to a
scratch dir (with `cp -a`, see the fixture README for why mtimes
matter), writes a `repos.conf` with the absolute repo path, and runs a
probe that mirrors `run_action` per root
(`lib/_emerge/actions.py:3946-3957`): `load_emerge_config`, then
`_global_updates(target.root, trees, target.mtimedb["updates"], quiet)`
(`lib/portage/_global_updates.py:22-56`, `_do_global_updates` `:59`),
then `target.mtimedb.commit()` when updates were applied. Only the
target root is processed — Portage has no `PORTAGE_RUNNING_ROOT`
knob, so a full `emerge` run would also process the host `/` root and
trip this host's profile gate before printing pretend output. The
config reload and the `AUTOCLEAN` force from `run_action` are Task 11
material, not captured here. `havecolor` is forced to 0 first because
`emerge_main` (`lib/_emerge/main.py:1255`) disables color at startup and
only re-enables it in `adjust_configs`, which runs after the updates.

- `stdout.txt` / `rc.txt` — the normal run (`updated = True`, rc 0).
- `stdout.quiet.txt` / `rc.quiet.txt` — second run on a fresh copy
  with `quiet=True` (legend and per-file dots suppressed; progress
  characters still printed).
- `stderr*.txt` — three oracle lines each (`secpass = 2`, the gpkg
  basename warning, `updated = True`) plus two harness-noise lines:
  the host `/etc/portage/make.profile` fails to parse only because
  `PORTAGE_REPOSITORIES` hides the host repos its parent line needs;
  harmless (the target tree loads its own profile fine).
- `before/` / `after/` — file tree snapshots (`tree.txt` listing,
  `md5sums.txt`, full file bytes) of the scratch ROOT before and
  after the normal run.
- `command.txt`, `portage-version.txt` — provenance.

## Reading the oracle

stdout anatomy (`_do_global_updates`, `_global_updates.py:99-137`
header/dots, `:190-206` moves, `:235-274` config/`update_ents`):

- `1Q-2026..` — one updates file, two valid directives (`.` each),
  from `grab_updates`/`parse_updates` (`lib/portage/update.py:130,165`).
- `@` — vdb `move_ent` renamed `oldpkg-1.0` to `newpkg-1.0`
  (`dbapi/vartree.py:621`; ebuild renamed too, content untouched).
- `%` — binpkg `move_ent` copied the xpak to
  `dev-libs/newpkg/newpkg-1.0-1.xpak` (`dbapi/bintree.py:639`; the old
  file stays and is dep-updated below).
- `s` / `S` — vdb `move_slot_ent` (`dbapi/__init__.py:449`) and the
  binpkg one rewrote `SLOT` to `1`.
- `pp` — `update_config_files` (`update.py:241`) rewrote
  `package.use` into `._cfg0000_package.use` (CONFIG_PROTECT=`/etc`
  from `make.globals:135`); the original file is byte-identical.
- `###` / `***` — `update_ents` over 3 vdb entries / 3 binpkgs
  (`dbapi/__init__.py:383`).

Resulting state: `world` holds `dev-libs/newpkg` (sorted);
`newpkg-1.0/{PF,CATEGORY}` rewritten; every `*DEPEND` mentioning the
old names/slots rewritten; `Packages` gains the `newpkg` entry with
fresh digests (`inject` → `_pkgindex_write`, `bintree.py:2011`);
`mtimedb` records the updates file mtime (`_global_updates.py:247-251`).

## Nondeterminism (normalize before comparing)

- `$T` scratch paths appear in `stdout.txt` (`…/repo/profiles/…`)
  and in `mtimedb` (`"updates"` keys): replace with a placeholder.
- `Packages` `TIMESTAMP` and `MTIME`/`SIZE` refresh on the
  post-move rewrite; `Packages.gz` embeds a gzip mtime.
- The rewritten gpkg embeds `datetime.now()` container/metadata
  mtimes (`gpkg.py:1047-…`).
- `Package basename mismatched, using slotpkg-1.0` (stderr):
  `binarydbapi.aux_update` (`bintree.py:253-…`) passes the full
  `dev-libs/slotpkg-1.0` CPV as the gpkg basename while the container
  prefix is `slotpkg-1.0`; `_get_inner_tarinfo` (`gpkg.py:2239`)
  falls back to the prefix with a WARN. Real behavior, fires on every
  gpkg `aux_update`.

## Manager rulings (2026-10-26)

- There is no malformed `updates` line in this fixture, so the capture
  has no `ERROR:` output. Task 10 compares ERROR lines against the #551
  material as it stands; this fixture is not extended.
- The gpkg `Package basename mismatched` stderr warning is recorded as
  Portage quirk Q16. portuale does not copy it.
- `mtimedb` records the scratch path of the updates file and its mtime.
  Comparisons normalize the path prefix.
