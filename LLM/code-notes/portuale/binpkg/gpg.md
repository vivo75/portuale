# crates/portuale/src/binpkg/gpg.rs
GPG binpkg signature policy and verification via the system `gpg`.

## struct GpgVerify
- Ports Portage's `lib/portage/gpkg.py` `checksum_helper(VERIFY)` and `gpkg._verify_binpkg`'s GPG layer, with the same "shell out to the actual tool" stance `tar`/compressors/`wget` already take.
- Policy mirrors Portage's `gpkg.__init__` (`gpkg.py:792-819`) with no per-binrepo override (that override is display/parse-only in portuale).
- `BINPKG_GPG_VERIFY_BASE_COMMAND` default is Portage's `cnf/make.globals:53`; the keyring default is `cnf/make.globals:56`.

## fn GpgVerify::from_env
- Portage's own `settings`-derived verify configuration, via the "read the env var, fall back to `make.globals`' default" shortcut every other execution CLI boundary in portuale already takes.

## fn GpgVerify::from_features
- The same policy from a resolved `FEATURES` list (Portage's `settings.features`).

## fn GpgVerify::from_binrepo
- Ports Portage's `gpkg.__init__` per-binrepo policy with `verify-signature` set (`gpkg.py:792-819`); Portage's shipped `cnf/binrepos.conf` `[DEFAULT] verify-signature = true` means an explicit `false` is the meaningful case. `FEATURES` then overrides in one direction if stronger, exactly Portage's own `if`/`elif` tail (backlog #43).

## fn gpg_policy_for_features
- Ports Portage's `gpkg.__init__` `request_signature`/`verify_signature` derivation (`gpkg.py:798-819`).

## fn gpg_verify_argv
- Fills Portage's `BINPKG_GPG_VERIFY_BASE_COMMAND` placeholders (`gpkg.py:500-525`). Whitespace splitting is a deliberate narrowing of Portage's `shlex.split` + `varexpand` (Portage's own default template and every test command split cleanly).

## fn run_gpg_verify
- Mirrors Portage's `checksum_helper.finish` + `_check_gpg_status` (`gpkg.py:624-659`, `gpkg.py:590-612`).
- Ports Portage's `show_gpg_error` single-cause summaries (`gpkg.py:563-579`).

## fn verify_detached_signature
- Ports Portage's `_verify_binpkg` `f_signature` branch (`gpkg.py:1764-1788`); Portage reports per-file.

## fn verify_clearsigned_manifest
- Ports Portage's `_verify_binpkg` Manifest branch (`gpkg.py:1714-1731`).
