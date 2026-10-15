# #350 probe (2026-10-15)

Host Portage, scratch repo `repo/` (one ebuild whose `pkg_pretend`, `pkg_setup`
and `src_unpack` append a line to a log), isolated config in `cfg/`.
`./probe.sh` rebuilds everything under `/var/tmp` and removes it at the end.

Result seen on 2026-10-15 (EAPI 8, Portage 3.x on the host):

| case | phase bodies run |
|---|---|
| `ebuild unpack`, clean build dir | pretend setup unpack |
| `ebuild setup unpack` | pretend setup **pretend setup** unpack |
| `ebuild setup unpack`, `FEATURES=keepwork` | pretend setup unpack |
| `ebuild unpack` x3 in a row | first run only; the others skip on the stamps |

Cause: `unpack` prints "Not marked as unpacked; recreating WORKDIR..." and the
cleanup (`phase-functions.sh:346`) removes `.pretended`/`.setuped`. Stamps are
written by `__dyn_pretend`/`__dyn_setup` (`phase-functions.sh:268-293`).
