# crates/portuale/src/test_watchdog.rs
Hang watchdog for the unit-test binary (backlog #376). Test builds only, Linux only.

## static START
- Lives in `.init_array`, so it runs before `main` and no test has to opt in. It spawns one thread named `test-watchdog`.
- Idle limit: `PORTUALE_TEST_WATCHDOG_IDLE_SECS` (default 600; `0` disables). Polled every 30 s, so the real trigger is the limit rounded up to a poll.

## fn watch / fn dump
- "Idle" means the summed `utime + stime` of this process and every descendant (from `/proc/*/stat`) did not move. Children matter: a long real build in a child must not look like a hang.
- On expiry it prints the process list and every thread's stack (`eu-stack`, else `gdb -batch`) to stderr, then exits 124 so a gate fails with evidence instead of sitting at 0 CPU. Verified 2026-10-17 with a throwaway 300 s sleeping test and a 30 s limit.
