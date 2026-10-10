//! `emerge --info`.

use crate::pretend::*;

/// Real `emerge --info` (`_emerge/actions.py::action_info`), **narrowed
/// to its deterministic config/repository block**: the `Repositories:`
/// list (real `repo.info_string()`), `Binary Repositories:`, `Installed
/// sets:`, then the sorted `VAR="value"` dump (real `myvars`) and the
/// `Unset:` line.
///
/// With one or more `atom_args`, also the per-atom handling real
/// `action_info` does in its `myfiles` loop: an atom whose `cat/pkg`
/// has no ebuild anywhere aborts with `emerge: there are no ebuilds to
/// satisfy "<atom>".` + `--misspell-suggestions` (exit 1, before the
/// config block); otherwise, after the config block, a `Package
/// Settings` section (real `mypkgs`): per atom, every installed vdb
/// match wins (`<cpv>::<repo> was built with the following:` + its vdb
/// `USE="…"` line + the `CHOST`/`CFLAGS`/`CXXFLAGS`/`FEATURES`/`LDFLAGS`
/// that differ from the current config + an `Unset: …` line); otherwise
/// the best visible ebuild candidate, but only if its ebuild defines
/// `pkg_info()` (`<cpv>::<repo> would be built with the following:` +
/// `USE="…"`); otherwise, under `--usepkg`/`--usepkgonly`, the highest
/// local `$PKGDIR` binary build whose EAPI is 4+ and whose
/// `DEFINED_PHASES` names `info` (`<cpv>::<repo> (non-installed binary)
/// was built with the following:` + the binary's own baked `USE="…"`).
///
/// For each selected package that defines `pkg_info()` -- the ebuild and
/// binary halves always, an installed match only when its vdb's own
/// `DEFINED_PHASES` names `info` -- portuale then prints `>>> Attempting
/// to run pkg_info() for '<cpv>'` and actually runs the phase
/// (`ebuild_phases::run_single_phase` for an ebuild or an extracted
/// binary, `ebuild_merge::run_vdb_saved_env_phase` for an installed
/// match), exactly the real `writemsg_stdout(...)` + `doebuild(...,
/// "info", ...)` step. The phase's own output is whatever the ebuild
/// `einfo`s; the contract suite pins the deterministic message and the
/// real phase run is covered in `test_portuale.py`.
///
/// **Large, deliberate cut:** real `action_info`'s output is dominated by
/// *host state* a fixture-driven test can't verify -- the
/// `Portage <ver> (python …, <profile>, <chost>, <gcc>, <libc>,
/// <kernel>)` header, `System uname`, `KiB Mem`, the
/// `sh:`/`gcc:`/`ld:`/`binutils`/`ccache`/`distcc` version probes, the
/// `info_pkgs` version table, repository timestamps / head commits. None
/// of that is reproduced. Narrower than real for the installed block:
/// the `CHOST`/`CFLAGS`/… values come only from the individual vdb
/// `build-info` files (real `_aux_env_search` falls back to the
/// package's `environment.bz2`). The `USE` line's `( )` force/mask wrap
/// and per-flag ANSI colour (real `pkg_use_display`'s own
/// `UseFlagDisplay`) are shipped -- see `resolve_installed_info`/
/// `resolve_info_candidate`'s own doc comments and `colorize_use_token`.
///
/// An installed match's `>>> Attempting` step follows real's own
/// `if metadata["DEFINED_PHASES"]: if "info" not in ...: continue`
/// (`actions.py:2350`): it fires when the vdb `DEFINED_PHASES` names
/// `info`, and also when `DEFINED_PHASES` is entirely empty (real's
/// falsy-check quirk -- an old / hand-written vdb entry with no
/// `DEFINED_PHASES` file). `"-"` (the modern "no phases" marker) is
/// truthy and does not fire. The
/// binary's ebuild is extracted from the archive and laid out as
/// `<cat>/<pn>/<pf>.ebuild` so the phase driver derives `CATEGORY`/`PN`
/// from the path (real `doebuild(tree="bintree")` gets them from the
/// binpkg metadata). `--info` runs its own `$PKGDIR` scan before
/// dispatching (the ordinary `--usepkg` resolve path's scan runs later
/// in the CLI flow).
///
/// `_hide_url_passwd` is also cut.
///
/// One `USE_EXPAND` variable's `emerge --info` display value, real
/// `config.regenerate()`'s "Generate global USE_EXPAND variables
/// settings that are consistent with USE" tail (`config.py`, guarded on
/// `self.mycpv is None`): the flags actually enabled for that prefix in
/// the final USE set, in the raw value's own order, followed by any
/// extra enabled flags sorted. This resolves `-*` / `-flag` /
/// `VIDEO_CARDS="-* intel …"`. `None` when nothing is enabled (real
/// appends the `VAR="…"` token to the `USE=` line only `if myval`, so an
/// empty value is simply omitted).
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

/// The resolved global USE set real `emerge --info` prints -- real
/// `config.regenerate()`'s last two lines before the USE_EXPAND display
/// pass: `myflags.update(self.useforce)` then
/// `myflags.difference_update(self.usemask)` (the `mycpv is None` /
/// global `getUseForce()` / `getUseMask()` case). `ARCH` is already in
/// `use_flags` via `USE_EXPAND_UNPREFIXED`. Portuale keeps
/// `use_force`/`use_mask` *out* of `Config::use_flags` on purpose (they
/// are applied per-candidate in `effective_use_flags`); this reunites
/// them for the one global display that needs it.
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

/// One `--info` variable-dump value for a plain scalar key: real
/// `action_info`'s `settings.get(k)` (`_emerge/actions.py:2240`) walks
/// `lookuplist` env-first (real `config.py:3178-3182`; `USE_ORDER`
/// `"env:pkg:conf:defaults:…"` at `:1031-1035`), so this is #180's
/// `env_over_config_scalar` chain (backlog #219), not `other_vars`
/// with an env fallback. For keys `resolve_config` already env-folds
/// (`ENV_SCALAR_VARS`, e.g. `PKGDIR`) the two reads agree; for the rest
/// (`PORTAGE_BZIP2_COMMAND`, `PORTAGE_BUNZIP2_COMMAND`, any profile
/// `info_vars` extra outside the allowlists) the env now wins, as real
/// prints it.
pub(crate) fn info_scalar_value(config: &portage_profile::Config, key: &str) -> Option<String> {
    portage_profile::env_over_config_scalar(config, key)
}

/// First line of `<cmd> --version` output, trimmed -- `None` if the
/// command isn't found or exits non-zero (real `subprocess.Popen` +
/// `os.EX_OK` guard). `args` is usually `["--version"]`.
pub(crate) fn tool_first_line(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next().unwrap_or("").trim();
    (!first.is_empty()).then(|| first.to_string())
}

/// The highest installed version of `cat/pkg`, `catpkgsplit`-style
/// `<pv>-<pr>` with a `-r0` revision dropped (real `get_libc_version` /
/// the `info_pkgs` join) -- or `None` when nothing is installed.
pub(crate) fn highest_installed_pvr(root: &Path, category: &str, package: &str) -> Option<String> {
    portage_repo::installed_versions(root, category, package)
        .into_iter()
        .max_by(|a, b| portage_versions::vercmp(a, b).unwrap_or(0).cmp(&0))
        .map(|v| v.strip_suffix("-r0").unwrap_or(&v).to_string())
}

/// Real `action_info`'s host-state header, printed before `Repositories:`
/// (`actions.py:1946-2165`): the `Portage <ver> (…)` line, the 65-char
/// rule, `System uname:`, `KiB Mem:`/`KiB Swap:`, per-repo
/// `Timestamp`/`Head commit of repository`, the `sh:`/`coreutils:`/`ld:`
/// probes and the `info_pkgs` version table. Everything here is
/// host-state a fixture cannot reproduce -- the contract normalizes the
/// volatile values to `XXX` and checks the structure.
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

    // --- `Portage <ver> (python …, <profile>, <gcc>, <libc>, <kernel>)` ---
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
        // Real `if myfiles:` -- a centred `System Settings` title
        // between the version line and the always-printed rule.
        let title = "System Settings";
        let pad = 65 / 2 + title.len() / 2;
        println!("{}", "=".repeat(65));
        println!("{title:>pad$}");
    }
    println!("{}", "=".repeat(65));

    // --- `System uname:` -- real `platform.platform(aliased=1)` ---
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

    // --- `KiB Mem:` / `KiB Swap:` -- real `get_vm_info` (`free`) ---
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
            // Real bakes `retrieve_head`'s trailing newline into the line.
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

/// Real `get_profile_version` (`_emerge/actions.py`): the active profile
/// path relative to the main repo's `profiles/` dir. If
/// `<config_root>/etc/portage/make.profile` points straight into the
/// main repo, that relative path; else its first `parent` entry (a bare
/// relative path or a `reponame:path`) resolved that way; else
/// `"!" + <symlink target>`; else `"unavailable"`.
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
                    // `reponame:path` -> `<main_repo>/profiles/<path>`
                    // (portuale's `info_pkgs` list only ever needs the
                    // main repo here).
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

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
#[allow(clippy::fn_params_excessive_bools, reason = "#336 Phase 5 worklist")]
pub(crate) fn run_info(
    config: &portage_profile::Config,
    repos: &[portage_repo::RepoConfig],
    root: &Path,
    atom_args: &[&str],
    misspell_suggestions: bool,
    usepkg: bool,
    config_root: &Path,
    shell: ebuild_phases::ShellBackend,
    debug: bool,
    color: &Colorizer,
) -> ExitCode {
    // The local binary index, built once for the whole `--info` run and
    // shared by the existence probe below and the binary half further
    // down (backlog #123: threaded as an `Arc`, never re-derived from
    // `&Config` per atom). Built lazily: without `--usepkg` neither
    // consumer runs.
    let local_binpkg = usepkg.then(|| portage_repo::build_local_binpkg_index(config));
    // Real `action_info`'s `myfiles` loop: a target with no ebuild
    // anywhere **and** nothing installed is fatal (exit 1) and printed
    // *before* the config block. `list_candidates` (all versions, masked
    // included) + the vdb is portuale's `cp_exists` proxy.
    for atom_str in atom_args {
        let Some(atom) = parse_atom(atom_str) else {
            continue;
        };
        let exists = portage_repo::list_candidates(repos, &atom.category, &atom.package)
            .map(|c| !c.is_empty())
            .unwrap_or(false)
            || !portage_repo::installed_candidates(root, &atom.category, &atom.package).is_empty()
            // Real `action_info` also checks `bindb.match(x.cp)` when
            // `--usepkg` is on (`actions.py:1877-1885`): a package only
            // ever available as a binary isn't a "no ebuilds" error.
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

    // Real `action_info`'s host-state header (`actions.py:1946-2165`),
    // printed before `Repositories:`.
    print_info_header(config, repos, root, config_root, !atom_args.is_empty());

    // Repositories.
    println!("Repositories:\n");
    let name_of: std::collections::HashMap<&Path, &str> = repos
        .iter()
        .map(|r| (r.location.as_path(), r.name.as_str()))
        .collect();
    for repo in repos {
        // Real `RepoConfig.info_string()` field order (the subset
        // portuale models): name, location, sync-type, sync-uri, masters,
        // priority, aliases, volatile, then any set module-specific opts.
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
            // Real `BinRepoConfig.info_string()` field order: name,
            // location (if set), priority, sync-uri, verify-signature.
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

    // Installed sets (real `root_config.sets["selected"].getNonAtoms()`
    // -> the `@name` entries of `world_sets`).
    let mut sets: Vec<String> = read_world_sets(root)
        .unwrap_or_default()
        .into_iter()
        .map(|s| format!("@{s}"))
        .collect();
    sets.sort();
    if !sets.is_empty() {
        println!("Installed sets: {}", sets.join(", "));
    }

    // The variable dump: real `action_info`'s hardcoded `myvars` list
    // plus `<PORTDIR>/profiles/info_vars` (real `actions.py:2221`), minus
    // the deprecated/skipped ones (`PORTAGE_REPOSITORIES`, `PORTDIR`,
    // `PORTDIR_OVERLAY`, `SYNC`), de-duplicated and sorted. Each is
    // `VAR="value"`, or listed on the trailing `Unset:` line when the
    // config carries no value. The `const.INCREMENTALS` members
    // (`CONFIG_PROTECT`/`CONFIG_PROTECT_MASK`/`ENV_UNSET`/`FEATURES`, plus
    // `ACCEPT_KEYWORDS`) are shown as real stores them: `-*`/`-tok`
    // resolved, then `" ".join(sorted(...))`.
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
                // Real `config.regenerate()`'s global USE = `use_flags`
                // with `useforce` folded in and `usemask` removed, minus
                // the `USE_EXPAND`-prefixed pseudo-flags (shown as their
                // own `VAR="…"` tokens).
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
            // Real `settings.get(k)` walks `lookuplist` env-first
            // (`actions.py:2240`; `config.py:3178-3182`, `USE_ORDER`
            // `"env:pkg:conf:…"` at `:1031-1035`), so the calling
            // environment wins over `make.conf`/profile/`make.globals`
            // -- including for keys `resolve_config` never env-folds
            // itself (backlog #219: `PORTAGE_BZIP2_COMMAND`,
            // `PORTAGE_BUNZIP2_COMMAND`). A curated `info_vars` entry
            // that is only ever an environment variable (`SHELL`,
            // `LC_ALL`, …) still shows via the same chain. (Backlog
            // #180's `env_over_config_scalar`, not a second reader,
            // and no raw `std::env` read.)
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
    // Real ends the buffer with two `append("")` -> `"\n".join` yields a
    // single trailing blank line (`…\n\n`); the `Package Settings` block
    // below, when present, starts with its own leading `\n`.
    println!();

    // Real `action_info`'s `Package Settings` section (real `mypkgs`):
    // per atom, every installed vdb match short-circuits the ebuild
    // lookup (`actions.py:1869-1875`); otherwise the best visible ebuild
    // candidate, but only if its ebuild defines `pkg_info()`. The header
    // (`header_width = 65`, `actions.py:1956`) is printed once, only if
    // at least one atom yielded something.
    enum InfoPkg {
        Installed(portage_repo::InstalledInfo),
        Ebuild(portage_repo::InfoCandidate),
        // Real `(bindb, "binary")` half of the search loop, reached only
        // under `--usepkg`/`--usepkgonly` when nothing is installed and
        // no visible ebuild defines `pkg_info()` -- see
        // `resolve_info_binary_candidate`.
        Binary(portage_repo::InfoBinaryCandidate),
    }
    // Real `pkg_use_display`'s own `UseFlagDisplay.__str__`: each flag
    // token is coloured (the same `colorize_use_token` scheme the
    // merge-list `USE="…"` column already uses), independent of
    // `--quiet`/`--verbose` -- `--info`'s block always shows every flag.
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
        // Real `(bindb, "binary")`: only when `--usepkg` and the ebuild
        // half yielded nothing (`actions.py:1876`).
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
            // Real `action_info`: header + `pkg_use_display` line (+ the
            // installed-only `mydesiredvars` diff), then two blank lines,
            // then `>>> Attempting to run pkg_info() for '<cpv>'` and the
            // phase itself (`actions.py:2314-2410`). `run_phase` is real's
            // `if metadata["DEFINED_PHASES"]: if "info" not in ...:
            // continue` -- always true for the ebuild/binary halves (they
            // were only selected *because* they define it), gated on the
            // vdb's own `DEFINED_PHASES` for an installed match.
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
            // Real `writemsg_stdout(f">>> Attempting to run pkg_info() for
            // '{pkg.cpv}'\n")` then `portage.doebuild(ebuildpath, "info",
            // ...)`. The Python contract reference stops here (no phase
            // machinery, like `--config`/`--regen`); the phase's own
            // output is covered Rust-only in `test_portuale.py`.
            println!(">>> Attempting to run pkg_info() for '{cpv}'");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let phase = match pkg {
                InfoPkg::Ebuild(c) => ebuild_phases::run_single_phase(
                    &c.ebuild_path,
                    "info",
                    root,
                    &portage_tmpdir,
                    debug,
                    config_root,
                    shell,
                    &[],
                    None,
                ),
                InfoPkg::Installed(i) => {
                    let opts = ebuild_merge::MergeOptions::from_env(shell, debug);
                    let scratch = portage_tmpdir.join("portage").join("_info_src");
                    ebuild_merge::run_vdb_saved_env_phase(
                        root,
                        &i.category,
                        &i.package,
                        &format!("{}-{}", i.package, i.version),
                        "info",
                        &scratch,
                        &portage_tmpdir,
                        &opts,
                    )
                }
                InfoPkg::Binary(b) => {
                    // Real `bindb.bintree.getname(cpv)` + extract the
                    // `<pf>.ebuild` member to a tmpdir, `doebuild(...,
                    // "info", tree="bintree")` -- portuale reuses the full
                    // binpkg extractor, then lays the extracted ebuild out
                    // as `<cat>/<pn>/<pf>.ebuild` so the phase driver
                    // derives `CATEGORY`/`PN` from the path (real
                    // `doebuild` gets them from the binpkg metadata).
                    let dest = portage_tmpdir.join("portage").join("_info_binpkg");
                    let _ = std::fs::remove_dir_all(&dest);
                    let image = dest.join("image");
                    let build_info = dest.join("build-info");
                    let run = |ep: &Path| {
                        ebuild_phases::run_single_phase(
                            ep,
                            "info",
                            root,
                            &portage_tmpdir,
                            debug,
                            config_root,
                            shell,
                            &[],
                            None,
                        )
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
