//! Width-portable conversions for libc's per-OS integer types (backlog #665).
//!
//! `mode_t` is `u32` on Linux but `u16` on FreeBSD, so `std`'s `u32` mode has
//! to be narrowed before it meets a libc call or a `libc::S_IF*` constant.

/// `std::fs::Metadata::mode()` (always `u32`) as the platform's `mode_t`.
/// Callers keep only permission/type bits, which fit every `mode_t`.
#[allow(clippy::unnecessary_cast)]
pub(crate) fn mode_t_of(mode: u32) -> libc::mode_t {
    mode as libc::mode_t
}
