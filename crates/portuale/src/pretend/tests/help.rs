use super::*;

#[test]
fn backtrack_default_matches_portage_twenty() {
    // Backlog #263 (R1): real's default `--backtrack` is 20, not 10.
    // Grounded in real Portage 3.0.82.2 (`/usr/sbin/emerge`):
    // `_emerge/depgraph.py:12188` (`_backtrack_depgraph`) reads
    // `max_retries = 0 if nodeps else myopts.get("--backtrack", 20)`,
    // while `--backtrack` itself is `action: store` with no default
    // (`lib/_emerge/main.py:385-389`) -- the 20 lives only in that
    // reader (`0` only under `--nodeps`). The flag-absent initializer
    // (`DEFAULT_BACKTRACK_MAX`) and the `--help` line must both carry
    // it, so the printed `(backtrack: N/M)` denominator and the
    // `--json` `"max"` agree with real's `M`.
    assert_eq!(DEFAULT_BACKTRACK_MAX, 20);
    assert!(
        HELP_TEXT.contains(
            "--backtrack N         maximum resolver backtracking passes (default 20; 0 disables)"
        ),
        "the --help line must advertise the real default"
    );
}
