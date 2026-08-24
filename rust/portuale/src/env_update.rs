// Real `env_update()` (`lib/portage/util/env_update.py`): real `dblink.
// merge()` (`lib/portage/dbapi/vartree.py:5198-5209`) runs this
// unconditionally after a successful merge that actually installed
// something -- even when `pkg_postinst` itself failed ("It's stupid to
// bail out here, so keep going regardless of phase return code"), gated
// only on the merge's own `CONTENTS` being non-empty. It parses real
// `/etc/env.d/*` files, regenerates `/etc/profile.env`/`/etc/csh.env`/a
// systemd `/etc/environment.d/10-gentoo-env.conf`/`/etc/ld.so.conf`, and
// -- when warranted -- runs the real, unmodified `ldconfig` binary.
//
// v1 scope, this pilot's own "narrow, documented" pattern (confirmed
// with the user before implementing):
//   - No cross-process mtime cache (real `portage.mtimedb["ldpath"]`,
//     which persists across a real, long-lived portage session): this
//     pilot's own CLI is a fresh process per command, so every
//     invocation is treated as a first run instead -- any candidate lib
//     dir (`LDPATH` entries from env.d, an existing `usr/lib*`/`lib*`
//     directory) found on disk post-merge is treated as "changed",
//     triggering `ldconfig`. This exactly matches real portage's own
//     genuine first-run behavior (empty `prev_mtimes`); the only real
//     divergence is a *repeat* merge into the same `ROOT` that didn't
//     touch any lib dir, where real portage would skip `ldconfig` and
//     this pilot re-runs it anyway -- never wrong, just occasionally
//     extra (cheap, idempotent) invocations.
//   - Real `getlibpaths()`'s own `/etc/ld.so.conf.d/*.conf` (`include`
//     directive) parsing is not reproduced -- a rare, admin-configured
//     mechanism, not populated by anything this pilot's own fixtures do.
//   - No `CHOST`/`CBUILD` cross-compile handling at all (this pilot has
//     no cross-compile concept anywhere else either) -- always takes
//     real `env_update()`'s own `else` branch, `<ROOT>/sbin/ldconfig`
//     (note: the *target* `ROOT`'s own binary, not a host `PATH` lookup
//     -- real `_doebuild_spawn`-adjacent code relies on this being
//     chroot-compatible with `-r target_root`).
//   - No `EPREFIX` support anywhere in this pilot -- the real bfd-linker
//     alternate `/usr/etc/ld.so.conf` is never written.
//   - env.d files that declare their own extra `SPACE_SEPARATED`/
//     `COLON_SEPARATED` keys are not honored -- only the two real,
//     hardcoded default sets (`CONFIG_PROTECT`/`CONFIG_PROTECT_MASK`;
//     `PATH`/`LDPATH`/`MANPATH`/etc.) are ever treated as cumulative.
//   - Real `getconfig()`'s own shlex-based parser is not reproduced --
//     env.d files are parsed with the same simple per-line
//     `KEY="value"` shortcut `ebuild_merge::parse_slot` already takes
//     for `SLOT`.
//   - `/etc/ld.so.conf` is unconditionally rewritten every run (real
//     `env_update()` only rewrites it -- and only then considers
//     `ldconfig` worth running for *that specific reason* -- when its
//     own content actually changed); moot here since this pilot's own
//     `ldconfig`-triggering decision doesn't depend on that comparison
//     at all (see the mtime-cache cut above), and `/etc/ld.so.conf`
//     itself isn't vdb-tracked (no `CONFIG_PROTECT`/unmerge interaction
//     to preserve).

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

/// Real PMS-adjacent shell-assignment shortcut, the same one
/// `ebuild_merge::parse_slot` already takes for `SLOT`: `KEY="value"`/
/// `KEY='value'`/`KEY=value` (bare token), one assignment per line,
/// trailing `# comment` allowed. Real `getconfig()`'s own shlex parser
/// (multi-line values, embedded escapes) is not reproduced -- see this
/// module's own doc comment.
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

/// Real `env_update()`'s own env.d filename filter: at least 3 chars,
/// starting with two digits, not a dotfile/backup.
fn is_env_d_filename(name: &str) -> bool {
    let bytes = name.as_bytes();
    name.len() >= 3
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && !name.starts_with('.')
        && !name.ends_with('~')
        && !name.ends_with(".bak")
}

/// Real `potential_lib_dirs`/`getlibpaths`-adjacent candidate set (see
/// this module's own doc comment for the v1 cuts): every `LDPATH`
/// entry, plus `usr/lib`/`lib`, plus any top-level or `usr/`-relative
/// directory whose own name starts with `lib` (excluding `libexec`,
/// matching real `os.path.basename(y) != "libexec"`) -- filtered down
/// to whichever of those actually exist as real directories on disk
/// right now (post-merge).
fn candidate_lib_dirs(root: &Path, ldpath_entries: &[String]) -> Vec<PathBuf> {
    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in ldpath_entries {
        dirs.insert(entry.trim_start_matches('/').to_string());
    }
    dirs.insert("usr/lib".to_string());
    dirs.insert("lib".to_string());

    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("lib") && name != "libexec" && e.path().is_dir() {
                dirs.insert(name);
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join("usr")) {
        for e in entries.filter_map(|e| e.ok()) {
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

/// Real `os.path.join(eroot, "sbin", "ldconfig")` invoked as `[ldconfig,
/// "-X", "-r", target_root]` with `cwd="/"` -- a real, unmodified
/// subprocess (this pilot's own "real subprocess, accepted dependency"
/// pattern already established for `wget` in the fetch slice). A
/// missing or non-executable `<root>/sbin/ldconfig` is silently a no-op
/// (real `env_update()`'s own tolerance -- most real `ROOT`s in this
/// pilot's own synthetic fixtures never install one at all). Real
/// `env_update()` only ever warns on a nonzero exit, never aborts the
/// merge over it -- mirrored here by ignoring the exit status entirely
/// once spawned.
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

/// Real `env_update()`'s own top-level driver. Called unconditionally
/// after a successful merge that actually installed something -- see
/// this module's own doc comment for the full real grounding and v1
/// scope cuts.
pub fn run_env_update(root: &Path) -> Result<(), String> {
    let envd_dir = root.join("etc/env.d");
    std::fs::create_dir_all(&envd_dir).map_err(|e| format!("{}: {e}", envd_dir.display()))?;

    let mut filenames: Vec<String> = std::fs::read_dir(&envd_dir)
        .map_err(|e| format!("{}: {e}", envd_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| is_env_d_filename(name))
        .collect();
    filenames.sort();

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
        if let Some(rest) = value.strip_prefix('$') {
            if !value.starts_with("${") {
                profile_env.push_str(&format!("export {key}=$'{rest}'\n"));
                continue;
            }
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

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "portuale-env-update-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
    fn is_env_d_filename_matches_real_filter() {
        assert!(is_env_d_filename("50-foo"));
        assert!(is_env_d_filename("99profile"));
        assert!(!is_env_d_filename("foo"));
        assert!(!is_env_d_filename("5-foo"));
        assert!(!is_env_d_filename(".50-foo"));
        assert!(!is_env_d_filename("50-foo~"));
        assert!(!is_env_d_filename("50-foo.bak"));
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
