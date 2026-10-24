# crates/portuale/src/pretend/usage_text.rs
Portage 3.0.83's full `usage:` block for `emerge`.

## const PORTAGE_USAGE_TEXT
- The 78-line block from `LLM/evidence/2026-10-23-507/captures/53-none.portage.err`
  (lines 1-78), printed above every argparse-native `invalid choice` error
  (backlog #520, plan residui-514-555 Task 3 Step 4). Verbatim, fixed order:
  Portage hash-randomizes the option order per process (`PYTHONHASHSEED`;
  quirks Q11), so no byte order matches every run -- portuale prints this
  capture instead, like the sorted `Change USE:` flags of quirks Q4.
- Gating lives in `parser_error` (`pretend/mod.rs`): only messages containing
  `invalid choice` get the block. `parser.error`-passed messages (trailing
  `\n` rule, 347-ERR) and `unrecognized arguments` keep the 347-USG cut
  (error line only).
