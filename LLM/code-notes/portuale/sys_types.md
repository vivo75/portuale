# crates/portuale/src/sys_types.rs
Context moved out of the code comments.

## module
- Backlog numbers removed from the comment (original text): Width-portable conversions for libc's per-OS integer types (backlog #665).  `mode_t` is `u32` on Linux but `u16` on FreeBSD, so `std`'s `u32` mode has to be narrowed before it meets a libc call or a `libc::S_IF*` constant.
