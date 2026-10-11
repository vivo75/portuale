# crates/portuale/src/remote/host_keys.rs
Known-hosts lookups and host-key fingerprints for the ssh transport.

## fn known_host_lines
- Follows a custom `UserKnownHostsFile` from `--remote-ssh-args` because without it ssh-keygen would read the default file while ssh used the custom one (and vice versa).

## fn lookup_ids
- The `[host]:port` spelling is needed for non-default ports, which a plain `-F host` lookup does not match.
