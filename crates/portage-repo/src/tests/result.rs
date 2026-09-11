use super::*;

#[test]
fn abort_path_gate_truth_table() {
    // `PORTUALE_ABORT_PATH=0` is the legacy fallback; unset or any
    // other value enables the abort path (Gate G0.4). A concurrent
    // resolve racing these mutations is behaviorally invisible: the
    // gate only guards the `Aborted` arm and Slice 2 never produces
    // one, so every concurrent outcome stays `Complete` either way.
    // (`set_var`/`remove_var` are `unsafe` in edition 2024 because
    // env is process-global; the blocks below are sound for exactly
    // the invisibility reason above.)
    unsafe {
        std::env::remove_var("PORTUALE_ABORT_PATH");
    }
    assert!(abort_path_enabled());
    unsafe {
        std::env::set_var("PORTUALE_ABORT_PATH", "1");
    }
    assert!(abort_path_enabled());
    unsafe {
        std::env::set_var("PORTUALE_ABORT_PATH", "yes");
    }
    assert!(abort_path_enabled());
    unsafe {
        std::env::set_var("PORTUALE_ABORT_PATH", "0");
    }
    assert!(!abort_path_enabled());
    unsafe {
        std::env::remove_var("PORTUALE_ABORT_PATH");
    }
}
