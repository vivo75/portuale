# crates/portage-versions/src/lib.rs
Port of Portage's `lib/portage/versions.py` version comparison (`ververify`, `vercmp`).

## module
- Ported by hand, structurally close to the Python original so it stays easy to diff against on future changes.
- `BigNum` only ever arises from genuine overflow (verified: Rust's `parse` accepts arbitrary leading zeros), so any `BigNum` exceeds `i128::MAX` and therefore any `Num`.
- This is a portuale artifact, not yet idiomatic-first Rust; see LLM/agent-context.md.
- The leading-zero float-like comparison mirrors the Python original's.

## fn vercmp
- Mirrors `portage.versions.vercmp(ver1, ver2, silent=1)`; `None` means at least one input failed `ververify`.
- Behaviour change between portage-2.0.x and portage-2.1: a bare letter suffix now sorts after the same version without one (e.g. "12.2.5" > "12.2b").
- Python wraps suffix numerals in unbounded `int()` when non-empty, hence the `BigNum` suffix tests with 30-digit numerals.

## fn parse_uint_or_zero
- Revision and suffix numbers may be absent, in which case Python treats the implicit value as zero (`int("")` would raise, so it fudges to 0).
