/// Operator-level pins for [`violated_parent_flags`] against
/// Portage's `Atom.violated_conditionals` (oracle: host Portage run on
/// these same inputs; each expectation is the oracle `arm` column).
use crate::*;

fn set_195d(flags: &[&str]) -> HashSet<String> {
    flags.iter().map(|s| s.to_string()).collect()
}

struct Cell195dArgs<'a> {
    token: &'a str,
    valid: bool,
    p: bool,
    c: bool,
    hit: bool,
}

/// One single-token oracle cell: `hit` is the oracle `arm` column
/// (`true` = `[foo]`, `false` = `[]`).
fn cell_195d(args: Cell195dArgs<'_>) {
    let Cell195dArgs {
        token,
        valid,
        p,
        c,
        hit,
    } = args;
    let atom =
        portage_dep::parse_atom(&format!("dev-libs/child[{token}]")).expect("oracle token parses");
    let child_valid = if valid {
        set_195d(&["foo"])
    } else {
        set_195d(&["other"])
    };
    // Child USE is filtered to valid IUSE, like Portage.
    // Portage: lib/_emerge/Package.py
    let child_use = if c && valid {
        set_195d(&["foo"])
    } else {
        HashSet::new()
    };
    let parent_use = if p {
        set_195d(&["foo"])
    } else {
        HashSet::new()
    };
    let want: Vec<String> = if hit { vec!["foo".to_string()] } else { vec![] };
    assert_eq!(
        violated_parent_flags(&atom, &parent_use, &child_use, &child_valid),
        want,
        "token={token} valid={valid} P={p} C={c}"
    );
}

/// Full operator x validity x default x P x C matrix (oracle rows
/// 1-72; invalid rows only carry C=0).
#[test]
fn violated_parent_flags_matches_portage_across_the_oracle_matrix() {
    // `flag?`: violated iff P && !C.
    for tok in ["foo?", "foo(+)?", "foo(-)?"] {
        for p in [false, true] {
            for c in [false, true] {
                cell_195d(Cell195dArgs {
                    token: tok,
                    valid: true,
                    p,
                    c,
                    hit: p && !c,
                });
            }
        }
    }
    // Invalid: parent-lacks is always satisfied; parent-has violates
    // only for `(-)`.
    for tok in ["foo?", "foo(+)?", "foo(-)?"] {
        cell_195d(Cell195dArgs {
            token: tok,
            valid: false,
            p: false,
            c: false,
            hit: false,
        });
    }
    cell_195d(Cell195dArgs {
        token: "foo?",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "foo(+)?",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "foo(-)?",
        valid: false,
        p: true,
        c: false,
        hit: true,
    });
    // `flag=`: violated iff P != C.
    for tok in ["foo=", "foo(+)=", "foo(-)="] {
        for p in [false, true] {
            for c in [false, true] {
                cell_195d(Cell195dArgs {
                    token: tok,
                    valid: true,
                    p,
                    c,
                    hit: p != c,
                });
            }
        }
    }
    // Invalid: defaultless poisons; `(+)` violates only when the
    // parent lacks the flag; `(-)` only when the parent has it.
    for tok in ["foo=", "foo(+)=", "foo(-)="] {
        // Only the `(+)` parent-lacks row violates (oracle `= invalid plus 0 0`).
        cell_195d(Cell195dArgs {
            token: tok,
            valid: false,
            p: false,
            c: false,
            hit: tok == "foo(+)=",
        });
    }
    cell_195d(Cell195dArgs {
        token: "foo=",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "foo(+)=",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "foo(-)=",
        valid: false,
        p: true,
        c: false,
        hit: true,
    });
    // `flag!=`.
    for p in [false, true] {
        for c in [false, true] {
            cell_195d(Cell195dArgs {
                token: "!foo=",
                valid: true,
                p,
                c,
                hit: p == c,
            });
            cell_195d(Cell195dArgs {
                token: "!foo(+)=",
                valid: true,
                p,
                c,
                hit: p == c,
            });
            cell_195d(Cell195dArgs {
                token: "!foo(-)=",
                valid: true,
                p,
                c,
                hit: p == c,
            });
        }
    }
    cell_195d(Cell195dArgs {
        token: "!foo=",
        valid: false,
        p: false,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "!foo=",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    // Invalid `(+)`: parent-has-child-lacks violates; parent-lacks-
    // child-lacks is satisfied.
    cell_195d(Cell195dArgs {
        token: "!foo(+)=",
        valid: false,
        p: false,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "!foo(+)=",
        valid: false,
        p: true,
        c: false,
        hit: true,
    });
    cell_195d(Cell195dArgs {
        token: "!foo(-)=",
        valid: false,
        p: false,
        c: false,
        hit: true,
    });
    cell_195d(Cell195dArgs {
        token: "!foo(-)=",
        valid: false,
        p: true,
        c: false,
        hit: false,
    });
    // `!flag?`.
    for p in [false, true] {
        for c in [false, true] {
            cell_195d(Cell195dArgs {
                token: "!foo?",
                valid: true,
                p,
                c,
                hit: !p && c,
            });
            cell_195d(Cell195dArgs {
                token: "!foo(+)?",
                valid: true,
                p,
                c,
                hit: !p && c,
            });
            cell_195d(Cell195dArgs {
                token: "!foo(-)?",
                valid: true,
                p,
                c,
                hit: !p && c,
            });
        }
    }
    for tok in ["!foo?", "!foo(+)?", "!foo(-)?"] {
        cell_195d(Cell195dArgs {
            token: tok,
            valid: false,
            p: true,
            c: false,
            hit: false,
        });
    }
    cell_195d(Cell195dArgs {
        token: "!foo?",
        valid: false,
        p: false,
        c: false,
        hit: false,
    });
    cell_195d(Cell195dArgs {
        token: "!foo(+)?",
        valid: false,
        p: false,
        c: false,
        hit: true,
    });
    cell_195d(Cell195dArgs {
        token: "!foo(-)?",
        valid: false,
        p: false,
        c: false,
        hit: false,
    });
}

struct Gate195dArgs<'a> {
    cond_tok: &'a str,
    uncond_tok: &'a str,
    bar_valid: bool,
    bar_use: bool,
    hit: bool,
}

/// The concrete gate on two-token atoms (oracle gate cells G1-G10):
/// a valid parent-active conditional plus one unconditional token.
fn gate_195d(args: Gate195dArgs<'_>) {
    let Gate195dArgs {
        cond_tok,
        uncond_tok,
        bar_valid,
        bar_use,
        hit,
    } = args;
    let atom = portage_dep::parse_atom(&format!("dev-libs/child[{cond_tok},{uncond_tok}]"))
        .expect("oracle gate atom parses");
    let mut valid = set_195d(&["foo"]);
    if bar_valid {
        valid.insert("bar".to_string());
    } else {
        valid.insert("other".to_string());
    }
    let mut child_use = HashSet::new();
    if bar_use && bar_valid {
        child_use.insert("bar".to_string());
    }
    let parent_use = set_195d(&["foo"]);
    let want: Vec<String> = if hit { vec!["foo".to_string()] } else { vec![] };
    assert_eq!(
        violated_parent_flags(&atom, &parent_use, &child_use, &valid),
        want,
        "cond={cond_tok} uncond={uncond_tok} bar_valid={bar_valid} bar_use={bar_use}"
    );
}

#[test]
fn violated_parent_flags_concrete_gate_matches_portage() {
    // Controls: satisfied unconditional tokens leave the parent:foo
    // hit alone.
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "bar",
        bar_valid: true,
        bar_use: true,
        hit: true,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "-bar",
        bar_valid: true,
        bar_use: false,
        hit: true,
    });
    // Valid concrete violations fail the gate (G1, G2).
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "bar",
        bar_valid: true,
        bar_use: false,
        hit: false,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "-bar",
        bar_valid: true,
        bar_use: true,
        hit: false,
    });
    // Invalid `(-)`-assumed `bar` / invalid `(+)`-assumed `-bar`
    // fail the gate (G3, G4, G9, G10).
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "bar(-)",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "-bar(+)",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo=",
        uncond_tok: "bar(-)",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo=",
        uncond_tok: "-bar(+)",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
    // The satisfied invalid-defaulted mirrors pass (G5, G6).
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "bar(+)",
        bar_valid: false,
        bar_use: false,
        hit: true,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "-bar(-)",
        bar_valid: false,
        bar_use: false,
        hit: true,
    });
    // Invalid defaultless unconditionals poison (G7, G8).
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "bar",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
    gate_195d(Gate195dArgs {
        cond_tok: "foo?",
        uncond_tok: "-bar",
        bar_valid: false,
        bar_use: false,
        hit: false,
    });
}
