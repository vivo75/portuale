# Oracle capture, plan 02.442 S1 (#500) and S2 (#501) -- 2026-10-23

Portage 3.0.83-r1 (host, `/usr/bin/emerge`) vs the current portuale
(`target/release/portuale`, built from eee9edee, copy sha256 prefix 9196e14ce307aec2,
mtime 2026-10-23 12:29). Hermetic: scratch ROOT, PORTAGE_CONFIGROOT, PKGDIR, PORTAGE_TMPDIR
under /var/tmp/ev500 (removed afterwards); config in harness/make.conf
(`binpkg-ignore-signature`, `-pkgdir-index-trusted`, base profile default/linux/amd64/23.0).
Command per shape: `emerge -1 --usepkgonly --nodeps hostile/<shape>` (S1),
`emerge -1 --getbinpkgonly --nodeps hostile/<pkg>` against a local http.server binhost (S2).
Casts: casts/<shape>.<pm>.cast; plain text (rc, output, resulting ROOT listing, leak check): txt/.
No file was written outside ROOT by either PM in any run (leak check: /var/tmp/outside-x stays empty).

## Reuse

    python3 build-hostile.py OUTDIR [shape...]     # OUTDIR/<shape>/hostile/<shape>/<shape>-1-1.gpkg.tar, each shape its own PKGDIR (all shapes if none given)
    harness/runpm.sh portage|portuale <shape>      # S1 run (expects /var/tmp/ev500/{arch,cfg,bin/emerge}; adapt paths)
    harness/bhrun.sh portage|portuale <hb-pkg>     # S2 run (expects /var/tmp/ev500/bh served as the binhost)
    asciinema rec --headless --overwrite -q -c '<cmd>' x.cast

`build-hostile.py` writes the outer gpkg itself (USTAR container, zstd inner tars, Manifest with BLAKE2B+SHA512)
because `..`-named members cannot come from a filesystem image; metadata base is pmtest
`fixtures/helpers/gpkg/metadata` (CATEGORY=hostile, PF=<shape>-1).
S2 index: `emaint binhost --fix` on four benign packages, then fields deleted by hand from `Packages`
(delete `Packages.gz`): hb-none (no SIZE/MD5/SHA1), hb-size-only (no MD5/SHA1), hb-sum-only (no SIZE), hb-full.

## S1 table

Portage on a hostile image refuses at extraction (`Danger: ...`, `!!! Error extracting '<pkg>', <reason>.`, rc 1,
nothing installed). On a bad metadata member it refuses at index time: `!!! Invalid binary package: '<pkg>', Metadata member is not
a regular file: metadata/<name>` (printed twice), then `emerge: there are no binary packages to satisfy ...`, rc 1.

| shape | Portage rc + message | portuale rc + message | diverges? |
|---|---|---|---|
| control-benign | 0: merged | 0: merged | no |
| control-contained-symlink-parent | 0: merged | 1: image.tar member "image/opt/h/bin/tool" is inside the symlink member "image/opt/h/bin" | **YES** |
| img-abs-path | 1: Danger: absolute path detected: /etc/evil | 1: image.tar member "/etc/evil" has an absolute name | no |
| img-device | 1: Danger: device file detected: image/hda | 1: image.tar member "image/hda" is a block device | no |
| img-dir-through-symlink | 1: Danger: symlink escape detected: image/./link | 0: merged | **YES** |
| img-dotdot-bare | 1: Danger: path traversal detected: .. | 1: tar (external GNU tar, not portuale): "Member name contains \'..\'", exit 2 | no (by accident) |
| img-dotdot-dotslash | 1: Danger: path traversal detected: ./../evil | 1: image.tar member "./../evil" has a path traversal | no |
| img-dotdot-leading | 1: Danger: path traversal detected: image/../outside | 1: image.tar member "image/../outside" has a path traversal | no |
| img-dotdot-sub | 1: Danger: path traversal detected: image/opt/.. | 1: tar (external GNU tar, not portuale): "Member name contains \'..\'", exit 2 | no (by accident) |
| img-duplicate | 1: Danger: duplicate files detected: image/opt/h/hello | 1: image.tar member "image/opt/h/hello" is a duplicate | no |
| img-hardlink-outside-archive | 1: Danger: hardlink escape detected: image/shadow | 1: image.tar member "image/shadow" hard link target "/etc/shadow" is not an earlier member | no |
| img-hardlink-relocated-rel | 1: Danger: symlink escape detected: image/../../marker | 0: merged | **YES** |
| img-hardlink-to-escaping-symlink-abs | 1: Danger: symlink escape detected: image/link | 0: merged | **YES** |
| img-symlink-parent-abs | 1: Danger: symlink escape detected: image/link | 1: image.tar member "image/link/marker" is inside the symlink member "image/link" | no |
| img-symlink-parent-rel | 1: Danger: symlink escape detected: image/link | 1: image.tar member "image/link/marker" is inside the symlink member "image/link" | no |
| img-symlink-root | 1: Danger: symlink escape detected: image/x/etc | 1: image.tar member "image/x/etc/evil" is inside the symlink member "image/x" | no |
| meta-chardev | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EAPI | 1: inner metadata member "metadata/EAPI" is a character device, not a regular file | no |
| meta-dir | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EAPI | 1: inner metadata member "metadata/EAPI" is a directory, not a regular file | no |
| meta-extra-dir | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/subdir | 1: inner metadata member "metadata/subdir" is a directory, not a regular file | no |
| meta-extra-symlink | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EXTRA | 0: merged | **YES** |
| meta-hardlink | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EAPI | 1: inner metadata member "metadata/EAPI" link target "metadata/SLOT" not in the archive | no |
| meta-symlink | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EAPI | 0: merged | **YES** |
| meta-symlink-abs | 1: !!! Invalid binary package: Metadata member is not a regular file: metadata/EAPI | 1: inner metadata member "metadata/EAPI" link target "/etc/hostname" not in the archive | no |

Divergences (6 of 23):
- control-contained-symlink-parent: Portage accepts a file under a symlink parent that stays inside the image
  (upstream `test_contained_symlink_parent`); portuale refuses by lexical rule -- a regression risk for S1 (must accept).
- img-dir-through-symlink, img-hardlink-relocated-rel, img-hardlink-to-escaping-symlink-abs: portuale merges, Portage refuses.
- meta-symlink, meta-extra-symlink: portuale merges (resolves the link to another member's bytes), Portage refuses.
- img-dotdot-sub / img-dotdot-bare: rc agrees, but portuale only fails because GNU tar rejects the name
  after the image dir has been partly populated (`pt-*/portage/hostile/<pf>/image` exists); no refusal before extraction,
  no portuale message text.
- Message texts differ everywhere (rc is the comparable part); S1 should copy Portage's wording where the bed compares it.

## S2 table (remote binhost, `--getbinpkgonly`)

| index entry | Portage rc | fetch attempted | Portage message | portuale rc | diverges? |
|---|---|---|---|---|---|
| hb-none (no SIZE, no checksum) | 1 | yes (GET .gpkg.tar 200) | see below | 0, merged | **YES** |
| hb-size-only | 1 | yes | same | 0, merged | **YES** |
| hb-sum-only (no SIZE) | 1 | yes | same | 0, merged | **YES** |
| hb-full | 0 | yes | none | 0 | no |

Portage message (identical for the three bad cases, even size-only / checksum-only), printed after the download, to the
`.partial` file, before install; nothing installed, ".partial" left in the binhost cache:

    !!! Digest verification failed:
    !!! <cache>/hostile/hb-none/hb-none-1-1.gpkg.tar.partial
    !!! Reason: binhost's Packages index lacks size and checksum
    !!! for this package

    >>> Failed to emerge hostile/hb-none-1 for <ROOT>/

So the fetch IS attempted (the check is in BinpkgVerifier, after download); S2 does not need to refuse before fetch.

Caveats: the default `binrepos.conf` still lists [gentoo]/[f1r] from the host cache ("within TTL", no network use seen);
casts show wget progress for the S2 fetch.

## S2 after the fix (#501)

Same hermetic setup rebuilt (the first one was removed): `build-hostile.py` for `hb-none`, `hb-size-only`, `hb-sum-only`, `hb-full`
into one served dir, `emaint binhost --fix`, fields then deleted from `Packages` as above; config as `harness/make.conf` plus
a private `repos.conf` (gentoo at `/.gentoo/repos/gentoo`) and an empty `binrepos.conf` (so the host's `[gentoo]`/`[f1r]` lines of the first capture are gone).
Portage 3.0.83-r1 re-run on `hb-none` gave the same block as `txt/s2-hb-none.portage.txt`; the Portage run leaves
`PORTAGE_TMPDIR/portage` empty (no build log), hence no `, Log file:` suffix in its tail.

portuale rebuilt from this working tree: casts `casts/s2-hb-*.portuale.after.cast`, text `txt/s2-hb-*.portuale.after.txt`.

| index entry | Portage rc | portuale rc (before) | portuale rc (after) | block vs Portage |
|---|---|---|---|---|
| hb-none | 1 | 0 | 1 | identical except ROOT trailing slash |
| hb-size-only | 1 | 0 | 1 | identical except ROOT trailing slash |
| hb-sum-only | 1 | 0 | 1 | identical except ROOT trailing slash |
| hb-full | 0 | 0 | 0 | no refusal text |

The ROOT difference is older: Portage prints ROOT with a trailing slash everywhere (`Emerging binary ... for <ROOT>/`), portuale echoes the
`ROOT` string it was given. The pmtest pin passes `ROOT` with a trailing slash. In each refused run the `.partial` stays in the binhost cache, nothing is installed.
The first `before` rows were measured against the build of eee9edee, see the S2 table above.

## S1 after the fix (#500)

Same 23 archives (`build-hostile.py`), same hermetic setup (`harness/runpm.sh`, config as `harness/make.conf` plus a private
`repos.conf` with `[DEFAULT] main-repo = gentoo` and `gentoo` at `/.gentoo/repos/gentoo`), portuale rebuilt from this working tree:
casts `casts/<shape>.portuale.after.cast`, text `txt/<shape>.portuale.after.txt`. rc equals Portage's on all 23 rows:
control-benign and control-contained-symlink-parent merge (rc 0, the latter leaves `opt/h/real/tool` like Portage); the 21 hostile
rows exit 1 with nothing installed and an empty image directory (`PORTAGE_TMPDIR/portage/hostile/<pf>/image` exists, empty);
`/var/tmp/outside-x` stays empty. Messages: `emerge: <archive>: Danger: <what> detected: <path>` (path as Portage prints it, except
for the shapes where Portage's root is one level higher than the unpack directory, see #514) and, for metadata rows,
`!!! Invalid binary package: '<archive>', Metadata member is not a regular file: metadata/<name>`.
