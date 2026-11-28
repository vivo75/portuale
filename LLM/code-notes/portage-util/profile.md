# crates/portage-util/src/profile.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): Phase timers behind `PORTUALE_PROFILE=1` (backlog #455, plan S0).  `span("name")` returns a guard that prints `[profile] name: <seconds>` to stderr when dropped, only when the variable is set (read once). Off by default: one cached bool check and no output, so it never changes `emerge` output.
