use super::*;

#[test]
fn drop_dependents_covers_diamonds_transitively() {
    use std::collections::{HashMap, HashSet};
    let cp = |p: &str| ("dev-libs".to_string(), p.to_string());
    // a <- b <- d, a <- c <- d (diamond): failing a drops b, c, d.
    let dependents: HashMap<(String, String), Vec<(String, String)>> = HashMap::from([
        (cp("a"), vec![cp("b"), cp("c")]),
        (cp("b"), vec![cp("d")]),
        (cp("c"), vec![cp("d")]),
        (cp("d"), vec![]),
    ]);
    let mut skip = HashMap::new();
    drop_dependents(&dependents, &mut skip, cp("a"), "dev-libs/a-1.0");
    assert_eq!(
        skip.keys().collect::<HashSet<_>>(),
        [cp("b"), cp("c"), cp("d")].iter().collect::<HashSet<_>>()
    );
    assert_eq!(skip[&cp("d")], "dev-libs/a-1.0");
    // Unrelated roots never join.
    let mut skip = HashMap::new();
    drop_dependents(&dependents, &mut skip, cp("b"), "dev-libs/b-1.0");
    assert!(skip.contains_key(&cp("d")));
    assert!(!skip.contains_key(&cp("c")));
}
