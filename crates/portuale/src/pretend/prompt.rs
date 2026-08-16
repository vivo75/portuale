//! `--ask` prompts and their terminal handling.

use crate::pretend::*;

/// `--ask-enter-invalid`: process-wide because the flag is CLI-global and
/// only read by the interactive prompts. `run` resets it at entry and sets
/// it while parsing, so in-process callers can't leak it between invocations.
pub(crate) static ASK_ENTER_INVALID: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
