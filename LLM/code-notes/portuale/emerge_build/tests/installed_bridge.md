# crates/portuale/src/emerge_build/tests/installed_bridge.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): Order edges through installed packages (#798). Expected behaviour comes from the build-order property in `docs/charter.md` (a package must not be built against a library the same plan is about to replace); Portage's complete graph keeps installed packages' edges, which is where the idea comes from.
