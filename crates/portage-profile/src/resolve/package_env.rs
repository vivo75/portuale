//! `package.env` and the `etc/portage/env` files it names.

use crate::resolve::*;

/// `package.env` and the `etc/portage/env` files it names.
pub(crate) fn fold_package_env(
    config_root: &Path,
    config: &mut Config,
    scalars: &HashMap<String, String>,
) -> Result<(), Error> {
    // package.env (real config.py:894 `grabdict_package` +
    // `_grab_pkg_env`): `/etc/portage/package.env` maps an atom to one or
    // more env-file names under `/etc/portage/env/`; each file is a
    // `make.conf`-style `KEY=value` snippet (with `source`) layered onto
    // a matching package's config. Same `<atom> <token...>` line grammar
    // as `package.accept_keywords` (real `grabdict` for both). The `USE=`
    // half of each file lands in `package_env_use` (the `pkg` `USE_ORDER`
    // layer, before user `package.use`); the non-`USE` scalar half lands
    // in `package_env_vars` (a per-package build-phase env override).
    let package_env_lines = read_config_lines(&config_root.join("etc/portage/package.env"))?;
    config.package_env = parse_package_accept_keywords_lines(&package_env_lines)
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .collect();
    let env_dir = config_root.join("etc/portage/env");
    // Each `package.env` entry's files share one expand-map copy (see
    // `expand_package_env_files`); the two halves split it afterwards.
    let package_env_expanded: Vec<(String, Vec<(String, String)>)> = config
        .package_env
        .iter()
        .map(|(atom, files)| {
            (
                atom.clone(),
                expand_package_env_files(&env_dir, files, config_root, scalars),
            )
        })
        .collect();
    config.package_env_use = package_env_expanded
        .iter()
        .filter_map(|(atom, pairs)| {
            let tokens: Vec<String> = pairs
                .iter()
                .filter(|(k, _)| k == "USE")
                .flat_map(|(_, v)| v.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect();
            (!tokens.is_empty()).then(|| (atom.clone(), tokens))
        })
        .collect();
    config.package_env_vars = package_env_expanded
        .iter()
        .filter_map(|(atom, pairs)| {
            let vars: Vec<(String, String)> =
                pairs.iter().filter(|(k, _)| k != "USE").cloned().collect();
            (!vars.is_empty()).then(|| (atom.clone(), vars))
        })
        .collect();
    Ok(())
}
