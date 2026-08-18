//! Preflight gates checked before a remote merge.

use crate::remote::*;

/// Parse `KEY=VALUE` stdout lines into a map (anything else ignored).
pub(crate) fn parse_kv(stdout: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=')
            && !key.is_empty()
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

/// Clock gate: `|server - client| <= max_skew` (`0` disables). Returns
/// the failure message, if any.
pub(crate) fn clock_gate(
    client_time: Option<&str>,
    server_time: u64,
    max_skew: u64,
) -> Option<String> {
    if max_skew == 0 {
        return None;
    }
    let client: u64 = client_time?.parse().ok()?;
    let skew = server_time.abs_diff(client);
    if skew > max_skew {
        Some(format!(
            "client/server clocks differ by {skew}s (limit {max_skew}s)"
        ))
    } else {
        None
    }
}
