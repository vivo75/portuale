# Task 1 report — version 300.83.0 and the version scheme (#590)

## Files changed (portuale repo only, no pmtest half needed)

- `Cargo.toml` — `[workspace.package]` gains `version = "300.83.0"` (with a
  three-line scheme comment); new `[workspace.metadata.portuale]` with
  `portage-version = "3.0.83"`.
- `crates/*/Cargo.toml` (all 16) — `version = "0.1.0"` →
  `version.workspace = true`. Verified:
  `grep -h '^version' crates/*/Cargo.toml | sort | uniq -c` → 16 ×
  `version.workspace = true`; a second grep for `0.1.0` in crate manifests
  finds nothing.
- `crates/portuale/src/pretend/info.rs` — appended `#[cfg(test)] mod tests`
  with the two tests below. `PORTED_PORTAGE_VERSION` value unchanged
  (`"3.0.83"`).
- `docs/CONTRIBUTING.md` — new "Version numbers" section (three rules, 0 /
  ≥1 meaning of PATCH, metadata field, fourth digit ignored with fix → PATCH
  bump, 3.0.83 → 300.83.0 example).
- `LLM/agent-context.md` — one line under "Current state" pointing at
  "Version numbers" in `docs/CONTRIBUTING.md`.
- `LLM/code-notes/portuale/pretend/info.md` — new `## tests version_scheme`
  section (scheme, metadata/constant/version lock-step, fourth-digit rule);
  ran `python3 LLM/tools/code_notes_index.py` afterwards.
- `Cargo.lock` — exactly the 16 version lines `0.1.0` → `300.83.0`
  (`git diff Cargo.lock | grep '^[+-]version'` → 16 × each; `git diff --stat`
  shows 16 insertions / 16 deletions and nothing else).

## Tests and what each pins (Portage citation: `3rdparty/portage/meson.build:4`, `version : '3.0.83'`)

- `pretend::info::tests::crate_version_follows_the_ported_portage_version`
  (plan-verbatim): `CARGO_PKG_VERSION` is plain three-part SemVer with
  MAJOR = Portage MAJOR × 100 + MINOR and MINOR = Portage PATCH.
- `pretend::info::tests::workspace_metadata_names_the_ported_portage_version`:
  reads the workspace `Cargo.toml` via
  `env!("CARGO_MANIFEST_DIR")/../../Cargo.toml` with a plain line search for
  `portage-version = "` (no `toml` dependency added — `toml` is not a direct
  dependency of the `portuale` crate) and asserts the value starts with
  `PORTED_PORTAGE_VERSION`, keeping metadata, constant and crate version in
  step.

## Commands run and results

- RED: `cargo test --release -p portuale --bin portuale crate_version_follows`
  → FAIL as expected (`left: 0, right: 300`). Note: the plan's `--lib` flag
  does not work here — the `portuale` crate is binary-only (no library
  target), so `--bin portuale` is the correct equivalent.
- `cargo build --release` → success; lockfile diff is the 16 version lines only.
- GREEN: `cargo test --release -p portuale --bin portuale pretend::info` →
  2 passed, 0 failed.
- `cargo fmt --check` → clean. `cargo clippy --release --all-targets` →
  zero warnings (verified with a forced re-check of the touched crate).
- No pmtest suite impact: the only `--version` contract pins
  (`test_552_version_prints_the_portage_line`,
  `test_552_version_ignores_the_privilege_gate`) assert `startswith("Portage ")`,
  which is unchanged since the constant's value did not move; no pmtest file
  pins `0.1.0` or `CARGO_PKG_VERSION`.

## Open doubts / notes for the manager

- `LLM/evidence/2026-10-26-575-residues/t8/targets.md` shows as modified in
  `git status` but I never touched it — it was already dirty when this task
  started (phase-0 Task 8 work in progress). Left alone.
- Skipped per brief: full gate (`cargo test --release` workspace-wide,
  `scripts/pytest-parallel.sh`), merge-path gate (no merge-path code touched —
  version strings only; `--version` output bytes unchanged), commit, and
  marking #590 DONE. No corpus drift is possible from this change (no output
  bytes change), but the manager's gate will confirm.
- No OWNER QUESTION: every step was settled from the plan text and Portage
  3.0.83 source (`meson.build:4`).
