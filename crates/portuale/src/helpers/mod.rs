// The `portuale __helper` dispatcher.
//
// Runs first in `main()`, from `args_os()` (paths may not be UTF-8),
// loading no config and writing only its own outputs (D7). Helpers:
// `python` (the script dispatcher), `chmod-lite`, `ebuild-ipc` (always
// 127: no IPC daemon) and `ping` (a fixed token). Anything unknown exits
// 127, so a new upstream call fails loudly (D1).

mod doins;
pub(crate) use doins::{xattr_excluded, xattr_get, xattr_list};
