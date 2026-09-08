# `porttest` — synthetic merge/packaging test overlay

Tiny, fast-building `EAPI=8` ebuilds, one per isolated merge/packaging
behaviour, baked into `localhost/test-portuale` at
`/var/db/repos/porttest` by `TEST/create-container.bash`. Used by L1/L5
(they build in seconds); **not** L3.

Slice 1 ships only this skeleton (repo metadata + an empty `porttest`
category). The ebuilds land in slice 3 — see the table in
[`docs/real-world-testing.md`](../../../../docs/real-world-testing.md) §7:

| pkg | exercises |
|---|---|
| `porttest/cfgprotect` | CONFIG_PROTECT `._cfg` creation |
| `porttest/setuid` | `4755` binary + file capability (xattr) |
| `porttest/hardlinks` | hardlinked regular files + `CONTENTS` dedup |
| `porttest/symfarm` | relative/absolute/dangling symlinks |
| `porttest/emptydirs` | `keepdir` + `.keep` naming |
| `porttest/docs` | `dodoc` tree → docompress, `newdoc`, `doinfo` |
| `porttest/unicode` | UTF-8 / space / `$` / newline in filenames |
| `porttest/bigfile` | one large sparse file |
| `porttest/installmask` | files an `INSTALL_MASK` should drop |
| `porttest/splitdebug` | C source → `FEATURES=splitdebug` `.debug` split |
| `porttest/soname-1` / `-2` | `libpt.so.1` → `.so.2` for preserve-libs |
| `porttest/slotdep-*` | slot-op rebuild chain |
| `porttest/phases` | every `pkg_*` phase writes a marker |
| `porttest/collision` | two pkgs shipping the same path |
| `porttest/config-script` | non-trivial `pkg_config` |
| `porttest/preserve-fail` | postinst exits 1 → non-fatal handling |

Keep each ebuild's `src_install` trivial and its build near-instant
(no `SRC_URI`, or a checked-in tiny tarball).
