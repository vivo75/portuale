// Rust port of real `portage.dep.check_required_use`
// (`lib/portage/dep/__init__.py`, `is_active`/`is_satisfied` plus the
// tokenizer/group-tree builder around them) -- the REQUIRED_USE
// (PMS 7.3.4/8.2) slice from docs/agent-context.md's follow-up work.
//
// PMS 8.2's own grammar for REQUIRED_USE (a "specification style
// variable"): a leaf is `flag` or `!flag`; a group is either a bare
// all-of (`( item+ )`, implicit AND -- REQUIRED_USE's own top level is
// itself an implicit all-of group, no wrapping parens needed), an any-of
// (`|| ( item+ )`), an exactly-one-of (`^^ ( item+ )`), an at-most-one-of
// (`?? ( item+ )` -- EAPI-gated per PMS table 8.5, `eapi >= 5` per real
// `lib/portage/eapi.py`'s own `required_use_at_most_one_of` attribute;
// always recognized here, matching this repo's EAPI 5+ profile floor and
// portuale's established "no EAPI parametrization" precedent
// elsewhere -- see e.g. `atom-harness`'s own scope-cut comment), or a
// use-conditional (`[!]flag? ( item+ )`). No `=`/`!=`/`?=` forms exist in
// REQUIRED_USE at all (those are USE-*dep*, atom-only forms -- see
// `portage-dep`'s own `UseDepOp`; a completely different grammar).
//
// [`check_required_use`] is a direct recursive-descent boolean evaluator
// (`parse_items`); real `check_required_use` instead builds a
// `_RequiredUseBranch` tree while it evaluates and returns that (its
// `__bool__` is the verdict). The two agree on the verdict for every
// input -- verified via the `required-use-harness`/`required_use_
// harness.py` pair driven by `test_required_use_contract.py`, the same
// wraps-the-real-thing pattern `use-reduce-harness` established.
//
// [`unsatisfied_reduced`] DOES port the tree (`build_required_use_tree`
// + `RuTree::tounicode`, the `)` handler's node surgery included): real
// `depgraph.py::_show_unsatisfied_dep` renders `tree.tounicode()` -- the
// minimal still-unsatisfied sub-expression -- for its "The following
// REQUIRED_USE flag constraints are unsatisfied:" line. Also verified
// against real, via the harness `reduce` op.
//
// KNOWN, DOCUMENTED SIMPLIFICATIONS vs. real `check_required_use`:
//   - `empty_groups_always_true` (real `lib/portage/eapi.py`:
//     `eapi <= Eapi("6")`) is never applied: an empty group (`( )`,
//     `|| ( )`, etc -- PMS's own formal grammar actually requires "one or
//     more" items, but real `check_required_use` is more lenient than
//     its own spec here) always falls through to ordinary per-operator
//     evaluation on an empty list (`||`/`^^` unsatisfied, `??`/a
//     use-conditional trivially satisfied) -- real EAPI 7+ behavior,
//     used unconditionally here, another instance of portuale's
//     established "no EAPI parametrization" precedent. The only real
//     divergence this causes: a literal, degenerate `|| ( )` or
//     `^^ ( )` (which no real-world ebuild has a reason to write) would
//     evaluate differently under EAPI <= 6 than it does here.
//   - A referenced flag that isn't a real, declared IUSE flag on the
//     package is an error here, exactly like real `is_active`'s own
//     `iuse_match` check (`InvalidDependString`) -- ported as
//     `Err(Error)`, propagated by `portage-repo` as a fatal error for
//     the whole `--pretend` run, matching real depgraph.py's own
//     REQUIRED_USE-violation severity (see that crate's own doc comment
//     for exactly where and why).

use std::collections::HashSet;

/// Error evaluating a REQUIRED_USE string. Distinct variants mirror the
/// real `lib/portage/package/ebuild/dochat.py` / `is_active` error
/// messages so `Display` reproduces them byte-for-byte.
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

/// Whether a REQUIRED_USE leaf token (`flag`, or `!flag` for negation --
/// real `is_active`) is satisfied against `enabled` (the package's own
/// effective USE) and `iuse` (its own declared IUSE) -- `Err` if the
/// flag (after stripping any leading `!`) isn't a real, declared IUSE
/// flag at all, exactly like real `is_active`'s own `iuse_match` check.
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

/// Consumes tokens starting at `*pos` up to (not including) the next
/// unmatched `)` or end of input, returning each top-level item's own
/// satisfied bool, in order -- real `is_satisfied`'s own `argument` list
/// for whichever group (or the implicit top-level all-of) contains them.
fn parse_items(
    tokens: &[&str],
    pos: &mut usize,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<Vec<bool>, Error> {
    let mut results = Vec::new();
    while *pos < tokens.len() && tokens[*pos] != ")" {
        let tok = tokens[*pos];
        if tok == "(" {
            // A bare all-of group (PMS 8.2's own "all-of" production) --
            // no preceding operator token.
            *pos += 1;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            results.push(!inner.contains(&false));
        } else if tok == "||" || tok == "^^" || tok == "??" {
            *pos += 1;
            expect_open_paren(tokens, pos)?;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            let true_count = inner.iter().filter(|&&b| b).count();
            let satisfied = match tok {
                "||" => true_count > 0,
                "^^" => true_count == 1,
                "??" => true_count <= 1,
                _ => unreachable!(),
            };
            results.push(satisfied);
        } else if let Some(cond) = tok.strip_suffix('?') {
            // A use-conditional group ("[!]flag? ( item+ )") -- cond is
            // "flag" or "!flag", passed to is_active exactly like a leaf
            // token would be, real is_active(op[:-1]).
            *pos += 1;
            expect_open_paren(tokens, pos)?;
            let inner = parse_items(tokens, pos, enabled, iuse)?;
            expect_close_paren(tokens, pos)?;
            let active = is_active(cond, enabled, iuse)?;
            results.push(!active || !inner.contains(&false));
        } else {
            results.push(is_active(tok, enabled, iuse)?);
            *pos += 1;
        }
    }
    Ok(results)
}

fn expect_open_paren(tokens: &[&str], pos: &mut usize) -> Result<(), Error> {
    if tokens.get(*pos) != Some(&"(") {
        return Err(Error::MissingOpenParen);
    }
    *pos += 1;
    Ok(())
}

fn expect_close_paren(tokens: &[&str], pos: &mut usize) -> Result<(), Error> {
    if tokens.get(*pos) != Some(&")") {
        return Err(Error::UnbalancedParens);
    }
    *pos += 1;
    Ok(())
}

/// Checks whether `required_use` (a package's own `REQUIRED_USE` string)
/// is satisfied by `enabled` (its own effective USE) given `iuse` (its
/// own declared IUSE) -- see the module doc comment for the full ported
/// algorithm and its documented simplifications vs. real
/// `check_required_use`. `Err` for either malformed syntax (unbalanced
/// parentheses, an operator/conditional not immediately followed by
/// `(`) or a referenced flag that isn't really declared in `iuse`.
pub fn check_required_use(
    required_use: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<bool, Error> {
    let tokens: Vec<&str> = required_use.split_whitespace().collect();
    let mut pos = 0;
    let results = parse_items(&tokens, &mut pos, enabled, iuse)?;
    if pos != tokens.len() {
        return Err(Error::UnbalancedParens);
    }
    Ok(!results.contains(&false))
}

/// Rewrites the `||`/`^^`/`??` operators of a REQUIRED_USE string to
/// their human-readable spellings -- real `portage.dep.
/// human_readable_required_use` (a plain three-way `str.replace`), which
/// real `depgraph.py::_show_unsatisfied_dep` applies to both the
/// reduced and the complete REQUIRED_USE expression before printing.
pub fn human_readable(required_use: &str) -> String {
    required_use
        .replace("^^", "exactly-one-of")
        .replace("||", "any-of")
        .replace("??", "at-most-one-of")
}

// --- reduced "unsatisfied subset" expression -------------------------
//
// Real `check_required_use` builds a `_RequiredUseBranch` /
// `_RequiredUseLeaf` tree while it evaluates; `bool(tree)` is the verdict
// (what portuale's own `check_required_use` above returns directly), and
// `tree.tounicode()` renders *only the still-unsatisfied portion* --
// real depgraph.py's "The following REQUIRED_USE flag constraints are
// unsatisfied: <this>" line. `unsatisfied_reduced` ports that tree build
// + the exact node-collapsing done in real's `)` handler + `tounicode`.
// The evaluation logic is duplicated from the recursive-descent
// `check_required_use` above rather than shared; the two are cross-checked
// (tree `_satisfied` == `check_required_use`) by the crate's own tests
// and the `required-use-harness` contract.

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

    /// Real `_RequiredUseBranch.tounicode` / `_RequiredUseLeaf.tounicode`
    /// -- a leaf is its own token; a branch prints its operator + parens
    /// (parens only when it has a parent), and, *unless* it is inside a
    /// `||`/`^^`/`??` ancestor, drops every already-satisfied child.
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
                // real: walk up ancestors; "complex nesting" == some
                // ancestor (or self) is a ||/^^/?? operator.
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

/// Builds real's `_RequiredUseBranch` tree for `required_use` under
/// `enabled`/`iuse` and returns `(tree, root, satisfied)`. A faithful
/// port of the tree-building half of real `portage.dep.
/// check_required_use` (the `)` handler's node surgery included).
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
                    // inactive use-conditional -> vacuously satisfied, and
                    // the whole group node is dropped from the tree.
                    tree.set_satisfied(node, true);
                    let popped = tree.pop_child(tree.parent(node).unwrap());
                    debug_assert_eq!(popped, Some(node));
                    node = tree.parent(node).unwrap();
                    continue;
                }
            }

            if op.is_none() {
                // bare all-of group "( ... )"
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
                // empty operator group -> drop it
                let popped = tree.pop_child(tree.parent(node).unwrap());
                debug_assert_eq!(popped, Some(node));
            } else if tree.children(node).len() == 1 && op.as_deref().is_some_and(is_op) {
                // single-child operator group -> replace with its child
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

/// Returns `None` when `required_use` is satisfied, else `Some(reduced)`
/// where `reduced` is real `tree.tounicode()` -- the minimal
/// still-unsatisfied sub-expression (operators NOT yet rewritten; run
/// [`human_readable`] for that). `Err` on the same malformed-syntax /
/// undeclared-flag conditions as [`check_required_use`].
pub fn unsatisfied_reduced(
    required_use: &str,
    enabled: &HashSet<String>,
    iuse: &HashSet<String>,
) -> Result<Option<String>, Error> {
    let (tree, root, satisfied) = build_required_use_tree(required_use, enabled, iuse)?;
    if satisfied {
        return Ok(None);
    }
    Ok(Some(tree.tounicode(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sets(enabled: &[&str], iuse: &[&str]) -> (HashSet<String>, HashSet<String>) {
        (
            enabled.iter().map(|s| s.to_string()).collect(),
            iuse.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn plain_flag_enabled_satisfies_itself() {
        let (enabled, iuse) = sets(&["foo"], &["foo"]);
        assert_eq!(check_required_use("foo", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn plain_flag_disabled_is_unsatisfied() {
        let (enabled, iuse) = sets(&[], &["foo"]);
        assert_eq!(check_required_use("foo", &enabled, &iuse), Ok(false));
    }

    #[test]
    fn negated_flag_disabled_satisfies_itself() {
        let (enabled, iuse) = sets(&[], &["foo"]);
        assert_eq!(check_required_use("!foo", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn undeclared_flag_is_an_error() {
        let (enabled, iuse) = sets(&[], &[]);
        assert!(check_required_use("foo", &enabled, &iuse).is_err());
    }

    #[test]
    fn any_of_needs_at_least_one() {
        let (enabled, iuse) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("|| ( a b )", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("|| ( a b )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn exactly_one_of_rejects_zero_and_two() {
        let (_, iuse) = sets(&[], &["a", "b"]);
        let (none, _) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &none, &iuse), Ok(false));
        let (both, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &both, &iuse), Ok(false));
        let (one, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("^^ ( a b )", &one, &iuse), Ok(true));
    }

    #[test]
    fn at_most_one_of_accepts_zero_and_one_rejects_two() {
        let (_, iuse) = sets(&[], &["a", "b"]);
        let (none, _) = sets(&[], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &none, &iuse), Ok(true));
        let (one, _) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &one, &iuse), Ok(true));
        let (both, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("?? ( a b )", &both, &iuse), Ok(false));
    }

    #[test]
    fn use_conditional_group_only_applies_when_the_flag_is_active() {
        let (enabled, iuse) = sets(&["foo"], &["foo", "bar"]);
        // foo active -> bar must be enabled too, but it isn't.
        assert_eq!(
            check_required_use("foo? ( bar )", &enabled, &iuse),
            Ok(false)
        );
        let (enabled, _) = sets(&[], &["foo", "bar"]);
        // foo inactive -> the whole group is trivially satisfied.
        assert_eq!(
            check_required_use("foo? ( bar )", &enabled, &iuse),
            Ok(true)
        );
    }

    #[test]
    fn negated_conditional_flag() {
        let (enabled, iuse) = sets(&[], &["foo", "bar"]);
        // !foo active (foo is disabled) -> bar must be enabled too.
        assert_eq!(
            check_required_use("!foo? ( bar )", &enabled, &iuse),
            Ok(false)
        );
    }

    #[test]
    fn bare_all_of_group_is_implicit_and() {
        let (enabled, iuse) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("( a b )", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("( a b )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn top_level_is_an_implicit_all_of_with_no_wrapping_parens_needed() {
        let (enabled, iuse) = sets(&["a"], &["a", "b"]);
        assert_eq!(check_required_use("a b", &enabled, &iuse), Ok(false));
        let (enabled, _) = sets(&["a", "b"], &["a", "b"]);
        assert_eq!(check_required_use("a b", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn nested_groups() {
        let (enabled, iuse) = sets(&["foo", "a"], &["foo", "a", "b", "c"]);
        // foo active -> exactly one of (a, ^^(b c)) -- a is enabled, b/c
        // aren't, so the inner ^^ is unsatisfied (0 true) and the outer
        // || needs at least one true among [a=true, ^^=false] -> true.
        assert_eq!(
            check_required_use("foo? ( || ( a ^^ ( b c ) ) )", &enabled, &iuse),
            Ok(true)
        );
    }

    #[test]
    fn empty_group_uses_ordinary_operator_evaluation_not_vacuous_true() {
        // Documented simplification: no EAPI<=6 "empty groups always
        // true" special case -- see the module doc comment.
        let (enabled, iuse) = sets(&[], &[]);
        assert_eq!(check_required_use("|| ( )", &enabled, &iuse), Ok(false));
        assert_eq!(check_required_use("?? ( )", &enabled, &iuse), Ok(true));
    }

    #[test]
    fn unbalanced_parens_is_an_error() {
        let (enabled, iuse) = sets(&[], &["a"]);
        assert!(check_required_use("( a", &enabled, &iuse).is_err());
        assert!(check_required_use("a )", &enabled, &iuse).is_err());
    }

    #[test]
    fn operator_not_followed_by_open_paren_is_an_error() {
        let (enabled, iuse) = sets(&[], &["a"]);
        assert!(check_required_use("||", &enabled, &iuse).is_err());
        assert!(check_required_use("|| a", &enabled, &iuse).is_err());
        assert!(check_required_use("foo?", &enabled, &iuse).is_err());
    }

    #[test]
    fn human_readable_rewrites_the_three_operators() {
        assert_eq!(
            human_readable("^^ ( a b ) || ( c d ) ?? ( e f )"),
            "exactly-one-of ( a b ) any-of ( c d ) at-most-one-of ( e f )"
        );
    }

    #[test]
    fn reduced_is_none_when_satisfied() {
        let (enabled, iuse) = sets(&["a"], &["a", "b"]);
        assert_eq!(unsatisfied_reduced("|| ( a b )", &enabled, &iuse), Ok(None));
    }

    #[test]
    fn reduced_keeps_only_the_unsatisfied_conditional() {
        // The libsdl2 / wine-vanilla shape: a big implicit-AND of
        // `flag? ( ... )` conditionals, only one unsatisfied.
        let ru = "alsa? ( sound ) haptic? ( joystick ) opengl? ( video ) \
                  wayland? ( gles2 ) xscreensaver? ( X )";
        let (enabled, _) = sets(
            &[
                "alsa", "sound", "haptic", "joystick", "opengl", "video", "wayland",
            ],
            &[],
        );
        let iuse: HashSet<String> = [
            "alsa",
            "sound",
            "haptic",
            "joystick",
            "opengl",
            "video",
            "wayland",
            "gles2",
            "xscreensaver",
            "X",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            unsatisfied_reduced(ru, &enabled, &iuse),
            Ok(Some("wayland? ( gles2 )".to_string()))
        );
    }

    #[test]
    fn reduced_top_level_all_of_drops_satisfied_leaves() {
        let (enabled, iuse) = sets(&["a"], &["a", "b", "c"]);
        assert_eq!(
            unsatisfied_reduced("a b c", &enabled, &iuse),
            Ok(Some("b c".to_string()))
        );
    }

    #[test]
    fn reduced_inside_an_operator_keeps_all_children() {
        // real "complex_nesting": under a ||/^^/??, tounicode keeps every
        // child (satisfied or not) so the operator still reads correctly.
        let (enabled, iuse) = sets(&[], &["a", "b"]);
        assert_eq!(
            unsatisfied_reduced("^^ ( a b )", &enabled, &iuse),
            Ok(Some(
                "exactly-one-of ( a b )".replace("exactly-one-of", "^^")
            ))
        );
    }
}
