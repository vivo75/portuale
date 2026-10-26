# crates/portage-repo/src/tests/dep_validate.rs
`validate_dep_string` pins, including the probe atom through the reducer hook.

## fn reducer_rejects_the_510_probe_atom_with_the_visible_wrapper (#531)
- The #510 probe atom through the *reducer* hook, which is the path `Package._validate_deps` uses. It passes `is_valid_flag`, so Portage's C fast path (`is_valid_flag is None` is one of its gates, `dep/__init__.py:1079-1086`) is skipped and the Python wrapper at :974 is the visible text: `emerge -p` masks the package with `invalid: RDEPEND: Invalid atom (Invalid use dep: '-a'), token 1`. The raw C text (`invalid use dep token: '-a'`) is what a `token_class=Atom` call *without* `is_valid_flag` raises, and it is not user-visible.
- Oracle: `LLM/evidence/2026-10-24-531/` (host Portage 3.0.83 vs portuale, the `masked by:` line byte-identical).
- The two atoms the same #510 probe shows the reducer accepts (`games-arcade/2048`, `dev-util/timidity--`).
