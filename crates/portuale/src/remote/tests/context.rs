use super::*;

#[test]
fn remote_exec_handoff_sets_and_takes() {
    assert!(take_remote_exec().is_none());
    let ctx = ctx_with_args("h", None);
    set_remote_exec(ctx.clone());
    assert_eq!(take_remote_exec(), Some(ctx));
    // Taking clears the slot: no leakage across calls.
    assert!(take_remote_exec().is_none());
}

/// A set `LC_ALL` fans out over the twelve categories and drops; an empty
/// `LC_ALL` drops without fanning out; `LANGUAGE` is untouched; non-posixish
/// EAPIs pass through.
#[test]
fn split_server_locale_fans_lc_all_out_like_portage() {
    // The twelve categories, hardcoded here (not via
    // `PORTAGE_LOCALE_CATEGORIES`) so the test pins the list's
    // content, not just its own reference to it.
    const TWELVE: &[&str] = &[
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
    assert_eq!(PORTAGE_LOCALE_CATEGORIES, TWELVE);
    assert!(eapi_is_posixish("6"));
    assert!(eapi_is_posixish("8"));
    assert!(!eapi_is_posixish("5"));
    let get = |pairs: &[(String, String)], name: &str| {
        pairs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    };
    // Server `LC_ALL=C.UTF-8` alone → `LANG` + all twelve categories
    // carry it, no `LC_ALL`.
    let out = split_server_locale(&[("LC_ALL".to_string(), "C.UTF-8".to_string())], "8");
    assert!(get(&out, "LC_ALL").is_none(), "LC_ALL survived:\n{out:?}");
    assert_eq!(get(&out, "LANG").as_deref(), Some("C.UTF-8"));
    for name in TWELVE {
        assert_eq!(
            get(&out, name).as_deref(),
            Some("C.UTF-8"),
            "{name} not split"
        );
    }
    // The fan-out overwrites unconditionally; an explicit `LANG` still wins.
    let out = split_server_locale(
        &[
            ("LC_ALL".to_string(), "C.UTF-8".to_string()),
            ("LC_MESSAGES".to_string(), "en_US.UTF-8".to_string()),
            ("LANG".to_string(), "POSIX".to_string()),
            ("LANGUAGE".to_string(), "de".to_string()),
        ],
        "8",
    );
    assert_eq!(get(&out, "LC_MESSAGES").as_deref(), Some("C.UTF-8"));
    assert_eq!(get(&out, "LANG").as_deref(), Some("POSIX"));
    assert_eq!(get(&out, "LANGUAGE").as_deref(), Some("de"));
    assert!(get(&out, "LC_ALL").is_none());
    // Set-but-empty `LC_ALL` is dropped, never fanned out.
    let out = split_server_locale(
        &[
            ("LC_ALL".to_string(), String::new()),
            ("LANG".to_string(), "C".to_string()),
        ],
        "8",
    );
    assert!(get(&out, "LC_ALL").is_none());
    assert_eq!(get(&out, "LANG").as_deref(), Some("C"));
    assert!(get(&out, "LC_CTYPE").is_none());
    // Non-posixish EAPIs keep `LC_ALL` as-is.
    let input = vec![("LC_ALL".to_string(), "C.UTF-8".to_string())];
    assert_eq!(split_server_locale(&input, "5"), input);
}

#[test]
fn strict_host_key_checking_parses() {
    assert_eq!(
        StrictHostKeyChecking::parse("accept-new"),
        Some(StrictHostKeyChecking::AcceptNew)
    );
    assert_eq!(
        StrictHostKeyChecking::parse("yes"),
        Some(StrictHostKeyChecking::Yes)
    );
    assert_eq!(
        StrictHostKeyChecking::parse("no"),
        Some(StrictHostKeyChecking::No)
    );
    assert_eq!(StrictHostKeyChecking::parse("sometimes"), None);
    assert_eq!(
        StrictHostKeyChecking::AcceptNew.as_ssh_value(),
        "accept-new"
    );
}

#[test]
fn etc_placement_parses_server_and_client_forms() {
    assert_eq!(
        ConfigPlacement::parse("--remote-etc-portage", "server:/etc/portage"),
        Ok(ConfigPlacement::Server("/etc/portage".to_string()))
    );
    assert_eq!(
        ConfigPlacement::parse("--remote-etc-portage", "client:/etc/portage"),
        Ok(ConfigPlacement::Client("/etc/portage".to_string()))
    );
    assert!(ConfigPlacement::parse("--remote-etc-portage", "/etc/portage").is_err());
    assert!(ConfigPlacement::parse("--remote-etc-portage", "server:").is_err());
    assert!(ConfigPlacement::parse("--remote-etc-portage", "bizarre").is_err());
}
