# crates/portage-repo/src/resolve/pass/finish_required_by.rs
Fill entry required_by and drop shadowed installed entries.

## fn finish_required_by
- Multi-slot support: several entries per `(category, package)`; `--json` reported `"required_by": []` for the starved ones. A `--root-deps` running-root build entry sets its own immediate requester at construction.

- The BFS can build `AlreadyInstalled` before a later atom forces a same-slot merge (`net-libs/nghttp2`'s `>=sys-apps/systemd-209` resolving to installed systemd, then `sys-auth/polkit`'s `sys-apps/systemd:0=[policykit]` forcing the reinstall).

