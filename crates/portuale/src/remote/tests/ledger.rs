use super::*;

#[test]
fn ledger_append_keeps_the_last_ten_lines() {
    let dir = TempDir::new("portuale-remote-ledger").keep();
    let path = dir.join("ledger");
    for n in 0..14 {
        ledger_append(&path, &format!("line{n}")).unwrap();
    }
    let kept: Vec<String> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(kept.len(), 10);
    assert_eq!(kept[0], "line4");
    assert_eq!(kept[9], "line13");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn newest_ledger_line_reads_the_last_non_empty_line() {
    assert_eq!(newest_ledger_line(""), None);
    assert_eq!(newest_ledger_line("\n\n"), None);
    assert_eq!(
        newest_ledger_line("10 old 0 pkg\n20 new 1 pkg\n"),
        Some("20 new 1 pkg".to_string())
    );
    assert_eq!(newest_ledger_line("a\nb\n\n"), Some("b".to_string()));
}

#[test]
fn ledger_lines_match_requires_byte_identical_newest_lines() {
    let line = "1700000000 testrepo abc123 dev-libs/x-1.0";
    assert!(ledger_lines_match(None, None));
    assert!(ledger_lines_match(Some(line), Some(line)));
    assert!(!ledger_lines_match(
        Some(line),
        Some("1700000001 testrepo def dev-libs/y-2.0")
    ));
    assert!(!ledger_lines_match(None, Some(line)));
    assert!(!ledger_lines_match(Some(line), None));
}
