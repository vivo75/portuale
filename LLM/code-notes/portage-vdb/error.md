# crates/portage-vdb/src/error.rs
Hand-written error type for the VDB crate (no dependencies, no `thiserror`).

## enum Error
- Non-exhaustive by design; later plan steps add variants.
- History: S5.2 added `Error::Busy`; S5.5 surfaces it in the CLIs (`convert`, `vdb status`).
- `Io` displays as `"<path>: <io error>"`, the format every caller prints today.
- `Unsupported` text names the operation and, for a stub, the plan step that adds it.
- `Invalid` covers e.g. an `aux_get` key outside the 23 fields or an unknown backend name.
- `Busy` names the file: a second `mrg`, converter or FUSE mount on the same redb file must wait for the first to finish.
