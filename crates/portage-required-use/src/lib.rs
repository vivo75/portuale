// Rust port of Portage's `portage.dep.check_required_use`: the
// REQUIRED_USE (PMS 7.3.4/8.2) evaluator.
//
// A leaf is `flag` or `!flag`; a group is a bare all-of (`( item+ )`,
// the top level itself is one), an any-of (`||`), an exactly-one-of
// (`^^`), an at-most-one-of (`??`), or a use-conditional
// (`[!]flag? ( item+ )`). No `=`/`!=`/`?=` forms exist here (those are
// USE-dep, atom-only forms).
//
// [`check_required_use`] evaluates directly; [`unsatisfied_reduced`]
// additionally builds the still-unsatisfied subtree Portage's depgraph
// prints. Not EAPI-parametrized: `??` and empty-group handling follow
// EAPI 7+ rules unconditionally. A referenced flag missing from IUSE is
// an error.
// Portage: lib/portage/dep/__init__.py, check_required_use

use std::collections::HashSet;

/// Error evaluating a REQUIRED_USE string. `Display` reproduces Portage's
/// messages byte-for-byte.
// Portage: lib/portage/dep/__init__.py, is_active
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// `USE flag '{flag}' is not in IUSE`
    UndeclaredFlag { flag: String },
    /// `malformed syntax: operator/conditional not followed by '('`
    MissingOpenParen,
    /// `malformed syntax: unbalanced parentheses`
    UnbalancedParens,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UndeclaredFlag { flag } => write!(f, "USE flag '{flag}' is not in IUSE"),
            Error::MissingOpenParen => {
                write!(
                    f,
                    "malformed syntax: operator/conditional not followed by '('"
                )
            }
            Error::UnbalancedParens => write!(f, "malformed syntax: unbalanced parentheses"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

/// Whether a REQUIRED_USE leaf token (`flag`, or `!flag` for negation)
/// is satisfied against `enabled` (effective USE) and `iuse` (declared
/// IUSE). `Err` if the flag is not a declared IUSE flag.
fn is_active(
    token: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<bool, Error> {
    let (flag, negated) = match token.strip_prefix('!') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    if flag.is_empty() || !iuse.contains(flag) {
        return Err(Error::UndeclaredFlag {
            flag: flag.to_string(),
        });
    }
    Ok(enabled.contains(flag) != negated)
}

/// Rewrites the `||`/`^^`/`??` operators of a REQUIRED_USE string to
/// their human-readable spellings, as Portage's depgraph does before
/// printing.
// Portage: lib/portage/dep/__init__.py, human_readable_required_use
pub fn human_readable(required_use: &str) -> String {
    required_use
        .replace("^^", "exactly-one-of")
        .replace("||", "any-of")
        .replace("??", "at-most-one-of")
}

// --- reduced "unsatisfied subset" expression -------------------------
//
// Builds Portage's `_RequiredUseBranch` tree while evaluating and renders
// only the still-unsatisfied portion (`tounicode`). The evaluation is
// duplicated from `check_required_use` above rather than shared; the two
// are cross-checked by this crate's tests and the harness contract.
// Portage: lib/portage/dep/__init__.py, check_required_use

const VALID_OPERATORS: [&str; 3] = ["||", "^^", "??"];

#[derive(Debug)]
enum RuNode {
    Leaf {
        token: String,
        satisfied: bool,
    },
    Branch {
        operator: Option<String>,
        children: Vec<usize>,
        satisfied: bool,
        parent: Option<usize>,
    },
}

#[derive(Debug)]
enum StackItem {
    Bool(bool),
    Op(String),
}

struct RuTree {
    nodes: Vec<RuNode>,
}

impl RuTree {
    fn push_branch(&mut self, operator: Option<String>, parent: Option<usize>) -> usize {
        self.nodes.push(RuNode::Branch {
            operator,
            children: Vec::new(),
            satisfied: false,
            parent,
        });
        self.nodes.len() - 1
    }

    fn push_leaf(&mut self, token: &str, satisfied: bool) -> usize {
        self.nodes.push(RuNode::Leaf {
            token: token.to_string(),
            satisfied,
        });
        self.nodes.len() - 1
    }

    fn is_branch(&self, i: usize) -> bool {
        matches!(self.nodes[i], RuNode::Branch { .. })
    }

    fn operator(&self, i: usize) -> Option<String> {
        match &self.nodes[i] {
            RuNode::Branch { operator, .. } => operator.clone(),
            RuNode::Leaf { .. } => None,
        }
    }

    fn parent(&self, i: usize) -> Option<usize> {
        match &self.nodes[i] {
            RuNode::Branch { parent, .. } => *parent,
            RuNode::Leaf { .. } => None,
        }
    }

    fn set_parent(&mut self, i: usize, p: Option<usize>) {
        if let RuNode::Branch { parent, .. } = &mut self.nodes[i] {
            *parent = p;
        }
    }

    fn set_satisfied(&mut self, i: usize, v: bool) {
        if let RuNode::Branch { satisfied, .. } = &mut self.nodes[i] {
            *satisfied = v;
        }
    }

    fn satisfied(&self, i: usize) -> bool {
        match &self.nodes[i] {
            RuNode::Branch { satisfied, .. } | RuNode::Leaf { satisfied, .. } => *satisfied,
        }
    }

    fn children(&self, i: usize) -> Vec<usize> {
        match &self.nodes[i] {
            RuNode::Branch { children, .. } => children.clone(),
            RuNode::Leaf { .. } => Vec::new(),
        }
    }

    fn push_child(&mut self, branch: usize, child: usize) {
        if let RuNode::Branch { children, .. } = &mut self.nodes[branch] {
            children.push(child);
        }
    }

    fn pop_child(&mut self, branch: usize) -> Option<usize> {
        if let RuNode::Branch { children, .. } = &mut self.nodes[branch] {
            children.pop()
        } else {
            None
        }
    }

    /// Renders only the still-unsatisfied portion: a leaf is its own
    /// token; a branch prints its operator + parens (parens only when it
    /// has a parent), and, *unless* inside a `||`/`^^`/`??` ancestor,
    /// drops every already-satisfied child.
    fn tounicode(&self, i: usize) -> String {
        match &self.nodes[i] {
            RuNode::Leaf { token, .. } => token.clone(),
            RuNode::Branch {
                operator,
                children,
                parent,
                ..
            } => {
                let include_parens = parent.is_some();
                let mut tokens: Vec<String> = Vec::new();
                if let Some(op) = operator {
                    tokens.push(op.clone());
                }
                if include_parens {
                    tokens.push("(".to_string());
                }
                // Walk up ancestors; "complex nesting" == some ancestor (or
                // self) is a ||/^^/?? operator.
                let mut complex_nesting = false;
                let mut node = Some(i);
                while let Some(n) = node {
                    if self
                        .operator(n)
                        .is_some_and(|o| VALID_OPERATORS.contains(&o.as_str()))
                    {
                        complex_nesting = true;
                        break;
                    }
                    node = self.parent(n);
                }
                for &child in children {
                    if complex_nesting || !self.satisfied(child) {
                        tokens.push(self.tounicode(child));
                    }
                }
                if include_parens {
                    tokens.push(")".to_string());
                }
                tokens.join(" ")
            }
        }
    }
}

/// Builds the `_RequiredUseBranch` tree for `required_use` under
/// `enabled`/`iuse`, returning `(tree, root, satisfied)`.
fn build_required_use_tree(
    required_use: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<(RuTree, usize, bool), Error> {
    let is_op = |s: &str| VALID_OPERATORS.contains(&s);
    let is_satisfied = |operator: &str, argument: &[bool]| -> bool {
        let tc = argument.iter().filter(|&&b| b).count();
        match operator {
            "||" => tc >= 1,
            "^^" => tc == 1,
            "??" => tc <= 1,
            _ => !argument.contains(&false), // "flag?" -> "False not in argument"
        }
    };

    let tokens: Vec<&str> = required_use.split_whitespace().collect();
    let mut tree = RuTree {
        nodes: vec![RuNode::Branch {
            operator: None,
            children: Vec::new(),
            satisfied: false,
            parent: None,
        }],
    };
    let root = 0usize;
    let mut node = root;
    let mut stack: Vec<Vec<StackItem>> = vec![Vec::new()];
    let mut level = 0usize;
    let mut need_bracket = false;

    for &token in &tokens {
        if token == "(" {
            if !need_bracket {
                let child = tree.push_branch(None, Some(node));
                tree.push_child(node, child);
                node = child;
            }
            need_bracket = false;
            stack.push(Vec::new());
            level += 1;
        } else if token == ")" {
            if need_bracket {
                return Err(Error::MissingOpenParen);
            }
            if level == 0 {
                return Err(Error::UnbalancedParens);
            }
            level -= 1;
            let l: Vec<bool> = stack
                .pop()
                .unwrap()
                .into_iter()
                .map(|it| match it {
                    StackItem::Bool(b) => b,
                    // a dangling operator inside a closed group == malformed
                    StackItem::Op(_) => false,
                })
                .collect();
            let mut op: Option<String> = None;
            let last_is_op = matches!(stack[level].last(), Some(StackItem::Op(s)) if is_op(s));
            let last_is_cond =
                matches!(stack[level].last(), Some(StackItem::Op(s)) if s.ends_with('?'));
            if last_is_op {
                let Some(StackItem::Op(o)) = stack[level].pop() else {
                    unreachable!()
                };
                let sat = is_satisfied(&o, &l);
                stack[level].push(StackItem::Bool(sat));
                tree.set_satisfied(node, sat);
                op = Some(o);
            } else if last_is_cond {
                let Some(StackItem::Op(o)) = stack[level].pop() else {
                    unreachable!()
                };
                op = Some(o.clone());
                if is_active(&o[..o.len() - 1], enabled, iuse)? {
                    let sat = is_satisfied(&o, &l);
                    stack[level].push(StackItem::Bool(sat));
                    tree.set_satisfied(node, sat);
                } else {
                    // inactive use-conditional -> vacuously satisfied; the
                    // group node is dropped from the tree.
                    tree.set_satisfied(node, true);
                    let popped = tree.pop_child(tree.parent(node).unwrap());
                    debug_assert_eq!(popped, Some(node));
                    node = tree.parent(node).unwrap();
                    continue;
                }
            }

            if op.is_none() {
                let sat = !l.contains(&false);
                tree.set_satisfied(node, sat);
                if !l.is_empty() {
                    stack[level].push(StackItem::Bool(sat));
                }
                let parent = tree.parent(node).unwrap();
                let parent_op_is_valid = tree.operator(parent).is_some_and(|o| is_op(&o));
                if tree.children(node).len() <= 1 || !parent_op_is_valid {
                    let popped = tree.pop_child(parent);
                    debug_assert_eq!(popped, Some(node));
                    for child in tree.children(node) {
                        tree.push_child(parent, child);
                        if tree.is_branch(child) {
                            tree.set_parent(child, Some(parent));
                        }
                    }
                }
            } else if tree.children(node).is_empty() {
                let popped = tree.pop_child(tree.parent(node).unwrap());
                debug_assert_eq!(popped, Some(node));
            } else if tree.children(node).len() == 1 && op.as_deref().is_some_and(is_op) {
                let parent = tree.parent(node).unwrap();
                let popped = tree.pop_child(parent);
                debug_assert_eq!(popped, Some(node));
                let only = tree.children(node)[0];
                tree.push_child(parent, only);
                if tree.is_branch(only) {
                    tree.set_parent(only, Some(parent));
                    node = only;
                    let np = tree.parent(node).unwrap();
                    let np_op_valid = tree.operator(np).is_some_and(|o| is_op(&o));
                    if tree.operator(node).is_none() && !np_op_valid {
                        let popped = tree.pop_child(np);
                        debug_assert_eq!(popped, Some(node));
                        for child in tree.children(node) {
                            tree.push_child(np, child);
                            if tree.is_branch(child) {
                                tree.set_parent(child, Some(np));
                            }
                        }
                    }
                }
            }

            node = tree.parent(node).unwrap_or(root);
        } else if is_op(token) {
            if need_bracket {
                return Err(Error::MissingOpenParen);
            }
            need_bracket = true;
            stack[level].push(StackItem::Op(token.to_string()));
            let child = tree.push_branch(Some(token.to_string()), Some(node));
            tree.push_child(node, child);
            node = child;
        } else if need_bracket {
            return Err(Error::MissingOpenParen);
        } else if let Some(cond) = token.strip_suffix('?') {
            let _ = cond;
            need_bracket = true;
            stack[level].push(StackItem::Op(token.to_string()));
            let child = tree.push_branch(Some(token.to_string()), Some(node));
            tree.push_child(node, child);
            node = child;
        } else {
            let sat = is_active(token, enabled, iuse)?;
            stack[level].push(StackItem::Bool(sat));
            let leaf = tree.push_leaf(token, sat);
            tree.push_child(node, leaf);
        }
    }

    if level != 0 || need_bracket {
        return Err(Error::UnbalancedParens);
    }
    let satisfied = !stack[0]
        .iter()
        .any(|it| matches!(it, StackItem::Bool(false)));
    Ok((tree, root, satisfied))
}
