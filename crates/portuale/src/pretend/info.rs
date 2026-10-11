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
/// Arguments of [`run_info`].
pub(crate) struct RunInfoArgs<'a> {
    pub(crate) config: &'a portage_profile::Config,
    pub(crate) repos: &'a [portage_repo::RepoConfig],
    pub(crate) root: &'a Path,
    pub(crate) atom_args: &'a [&'a str],
    pub(crate) misspell_suggestions: bool,
    pub(crate) usepkg: bool,
    pub(crate) config_root: &'a Path,
    pub(crate) shell: ebuild_phases::ShellBackend,
    pub(crate) debug: bool,
    pub(crate) color: &'a Colorizer,
}

pub(crate) fn run_info(args: RunInfoArgs<'_>) -> ExitCode {
    let RunInfoArgs {
        config,
        repos,
        root,
        atom_args,
        misspell_suggestions,
        usepkg,
        config_root,
        shell,
        debug,
        color,
    } = args;
    // The local binary index, built once for the whole `--info` run and shared
    // by the existence probe and the binary half below. Built lazily: without
    // `--usepkg` neither consumer runs.
    let local_binpkg = usepkg.then(|| portage_repo::build_local_binpkg_index(config));
    // A target with no ebuild anywhere and nothing installed is fatal (exit 1),
    // printed before the config block.
    // Portage: _emerge/actions.py, action_info
    for atom_str in atom_args {
        let Some(atom) = parse_atom(atom_str) else {
            continue;
        };
        let exists = portage_repo::list_candidates(repos, &atom.category, &atom.package)
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            || !portage_repo::installed_candidates(root, &atom.category, &atom.package).is_empty()
            // Under `--usepkg`, a package only ever available as a binary is
            // not a "no ebuilds" error either.
            // Portage: _emerge/actions.py, action_info
            || local_binpkg.as_ref().is_some_and(|idx| {
                portage_repo::has_local_binary_candidate(idx, atom_str)
            });
        if !exists {
            let mut xinfo = format!("\"{atom_str}\"");
            if root != Path::new("/") {
                xinfo = format!("{xinfo} for {}", root.display());
            }
            eprintln!(
                "\nemerge: there are no ebuilds to satisfy {}.",
                color.c("INFORM", &xinfo)
            );
            let msg = format!("there are no ebuilds to satisfy \"{atom_str}\".");
            if let Some(extra) = misspell_suggestion_block(&msg, repos, misspell_suggestions) {
                eprintln!("{extra}");
            }
            return ExitCode::from(1);
        }
    }

    // The host-state header, printed before `Repositories:`.
    print_info_header(config, repos, root, config_root, !atom_args.is_empty());

    // Repositories.
    println!("Repositories:\n");
    let name_of: std::collections::HashMap<&Path, &str> = repos
        .iter()
        .map(|r| (r.location.as_path(), r.name.as_str()))
        .collect();
    for repo in repos {
        // Field order: name, location, sync-type, sync-uri, masters,
        // priority, aliases, volatile, then module-specific opts.
        // Portage: repositories.py, RepoConfig.info_string
        println!("{}", repo.name);
        println!("    location: {}", repo.location.display());
        if let Some(t) = &repo.sync_type {
            println!("    sync-type: {t}");
        }
        if let Some(u) = &repo.sync_uri {
            println!("    sync-uri: {u}");
        }
        if !repo.masters.is_empty() {
            let ms: Vec<&str> = repo
                .masters
                .iter()
                .filter_map(|m| name_of.get(m.as_path()).copied())
                .collect();
            if !ms.is_empty() {
                println!("    masters: {}", ms.join(" "));
            }
        }
        println!("    priority: {}", repo.priority);
        if !repo.aliases.is_empty() {
            println!("    aliases: {}", repo.aliases.join(" "));
        }
        println!(
            "    volatile: {}",
            if repo.volatile { "True" } else { "False" }
        );
        for (k, v) in &repo.module_specific_options {
            println!("    {k}: {v}");
        }
        println!();
    }

    // Binary Repositories.
    if !config.binrepos.is_empty() {
        println!("Binary Repositories:\n");
        for br in config.binrepos.iter().rev() {
            if br.name.is_empty() {
                continue;
            }
            // Field order: name, location (if set), priority, sync-uri,
            // verify-signature. Portage: repositories.py, BinRepoConfig.info_string
            println!("{}", br.name);
            if let Some(loc) = &br.location {
                println!("    location: {loc}");
            }
            println!("    priority: {}", br.priority);
            println!("    sync-uri: {}", br.sync_uri);
            println!(
                "    verify-signature: {}",
                if br.verify_signature { "True" } else { "False" }
            );
            println!();
        }
    }

    // Installed sets: the `@name` entries of `world_sets`.
    let mut sets: Vec<String> = read_world_sets(root)
        .unwrap_or_default()
        .into_iter()
        .map(|s| format!("@{s}"))
        .collect();
    sets.sort();
    if !sets.is_empty() {
        println!("Installed sets: {}", sets.join(", "));
    }

    // The variable dump: the hardcoded `myvars` list plus the main repo's
    // `profiles/info_vars`, minus deprecated/skipped ones, de-duplicated and
    // sorted. Each prints as `VAR="value"`, or joins the trailing `Unset:` line.
    // The incremental members print resolved, then space-joined and sorted.
    // Portage: _emerge/actions.py, action_info
    let use_expand: Vec<String> = {
        let mut v: Vec<String> = config.use_expand.iter().cloned().collect();
        v.sort();
        v
    };
    let resolved_use = resolved_global_use(config);
    let mut myvars: Vec<String> = [
        "GENTOO_MIRRORS",
        "CONFIG_PROTECT",
        "CONFIG_PROTECT_MASK",
        "DISTDIR",
        "ENV_UNSET",
        "PKGDIR",
        "PORTAGE_TMPDIR",
        "PORTAGE_BINHOST",
        "PORTAGE_BUNZIP2_COMMAND",
        "PORTAGE_BZIP2_COMMAND",
        "USE",
        "CHOST",
        "CFLAGS",
        "CXXFLAGS",
        "ACCEPT_KEYWORDS",
        "ACCEPT_LICENSE",
        "FEATURES",
        "EMERGE_DEFAULT_OPTS",
    ]
    .map(String::from)
    .to_vec();
    if let Some(main) = repos.iter().find(|r| r.is_main)
        && let Ok(text) = std::fs::read_to_string(main.location.join("profiles/info_vars"))
    {
        for line in text.lines() {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with('#') {
                myvars.push(l.to_string());
            }
        }
    }
    let skipped = ["PORTAGE_REPOSITORIES", "PORTDIR", "PORTDIR_OVERLAY", "SYNC"];
    myvars.retain(|v| !skipped.contains(&v.as_str()));
    myvars.sort();
    myvars.dedup();
    let mut unset: Vec<String> = Vec::new();
    for k in &myvars {
        let k = k.as_str();
        let value: Option<String> = match k {
            "ACCEPT_KEYWORDS" => {
                let mut kw: Vec<&String> = config.accept_keywords.iter().collect();
                kw.sort();
                (!kw.is_empty())
                    .then(|| kw.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" "))
            }
            "ACCEPT_LICENSE" => {
                (!config.accept_license.is_empty()).then(|| config.accept_license.join(" "))
            }
            "CONFIG_PROTECT" | "CONFIG_PROTECT_MASK" | "ENV_UNSET" | "FEATURES" => config
                .resolved_incremental(k)
                .filter(|v| !v.is_empty())
                .map(|v| v.join(" "))
                .or_else(|| config.other_vars.get(k).cloned()),
            "USE" => {
                // Global USE minus the `USE_EXPAND`-prefixed pseudo-flags
                // (shown as their own `VAR="…"` tokens).
                let flags: Vec<&str> = resolved_use
                    .iter()
                    .map(String::as_str)
                    .filter(|f| {
                        !use_expand
                            .iter()
                            .any(|v| f.starts_with(&format!("{}_", v.to_lowercase())))
                    })
                    .collect();
                Some(flags.join(" "))
            }
            // Every other key reads through the env-first chain (see
            // `info_scalar_value`), so the calling environment wins.
            _ => info_scalar_value(config, k),
        };
        match value {
            Some(v) if k == "PORTAGE_BZIP2_COMMAND" && v == "bzip2" => {}
            Some(v) if k == "USE" => {
                let mut line = format!("USE=\"{v}\"");
                for var in &use_expand {
                    if let Some(val) = use_expand_display_value(var, &resolved_use, config) {
                        line.push_str(&format!(" {var}=\"{val}\""));
                    }
                }
                println!("{line}");
            }
            Some(v) => println!("{k}=\"{v}\""),
            None => unset.push(k.to_string()),
        }
    }
    if !unset.is_empty() {
        println!("Unset:  {}", unset.join(", "));
    }
    // A single trailing blank line; the `Package Settings` block below, when
    // present, starts with its own leading `\n`.
    println!();

    // The `Package Settings` section: per atom, every installed vdb match wins;
    // otherwise the best visible ebuild candidate defining `pkg_info()`. The
    // header prints once, only if at least one atom yielded something.
    // Portage: _emerge/actions.py, action_info
    enum InfoPkg {
        Installed(portage_repo::InstalledInfo),
        Ebuild(portage_repo::InfoCandidate),
        // The `(bindb, "binary")` half: only under `--usepkg`/`--usepkgonly`
        // when nothing is installed and no visible ebuild defines `pkg_info()`.
        Binary(portage_repo::InfoBinaryCandidate),
    }
    // Each flag token is coloured; `--info` always shows every flag.
    let use_line = |disp: &[(String, String)]| -> String {
        disp.iter()
            .map(|(name, body)| {
                let colored = if body.is_empty() {
                    String::new()
                } else {
                    body.split(' ')
                        .map(|t| colorize_use_token(t, color))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                format!("{name}=\"{colored}\"")
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut pkgs: Vec<InfoPkg> = Vec::new();
    for a in atom_args {
        let installed = portage_repo::resolve_installed_info(root, a, config);
        if !installed.is_empty() {
            pkgs.extend(installed.into_iter().map(InfoPkg::Installed));
            continue;
        }
        if let Some(c) = portage_repo::resolve_info_candidate(repos, a, config)
            .ok()
            .flatten()
            .filter(|c| c.defines_pkg_info)
        {
            pkgs.push(InfoPkg::Ebuild(c));
            continue;
        }
        // Only when `--usepkg` and the ebuild half yielded nothing.
        if let Some(idx) = local_binpkg.as_ref()
            && let Some(b) = portage_repo::resolve_info_binary_candidate(config, a, idx)
                .ok()
                .flatten()
        {
            pkgs.push(InfoPkg::Binary(b));
        }
    }
    if !pkgs.is_empty() {
        let title = "Package Settings";
        // Real `header_title.rjust(int(header_width / 2 + len(title) / 2))`.
        let pad = 65 / 2 + title.len() / 2;
        println!("{}", "=".repeat(65));
        println!("{title:>pad$}");
        println!("{}", "=".repeat(65));
        println!();
        let portage_tmpdir = std::env::var_os("PORTAGE_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| portage_repo::portage_tmpdir_from_config(config));
        for pkg in &pkgs {
            // Header + USE line (+ the installed-only var diff), then two blank
            // lines, then `>>> Attempting to run pkg_info() ...` and the phase.
            // The ebuild/binary halves always run (selected for defining it);
            // an installed match runs only when its vdb `DEFINED_PHASES` names
            // `info` -- or is entirely empty (an old/hand-written entry quirk).
            // Portage: _emerge/actions.py, action_info
            let (cpv, run_phase): (&str, bool) = match pkg {
                InfoPkg::Ebuild(c) => {
                    println!(
                        "\n{} would be built with the following:",
                        color.c("INFORM", &format!("{}::{}", c.cpv, c.repo_name))
                    );
                    println!("{}", use_line(&c.use_expand_display));
                    (&c.cpv, true)
                }
                InfoPkg::Binary(b) => {
                    println!(
                        "\n{} (non-installed binary) was built with the following:",
                        color.c("INFORM", &format!("{}::{}", b.cpv, b.repo_name))
                    );
                    println!("{}", use_line(&b.use_expand_display));
                    (&b.cpv, true)
                }
                InfoPkg::Installed(i) => {
                    println!(
                        "\n{} was built with the following:",
                        color.c("INFORM", &format!("{}::{}", i.cpv, i.repo_name))
                    );
                    println!("{}", use_line(&i.use_expand_display));
                    for (name, value) in &i.differing_vars {
                        println!("{name}=\"{value}\"");
                    }
                    if !i.unset_vars.is_empty() {
                        println!("Unset: {}", i.unset_vars.join(", "));
                    }
                    (&i.cpv, i.defines_pkg_info)
                }
            };
            println!();
            println!();
            if !run_phase {
                continue;
            }
            println!(">>> Attempting to run pkg_info() for '{cpv}'");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let phase = match pkg {
                InfoPkg::Ebuild(c) => {
                    ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
                        ebuild_path: &c.ebuild_path,
                        phase: "info",
                        root,
                        portage_tmpdir: &portage_tmpdir,
                        debug,
                        config_root,
                        shell,
                        build_env: &[],
                        log_file: None,
                    })
                }
                InfoPkg::Installed(i) => {
                    let opts = ebuild_merge::MergeOptions::from_env(shell, debug);
                    let scratch = portage_tmpdir.join("portage").join("_info_src");
                    ebuild_merge::run_vdb_saved_env_phase(ebuild_merge::RunVdbSavedEnvPhaseArgs {
                        root,
                        category: &i.category,
                        package: &i.package,
                        pf: &format!("{}-{}", i.package, i.version),
                        phase: "info",
                        scratch_dir: &scratch,
                        portage_tmpdir: &portage_tmpdir,
                        options: &opts,
                    })
                }
                InfoPkg::Binary(b) => {
                    // The extracted ebuild is laid out as `<cat>/<pn>/<pf>.ebuild`
                    // so the phase driver derives `CATEGORY`/`PN` from the path.
                    let dest = portage_tmpdir.join("portage").join("_info_binpkg");
                    let _ = std::fs::remove_dir_all(&dest);
                    let image = dest.join("image");
                    let build_info = dest.join("build-info");
                    let run = |ep: &Path| {
                        ebuild_phases::run_single_phase(ebuild_phases::RunSinglePhaseArgs {
                            ebuild_path: ep,
                            phase: "info",
                            root,
                            portage_tmpdir: &portage_tmpdir,
                            debug,
                            config_root,
                            shell,
                            build_env: &[],
                            log_file: None,
                        })
                    };
                    match crate::binpkg::extract_binpkg(
                        &b.archive_path,
                        &image,
                        &build_info,
                        &crate::binpkg::GpgVerify::from_env(),
                        // `pkg_info` installs nothing: no xattr handling.
                        &crate::ebuild_merge::XattrPolicy::default(),
                    ) {
                        Ok(()) => {
                            let extracted = portage_util::read_dir_entries(&build_info)
                                .ok()
                                .into_iter()
                                .flatten()
                                .map(|e| e.path())
                                .find(|p| p.extension().is_some_and(|x| x == "ebuild"));
                            match extracted {
                                Some(src) => {
                                    let pkg_dir = dest.join(&b.category).join(&b.package);
                                    let laid_out = pkg_dir.join(format!("{}.ebuild", b.pf));
                                    match std::fs::create_dir_all(&pkg_dir)
                                        .and_then(|()| std::fs::copy(&src, &laid_out))
                                    {
                                        Ok(_) => run(&laid_out),
                                        Err(e) => Err(format!("{}: {e}", laid_out.display())),
                                    }
                                }
                                None => Err(format!(
                                    "No ebuild found for '{cpv}' in {}",
                                    b.archive_path.display()
                                )),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            if let Err(e) = phase {
                eprintln!("emerge: {e}");
            }
        }
    }

    ExitCode::SUCCESS
}
