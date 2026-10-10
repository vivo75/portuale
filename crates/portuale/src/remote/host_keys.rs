//! known_hosts lookups and host-key fingerprints.

use crate::remote::*;

/// `ssh-keygen -F <id>` output, if the id is already known (empty =
/// first contact). Best-effort: failures mean "unknown", never fatal.
/// Follows a custom `UserKnownHostsFile` from `--remote-ssh-args`.
pub(crate) fn known_host_lines(ctx: &RemoteContext, id: &str) -> String {
    let mut cmd = std::process::Command::new("ssh-keygen");
    if let Some(file) = known_hosts_file(ctx) {
        cmd.args(["-f", &file]);
    }
    cmd.args(["-F", id])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// Fingerprint lines for a known id (`ssh-keygen -l -F`), best-effort.
pub(crate) fn host_fingerprint(ctx: &RemoteContext, id: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("ssh-keygen");
    if let Some(file) = known_hosts_file(ctx) {
        cmd.args(["-f", &file]);
    }
    let out = cmd.args(["-l", "-F", id]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A `-o Name=Value` ssh option's value inside `--remote-ssh-args`
/// (joined `-oName=V` and split `-o Name=V` spellings), if present.
pub(crate) fn ssh_option_value(ctx: &RemoteContext, name: &str) -> Option<String> {
    let extra = ctx.ssh_args.as_ref()?;
    let prefix = format!("{name}=");
    let mut tokens = extra.split_whitespace().peekable();
    while let Some(token) = tokens.next() {
        if let Some(rest) = token.strip_prefix("-o") {
            if rest.is_empty() {
                if let Some(next) = tokens.next()
                    && let Some(value) = next.strip_prefix(&prefix)
                    && !value.is_empty()
                {
                    return Some(value.to_string());
                }
            } else if let Some(value) = rest.strip_prefix(&prefix)
                && !value.is_empty()
            {
                return Some(value.to_string());
            }
        } else if let Some(value) = token.strip_prefix(&prefix)
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Custom known-hosts file for both ssh and ssh-keygen lookups, if the
/// caller set one (tests isolate it; production defaults to ssh's own).
pub(crate) fn known_hosts_file(ctx: &RemoteContext) -> Option<String> {
    ssh_option_value(ctx, "UserKnownHostsFile")
}

/// The known_hosts id(s) this connection is stored under: a
/// `-o HostKeyAlias=X` redirects it, else the hostname plus the
/// `[host]:port` spelling for non-default ports.
pub(crate) fn lookup_ids(ctx: &RemoteContext) -> Vec<String> {
    if let Some(alias) = ssh_option_value(ctx, "HostKeyAlias") {
        return vec![alias];
    }
    vec![
        ctx.hostname.clone(),
        format!("[{}]:{}", ctx.hostname, ctx.port),
    ]
}

/// True when none of the connection's lookup ids is already known.
pub(crate) fn is_first_contact(ctx: &RemoteContext) -> bool {
    lookup_ids(ctx)
        .iter()
        .all(|id| known_host_lines(ctx, id).trim().is_empty())
}

/// Fingerprint of the key used for this connection, if known now.
pub(crate) fn connection_fingerprint(ctx: &RemoteContext) -> Option<String> {
    lookup_ids(ctx)
        .iter()
        .find_map(|id| host_fingerprint(ctx, id))
}
