//! env.d files.

use crate::*;

/// One `<eroot>/etc/profile.env` read: the scalar layer, the tracked
/// incremental layers, and whether `PATH` was assigned. Shared by the
/// first load and `Config::reload_env_d` so the two cannot drift apart.
// Portage: config.py, _get_env_d (getconfig with expand=False)
pub(crate) struct EnvdParse {
    pub(crate) vars: HashMap<String, String>,
    pub(crate) incremental_layers: HashMap<String, Vec<String>>,
    pub(crate) sets_path: bool,
}

/// Every `USE=` value in `<eroot>/etc/profile.env`, in file order, for
/// `effective_use_flags` to fold in. A missing file yields an empty list.
// Portage: config.py, configdict["env.d"]["USE"]
pub(crate) fn read_envd_use_tokens(eroot: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(eroot.join("etc/profile.env")) else {
        return Vec::new();
    };
    read_envd_use_tokens_text(&text)
}

/// [`read_envd_use_tokens`] over already-read text, so `reload_env_d`
/// derives the tokens from the same read as the scalar layer.
pub(crate) fn read_envd_use_tokens_text(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = parse_kv_line(line)?;
            (key == "USE").then(|| value.to_string())
        })
        .collect()
}
