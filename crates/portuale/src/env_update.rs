//! Portage's `env_update()` (`lib/portage/util/env_update.py`), run after a
//! merge that installed something: collate `/etc/env.d/*` into
//! `profile.env`, `csh.env`, the systemd `environment.d` file and
//! `ld.so.conf`, running the target root's own `ldconfig` when a
//! candidate lib dir exists on disk.
//!
//! Scope cuts a caller must know: no cross-process mtime cache, so every
//! run is treated as a first run (`ldconfig` may run redundantly but never
//! wrongly); no `ld.so.conf.d` includes, `CHOST`/`CBUILD`, `EPREFIX`,
//! extra cumulative keys, or shlex parsing; `/etc/ld.so.conf` is always
//! rewritten.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One `KEY="value"`/`KEY='value'`/`KEY=value` assignment per line,
/// trailing `# comment` allowed. No shlex (multi-line values, escapes).
fn parse_env_d_line(line: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(
        r#"^[ \t]*([A-Za-z_][A-Za-z0-9_]*)=(?:"([^"]*)"|'([^']*)'|(\S*))[ \t]*(#.*)?$"#,
    )
    .expect("static regex is valid");
    let caps = re.captures(line)?;
    let key = caps.get(1)?.as_str().to_string();
    let value = caps
        .get(2)
        .or_else(|| caps.get(3))
        .or_else(|| caps.get(4))
        .map(|m| m.as_str())
        .unwrap_or("");
    Some((key, value.to_string()))
}
