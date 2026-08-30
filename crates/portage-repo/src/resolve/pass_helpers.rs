//! Small helpers of the resolver pass.

use crate::*;

/// The effective value of one autounmask flag for the current pass: this pass's
/// overlay first, then the cross-pass accumulator. The walk's two flip sites must
/// see both (a flip folded on an *earlier* pass lives in `committed`; one folded
/// *earlier this pass* lives in `overlay`).
pub(crate) fn overlay_use_want(
    overlay: &HashMap<(String, String), HashMap<String, bool>>,
    committed: &HashMap<(String, String), HashMap<String, bool>>,
    key: &(String, String),
    flag: &str,
) -> Option<bool> {
    overlay
        .get(key)
        .and_then(|b| b.get(flag))
        .copied()
        .or_else(|| committed.get(key).and_then(|b| b.get(flag)).copied())
}
