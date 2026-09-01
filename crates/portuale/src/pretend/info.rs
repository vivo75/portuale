//! `emerge --info`.

use crate::pretend::*;

/// `emerge --info`: the deterministic config/repository block plus the per-atom
/// `Package Settings` section. Host-state output (version probes, timestamps)
/// is deliberately cut; see the notes. With `pkg_info()` defined, the phase
/// actually runs.
/// Portage: _emerge/actions.py, action_info
///
/// One `USE_EXPAND` variable's `emerge --info` display value: the flags actually
/// enabled for that prefix in the final USE set, in the raw value's own order
/// followed by any extra enabled flags sorted. `None` when nothing is enabled.
/// Portage: config.py, regenerate
pub(crate) fn use_expand_display_value(
    var: &str,
    resolved_use: &std::collections::BTreeSet<String>,
    config: &portage_profile::Config,
) -> Option<String> {
    let prefix = format!("{}_", var.to_lowercase());
    let enabled: std::collections::BTreeSet<&str> = resolved_use
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix))
        .collect();
    if enabled.is_empty() {
        return None;
    }
    let raw = config.other_vars.get(var).map(String::as_str).unwrap_or("");
    let mut out: Vec<&str> = raw
        .split_whitespace()
        .filter(|t| enabled.contains(t))
        .collect();
    for f in &enabled {
        if !out.contains(f) {
            out.push(f);
        }
    }
    // `out` is now raw-order kept + the rest appended; the appended tail
    // is already sorted because `enabled` is a `BTreeSet`.
    Some(out.join(" "))
}

/// The resolved global USE set printed by `emerge --info`: `use_flags` with
/// `use_force` folded in and `use_mask` removed (`use_force`/`use_mask` stay
/// out of `Config::use_flags` on purpose; they apply per-candidate instead).
/// Portage: config.py, regenerate
pub(crate) fn resolved_global_use(
    config: &portage_profile::Config,
) -> std::collections::BTreeSet<String> {
    let mut set: std::collections::BTreeSet<String> = config.use_flags.iter().cloned().collect();
    set.extend(config.use_force.iter().cloned());
    for m in &config.use_mask {
        set.remove(m);
    }
    set
}

/// One `--info` variable-dump value for a plain scalar key: the
/// calling-env-over-files chain, so the environment wins as Portage prints it.
/// Portage: _emerge/actions.py, action_info
pub(crate) fn info_scalar_value(config: &portage_profile::Config, key: &str) -> Option<String> {
    portage_profile::env_over_config_scalar(config, key)
}

/// First line of `<cmd> --version` output, trimmed -- `None` if the command
/// is missing or fails.
pub(crate) fn tool_first_line(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or("").trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// The highest installed version of `cat/pkg` (`-r0` dropped), if any.
pub(crate) fn highest_installed_pvr(root: &Path, category: &str, package: &str) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .max_by(|a, b| portage_versions::vercmp(a, b).unwrap_or(0).cmp(&0))
        .map(|v| v.strip_suffix("-r0").unwrap_or(&v).to_string())
}

/// The host-state header printed before `Repositories:`. Everything here is
/// host state a fixture cannot reproduce; the contract normalizes volatile
/// values to `XXX` and checks the structure.
/// Portage: _emerge/actions.py, action_info
pub(crate) fn print_info_header(
    config: &portage_profile::Config,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    config_root: &Path,
    has_atoms: bool,
) {
    let main_repo_loc = repos
        .iter()
        .find(|r| r.is_main)
        .map(|r| r.location.clone())
        .unwrap_or_default();

    // --- `Portage <ver> (...)` ---
    let portage_ver =
        highest_installed_pvr(root, "sys-apps", "portage").unwrap_or_else(|| "unavailable".into());
    let pyver = std::process::Command::new("python3")
        .args([
            "-c",
            "import sys;v=sys.version_info;print(f'{v[0]}.{v[1]}.{v[2]}-{v[3]}-{v[4]}')",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unavailable".into());
    let profile_ver = info_profile_version(config_root, &main_repo_loc);
    let gccver = tool_first_line("gcc", &["-dumpversion"])
        .map(|v| format!("gcc-{v}"))
        .unwrap_or_else(|| "[unavailable]".into());
    let libcver = highest_installed_pvr(root, "sys-libs", "glibc")
        .map(|v| format!("glibc-{v}"))
        .or_else(|| highest_installed_pvr(root, "sys-libs", "musl").map(|v| format!("musl-{v}")))
        .unwrap_or_else(|| "unavailable".into());
    let kernel = {
        let r = tool_first_line("uname", &["-r"]).unwrap_or_default();
        let m = tool_first_line("uname", &["-m"]).unwrap_or_default();
        format!("{r} {m}")
    };
    println!(
        "Portage {portage_ver} (python {pyver}, {profile_ver}, {gccver}, {libcver}, {kernel})"
    );
    if has_atoms {
        // With atoms, a centred `System Settings` title precedes the rule.
        let title = "System Settings";
        let pad = 65 / 2 + title.len() / 2;
        println!("{}", "=".repeat(65));
        println!("{title:>pad$}");
    }
    println!("{}", "=".repeat(65));

    // --- `System uname:` ---
    let sys = tool_first_line("uname", &["-s"]).unwrap_or_else(|| "Linux".into());
    let rel = tool_first_line("uname", &["-r"]).unwrap_or_default();
    let mach = tool_first_line("uname", &["-m"]).unwrap_or_default();
    let processor = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|c| {
            c.lines()
                .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim() == "model name"))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| mach.clone());
    let libc_short = highest_installed_pvr(root, "sys-libs", "glibc")
        .map(|v| {
            let base = v.split('-').next().unwrap_or(&v);
            format!("glibc{base}")
        })
        .unwrap_or_default();
    let uname_str = format!("{sys}-{rel}-{mach}-{processor}-with-{libc_short}").replace(' ', "_");
    println!("System uname: {uname_str}");

    // --- `KiB Mem:` / `KiB Swap:` ---
    if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
        let kb = |key: &str| -> Option<i64> {
            meminfo.lines().find_map(|l| {
                let l = l.strip_prefix(key)?;
                l.trim().trim_end_matches(" kB").trim().parse().ok()
            })
        };
        if let Some(total) = kb("MemTotal:") {
            let mut line = format!("KiB Mem:  {total:>10} total");
            if let Some(free) = kb("MemFree:") {
                line.push_str(&format!(",{free:>10} free"));
            }
            println!("{line}");
        }
        if let Some(total) = kb("SwapTotal:") {
            let mut line = format!("KiB Swap: {total:>10} total");
            if let Some(free) = kb("SwapFree:") {
                line.push_str(&format!(",{free:>10} free"));
            }
            println!("{line}");
        }
    }

    // --- per-repo `Timestamp` / `Head commit of repository` ---
    for repo in repos {
        if let Ok(text) = std::fs::read_to_string(repo.location.join("metadata/timestamp.chk"))
            && let Some(first) = text.lines().next()
        {
            println!("Timestamp of repository {}: {}", repo.name, first.trim());
        }
        if repo.sync_type.as_deref() == Some("git")
            && let Some(head) = std::process::Command::new("git")
                .args(["-C"])
                .arg(&repo.location)
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
        {
            println!("Head commit of repository {}: {head}", repo.name);
            println!();
        }
    }

    // --- `sh:` -- the /bin/sh provider ---
    let sh_target = std::fs::canonicalize(config_root.join("bin/sh"))
        .or_else(|_| std::fs::canonicalize("/bin/sh"))
        .ok();
    let sh_base = sh_target
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "sh".into());
    let sh_str = ["app-shells", "sys-apps", "dev-lang"]
        .iter()
        .find_map(|cat| {
            highest_installed_pvr(root, cat, &sh_base).map(|v| {
                let name = if *cat == "app-shells" {
                    sh_base.clone()
                } else {
                    format!("{cat}/{sh_base}")
                };
                format!("{name} {v}")
            })
        })
        .unwrap_or_else(|| sh_base.clone());
    println!("sh: {sh_str}");

    // --- `coreutils:` / `ld:` version probes ---
    if let Some(line) = tool_first_line("install", &["--version"]) {
        match line.find("install ") {
            Some(pos) => println!("coreutils: {}", &line[pos + 8..]),
            None => println!("coreutils: {line}"),
        }
    }
    let chost = config.other_vars.get("CHOST").map(String::as_str);
    let ld_names: Vec<String> = chost
        .map(|c| vec![format!("{c}-ld"), "ld".to_string()])
        .unwrap_or_else(|| vec!["ld".to_string()]);
    for name in ld_names {
        if let Some(line) = tool_first_line(&name, &["--version"]) {
            println!("ld: {line}");
            break;
        }
    }

    // --- `info_pkgs` version table ---
    let rows = portage_repo::info_pkgs_table(root, &main_repo_loc);
    let cp_max = rows.iter().map(|r| r.cp.len()).max().unwrap_or(0);
    for row in rows {
        let cp = format!("{}:", row.cp);
        println!(
            "{:<width$} {}",
            cp,
            row.versions.join(", "),
            width = cp_max + 1
        );
    }
}

/// The active profile path relative to the main repo's `profiles/` dir; else
/// `"!" + <symlink target>`; else `"unavailable"`.
/// Portage: _emerge/actions.py, get_profile_version
pub(crate) fn info_profile_version(config_root: &Path, main_repo_loc: &Path) -> String {
    let base = main_repo_loc.join("profiles");
    let base = std::fs::canonicalize(&base).unwrap_or(base);
    let rel = |p: &Path| -> Option<String> {
        let c = std::fs::canonicalize(p).ok()?;
        c.strip_prefix(&base)
            .ok()
            .map(|r| r.to_string_lossy().into_owned())
    };
    let make_profile = config_root.join("etc/portage/make.profile");
    if let Some(v) = rel(&make_profile) {
        return v;
    }
    if let Ok(target) = std::fs::canonicalize(&make_profile) {
        if let Ok(parent_txt) = std::fs::read_to_string(target.join("parent")) {
            for line in parent_txt.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((_repo, path)) = line.split_once(':') {
                    // `reponame:path` resolves under the main repo's `profiles/`.
                    if let Some(v) = rel(&base.join(path)) {
                        return v;
                    }
                } else if let Some(v) = rel(&target.join(line)) {
                    return v;
                }
            }
        }
        if let Ok(link) = std::fs::read_link(&make_profile) {
            return format!("!{}", link.display());
        }
    }
    "unavailable".to_string()
}
