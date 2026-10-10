# crates/portuale/src/pretend/tests/conflicts.rs
Slot-conflict reasons, caret spans, and colorized-marker alignment.

## fn slot_conflict_reason_and_caret_span
- `">=dev-libs/t-2.0"` does not accept 1.0, hence `("version", "ge")`; a bare atom accepts any version; `"<dev-libs/t-2.0"` accepts 1.0; `">=dev-libs/t-1.0[x]"` accepts 1.0 on version+slot with x on here; x off the other side is a violated use key; x gone from IUSE is an unconditional one.
- Portage's `elif` chain (`slot_collision.py:307-397`): a version-mismatched pair skips the USE check even when the atom's USE deps would also fail against the other instance.
- Caret span: under `">="` (idx 0,1) and under `"2.0"` (rfind); under the violated `"[x]"` token.

## fn slot_conflict_colorized_spans_keep_markers_aligned
- Color off: identity -- same string, same indices.
- Color on: every marked char wrapped, and each caret lands on visible text (never inside an escape), spelling the violated spans back out.
- Map each raw offset to its visible char (None inside escapes).
