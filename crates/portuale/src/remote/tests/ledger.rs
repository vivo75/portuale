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
