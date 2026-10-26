//! Shared dependency-graph helpers.

/// Tarjan's strongly connected components over `adj` (node -> successors),
/// iterative so a long dependency chain cannot exhaust the stack. Returns the
/// component number of every node; nodes share a number iff they lie on a
/// common cycle. Portage: `digraph.strongly_connected_components()`.
pub fn strongly_connected_components(adj: &[Vec<usize>]) -> Vec<usize> {
    const UNVISITED: usize = usize::MAX;
    let n = adj.len();
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNVISITED; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0usize;
    let mut next_comp = 0usize;
    for root in 0..n {
        if index[root] != UNVISITED {
            continue;
        }
        // (node, index of the next successor to look at)
        let mut work: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, next_edge)) = work.last() {
            if next_edge < adj[v].len() {
                work.last_mut().expect("non-empty work stack").1 += 1;
                let w = adj[v][next_edge];
                if index[w] == UNVISITED {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    comp[w] = next_comp;
                    if w == v {
                        break;
                    }
                }
                next_comp += 1;
            }
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
        }
    }
    comp
}
