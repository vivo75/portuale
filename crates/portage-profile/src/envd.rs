//! env.d files.

use crate::*;

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
