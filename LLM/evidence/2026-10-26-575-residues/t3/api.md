# Task 3 S0: `digest 0.11` API changes at our call sites

Reference: `digest 0.11.3` in `~/.cargo/registry/src/.../digest-0.11.3`
(CHANGELOG.md, `src/digest.rs`); `hybrid-array 0.4.15` (`src/lib.rs`);
`blake2-0.11.0` (`src/lib.rs`); `sha3-0.12.0` (`src/lib.rs`).
Target hasher releases: `md-5 0.11.0`, `sha1 0.11.0`, `sha2 0.11.1`,
`sha3 0.12.0`, `blake2 0.11.0` (all present in the local registry cache).

## 1. `finalize()` / `digest()` output type: `GenericArray` -> `hybrid-array` `Array`

`digest` 0.11.0 replaced `generic-array` with `hybrid-array`
(CHANGELOG 0.11.0, "Replaced `generic-array` with `hybrid-array` ([#1358])").
`Digest::finalize(self) -> Output<Self>` and
`Digest::digest(data: impl AsRef<[u8]>) -> Output<Self>`
(`src/digest.rs:32,56`), where `Output<T>` is now
`hybrid_array::Array<u8, <T as OutputSizeUser>::OutputSize>`.

What still compiles unchanged at our call sites:

- `.iter().map(|b| format!("{b:02x}"))` — `Array` implements `Deref`
  (`hybrid-array/src/lib.rs:718`) and `AsRef<[T]>` (`:588`), so every
  existing byte-wise hex fold keeps working. This covers most call sites
  (`ebuild_package.rs`, `ebuild_merge/tree.rs`, `helpers/gpkg.rs`,
  `helpers/{xattr,doins}.rs`, `binpkg/digest.rs`, `binpkg/tests/*`,
  `emerge_getbinpkg.rs:2857-2859`, `pretend/tests/remote.rs`).
- `hex(&md5::Md5::digest(bytes))` with `hex(d: &[u8])` (`binpkg/digest.rs:204`,
  `helpers/gpkg.rs:525-526`): `&Array<u8, N>` coerces to `&[u8]` through the
  same `Deref` impl.
- `.to_vec()` (`portage-fetch/src/layout.rs:82-87`): resolves through `Deref`
  to the slice method.
- `to_hex(&blake2::Blake2b512::digest(&bytes))` with `to_hex(bytes: &[u8])`
  (`portage-fetch/src/lib.rs:856-857,908-909`): same deref coercion.
- `Digest::new` / `update` / `finalize` streaming shape: unchanged.
  (`update` now takes `impl AsRef<[u8]>` instead of `&[u8]`, which accepts
  every existing argument.)

## 2. `format!("{:x}", …)` no longer works on a digest output

In `digest 0.10`, `{:x}` worked because `generic-array` implements
`LowerHex`. `hybrid-array 0.4.15` implements only `Debug` for `Array`
(`src/lib.rs:696-704`); there is no `LowerHex`/`UpperHex`/`Display` impl,
and `digest 0.11.3` adds none either (only `Display` for its error types).
So every `format!("{:x}", <digest output>)` is a compile error after the
bump and must switch to the single shared helper
`portage_util::hex_lower` (no shared hex helper exists today — S0 grep for
`hex_lower|fn to_hex|fn hex` found only private per-file copies:
`portage-fetch/src/lib.rs:321`, `helpers/gpkg.rs:1754` (test-only),
`binpkg/digest.rs:204` (closure), `helpers/gpkg.rs:524` (closure)).

`{:x}` sites to convert (from the plan-time grep):

- `crates/portage-profile/src/user_patches.rs:193` (+ its test `:44-45`)
- `crates/portuale/src/regen.rs:456,536,681,717`
- `crates/portuale/src/remote_bundle.rs:212,625`
- `crates/portuale/src/remote/client_binary.rs:79,311`
  (+ `remote/tests/mod.rs:668` `s8_sha256`, `remote/tests/preflight.rs:112,256`)
- `crates/portuale/src/emerge_build/tests/buildpkgonly.rs:839,866,876,889`
- `crates/portage-repo/src/md5_dict.rs:197,273,369`
- `crates/portage-repo/src/solver_bridge.rs:9187`
  (+ ~30 `portage-repo/src/tests/**` copies of the same one-liner)

## 3. Type names: unchanged

- `Blake2b512` / `Blake2s256` still exist as aliases in `blake2-0.11.0`
  (`src/lib.rs:81,127`).
- `Sha3_256` / `Sha3_512` still exist in `sha3-0.12.0`
  (`src/lib.rs:149,153`).
- `Md5`, `Sha1`, `Sha256`, `Sha512` unchanged.

## 4. No other API hits

No call site uses `finalize_reset`, `finalize_into`, `VariableOutput`,
`DynDigest`, `digest::core_api`, or names `GenericArray`/`Output<…>` in a
type position (grep over `crates --include=*.rs` is clean apart from
iterator `.chain(` hits). The `Verifier`/`Mac` removals in the 0.11
changelog do not touch us. So the move is: bump four `Cargo.toml` files,
convert the `{:x}` list above, nothing else.
