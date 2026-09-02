//! The phase and merge shell scripts sent to the client.

use crate::remote::*;

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver; `None`
/// file skips the client record. `portuale_bin` is the resolved absolute
/// client path, exported for the whole driver (including the old hooks).
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}
