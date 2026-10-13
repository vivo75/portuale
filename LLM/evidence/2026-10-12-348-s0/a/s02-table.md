
| id | file | vs tag `portage-3.0.82.2` | vs host `/usr/lib/portage/python3.14` | verdict |
|----|------|---------------------------|---------------------------------------------|---------|
| D-README.md | `bin/README.md` | OWNED: README (portuale doc, no upstream counterpart) | n/a (portuale-only) | same (intentional) |
| D-bashrc-functions.sh | `bin/bashrc-functions.sh` | identical | identical | same |
| D-chmod-lite | `bin/chmod-lite` | OWNED: shim execing $PORTUALE_BIN __helper chmod-lite (#326 D1/D2) | n/a (portuale-only) | same (intentional) |
| D-eapi.sh | `bin/eapi.sh` | identical | identical | same |
| D-sed | `bin/ebuild-helpers/bsd/sed` | identical | identical | same |
| D-die | `bin/ebuild-helpers/die` | identical | identical | same |
| D-dobin | `bin/ebuild-helpers/dobin` | identical | identical | same |
| D-doconfd | `bin/ebuild-helpers/doconfd` | identical | identical | same |
| D-dodir | `bin/ebuild-helpers/dodir` | identical | identical | same |
| D-dodoc | `bin/ebuild-helpers/dodoc` | identical | identical | same |
| D-doenvd | `bin/ebuild-helpers/doenvd` | identical | identical | same |
| D-doexe | `bin/ebuild-helpers/doexe` | identical | identical | same |
| D-dohard | `bin/ebuild-helpers/dohard` | identical | identical | same |
| D-doheader | `bin/ebuild-helpers/doheader` | identical | identical | same |
| D-dohtml | `bin/ebuild-helpers/dohtml` | identical | identical | same |
| D-doinfo | `bin/ebuild-helpers/doinfo` | identical | identical | same |
| D-doinitd | `bin/ebuild-helpers/doinitd` | identical | identical | same |
| D-doins | `bin/ebuild-helpers/doins` | identical | identical | same |
| D-dolib | `bin/ebuild-helpers/dolib` | identical | identical | same |
| D-dolib.a | `bin/ebuild-helpers/dolib.a` | identical | identical | same |
| D-dolib.so | `bin/ebuild-helpers/dolib.so` | identical | identical | same |
| D-doman | `bin/ebuild-helpers/doman` | identical | identical | same |
| D-domo | `bin/ebuild-helpers/domo` | identical | identical | same |
| D-dosbin | `bin/ebuild-helpers/dosbin` | identical | identical | same |
| D-dosed | `bin/ebuild-helpers/dosed` | identical | identical | same |
| D-dosym | `bin/ebuild-helpers/dosym` | identical | identical | same |
| D-elog | `bin/ebuild-helpers/elog` | identical | identical | same |
| D-emake | `bin/ebuild-helpers/emake` | identical | identical | same |
| D-fowners | `bin/ebuild-helpers/fowners` | identical | identical | same |
| D-fperms | `bin/ebuild-helpers/fperms` | identical | identical | same |
| D-keepdir | `bin/ebuild-helpers/keepdir` | identical | identical | same |
| D-newins | `bin/ebuild-helpers/newins` | identical | identical | same |
| D-nonfatal | `bin/ebuild-helpers/nonfatal` | identical | identical | same |
| D-prepall | `bin/ebuild-helpers/prepall` | identical | identical | same |
| D-chown | `bin/ebuild-helpers/unprivileged/chown` | identical | identical | same |
| D-install | `bin/ebuild-helpers/xattr/install` | identical | identical | same |
| D-ebuild-ipc | `bin/ebuild-ipc` | OWNED: always exits 127, no IPC daemon (#326 D6) | n/a (portuale-only) | same (intentional) |
| D-ebuild.sh | `bin/ebuild.sh` | identical | identical | same |
| D-ecompress | `bin/ecompress` | identical | identical | same |
| D-ecompress-file | `bin/ecompress-file` | identical | identical | same |
| D-estrip | `bin/estrip` | identical | identical | same |
| D-filter-bash-environment.py | `bin/filter-bash-environment.py` | identical | DIFF vs host: shebang `env python` vs host `python3.14` (distro rewrite; tag has `env python`) | same (intentional) |
| D-helper-functions.sh | `bin/helper-functions.sh` | identical | identical | same |
| D-05double-D | `bin/install-qa-check.d/05double-D` | identical | identical | same |
| D-05prefix | `bin/install-qa-check.d/05prefix` | identical | identical | same |
| D-10executable-issues | `bin/install-qa-check.d/10executable-issues` | identical | identical | same |
| D-10ignored-flags | `bin/install-qa-check.d/10ignored-flags` | identical | identical | same |
| D-20deprecated-directories | `bin/install-qa-check.d/20deprecated-directories` | identical | identical | same |
| D-20runtime-directories | `bin/install-qa-check.d/20runtime-directories` | identical | identical | same |
| D-60bash-completion | `bin/install-qa-check.d/60bash-completion` | identical | identical | same |
| D-60openrc | `bin/install-qa-check.d/60openrc` | identical | identical | same |
| D-60pkgconfig | `bin/install-qa-check.d/60pkgconfig` | identical | identical | same |
| D-60pngfix | `bin/install-qa-check.d/60pngfix` | identical | identical | same |
| D-60systemd | `bin/install-qa-check.d/60systemd` | identical | identical | same |
| D-60udev | `bin/install-qa-check.d/60udev` | identical | identical | same |
| D-80libraries | `bin/install-qa-check.d/80libraries` | identical | identical | same |
| D-80multilib-strict | `bin/install-qa-check.d/80multilib-strict` | identical | identical | same |
| D-90bad-bin-group-write | `bin/install-qa-check.d/90bad-bin-group-write` | identical | identical | same |
| D-90bad-bin-owner | `bin/install-qa-check.d/90bad-bin-owner` | identical | identical | same |
| D-90cmake-warnings | `bin/install-qa-check.d/90cmake-warnings` | identical | identical | same |
| D-90config-impl-decl | `bin/install-qa-check.d/90config-impl-decl` | identical | identical | same |
| D-90cython-dep | `bin/install-qa-check.d/90cython-dep` | identical | identical | same |
| D-90gcc-warnings | `bin/install-qa-check.d/90gcc-warnings` | identical | identical | same |
| D-90world-writable | `bin/install-qa-check.d/90world-writable` | identical | identical | same |
| D-95empty-dirs | `bin/install-qa-check.d/95empty-dirs` | identical | identical | same |
| D-isolated-functions.sh | `bin/isolated-functions.sh` | identical | identical | same |
| D-misc-functions.sh | `bin/misc-functions.sh` | identical | identical | same |
| D-phase-functions.sh | `bin/phase-functions.sh` | DIFF: header comment + `filtered_vars+=( "PORTUALE_.*" )` + `___save_and_filter_ebuild_env` (brush strategy #2, header-documented) | same DIFF (host lacks it too) | same (intentional) |
| D-phase-helpers.sh | `bin/phase-helpers.sh` | identical | identical | same |
| D-portageq-wrapper | `bin/portageq-wrapper` | OWNED: execs $PORTUALE_BIN portageq (#326 D5, feat#157 S6) | n/a (portuale-only) | same (intentional) |
| D-portuale-python | `bin/portuale-python` | OWNED: execs $PORTUALE_BIN __helper python (#326 D1) | n/a (portuale-only) | same (intentional) |
| D-50xdg-utils | `bin/postinst-qa-check.d/50xdg-utils` | identical | identical | same |
| D-save-ebuild-env.sh | `bin/save-ebuild-env.sh` | identical | identical | same |
| D-version-functions.sh | `bin/version-functions.sh` | identical | identical | same |
