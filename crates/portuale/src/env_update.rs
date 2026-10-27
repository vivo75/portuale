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

const SPACE_SEPARATED: [&str; 2] = ["CONFIG_PROTECT", "CONFIG_PROTECT_MASK"];
const COLON_SEPARATED: [&str; 12] = [
    "ADA_INCLUDE_PATH",
    "ADA_OBJECTS_PATH",
    "CLASSPATH",
    "INFODIR",
    "INFOPATH",
    "KDEDIRS",
    "LDPATH",
    "MANPATH",
    "PATH",
    "PKG_CONFIG_PATH",
    "PYTHONPATH",
    "ROOTPATH",
];

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

/// At least 3 chars, starting with two digits, not a dotfile/backup.
// Portage: lib/portage/util/env_update.py, env_update
fn is_env_d_filename(name: &str) -> bool {
    let bytes = name.as_bytes();
    name.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && !name.starts_with('.')
        && !name.ends_with('~')
        && !name.ends_with(".bak")
}

/// `INFOPATH` entries first, then `INFODIR` entries; empty entries
/// dropped, duplicates kept. Shared by `info_dirs_inodes` and `info_files`
/// so there is exactly one env.d parser for both.
// Portage: lib/_emerge/post_emerge.py, post_emerge
pub fn info_dir_values(root: &Path) -> Vec<String> {
    let mut infopath: Vec<String> = Vec::new();
    let mut infodir: Vec<String> = Vec::new();
    let envd_dir = root.join("etc/env.d");
    if let Ok(entries) = portage_util::read_dir_entries(&envd_dir) {
        let filenames: Vec<String> = entries
            .into_iter()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| is_env_d_filename(name))
            .collect();
        for fname in &filenames {
            let Ok(text) = std::fs::read_to_string(envd_dir.join(fname)) else {
                continue;
            };
            for line in text.lines() {
                let Some((key, value)) = parse_env_d_line(line) else {
                    continue;
                };
                if key == "INFOPATH" {
                    infopath.extend(value.split(':').filter(|s| !s.is_empty()).map(String::from));
                } else if key == "INFODIR" {
                    infodir.extend(value.split(':').filter(|s| !s.is_empty()).map(String::from));
                }
            }
        }
    }
    infopath.extend(infodir);
    infopath
}
/// Each existing `root`-joined candidate directory from
/// `info_dir_values` contributes its own `(dev, ino)`, via `stat`
/// (follows symlinks, so an `INFOPATH` entry naming a symlink still
/// matches). Missing entries degrade to an empty/smaller set.
// Portage: lib/portage/dbapi/vartree.py, _unmerge_pkgfiles
pub fn info_dirs_inodes(root: &Path) -> std::collections::BTreeSet<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    info_dir_values(root)
        .iter()
        .filter_map(|dir| std::fs::metadata(root.join(dir.trim_start_matches('/'))).ok())
        .map(|meta| (meta.dev(), meta.ino()))
        .collect()
}

/// Every `LDPATH` entry, plus `usr/lib`/`lib`, plus any top-level or
/// `usr/`-relative directory starting with `lib` (excluding `libexec`),
/// filtered to directories actually on disk post-merge.
// Portage: lib/portage/util/env_update.py, getlibpaths
fn candidate_lib_dirs(root: &Path, ldpath_entries: &[String]) -> Vec<PathBuf> {
    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in ldpath_entries {
        dirs.insert(entry.trim_start_matches('/').to_string());
    }
    dirs.insert("usr/lib".to_string());
    dirs.insert("lib".to_string());

    if let Ok(entries) = portage_util::read_dir_entries(root) {
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("lib") && name != "libexec" && e.path().is_dir() {
                dirs.insert(name);
            }
        }
    }
    if let Ok(entries) = portage_util::read_dir_entries(&root.join("usr")) {
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("lib") && name != "libexec" && e.path().is_dir() {
                dirs.insert(format!("usr/{name}"));
            }
        }
    }

    dirs.into_iter()
        .map(PathBuf::from)
        .filter(|d| root.join(d).is_dir())
        .collect()
}

/// Run `<root>/sbin/ldconfig -X -r <root>` as an unmodified subprocess.
/// A missing or non-executable binary is silently a no-op; the exit
/// status is ignored once spawned (a failed `ldconfig` never aborts
/// the merge).
fn run_ldconfig(root: &Path) -> Result<(), String> {
    let ldconfig = root.join("sbin/ldconfig");
    let Ok(metadata) = std::fs::metadata(&ldconfig) else {
        return Ok(());
    };
    if !metadata.is_file() {
        return Ok(());
    }
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Ok(());
    }
    std::process::Command::new(&ldconfig)
        .arg("-X")
        .arg("-r")
        .arg(root)
        .current_dir("/")
        .status()
        .map_err(|e| format!("{}: {e}", ldconfig.display()))?;
    Ok(())
}

fn write_generated(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Collate env.d and write the four generated files, then run
/// `ldconfig` when a candidate lib dir exists. Called unconditionally
/// after a successful merge that installed something.
pub fn run_env_update(root: &Path) -> Result<(), String> {
    let envd_dir = root.join("etc/env.d");
    std::fs::create_dir_all(&envd_dir).map_err(|e| format!("{}: {e}", envd_dir.display()))?;

    let filenames: Vec<String> = portage_util::read_dir_entries(&envd_dir)
        .map_err(|e| format!("{}: {e}", envd_dir.display()))?
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| is_env_d_filename(name))
        .collect();

    let mut space_values: BTreeMap<&str, Vec<String>> =
        SPACE_SEPARATED.iter().map(|k| (*k, Vec::new())).collect();
    let mut colon_values: BTreeMap<&str, Vec<String>> =
        COLON_SEPARATED.iter().map(|k| (*k, Vec::new())).collect();
    let mut scalar_env: BTreeMap<String, String> = BTreeMap::new();

    for fname in &filenames {
        let Ok(text) = std::fs::read_to_string(envd_dir.join(fname)) else {
            continue;
        };
        for line in text.lines() {
            let Some((key, value)) = parse_env_d_line(line) else {
                continue;
            };
            if let Some(list) = space_values.get_mut(key.as_str()) {
                for item in value.split_whitespace() {
                    if !list.iter().any(|existing| existing == item) {
                        list.push(item.to_string());
                    }
                }
            } else if let Some(list) = colon_values.get_mut(key.as_str()) {
                for item in value.split(':') {
                    if !item.is_empty() && !list.iter().any(|existing| existing == item) {
                        list.push(item.to_string());
                    }
                }
            } else {
                scalar_env.insert(key, value);
            }
        }
    }

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    for (key, list) in &space_values {
        if !list.is_empty() {
            env.insert(key.to_string(), list.join(" "));
        }
    }
    for (key, list) in &colon_values {
        if !list.is_empty() {
            env.insert(key.to_string(), list.join(":"));
        }
    }
    for (key, value) in &scalar_env {
        env.insert(key.clone(), value.clone());
    }

    let ldpath_list = colon_values.get("LDPATH").cloned().unwrap_or_default();

    // /etc/ld.so.conf
    let mut ldsoconf = String::from(
        "# ld.so.conf autogenerated by env-update; make all changes to\n\
         # contents of /etc/env.d directory.\n",
    );
    for path in &ldpath_list {
        ldsoconf.push_str(path);
        ldsoconf.push('\n');
    }
    write_generated(&root.join("etc/ld.so.conf"), &ldsoconf)?;

    let notice = "# THIS FILE IS AUTOMATICALLY GENERATED BY env-update.\n# DO NOT EDIT THIS FILE.";
    let env_keys: Vec<&String> = env.keys().filter(|k| k.as_str() != "LDPATH").collect();

    // /etc/profile.env (bash)
    let mut profile_env = format!(
        "{notice} CHANGES TO STARTUP PROFILES\n\
         # GO INTO /etc/profile NOT /etc/profile.env\n\n"
    );
    for key in &env_keys {
        let value = &env[key.as_str()];
        if let Some(rest) = value.strip_prefix('$')
            && !value.starts_with("${")
        {
            profile_env.push_str(&format!("export {key}=$'{rest}'\n"));
            continue;
        }
        profile_env.push_str(&format!("export {key}='{value}'\n"));
    }
    write_generated(&root.join("etc/profile.env"), &profile_env)?;

    // /etc/csh.env (tcsh)
    let mut csh_env = format!(
        "{notice} CHANGES TO STARTUP PROFILES\n\
         # GO INTO /etc/csh.cshrc NOT /etc/csh.env\n\n"
    );
    for key in &env_keys {
        csh_env.push_str(&format!("setenv {key} '{}'\n", env[key.as_str()]));
    }
    write_generated(&root.join("etc/csh.env"), &csh_env)?;

    // /etc/environment.d/10-gentoo-env.conf (systemd)
    let mut systemd_env = format!("{notice}\n\n");
    for key in &env_keys {
        let value = &env[key.as_str()];
        if value.is_empty() {
            continue;
        }
        systemd_env.push_str(&format!("{key}={value}\n"));
    }
    write_generated(
        &root.join("etc/environment.d/10-gentoo-env.conf"),
        &systemd_env,
    )?;

    if !candidate_lib_dirs(root, &ldpath_list).is_empty() {
        run_ldconfig(root)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use portage_util::TempDir;
    use std::os::unix::fs::MetadataExt;

    fn tempdir() -> TempDir {
        TempDir::new("portuale-env-update-test")
    }

    #[test]
    fn parse_env_d_line_handles_quoted_and_bare_values() {
        assert_eq!(
            parse_env_d_line(r#"LDPATH="/usr/lib/foo""#),
            Some(("LDPATH".to_string(), "/usr/lib/foo".to_string()))
        );
        assert_eq!(
            parse_env_d_line("FOO=bar"),
            Some(("FOO".to_string(), "bar".to_string()))
        );
        assert_eq!(parse_env_d_line("# a comment"), None);
        assert_eq!(parse_env_d_line(""), None);
    }

    #[test]
    fn is_env_d_filename_matches_portage_filter() {
        assert!(is_env_d_filename("50-foo"));
        assert!(is_env_d_filename("99profile"));
        assert!(!is_env_d_filename("foo"));
        assert!(!is_env_d_filename("5-foo"));
        assert!(!is_env_d_filename(".50-foo"));
        assert!(!is_env_d_filename("50-foo~"));
        assert!(!is_env_d_filename("50-foo.bak"));
    }

    #[test]
    fn info_dir_values_lists_infopath_before_infodir() {
        // Expected order from Portage's post_emerge.py.
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::write(
            tmp.join("etc/env.d/50-foo"),
            "INFOPATH=\"/usr/share/info\"\nINFODIR=\"/opt/pkg/info\"\n",
        )
        .unwrap();

        assert_eq!(
            info_dir_values(&tmp),
            vec!["/usr/share/info".to_string(), "/opt/pkg/info".to_string()]
        );
    }

    #[test]
    fn info_dirs_inodes_resolves_real_infopath_and_infodir_entries() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::create_dir_all(tmp.join("usr/share/info")).unwrap();
        std::fs::create_dir_all(tmp.join("opt/pkg/info")).unwrap();
        std::fs::write(
            tmp.join("etc/env.d/50-foo"),
            "INFOPATH=\"/usr/share/info:/opt/pkg/info\"\n",
        )
        .unwrap();
        std::fs::write(tmp.join("etc/env.d/60-bar"), "INFODIR=\"/opt/pkg/info\"\n").unwrap();

        let inodes = info_dirs_inodes(&tmp);

        let expected_a = std::fs::metadata(tmp.join("usr/share/info")).unwrap();
        let expected_b = std::fs::metadata(tmp.join("opt/pkg/info")).unwrap();
        assert!(inodes.contains(&(expected_a.dev(), expected_a.ino())));
        assert!(inodes.contains(&(expected_b.dev(), expected_b.ino())));
        assert_eq!(inodes.len(), 2);
    }

    #[test]
    fn info_dirs_inodes_ignores_a_candidate_that_does_not_actually_exist() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::write(
            tmp.join("etc/env.d/50-foo"),
            "INFOPATH=\"/does/not/exist\"\n",
        )
        .unwrap();

        assert!(info_dirs_inodes(&tmp).is_empty());
    }

    #[test]
    fn info_dirs_inodes_degrades_gracefully_when_env_d_is_missing() {
        let tmp = tempdir();
        assert!(info_dirs_inodes(&tmp).is_empty());
    }

    #[test]
    fn candidate_lib_dirs_only_returns_dirs_that_actually_exist() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("usr/lib")).unwrap();

        let dirs = candidate_lib_dirs(&tmp, &["/usr/lib/foo".to_string()]);
        // "/usr/lib/foo" doesn't exist -- only usr/lib itself, which does.
        assert_eq!(dirs, vec![PathBuf::from("usr/lib")]);
    }

    #[test]
    fn candidate_lib_dirs_excludes_libexec() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("usr/libexec")).unwrap();
        std::fs::create_dir_all(tmp.join("libfoo")).unwrap();

        let dirs = candidate_lib_dirs(&tmp, &[]);
        assert!(dirs.contains(&PathBuf::from("libfoo")));
        assert!(!dirs.iter().any(|d| d.ends_with("libexec")));
    }

    #[test]
    fn run_env_update_writes_all_four_generated_files_from_env_d() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::write(
            tmp.join("etc/env.d/50-test"),
            "LDPATH=\"/usr/lib/testenv\"\nGREETING=\"hello world\"\n",
        )
        .unwrap();

        run_env_update(&tmp).expect("run_env_update succeeds");

        let ldsoconf = std::fs::read_to_string(tmp.join("etc/ld.so.conf")).unwrap();
        assert!(ldsoconf.contains("/usr/lib/testenv"));

        let profile_env = std::fs::read_to_string(tmp.join("etc/profile.env")).unwrap();
        assert!(profile_env.contains("export GREETING='hello world'"));
        assert!(!profile_env.contains("LDPATH"));

        let csh_env = std::fs::read_to_string(tmp.join("etc/csh.env")).unwrap();
        assert!(csh_env.contains("setenv GREETING 'hello world'"));

        let systemd_env =
            std::fs::read_to_string(tmp.join("etc/environment.d/10-gentoo-env.conf")).unwrap();
        assert!(systemd_env.contains("GREETING=hello world"));
    }

    #[test]
    fn run_env_update_invokes_a_real_ldconfig_when_present_and_executable() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::create_dir_all(tmp.join("usr/lib/testenv")).unwrap();
        std::fs::write(
            tmp.join("etc/env.d/50-test"),
            "LDPATH=\"/usr/lib/testenv\"\n",
        )
        .unwrap();

        std::fs::create_dir_all(tmp.join("sbin")).unwrap();
        std::fs::write(
            tmp.join("sbin/ldconfig"),
            "#!/bin/sh\necho \"$@\" > \"$3/ldconfig-was-invoked\"\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            tmp.join("sbin/ldconfig"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        run_env_update(&tmp).expect("run_env_update succeeds");

        let marker = std::fs::read_to_string(tmp.join("ldconfig-was-invoked"))
            .expect("the fake ldconfig binary was really invoked as a subprocess");
        assert!(marker.contains("-X"), "{marker}");
        assert!(marker.contains("-r"), "{marker}");
    }

    #[test]
    fn run_env_update_does_not_invoke_a_non_executable_ldconfig() {
        let tmp = tempdir();
        std::fs::create_dir_all(tmp.join("etc/env.d")).unwrap();
        std::fs::create_dir_all(tmp.join("sbin")).unwrap();
        std::fs::write(
            tmp.join("sbin/ldconfig"),
            "#!/bin/sh\ntouch should-not-run\n",
        )
        .unwrap();

        run_env_update(&tmp).expect("run_env_update succeeds");
        assert!(!tmp.join("ldconfig-was-invoked").exists());
    }
}
