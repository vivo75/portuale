//! USE_EXPAND, USE_EXPAND_UNPREFIXED and IUSE_EFFECTIVE from the folded scalars.

use crate::resolve::*;

/// USE_EXPAND, USE_EXPAND_UNPREFIXED and IUSE_EFFECTIVE from the folded scalars.
pub(crate) fn expand_use_vars(
    config: &mut Config,
    scalars: &mut HashMap<String, String>,
    profile_scalars: &HashMap<String, String>,
    level_scalar_deltas: &[HashMap<String, String>],
) {
    // USE_EXPAND (PMS 7.3.4; real config.py's own regenerate(), "Do the
    // USE calculation last because it depends on USE_EXPAND"): now that
    // every profile level's own make.defaults plus make.conf have been
    // read, `config.use_expand` holds the final, incrementally-stacked
    // set of USE_EXPAND variable NAMES (e.g. "VIDEO_CARDS"). Each named
    // variable's own current VALUE -- read from `scalars`, the same
    // last-level-wins mechanism every other non-USE/ACCEPT_KEYWORDS
    // variable already uses (see the module doc comment; a deliberate
    // simplification of real portage's own genuinely-incremental
    // per-USE_EXPAND-variable behavior -- extending the pre-existing
    // "no incremental merge outside USE/ACCEPT_KEYWORDS" cut to these
    // variables too, not a new one) -- is expanded into
    // lowercase-prefixed pseudo-USE-flags via the exact same
    // apply_incremental token semantics (`-flag`/`flag`/`+flag`/`-*`)
    // USE itself already uses, folded directly into `use_flags`.
    // `USE_EXPAND_UNPREFIXED` (real `config.py`'s own companion
    // mechanism -- no prefix at all, applied in the loop right below
    // this one) IS now read too. `USE_EXPAND_IMPLICIT` + `IUSE_IMPLICIT`
    // + `USE_EXPAND_VALUES_<v>` are now read as well, feeding the real
    // EAPI 5+ `IUSE_EFFECTIVE` computation just after the unprefixed loop
    // -- **the earlier "`USE_EXPAND_HIDDEN`/`USE_EXPAND_IMPLICIT` are
    // display-only" note here was wrong for `USE_EXPAND_IMPLICIT`**: it
    // drives `pkg.iuse.is_valid_flag` (so a `foo[elibc_glibc]` USE-dep
    // matches a `foo` that never lists `elibc_glibc`), not just
    // `emerge --info` display. `USE_EXPAND_HIDDEN` genuinely *is*
    // display-only for EAPI 5+ (see `Config::iuse_effective`); it is read
    // now (`Config::use_expand_hidden`) and honoured only by
    // `emerge --pretend -v`'s own `USE_EXPAND` grouping in `portage_repo`.
    // IUSE-aware `_*`
    // wildcard expansion (`linguas_*`) IS now done, but in
    // `portage_repo::effective_use_flags` (it needs a specific
    // candidate's own IUSE). `package.use`'s own USE_EXPAND-prefix
    // shorthand (`VIDEO_CARDS: nvidia` lines) is read too (see the
    // `package.use` bullet in this comment).
    //
    // `env` `USE_ORDER` layer for the USE_EXPAND / USE_EXPAND_UNPREFIXED
    // variable *values* (`VIDEO_CARDS=nouveau emerge …`): their names
    // aren't known until `USE_EXPAND` itself is finalized above, so the
    // env override is folded into `scalars` here, right before both
    // expansion loops read them. Real portage treats each as its own
    // incremental; portuale's last-wins `scalars` model just lets the
    // env value replace the profile/`make.conf` one.
    let env_expand_names: Vec<String> = config
        .use_expand
        .iter()
        .chain(config.use_expand_unprefixed.iter())
        .cloned()
        .collect();
    for var in env_expand_names {
        if let Some(value) = config_env_var(&var) {
            note_higher(&mut *config, &var);
            scalars.insert(var, value);
        }
    }

    // Real config.py (2849-2889): "In order to best accommodate the
    // long-standing practice of setting default USE_EXPAND variables in
    // the profile's make.defaults, we translate these variables into
    // their equivalent USE flags" -- **per profile level**, with the
    // folded flags *prepended* before that level's own `USE=` line, so a
    // sub-profile (or that same level's `package.use`) can incrementally
    // cancel them. Real then skips re-folding USE_EXPAND for the
    // `defaults` configdict (2964); portuale mirrors that with the
    // `profile_scalars` guard in the two global loops just below.
    //
    // Without this, `LUA_SINGLE_TARGET="lua5-1"` from `profiles/base/
    // make.defaults` folded once (last-wins) into the high-priority
    // `conf` tier would be re-applied *after* `base/package.use`'s
    // `app-editors/neovim lua_single_target_luajit
    // -lua_single_target_lua5-1`, leaving both single-target flags on and
    // tripping neovim's `^^ ( ... )` REQUIRED_USE.
    {
        let expand_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand.iter().cloned().collect();
            v.sort();
            v
        };
        let unprefixed_names: Vec<String> = {
            let mut v: Vec<String> = config.use_expand_unprefixed.iter().cloned().collect();
            v.sort();
            v
        };
        let prefix_tok = |var: &str, tok: &str| -> String {
            let prefix = var.to_lowercase();
            if let Some(rest) = tok.strip_prefix('-') {
                format!("-{prefix}_{rest}")
            } else if let Some(rest) = tok.strip_prefix('+') {
                format!("{prefix}_{rest}")
            } else {
                format!("{prefix}_{tok}")
            }
        };
        // Real `config.py` (2853-2866) folds *every* profile level's own
        // `make.defaults` value for each USE_EXPAND var into that level's
        // USE contribution -- the flags then stack incrementally through
        // `USE_ORDER` (a later `arch/amd64` `VIDEO_CARDS="amdgpu fbdev …"`
        // does NOT wipe an earlier `default/linux` `VIDEO_CARDS="dummy
        // fbdev"`; the union stands). The only thing that suppresses the
        // per-level fold is `make.conf`/env overriding the var: real then
        // hits `is_not_incremental` in the `conf`/`env` configdict pass
        // (2961-2978), which *clears* every `<var>_*` flag and re-adds
        // that tier's value wholesale -- handled by the global loops
        // below. So the guard is per-var ("did conf/env touch it"), never
        // per-level ("is this level's value the one that survived").
        let conf_overrode = |var: &str| profile_scalars.get(var) != scalars.get(var);
        for (i, delta) in level_scalar_deltas.iter().enumerate() {
            let mut expand_use: Vec<String> = Vec::new();
            for var in &unprefixed_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(String::from));
                }
            }
            for var in &expand_names {
                if conf_overrode(var) {
                    continue;
                }
                if let Some(v) = delta.get(var) {
                    expand_use.extend(v.split_whitespace().map(|t| prefix_tok(var, t)));
                }
            }
            if expand_use.is_empty() {
                continue;
            }
            let joined = expand_use.join(" ");
            apply_use_incremental(&joined, &mut config.use_flags);
            config.profile_use_layers[i]
                .make_defaults_use
                .insert(0, joined);
        }
    }

    let use_expand_vars: Vec<String> = config.use_expand.iter().cloned().collect();
    for var in use_expand_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Folded per-level above -- the profile chain had the final say
        // and `make.conf`/env did not touch it (real skips the `defaults`
        // configdict in its own USE_EXPAND re-fold, 2964).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        let prefix = var.to_lowercase();
        let prefixed: String = value
            .split_whitespace()
            .map(|tok| {
                // Real config.py's own early-expand loop: "-flag" keeps
                // its own "-" outside the new prefix ("-video_cards_x",
                // still recognized as a removal by apply_incremental
                // below); a leading "+" is stripped first, matching
                // ordinary USE token handling, so it doesn't get
                // literally baked into the prefixed flag name.
                if let Some(rest) = tok.strip_prefix('-') {
                    format!("-{prefix}_{rest}")
                } else if let Some(rest) = tok.strip_prefix('+') {
                    format!("{prefix}_{rest}")
                } else {
                    format!("{prefix}_{tok}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        apply_use_incremental(&prefixed, &mut config.use_flags);
        config.conf_use_tokens.push(prefixed);
    }

    // USE_EXPAND_UNPREFIXED: real config.py's own companion to
    // USE_EXPAND -- the exact same mechanism (variable NAMES
    // incrementally stacked into `config.use_expand_unprefixed` via
    // "USE_EXPAND_UNPREFIXED" during make.defaults/make.conf processing
    // above, each named variable's own current VALUE read from
    // `scalars`), except the value is folded into `use_flags` via
    // `apply_incremental` *directly*, with no `lowercase(name)_` prefix
    // at all -- real Gentoo's own profile sets
    // `USE_EXPAND_UNPREFIXED="ARCH"`, so `ARCH="amd64"` contributes the
    // bare `amd64` flag, not `arch_amd64` (this is literally how
    // `amd64`/`x86`/`arm64` etc. exist as real USE flags at all).
    let use_expand_unprefixed_vars: Vec<String> =
        config.use_expand_unprefixed.iter().cloned().collect();
    for var in use_expand_unprefixed_vars {
        let Some(value) = scalars.get(&var) else {
            continue;
        };
        // Profile-only values are folded per-level above (real 2863-2866).
        if profile_scalars.get(&var) == scalars.get(&var) {
            continue;
        }
        apply_use_incremental(value, &mut config.use_flags);
        config.conf_use_tokens.push(value.clone());
    }

    // Real EAPI 5+ `IUSE_EFFECTIVE` (`config.py::_calc_iuse_effective`) --
    // computed now that `use_expand`/`use_expand_unprefixed`/
    // `use_expand_implicit`/`iuse_implicit` are all fully stacked and
    // every `USE_EXPAND_VALUES_<v>` scalar has been read. See
    // `Config::iuse_effective`'s own doc comment for what this grants a
    // candidate's `is_valid_flag` domain on top of its declared `IUSE`.
    // Deliberately a plain scalar read of each `USE_EXPAND_VALUES_<v>`
    // (portuale's established "no incremental merge outside USE/
    // ACCEPT_KEYWORDS" cut -- `USE_EXPAND_VALUES_*` isn't even in real
    // portage's own INCREMENTALS list anyway).
    config.iuse_effective = config.iuse_implicit.clone();
    for var in &config.use_expand_unprefixed {
        if !config.use_expand_implicit.contains(var) {
            continue;
        }
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(String::from));
        }
    }
    for var in &config.use_expand_implicit {
        if !config.use_expand.contains(var) {
            continue;
        }
        let prefix = var.to_lowercase();
        if let Some(values) = scalars.get(&format!("USE_EXPAND_VALUES_{var}")) {
            config
                .iuse_effective
                .extend(values.split_whitespace().map(|v| format!("{prefix}_{v}")));
        }
    }
}
