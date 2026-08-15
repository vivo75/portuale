/// Direct predicate-result legs in the scratch style of `tests_161`/`tests_162`: each
/// test observes the predicate's own return value. Do not edit other
/// items' `mod tests_16x` blocks.
use crate::*;

// ---- `direct_solve_arg_mode` (Portage `_want_installed_pkg`). ----

/// Minimal visibility config in the `test_config` shape
/// (amd64-only, no overrides).
fn cfg_166() -> portage_profile::Config {
    portage_profile::Config {
        accept_keywords: HashSet::from(["amd64".to_string()]),
        ..Default::default()
    }
}
