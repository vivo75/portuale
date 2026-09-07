use super::*;

#[test]
fn clock_gate_bounds_skew_and_disables_at_zero() {
    assert_eq!(clock_gate(Some("1000"), 1000, 900), None);
    assert_eq!(clock_gate(Some("100"), 1000, 900), None);
    assert_eq!(
        clock_gate(Some("99"), 1000, 900),
        Some("client/server clocks differ by 901s (limit 900s)".to_string())
    );
    // `0` disables the check entirely.
    assert_eq!(clock_gate(Some("1"), 1_000_000, 0), None);
    // Unparsable client time fails open here (the bash gate already
    // failed -- no CLIENT_TIME means the script never ran right).
    assert_eq!(clock_gate(None, 1000, 900), None);
    assert_eq!(clock_gate(Some("soon"), 1000, 900), None);
}

#[test]
fn preflight_gates_catch_old_bash_missing_tools_and_roots() {
    let mut ok = HashMap::new();
    for (k, v) in [
        ("BASH_MAJOR", "5"),
        ("BASH_MINOR", "3"),
        ("TOOL_tar", "yes"),
        ("TOOL_mkdir", "yes"),
        ("TOOL_rm", "yes"),
        ("TOOL_cat", "yes"),
        ("TOOL_chmod", "yes"),
        ("TOOL_ln", "yes"),
        ("TOOL_find", "yes"),
        ("TOOL_grep", "yes"),
        ("TOOL_sed", "yes"),
        ("TOOL_cmp", "yes"),
        ("TOOL_stat", "yes"),
        ("TOOL_readlink", "yes"),
        ("TOOL_id", "yes"),
        ("TOOL_tail", "yes"),
        ("TOOL_sha256sum", "yes"),
        ("TOOL_mv", "yes"),
        ("TOOL_uname", "yes"),
        ("TOOL_mktemp", "yes"),
        ("ROOT_WRITABLE", "yes"),
        ("VDB_DIR", "yes"),
        ("WORKDIR", "writable"),
    ] {
        ok.insert(k.to_string(), v.to_string());
    }
    assert_eq!(preflight_gates(&ok), (Vec::new(), Vec::new()));

    let mut old = ok.clone();
    old.insert("BASH_MINOR".to_string(), "2".to_string());
    let (failures, _) = preflight_gates(&old);
    assert!(failures.iter().any(|f| f.contains("5.3")), "{failures:?}");

    let mut missing = ok.clone();
    missing.insert("TOOL_tar".to_string(), "no".to_string());
    missing.insert("TOOL_tail".to_string(), "no".to_string());
    missing.insert("TOOL_sha256sum".to_string(), "no".to_string());
    missing.insert("ROOT_WRITABLE".to_string(), "no".to_string());
    missing.insert("WORKDIR".to_string(), "uncreatable".to_string());
    let (failures, _) = preflight_gates(&missing);
    assert_eq!(failures.len(), 5, "{failures:?}");

    // A missing vdb warns, never fails.
    let mut novdb = ok.clone();
    novdb.insert("VDB_DIR".to_string(), "no".to_string());
    let (failures, warnings) = preflight_gates(&novdb);
    assert!(failures.is_empty());
    assert_eq!(warnings.len(), 1);
}
