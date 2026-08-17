use super::*;

use portage_util::TempDir;

fn tokens(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn plain_edge(atom: &str, category: &str, package: &str, priority: DepPriority) -> DepEdge {
    DepEdge {
        atom: atom.to_string(),
        evaluated: atom.to_string().clone(),
        category: category.to_string(),
        package: package.to_string(),
        priority,
        disjunctive: false,
        alt: None,
        key: 3,
    }
}
