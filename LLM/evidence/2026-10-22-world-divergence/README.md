# `emerge -uDpvN @world`: Portage vs portuale (2026-10-22)

Recordings: `helpers/portage-uDpvN-world`, `helpers/portuale-uDpvN-world` (asciinema, rendered
here as `portage.txt` / `portuale.txt`). `portuale-after-fixes-world.out` is portuale after the two
fixes of backlog #476; `portage-debug-masks.txt` the `runtime_pkg_mask` lines of Portage's `--debug` run.

Portage: 374 packages, `backtrack: 5/20`, no conflict. Portuale before: error (mime-types[nginx]);
after #476: 368 packages, `backtrack: 20/20`, one unsatisfied soft block
(`plasma-login-sessions` vs `plasma-workspace-6.8.0`) and boost/karchive slot conflicts.

Portage's winning path: mask_built on binary mesa/kwin; slot-conflict mask group on plasma-activities
(ebuild + binary + installed 6.7.5, ebuild 6.6.6-r1); ebuild libdisplay-info-0.4.0; installed
libplasma-6.7.5 "missing dependency"; binary libplasma-6.7.5 mask_built. Open divergences: #477-#479.
