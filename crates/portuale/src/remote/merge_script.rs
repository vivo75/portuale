//! The phase and merge shell scripts sent to the client.

use crate::remote::*;

// --- Client merge ------------------------------------------------------------------

/// Whether `MERGE_TYPE` reaches the phase env: only for EAPI 4+. An
/// unsupported or unparseable EAPI takes the exporting fallback row.
// Portage: lib/portage/eapi.py, eapi_exports_merge_type
pub(crate) fn eapi_exports_merge_type(eapi: &str) -> bool {
    // Portage tests the *whole* string against `_supported_eapis`;
    // anything that is not a plain supported number (e.g. `3-foo`)
    // takes the exporting fallback row.
    match eapi.trim().parse::<u32>() {
        Ok(n) => n >= 4,
        Err(_) => true,
    }
}

/// Arguments of [`phase_exports`].
pub(crate) struct PhaseExportsArgs<'a> {
    pub(crate) ebuild: &'a str,
    pub(crate) build_dir: &'a str,
    pub(crate) root: &'a str,
    pub(crate) image_dir: &'a str,
    pub(crate) temp_dir: &'a str,
    pub(crate) work_subdir: &'a str,
    pub(crate) home_dir: &'a str,
    pub(crate) files_dir: &'a str,
    pub(crate) bin_dir: &'a str,
    pub(crate) tmpdir: &'a str,
    pub(crate) colormap: &'a str,
    pub(crate) staged: &'a crate::remote_bundle::StagedBundle,
    pub(crate) phase: &'a str,
    pub(crate) portuale_bin: &'a str,
}

/// `export` block for the new instance's phase script: path overrides
/// with caller-side values, everything else from the sourced saved env.
/// `MERGE_TYPE=binary` rides the same line as `EMERGE_FROM` (only binpkgs
/// merge here), gated on [`eapi_exports_merge_type`]. `portuale_bin` is
/// the resolved absolute client path: every client call uses it, never
/// `$PATH`.
pub(crate) fn phase_exports(args: PhaseExportsArgs<'_>) -> String {
    let PhaseExportsArgs {
        ebuild,
        build_dir,
        root,
        image_dir,
        temp_dir,
        work_subdir,
        home_dir,
        files_dir,
        bin_dir,
        tmpdir,
        colormap,
        staged,
        phase,
        portuale_bin,
    } = args;
    format!(
        concat!(
            "export EAPI={eapi} CATEGORY={category} PN={pn} PV={pv} PR={pr} PVR={pvr} P={p} PF={pf}\n",
            "export EBUILD={ebuild} O={obuild}\n",
            "export ROOT={root} EROOT={root}\n",
            "export PORTAGE_BUILDDIR={builddir}\n",
            "export WORKDIR={workdir}\n",
            "export D={imaged}/ ED={imaged}/\n",
            "export T={temp} HOME={home} FILESDIR={filesdir}\n",
            "export PORTAGE_BIN_PATH={bindir}\n",
            "export PORTAGE_ECLASS_LOCATIONS=\"\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "export PORTAGE_PYTHON=\"$UNITBIN/portuale-python\"\n",
            "export PORTAGE_COLORMAP={colormap}\n",
            "export PORTAGE_TMPDIR={tmpdir}\n",
            "export SANDBOX_LOG={temp}/sandbox.log\n",
            "export EBUILD_PHASE={phase} EMERGE_FROM=binary{merge_type}\n",
        ),
        merge_type = if eapi_exports_merge_type(&staged.eapi) {
            " MERGE_TYPE=binary"
        } else {
            ""
        },
        eapi = sh_quote(&staged.eapi),
        category = sh_quote(&staged.category),
        pn = sh_quote(&staged.pn),
        pv = sh_quote(&staged.pv),
        pr = sh_quote(&staged.pr),
        pvr = sh_quote(&staged.pvr),
        p = sh_quote(&staged.p),
        pf = sh_quote(&staged.pf),
        ebuild = sh_quote(ebuild),
        obuild = sh_quote(&format!("{build_dir}/build-info")),
        root = sh_quote(root),
        builddir = sh_quote(build_dir),
        workdir = sh_quote(work_subdir),
        imaged = sh_quote(image_dir),
        temp = sh_quote(temp_dir),
        home = sh_quote(home_dir),
        filesdir = sh_quote(files_dir),
        bindir = sh_quote(bin_dir),
        tmpdir = sh_quote(tmpdir),
        colormap = sh_quote(colormap),
        phase = phase,
        portuale_bin = sh_quote(portuale_bin),
    )
}

// --- Client phases -----------------------------------------------------------------

/// Arguments of [`phase_script`].
pub(crate) struct PhaseScriptArgs<'a> {
    pub(crate) unit_dir: &'a str,
    pub(crate) staged: &'a crate::remote_bundle::StagedBundle,
    pub(crate) phase: &'a str,
    pub(crate) root: &'a str,
    pub(crate) workdir_parent: &'a str,
    pub(crate) colormap: &'a str,
    pub(crate) portuale_bin: &'a str,
}

/// One phase invocation, generated bash: recreate the local phase setup
/// client-side (fresh `bash bin/ebuild.sh <phase>` process per phase --
/// the readonly `EBUILD_PHASE` semantics demand it, exactly like local
/// `spawnebuild`). `S` is deliberately *not* exported: ebuild.sh
/// defaults it to `${WORKDIR}/${P}` (exact local behavior). Reports
/// `PHASE_<phase>=<rc>` on stdout, phase log raw on stdout/stderr.
pub(crate) fn phase_script(args: PhaseScriptArgs<'_>) -> String {
    let PhaseScriptArgs {
        unit_dir,
        staged,
        phase,
        root,
        workdir_parent,
        colormap,
        portuale_bin,
    } = args;
    // `UNITBIN` is the shipped runtime dir: `PORTAGE_PYTHON` resolves
    // through it, so no shim falls back to `PATH`.
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" {phase}\n",
            "rc=$?\n",
            "echo \"PHASE_{phase}=$rc\"\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        exports = phase_exports(PhaseExportsArgs {
            ebuild: &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            build_dir: unit_dir,
            root,
            image_dir: &format!("{unit_dir}/image"),
            temp_dir: &format!("{unit_dir}/temp"),
            work_subdir: &format!("{unit_dir}/work"),
            home_dir: &format!("{unit_dir}/homedir"),
            files_dir: &format!("{unit_dir}/files"),
            bin_dir: &format!("{unit_dir}/bin"),
            tmpdir: workdir_parent,
            colormap,
            staged,
            phase,
            portuale_bin
        }),
        phase = phase,
    )
}

/// Locale variables the regen postinst run re-exports into the client
/// phase env. Only set variables are ever forwarded (never invented
/// defaults). `LANGUAGE` is not in Portage's whitelist -- it rides along
/// because the slice brief names it explicitly. `LOCPATH` is in Portage's
/// calling-env whitelist (`special_env_vars.py`, `environ_whitelist`), so a
/// `locale-gen`d client root keeps working after the regen (backlog #516).
pub(crate) const REGEN_LOCALE_VARS: &[&str] = &[
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_PAPER",
    "LOCPATH",
];

/// The server process's own locale values for the client regen phase env,
/// as `(name, value)` pairs in `REGEN_LOCALE_VARS` order. Production
/// passes `|name| std::env::var(name).ok()`; tests inject a fake map.
pub(crate) fn collect_server_locale(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    REGEN_LOCALE_VARS
        .iter()
        .filter_map(|name| get(name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Whether an EAPI runs posixish locale handling: true from EAPI 6 on.
/// An unparseable EAPI defaults to the modern rule.
// Portage: eapi.py, posixish_locale
pub(crate) fn eapi_is_posixish(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    // Anything reaching us parsed off an ebuild, so default an
    // unparseable value to the modern rule.
    digits.parse::<u64>().map(|n| n >= 6).unwrap_or(true)
}

/// The twelve locale categories a set `LC_ALL` fans out over. The
/// fan-out below iterates this list, not the forwarding set.
pub(crate) const PORTAGE_LOCALE_CATEGORIES: &[&str] = &[
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_ADDRESS",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_NAME",
    "LC_PAPER",
    "LC_TELEPHONE",
];

/// Fan out a set `LC_ALL` over all twelve `PORTAGE_LOCALE_CATEGORIES`
/// for a posixish phase (itself disappearing), filling `LANG` only when
/// the server left it unset. `LANGUAGE` is never touched; a set-but-empty
/// `LC_ALL` is dropped without fanning out. Non-posixish EAPIs return
/// the pairs unchanged.
// Portage: portage/util/locale.py, split_LC_ALL
pub(crate) fn split_server_locale(
    locale: &[(String, String)],
    eapi: &str,
) -> Vec<(String, String)> {
    if !eapi_is_posixish(eapi) {
        return locale.to_vec();
    }
    let lc_all = locale
        .iter()
        .rev()
        .find(|(name, _)| name == "LC_ALL")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    let mut out: Vec<(String, String)> = locale
        .iter()
        .filter(|(name, _)| name != "LC_ALL")
        .cloned()
        .collect();
    if lc_all.is_empty() {
        return out;
    }
    for name in PORTAGE_LOCALE_CATEGORIES {
        match out.iter_mut().find(|(n, _)| n == name) {
            Some(pair) => pair.1 = lc_all.clone(),
            None => out.push((name.to_string(), lc_all.clone())),
        }
    }
    if !out.iter().any(|(n, _)| n == "LANG") {
        out.push(("LANG".to_string(), lc_all));
    }
    out
}

/// Arguments of [`postinst_regen_script`].
pub(crate) struct PostinstRegenScriptArgs<'a> {
    pub(crate) unit_dir: &'a str,
    pub(crate) staged: &'a crate::remote_bundle::StagedBundle,
    pub(crate) root: &'a str,
    pub(crate) workdir_parent: &'a str,
    pub(crate) colormap: &'a str,
    pub(crate) features: Option<&'a str>,
    pub(crate) locale: &'a [(String, String)],
    pub(crate) portuale_bin: &'a str,
}

/// The merge-time environment-regeneration postinst run: `mrg` runs this
/// phase on the client for every merged unit, **whether or not
/// `pkg_postinst` is defined** (an undefined function is a no-op that
/// still regenerates the env).
///
/// `PORTAGE_UPDATE_ENV` points at a unit-local file and
/// `PORTAGE_BZIP2_COMMAND` at the bundle's `bzip2-passthrough` (plain
/// text out; the server compresses -- the client must have no `bzip2`).
/// `FEATURES` / `PORTAGE_FEATURES` carry the resolved client list, and
/// `locale` re-exports the server's own values (split per
/// `split_server_locale` on posixish EAPIs). The script exits with the
/// phase rc (a non-zero postinst is non-fatal); `PHASE_postinst=<rc>`
/// plus `REGEN_ENV=ok|missing|skip:no-hooks` come back on stdout.
///
/// `O` and `SHELL` are unset before the phase runs, like Portage's phase
/// shell; `LC_ALL` is unset on posixish EAPIs (also killing leaks from
/// the sourced build-time environment).
// Portage: vartree.py, PORTAGE_UPDATE_ENV; phase-functions.sh, PORTAGE_UPDATE_ENV block
pub(crate) fn postinst_regen_script(args: PostinstRegenScriptArgs<'_>) -> String {
    let PostinstRegenScriptArgs {
        unit_dir,
        staged,
        root,
        workdir_parent,
        colormap,
        features,
        locale,
        portuale_bin,
    } = args;
    let mut extra = String::new();
    if let Some(list) = features.filter(|f| !f.is_empty()) {
        extra.push_str(&format!(
            "export FEATURES={features} PORTAGE_FEATURES={features}\n",
            features = sh_quote(list),
        ));
    }
    extra.push_str(&format!(
        "export PORTAGE_UPDATE_ENV={regen} PORTAGE_BZIP2_COMMAND={passthrough}\n",
        regen = sh_quote(&format!("{unit_dir}/environment.regen")),
        passthrough = sh_quote(&format!("{unit_dir}/bin/bzip2-passthrough")),
    ));
    // Match Portage's phase shell (see the fn doc comment) so the
    // `PORTAGE_UPDATE_ENV` save matches Portage's saved env.
    extra.push_str("unset O\n");
    // `unset` (not `export -n`) reproduces Portage exactly: bash
    // self-initializes an unexported `SHELL`.
    extra.push_str("unset SHELL\n");
    // Posixish phases never see `LC_ALL`; the unset also covers an
    // `LC_ALL` leaking in from the sourced build-time environment.
    let posixish = eapi_is_posixish(&staged.eapi);
    if posixish {
        extra.push_str("unset LC_ALL\n");
    }
    for (name, value) in split_server_locale(locale, &staged.eapi).iter() {
        extra.push_str(&format!(
            "export {name}={quoted}\n",
            quoted = sh_quote(value)
        ));
    }
    format!(
        concat!(
            "UNIT={unit}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "T=\"$UNIT/temp\"\n",
            "mkdir -p \"$T\" \"$UNIT/work\" \"$UNIT/homedir\" \"$UNIT/files\" \"$UNIT/empty\"\n",
            "if [ ! -f \"$UNIT/build-info/{pf}.ebuild\" ] || [ ! -f \"$UNIT/environment\" ]; then\n",
            "  echo \"REGEN_ENV=skip:no-hooks\"\n",
            "  echo \"PHASE_postinst=0\"\n",
            "  exit 0\n",
            "fi\n",
            "cp \"$UNIT/environment\" \"$T/environment\"\n",
            ": > \"$T/environment.raw\"\n",
            "{exports}",
            "{extra}",
            "export PATH=\"$UNIT/bin/ebuild-helpers:$PATH\"\n",
            "bash \"$UNIT/bin/ebuild.sh\" postinst\n",
            "rc=$?\n",
            "echo \"PHASE_postinst=$rc\"\n",
            "if [ -s \"$UNIT/environment.regen\" ]; then echo \"REGEN_ENV=ok\"; else echo \"REGEN_ENV=missing\"; fi\n",
            "exit $rc\n",
        ),
        unit = sh_quote(unit_dir),
        pf = staged.pf,
        exports = phase_exports(PhaseExportsArgs {
            ebuild: &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            build_dir: unit_dir,
            root,
            image_dir: &format!("{unit_dir}/image"),
            temp_dir: &format!("{unit_dir}/temp"),
            work_subdir: &format!("{unit_dir}/work"),
            home_dir: &format!("{unit_dir}/homedir"),
            files_dir: &format!("{unit_dir}/files"),
            bin_dir: &format!("{unit_dir}/bin"),
            tmpdir: workdir_parent,
            colormap,
            staged,
            phase: "postinst",
            portuale_bin
        }),
        extra = extra,
    )
}

// --- Client merge driver: helpers --------------------------------------------------

/// `is_protected` + `alloc_cfg` + `env_val` + `run_old_hook` helpers,
/// shared by the merge flow.
pub(crate) const MERGE_HELPERS: &str = include_str!("merge_helpers.sh");

// --- Client merge flow -------------------------------------------------------------

/// Merge flow: ownership scan → gate+copy → vdb → Rust unmerge of the
/// old instance → env-update, appended after `MERGE_HELPERS`. Machine
/// lines `MERGE_<STEP>=…` plus a final `STATUS=merged` (or
/// `STATUS=failed:<step>` from `mfail`); any `mfail` prints
/// `MERGE_FAIL=<step> <detail>` and exits 1. `STATELESS=1` (server-side
/// vdb placement) skips old-version discovery and the ownership scan.
/// New-postinst runs separately afterwards.
pub(crate) const MERGE_FLOW: &str = include_str!("merge_flow.sh");

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver; `None`
/// file skips the client record. `portuale_bin` is the resolved absolute
/// client path, exported for the whole driver (the old-instance unmerge
/// child runs through it).
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}
/// Arguments of [`merge_script()`].
pub(crate) struct MergeScriptArgs<'a> {
    pub(crate) unit_dir: &'a str,
    pub(crate) staged: &'a crate::remote_bundle::StagedBundle,
    pub(crate) root: &'a str,
    pub(crate) vdb: &'a str,
    pub(crate) stateless: bool,
    pub(crate) protect_list: &'a str,
    pub(crate) mask_list: &'a str,
    pub(crate) ledger: Option<&'a LedgerSpec>,
    pub(crate) portuale_bin: &'a str,
}

pub(crate) fn merge_script(args: MergeScriptArgs<'_>) -> Result<String, String> {
    let MergeScriptArgs {
        unit_dir,
        staged,
        root,
        vdb,
        stateless,
        protect_list,
        mask_list,
        ledger,
        portuale_bin,
    } = args;
    // A placed client vdb must stay inside the unit's world: reject the
    // degenerate empty path before it becomes `//var/db/pkg`.
    if vdb.trim().is_empty() {
        return Err("mrg: empty client vdb path".to_string());
    }
    Ok(format!(
        concat!(
            "UNIT={unit}\n",
            "IMAGE=\"$UNIT/image\"\n",
            "VDB={vdb}\n",
            "VDBROOT={vdb}/{category}\n",
            "STATELESS={stateless}\n",
            "CAT={category}\n",
            "PKG={pkg}\n",
            "NEWPF={pf}\n",
            "NEWVDB=\"$VDBROOT/$NEWPF\"\n",
            "TMPVDB=\"$VDBROOT/-MERGING-$NEWPF\"\n",
            "FULLSLOT={slot}\n",
            "MAINS={mainslot}\n",
            "REPO={repo}\n",
            "ROOT={root}\n",
            "WORKDIR={workdir}\n",
            "UNITBIN=\"$UNIT/bin\"\n",
            "export PORTUALE_BIN={portuale_bin}\n",
            "export PORTAGE_PYM_PATH=/\n",
            "unset PORTAGE_IPC_DAEMON\n",
            "COLORMAP={colormap}\n",
            "PROTECT={protect}\n",
            "MASK={mask}\n",
            "LEDGER_FILE={ledger_file}\n",
            "LEDGER_LINE={ledger_line}\n",
            "NEWCONTENTS=\"$UNIT/CONTENTS.new\"\n",
            "REPLACED_OWN=\"$UNIT/replaced.own\"\n",
            "OTHERS_OWN=\"$UNIT/others.own\"\n",
            "{helpers}",
            "{flow}",
        ),
        unit = sh_quote(unit_dir),
        root = sh_quote(root),
        vdb = sh_quote(vdb),
        stateless = if stateless { "1" } else { "0" },
        category = sh_quote(&staged.category),
        pkg = sh_quote(&staged.pn),
        pf = sh_quote(&staged.pf),
        slot = sh_quote(&staged.manifest.slot),
        mainslot = sh_quote(staged.manifest.slot.split('/').next().unwrap_or("0")),
        repo = sh_quote(&staged.manifest.repo),
        workdir = sh_quote(
            &std::path::Path::new(unit_dir)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/var/tmp".to_string())
        ),
        colormap = sh_quote(&crate::color::phase_colormap_export()),
        protect = sh_quote(protect_list),
        mask = sh_quote(mask_list),
        ledger_file = sh_quote(&ledger.map(|l| l.file.clone()).unwrap_or_default()),
        ledger_line = sh_quote(&ledger.map(|l| l.line.clone()).unwrap_or_default()),
        helpers = MERGE_HELPERS,
        flow = MERGE_FLOW,
        portuale_bin = sh_quote(portuale_bin),
    ))
}
