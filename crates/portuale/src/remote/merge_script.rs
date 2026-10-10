//! The phase and merge shell scripts sent to the client.

use crate::remote::*;

// --- Client merge (slice 4) ----------------------------------------------------

/// Real `eapi_exports_merge_type` (`lib/portage/eapi.py:63`, attrs table
/// `:300`: `exports_merge_type = eapi >= Eapi("4")`): `MERGE_TYPE` reaches
/// the phase env only for EAPI 4+ (real `config.py:3335-3336` pops it
/// otherwise). An unsupported or unparseable EAPI takes real's
/// `_get_eapi_attrs` fallback row (`eapi.py:247-258`), which exports.
pub(crate) fn eapi_exports_merge_type(eapi: &str) -> bool {
    // Real tests the *whole* string against `_supported_eapis`; anything
    // that is not a plain supported number (e.g. `3-foo`) takes the
    // exporting fallback row.
    match eapi.trim().parse::<u32>() {
        Ok(n) => n >= 4,
        Err(_) => true,
    }
}

/// `export` block for the new instance's phase script (`phase_script`;
/// the replaced instance's prerm/postrm go through the merge driver's own
/// `run_old_hook`, which deliberately exports no `MERGE_TYPE`: real's
/// unmerge re-creates the old instance's settings, `vartree.py:4535-4540`,
/// and never passes through the scheduler that sets it): path overrides with
/// caller-side values, everything else from the sourced saved env.
/// `unit_bin` is the shipped runtime dir (`$UNIT/bin`,
/// `$OLD_TMP/bin` symlink or copy -- callers decide).
/// `MERGE_TYPE=binary` rides the same line as `EMERGE_FROM` (real
/// `_emerge/Binpkg.py:92` sets it for every binpkg merge; `mrg` only
/// merges binpkgs), gated on [`eapi_exports_merge_type`].
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5):
/// every client call uses it, never `$PATH` -- `PORTAGE_PYTHON` is the
/// shipped `portuale-python` shim through the script's own `$UNITBIN`
/// (`$UNIT/bin`, defined by each script header), `PORTAGE_PYM_PATH` is
/// `/` (no checkout on the client), and `PORTAGE_IPC_DAEMON` is unset
/// (no IPC daemon, #326 D6 -- this also scrubs a value inherited over
/// the local transport).
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_exports(
    ebuild: &str,
    build_dir: &str,
    root: &str,
    image_dir: &str,
    temp_dir: &str,
    work_subdir: &str,
    home_dir: &str,
    files_dir: &str,
    bin_dir: &str,
    tmpdir: &str,
    colormap: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    portuale_bin: &str,
) -> String {
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

// --- Client phases (slice 3) -------------------------------------------------

/// One phase invocation, generated bash: recreate the local
/// `run_phase_from_saved_env` + `run_one_phase_bash` setup client-side
/// (fresh `bash bin/ebuild.sh <phase>` process per phase -- the readonly
/// `EBUILD_PHASE` semantics demand it, exactly like local `spawnebuild`).
/// Path overrides are exported with client-side values; everything else
/// (EAPI, PN/PV/…, USE) rides the sourced saved environment, whose stale
/// path copies ebuild.sh's own `__preprocess_ebuild_env` strips (the
/// `environment.raw` marker enables that filtering, mirroring local).
/// `S` is deliberately *not* exported: ebuild.sh defaults it to
/// `${WORKDIR}/${P}` with `P` from the saved env (exact local behavior).
/// `PHASE_<phase>=<rc>` on stdout, phase log raw on stdout/stderr.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn phase_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    phase: &str,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    portuale_bin: &str,
) -> String {
    // D keeps local's trailing slash. `UNITBIN` is the shipped runtime
    // dir (`$UNIT/bin`): `phase_exports` resolves `PORTAGE_PYTHON`
    // through it, so no shim falls back to `PATH` (#326 D5/S8.3).
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
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            phase,
            portuale_bin,
        ),
        phase = phase,
    )
}

/// Locale variables the regen postinst run re-exports into the client
/// phase env: real's saved locale comes from the calling environment
/// through `environ_whitelist` (`special_env_vars.py`: `LANG` plus the
/// `LC_*` list), and for `mrg` the calling environment is the server
/// process. `LANGUAGE` is **not** in real's whitelist -- it rides along
/// here only because the slice brief names it explicitly; see
/// `collect_server_locale`. Only set variables are ever forwarded
/// (never invented defaults).
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
];

/// The server process's own locale values for the client regen phase
/// env, as `(name, value)` pairs in `REGEN_LOCALE_VARS` order -- one
/// entry per variable the getter reports as set (even empty: a set-but-
/// empty `LANG` is still the calling environment's value, which is what
/// real's whitelist passes through). Production passes
/// `|name| std::env::var(name).ok()`; tests inject a fake map.
pub(crate) fn collect_server_locale(get: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    REGEN_LOCALE_VARS
        .iter()
        .filter_map(|name| get(name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Real's `posixish_locale` EAPI attribute (`eapi.py:182,309`): true
/// from EAPI 6 on. Only posixish phases run the `LC_ALL` split below
/// (`EbuildPhase.py:51-56`); older EAPIs keep `LC_ALL` as-is.
pub(crate) fn eapi_is_posixish(eapi: &str) -> bool {
    let digits: String = eapi
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    // Real rejects an unknown EAPI outright; anything reaching us parsed
    // off an ebuild, so default an unparseable value to the modern rule
    // (the same direction the local phase path's unconditional split
    // already takes).
    digits.parse::<u64>().map(|n| n >= 6).unwrap_or(true)
}

/// Real's `locale_categories` (`portage/util/locale.py:22-36`): the full
/// set `split_LC_ALL` copies a set `LC_ALL` over -- the six POSIX
/// categories, `LC_PAPER`, and the five GNU extensions
/// (`LC_ADDRESS`, `LC_IDENTIFICATION`, `LC_MEASUREMENT`, `LC_NAME`,
/// `LC_TELEPHONE`). The fan-out below iterates this list, not the
/// forwarding set: real's split operates on settings, and for a
/// binpkg-merge postinst `$T/environment` does not exist yet, so
/// `config.environ()` applies no whitelist filter
/// (`config.py:3286-3299`) and all twelve reach the phase env and the
/// `PORTAGE_UPDATE_ENV` save (`EbuildPhase.py:158,259` runs the env
/// extractor for `pretend`/`prerm` only). Probed against real's own
/// `split_LC_ALL` + `environ()`: with `LC_ALL` set, all twelve export
/// when `$T/environment` is absent, seven when present.
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

/// Port of real's `split_LC_ALL` (`portage/util/locale.py:160`) to the
/// forwarded regen locale set, for a posixish phase: a set `LC_ALL` fans
/// out over all twelve real `locale_categories` (see
/// `PORTAGE_LOCALE_CATEGORIES`; real copies it over the same list,
/// unconditionally overwriting) and itself
/// disappears (real blanks it, then `config.environ()` deletes the
/// placeholder, `config.py:3374-3385`). `LANG` is filled from `LC_ALL`
/// only when the server left it unset -- real's split never touches
/// `LANG`, but with `LC_ALL` set and `LANG` unset the effective locale
/// is `LC_ALL`'s value everywhere, and the bed shows real's saved env
/// carrying `LANG` alongside the split categories. An explicitly set
/// `LANG` is never clobbered; `LANGUAGE` (not a locale category, rides
/// along per the l171b brief) is never touched. A set-but-empty
/// `LC_ALL` is dropped without fanning out (real's `if lc_all:` is
/// falsy, then `environ()` deletes the placeholder). Non-posixish
/// EAPIs return the pairs unchanged (real never calls the split there).
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

/// The merge-time environment-regeneration postinst run (backlog #171):
/// real `vartree.py:5334` sets `PORTAGE_UPDATE_ENV=<dbpkgdir>/
/// environment.bz2` around *every* merge's postinst phase, and
/// `bin/phase-functions.sh:1072-1082` re-saves the vdb env from the
/// live phase env through it -- so the vdb env carries the merge-time
/// config, not the binpkg's build-time one. `mrg` runs this phase on
/// the client for every merged unit, **whether or not `pkg_postinst`
/// is defined**: real's `prerm|postrm|preinst|postinst|config|info`
/// case (`phase-functions.sh:1061-1083`) runs
/// `__ebuild_phase_with_hooks pkg_${1}` unconditionally
/// (`__ebuild_phase` is a no-op for an undefined function) and *then*
/// the `PORTAGE_UPDATE_ENV` block -- so an undefined `pkg_postinst`
/// still regenerates the env. `pkg_postinst` itself still runs only if
/// defined (the same no-op rule; nothing faked).
///
/// `PORTAGE_UPDATE_ENV` points at a unit-local file
/// (`$UNIT/environment.regen`) and `PORTAGE_BZIP2_COMMAND` at the
/// bundle's `bzip2-passthrough` (plain text out; the server compresses
/// -- the client must have no `bzip2`, plan §6). `FEATURES` /
/// `PORTAGE_FEATURES` carry the resolved client list (the local
/// `refresh_features` rule), so the env records the merge-time
/// features (e.g. no `buildpkg`). `locale` re-exports the server's own
/// locale values (see `REGEN_LOCALE_VARS` / `collect_server_locale`,
/// split per `split_server_locale` on posixish EAPIs) so
/// the regen'd env carries them exactly as real's calling-environment
/// whitelist does. `PHASE_postinst=<rc>` plus
/// `REGEN_ENV=ok|missing|skip:no-hooks` come back on stdout; the
/// script exits with the phase rc (a non-zero postinst is non-fatal --
/// real `_postinst_failure` -- as long as the regen file exists).
///
/// `O` is unset and `SHELL` unset before the phase runs: real's
/// phase shell never has `O` (`config.environ()` drops it via
/// `special_env_vars.py: environ_filter` even though `doebuild.py:475`
/// sets `mysettings["O"]`) and its saved `SHELL` is `declare --`
/// (bash initializes its own -- `SHELL` is in neither the whitelist
/// nor the config), while `mrg` exported a unit-local `O` and usually
/// inherits an exported `SHELL`. `LC_ALL` is unset on posixish EAPIs
/// and the forwarded locale arrives pre-split (see
/// `split_server_locale`); the unset also kills any `LC_ALL` leaking
/// in from the sourced build-time environment.
#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn postinst_regen_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    workdir_parent: &str,
    colormap: &str,
    features: Option<&str>,
    locale: &[(String, String)],
    portuale_bin: &str,
) -> String {
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
    // Backlog #171b: match real's phase shell (see the fn doc comment)
    // so the `PORTAGE_UPDATE_ENV` save matches real's saved env.
    extra.push_str("unset O\n");
    // Backlog #171c: `unset` (not `export -n`) reproduces real exactly:
    // bash self-initializes an unexported `SHELL`, while `export -n`
    // would keep a non-default server value.
    extra.push_str("unset SHELL\n");
    // Backlog #171c: real's posixish phases never see `LC_ALL`
    // (`split_LC_ALL` + `config.environ()`); the unset also covers an
    // `LC_ALL` leaking in from the sourced build-time environment, and
    // the exports below carry the pre-split values.
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
        exports = phase_exports(
            &format!("{unit_dir}/build-info/{}.ebuild", staged.pf),
            unit_dir,
            root,
            &format!("{unit_dir}/image"),
            &format!("{unit_dir}/temp"),
            &format!("{unit_dir}/work"),
            &format!("{unit_dir}/homedir"),
            &format!("{unit_dir}/files"),
            &format!("{unit_dir}/bin"),
            workdir_parent,
            colormap,
            staged,
            "postinst",
            portuale_bin,
        ),
        extra = extra,
    )
}

// --- Client merge driver (slice 4): helpers ----------------------------------

/// Longest-prefix `is_protected` + `alloc_cfg` + `env_val` + `run_old_hook`
/// helpers shared by the merge flow. Pure bash, no placeholders except
/// the colormap/PATH roots baked by the caller template below.
pub(crate) const MERGE_HELPERS: &str = r#"mfail() { echo "MERGE_FAIL=$1 $2"; echo "STATUS=failed:$1"; exit 1; }
warn() { echo "MERGE_WARN=$1 $2"; }
env_val() {
  sed -n "s/^declare -x $2=\"\\(.*\\)\"$/\\1/p" "$1" | head -n 1
}
is_protected() {
  dest=$1; best_p=0; best_m=0
  for e in $PROTECT; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_p" ] && best_p=$len; }
    fi
  done
  for e in $MASK; do
    pp="$ROOT/${e#/}"
    if [ -d "$pp" ]; then
      case "$dest" in "$pp"|"$pp"/*)
        len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len ;;
      esac
    else
      [ "$dest" = "$pp" ] && { len=${#pp}; [ "$len" -gt "$best_m" ] && best_m=$len; }
    fi
  done
  [ "$best_p" -gt 0 ] && [ "$best_p" -gt "$best_m" ] && echo yes || echo no
}
alloc_cfg() {
  dir=${1%/*}; base=${1##*/}; n=0
  while [ -e "$dir/._cfg$(printf '%04d' $n)_$base" ]; do n=$((n + 1)); done
  echo "$dir/._cfg$(printf '%04d' $n)_$base"
}
run_old_hook() {
  vdbdir=$1; phase=$2
  set -- "$vdbdir"/*.ebuild
  [ -f "$1" ] || { echo "OLDHOOK=missing-ebuild"; return 2; }
  ebuild=$1
  OTMP="$WORKDIR/oldtmp"
  rm -rf "$OTMP"
  mkdir -p "$OTMP/temp" "$OTMP/work" "$OTMP/homedir" "$OTMP/files" "$OTMP/empty" "$OTMP/image" || return 1
  # Backlog #171: prefer the server-decompressed env the server staged
  # at `$UNIT/old-env/<pf>` (no client bzip2 needed); then the legacy
  # plain `environment` older `mrg` versions left, used only when no
  # `environment.bz2` exists; client `bzip2 -dc` only as a last resort;
  # then any plain `environment` at all (an older-`mrg` entry with both
  # files whose staging failed, on a bzip2-less client).
  pf=${vdbdir##*/}
  if [ -f "$UNIT/old-env/$pf" ]; then
    cp "$UNIT/old-env/$pf" "$OTMP/temp/environment" || return 1
  elif [ ! -f "$vdbdir/environment.bz2" ] && [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment.bz2" ] && command -v bzip2 >/dev/null 2>&1; then
    bzip2 -dc -- "$vdbdir/environment.bz2" > "$OTMP/temp/environment" || return 1
  elif [ -f "$vdbdir/environment" ]; then
    cp "$vdbdir/environment" "$OTMP/temp/environment" || return 1
  else
    echo "OLDHOOK=no-env-bzip2-missing"; return 2
  fi
  : > "$OTMP/temp/environment.raw"
  EAPI=$(env_val "$OTMP/temp/environment" EAPI)
  [ -n "$EAPI" ] || { echo "OLDHOOK=no-eapi"; return 2; }
  export EAPI
  for v in CATEGORY PN PV PR PVR P PF; do
    val=$(env_val "$OTMP/temp/environment" "$v")
    [ -n "$val" ] || { echo "OLDHOOK=no-$v"; return 2; }
    export "$v=$val"
  done
  export EBUILD="$ebuild" O="$vdbdir"
  export ROOT="$ROOT" EROOT="$ROOT"
  export PORTAGE_BUILDDIR="$OTMP"
  export WORKDIR="$OTMP/work"
  export D="$OTMP/image/" ED="$OTMP/image/"
  export T="$OTMP/temp" HOME="$OTMP/homedir" FILESDIR="$OTMP/files"
  export PORTAGE_BIN_PATH="$UNITBIN"
  export PORTAGE_ECLASS_LOCATIONS=""
  export PORTAGE_PYTHON="$UNITBIN/portuale-python"
  export PORTAGE_PYM_PATH=/
  unset PORTAGE_IPC_DAEMON
  export PORTAGE_COLORMAP="$COLORMAP"
  export PORTAGE_TMPDIR="$WORKDIR"
  export SANDBOX_LOG="$OTMP/temp/sandbox.log"
  export EBUILD_PHASE="$phase" EMERGE_FROM=binary
  export PATH="$UNITBIN/ebuild-helpers:$PATH"
  bash "$UNITBIN/ebuild.sh" "$phase"
  rc=$?
  echo "OLDHOOK_$phase=$rc"
  return $rc
}
"#;

// --- Client merge flow (slice 4) -----------------------------------------------

/// Merge flow: old-prerm → ownership scan → gate+copy → vdb → remove-old
/// → old-postrm → env-update, appended after `MERGE_HELPERS`. Shell vars
/// (`UNIT`, `ROOT`, `VDB`, `VDBROOT`, `NEWPF`, `PKG`, `MAINS`, `PROTECT`,
/// `MASK`, `STATELESS`, …) come from `merge_script`'s header. Machine
/// lines `MERGE_<STEP>=…` plus a final `STATUS=merged` (or
/// `STATUS=failed:<step>` from `mfail`); any `mfail` prints
/// `MERGE_FAIL=<step> <detail>` and exits 1. `STATELESS=1` (server-side
/// vdb placement) skips old-version discovery and the ownership scan --
/// any existing file/symlink destination (outside CONFIG_PROTECT
/// divert) is a fail-closed collision, and old hooks never run.
/// New-postinst runs separately afterwards (see `run_bundle_stage`).
///
/// The `client:<path>` VDB is read and written by this bash on the
/// client, so it stays in the files format by construction: portuale
/// has no code there to call `portage-vdb` from (feat#157 design §6.3).
/// Only the server-side `VdbShadow` read goes through `portage-vdb`.
pub(crate) const MERGE_FLOW: &str = r##"OLD_PF=""; OLD_COUNTER=-1
if [ "$STATELESS" = 1 ]; then :; else
for d in "$VDBROOT"/"$PKG"-*/; do

  [ -d "$d" ] || continue
  cpf=${d%/}; cpf=${cpf##*/}
  [ "$cpf" = "$NEWPF" ] && continue
  rest=${cpf#"$PKG"-}
  case "$rest" in [0-9]*) ;; *) continue;; esac
  slot=$(cat "$d/SLOT" 2>/dev/null | cut -d/ -f1)
  [ "$slot" = "$MAINS" ] || continue
  c=$(cat "$d/COUNTER" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) c=-1;; esac
  if [ "$c" -gt "$OLD_COUNTER" ]; then OLD_COUNTER=$c; OLD_PF=$cpf; fi
done
fi
if [ -n "$OLD_PF" ]; then OLDVDB="$VDBROOT/$OLD_PF"; else OLDVDB=""; fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_PRERM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" prerm; then
    echo "MERGE_PRERM=ok $OLD_PF"
  else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_PRERM=skip $OLD_PF"; else echo "MERGE_PRERM=warn $OLD_PF"; fi
  fi
else
  echo "MERGE_PRERM=skip:none-installed"
fi
: > "$REPLACED_OWN"; : > "$OTHERS_OWN"
if [ "$STATELESS" != 1 ]; then
for c in "$VDB"/*/*/CONTENTS; do
  [ -f "$c" ] || continue
  pfdir=${c%/*}; pfdir=${pfdir##*/}
  awk '$1=="obj"||$1=="sym"{print $2}' "$c" > "$UNIT/scan.list"
  if [ "$pfdir" = "$NEWPF" ] || { [ -n "$OLD_PF" ] && [ "$pfdir" = "$OLD_PF" ]; }; then
    cat "$UNIT/scan.list" >> "$REPLACED_OWN"
  else
    cat "$UNIT/scan.list" >> "$OTHERS_OWN"
  fi
done
rm -f "$UNIT/scan.list"
fi
: > "$NEWCONTENTS"; : > "$NEWPATHS"
ROOTUID=$(id -u)
while read -r line; do
  [ -n "$line" ] || continue
  set -f; set -- $line; set +f; kind=$1
  case "$kind" in
    dir) rel=$4 ;;
    obj) rel=$4; md5=$2; mtime=$3 ;;
    sym) rel=$4; mtime=$3; target=$5 ;;
    *) mfail copy "unknown filemeta kind $kind" ;;
  esac
  src="$IMAGE/$rel"; dest="$ROOT/$rel"; apath="/$rel"
  if [ -e "$dest" ] || [ -L "$dest" ]; then
    if [ "$STATELESS" = 1 ]; then
      # No ownership proof without a client vdb: the shape rules still
      # apply, but any existing file/symlink destination outside
      # CONFIG_PROTECT divert is a fail-closed collision (merging
      # directories and the divert itself need no ownership).
      if [ -d "$dest" ] && [ ! -L "$dest" ]; then
        [ "$kind" = dir ] || mfail collision "file over directory at $apath (stateless)";
      elif [ "$kind" = dir ]; then
        mfail collision "directory over file at $apath (stateless)";
      elif [ "$(is_protected "$dest")" = yes ]; then :;
      else mfail collision "stateless refusing to overwrite $apath"; fi
    elif grep -Fxq "$apath" "$REPLACED_OWN"; then :;
    elif grep -Fxq "$apath" "$OTHERS_OWN"; then mfail collision "$apath owned by another package";
    elif [ -d "$dest" ] && [ ! -L "$dest" ]; then
      [ "$kind" = dir ] || mfail collision "file over directory at $apath";
    elif [ "$kind" = dir ]; then
      mfail collision "directory over file at $apath";
    else :; fi
  fi
  case "$kind" in
    dir)
      if [ ! -d "$dest" ]; then
        mkdir -p "$dest" || mfail copy "mkdir $apath"
        [ "$ROOTUID" = 0 ] && chown --reference "$src" "$dest" || true
        chmod --reference "$src" "$dest" || mfail copy "chmod $apath"
      fi
      echo "dir $apath" >> "$NEWCONTENTS"
      ;;
    obj)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        if cmp -s "$src" "$dest"; then :;
        else dest=$(alloc_cfg "$dest"); fi
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # A symlink at dest would make `mv` treat a symlink-to-directory as
      # a destination directory (and `cp` follow it): drop the link
      # first, exactly like real movefile's `os.unlink(dest)` for a
      # symlink destination.
      [ -L "$dest" ] && rm -f "$dest"
      # Real movefile / local replace_file_atomic (portuale #96/#97):
      # materialize the new file at a temporary sibling and rename(2) it
      # over the destination. Never `cp` into the live inode -- ETXTBSY
      # protects a running executable, but an mmap'd shared library
      # opens fine, and rewriting it kills every process mapping it.
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      cp -p "$src" "$tmp" || { rm -f "$tmp"; mfail copy "$apath"; }
      [ "$ROOTUID" = 0 ] && chown --reference "$src" "$tmp" || true
      chmod --reference "$src" "$tmp" || { rm -f "$tmp"; mfail copy "chmod $apath"; }
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "obj $apath $md5 $mtime" >> "$NEWCONTENTS"
      ;;
    sym)
      if [ "$(is_protected "$dest")" = yes ] && { [ -f "$dest" ] || [ -L "$dest" ]; }; then
        cur=""; [ -L "$dest" ] && cur=$(readlink "$dest")
        [ "$cur" = "$target" ] || dest=$(alloc_cfg "$dest")
      fi
      mkdir -p "${dest%/*}" || mfail copy "mkdir parent of $apath"
      # Same atomic replacement for the link: a symlink destination is
      # unlinked first (so `mv` cannot resolve it as a directory), then
      # the new link is built at a temporary sibling and renamed over
      # the destination -- never `rm` then `ln` on the live path.
      [ -L "$dest" ] && rm -f "$dest"
      tmp="${dest%/*}/.${dest##*/}._portage_merge_.$$"
      rm -f "$tmp"
      ln -s "$target" "$tmp" || { rm -f "$tmp"; mfail copy "symlink $apath"; }
      touch -h -r "$src" "$tmp" 2>/dev/null || true
      if [ "$ROOTUID" = 0 ]; then chown -h --reference "$src" "$tmp" 2>/dev/null || true; fi
      mv -f "$tmp" "$dest" || { rm -f "$tmp"; mfail copy "$apath"; }
      echo "sym $apath -> $target $mtime" >> "$NEWCONTENTS"
      ;;
  esac
  echo "$apath" >> "$NEWPATHS"
done < "$UNIT/filemeta"
echo "MERGE_COPY=ok"
# Stateless clients keep no vdb at all (plan §7: the server owns
# installed-db state) -- files merge, but no entry, COUNTER, or
# edb-counter is recorded.
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_VDB=skip:stateless-no-vdb"
else
rm -rf "$TMPVDB"; mkdir -p "$TMPVDB" || mfail vdb "mkdir tmp"
for f in "$UNIT/build-info"/*; do
  # NOTE: `cond && action || fail` misfires when cond is false -- always
  # an explicit if for fallible steps.
  if [ -f "$f" ]; then cp "$f" "$TMPVDB"/ || mfail vdb "copy build-info"; fi
done
# Backlog #171: no plain `environment` in the vdb (real writes
# `environment.bz2` only). The build-time `environment.bz2` rides the
# verbatim `build-info/*` copies above as the fallback until the
# postinst regen phase overwrites it; future unmerge hooks source that
# file (server-decompressed at ship time, never a plain copy).
printf '%s\n' "$CAT" > "$TMPVDB/CATEGORY"
printf '%s\n' "$FULLSLOT" > "$TMPVDB/SLOT"
printf '%s\n' "$REPO" > "$TMPVDB/repository"
cp "$NEWCONTENTS" "$TMPVDB/CONTENTS" || mfail vdb "write CONTENTS"
max_c=-1
for f in "$VDBROOT"/*/COUNTER "$ROOT/var/cache/edb/counter"; do
  [ -f "$f" ] || continue
  c=$(cat "$f" 2>/dev/null | tr -d ' \t\n'); case "$c" in ''|*[!0-9]*) continue;; esac
  [ "$c" -gt "$max_c" ] && max_c=$c
done
NEXT=$((max_c + 1))
printf '%s' "$NEXT" > "$TMPVDB/COUNTER"
mkdir -p "$ROOT/var/cache/edb" && printf '%s' "$NEXT" > "$ROOT/var/cache/edb/counter"
tmpmeta="$TMPVDB/metadata.tmp"
: > "$tmpmeta"
for f in BDEPEND BUILD_ID BUILD_TIME CHOST COUNTER DEFINED_PHASES DEPEND DESCRIPTION EAPI HOMEPAGE IDEPEND IUSE KEYWORDS LICENSE PDEPEND PROPERTIES PROVIDES RDEPEND REQUIRES RESTRICT SLOT USE repository; do
  [ -f "$TMPVDB/$f" ] || continue
  printf '%s=%s\n' "$f" "$(tr -s '[:space:]' ' ' < "$TMPVDB/$f" | sed -e 's/^ *//' -e 's/ *$//')" >> "$tmpmeta"
done
if [ -s "$tmpmeta" ]; then
  { echo "#format=1"; LC_ALL=C sort "$tmpmeta"; } > "$TMPVDB/metadata"
  printf '#dir_mtime=%s\n' "$(stat -c %Y "$TMPVDB")" >> "$TMPVDB/metadata"
  rm -f "$tmpmeta"
fi
rm -rf "$NEWVDB"
mv "$TMPVDB" "$NEWVDB" || mfail vdb "rename into place"
echo "MERGE_VDB=ok"
fi
if [ -n "$OLD_PF" ]; then
  if [ -f "$OLDVDB/CONTENTS" ]; then
    : > "$UNIT/olddirs.list"
    while read -r line; do
      [ -n "$line" ] || continue
      set -f; set -- $line; set +f; kind=$1
      case "$kind" in
        obj|sym)
          apath=$2; recorded=${line##* }
          grep -Fxq "$apath" "$NEWPATHS" && continue
          if [ ! -e "$ROOT$apath" ] && [ ! -L "$ROOT$apath" ]; then continue; fi
          if [ "$(stat -c %Y "$ROOT$apath" 2>/dev/null)" = "$recorded" ]; then
            rm -f "$ROOT$apath" || warn remove "cannot remove $apath"
          else
            warn remove "modified-kept $apath"
          fi
          ;;
        dir) echo "$2" >> "$UNIT/olddirs.list" ;;
        *) warn remove "unmerge-skip $line" ;;
      esac
    done < "$OLDVDB/CONTENTS"
    sort -r "$UNIT/olddirs.list" | while read -r d; do
      [ -n "$d" ] && rmdir "$ROOT$d" 2>/dev/null || true
    done
    rm -f "$UNIT/olddirs.list"
  else
    warn remove "no CONTENTS in $OLD_PF, files kept"
  fi
  echo "MERGE_REMOVE=ok $OLD_PF"
elif [ "$STATELESS" = 1 ]; then
  echo "MERGE_REMOVE=skip:stateless-no-vdb"
else
  echo "MERGE_REMOVE=skip:none-installed"
fi
if [ "$STATELESS" = 1 ]; then
  echo "MERGE_POSTRM=skip:stateless-no-vdb"
elif [ -n "$OLD_PF" ]; then
  if run_old_hook "$OLDVDB" postrm; then echo "MERGE_POSTRM=ok $OLD_PF"; else
    rc=$?
    if [ "$rc" = 2 ]; then echo "MERGE_POSTRM=skip $OLD_PF"; else echo "MERGE_POSTRM=warn $OLD_PF"; fi
  fi
  rm -rf "$OLDVDB"
else
  echo "MERGE_POSTRM=skip:none-installed"
fi
if command -v ldconfig >/dev/null 2>&1; then
  if ldconfig -r "$ROOT" 2>/dev/null; then echo "MERGE_ENVUPDATE=ok ldconfig"; else echo "MERGE_ENVUPDATE=skip:ldconfig-failed"; fi
else
  echo "MERGE_ENVUPDATE=skip:no-ldconfig"
fi
if [ -n "$LEDGER_FILE" ]; then
  mkdir -p "${LEDGER_FILE%/*}" 2>/dev/null || warn ledger "mkdir for $LEDGER_FILE"
  if printf '%s\n' "$LEDGER_LINE" >> "$LEDGER_FILE" 2>/dev/null; then
    tail -n 10 "$LEDGER_FILE" > "$LEDGER_FILE.tmp" 2>/dev/null && mv "$LEDGER_FILE.tmp" "$LEDGER_FILE" 2>/dev/null || warn ledger "rotate $LEDGER_FILE"
    echo "MERGE_LEDGER=ok"
  else
    warn ledger "append $LEDGER_FILE"
  fi
else
  echo "MERGE_LEDGER=skip:none-requested"
fi
echo "MERGE_DONE=ok"
echo "STATUS=merged"
"##;

/// Merge header: all values the flow needs, pre-quoted. The unit layout
/// mirrors the local merge (`image/`, `build-info/`, shipped `bin/`).
/// Client ledger destination + line baked into the merge driver.
/// `None` file skips the client record (the `--remote-binpkg` trial path
/// keeps no ledger; the resolve path always records).
/// `portuale_bin` is the S8.1-resolved absolute client path (#326 D5),
/// exported for the whole driver (including the old hooks, which inherit
/// the header's exports); `PORTAGE_PYM_PATH` is `/` and
/// `PORTAGE_IPC_DAEMON` is unset, like the phase scripts.
#[derive(Debug, Clone)]
pub(crate) struct LedgerSpec {
    pub file: String,
    pub line: String,
}

#[allow(clippy::too_many_arguments, reason = "#336 Phase 5 worklist")]
pub(crate) fn merge_script(
    unit_dir: &str,
    staged: &crate::remote_bundle::StagedBundle,
    root: &str,
    vdb: &str,
    stateless: bool,
    protect_list: &str,
    mask_list: &str,
    ledger: Option<&LedgerSpec>,
    portuale_bin: &str,
) -> Result<String, String> {
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
            "NEWPATHS=\"$UNIT/paths.new\"\n",
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
