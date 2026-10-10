# crates/portuale/src/helpers/python.rs
The `$PORTAGE_PYTHON` dispatcher: routes vendored-script calls to the native helpers.

## module
- #326 D1 + Q7, S7: the vendored `bin/` keeps upstream's call sites byte for byte, and `PORTAGE_PYTHON` points at `bin/portuale-python`, which execs `"$PORTUALE_BIN" __helper python "$@"`. This dispatcher picks the helper from the script basename (the first argument at every 0.3 call site), or, for `-c`, from the exact `90config-impl-decl` program string.
- Every script of 0.3 is natively ported now (S7 was the last one, so the D1 transition table is empty and gone): `filter-bash-environment.py` (`helpers/filter_env.rs`, S3), `xpak-helper.py` (`helpers/xpak.rs`, S5), `gpkg-helper.py` (`helpers/gpkg.rs`, S4), `doins.py` (`helpers/doins.rs`, S6), `xattr-helper.py` + `install.py` (`helpers/xattr.rs`, S7). Anything else -- `dohtml.py` included (`dohtml` dies first for EAPI >= 7, the ebuild floor, so it is never in scope) -- exits 127 with the unknown-helper message, so an upstream re-sync that adds a Python call fails loudly (D1).

## fn run
- S3: `filter-bash-environment.py` keeps Portage's `sys.argv[0]` role (the usage basename); everything after it is the filter's argv (Portage's `sys.argv[1:]`). S5/S4/S6/S7 rows likewise. No transition rows remain (S7 deleted the last ones): anything else is an unknown helper (D1). The QA probe row: `-c` with the exact program string (S2.4, D3).
