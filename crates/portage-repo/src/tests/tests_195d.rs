/// Backlog #195, round 4 (g195d Important-1): operator-level pins for
/// [`violated_parent_flags`] against real `Atom.violated_conditionals`
/// (`lib/portage/dep/__init__.py:1465`, branches `:1525-1576`).
///
/// Oracle: `LLM/evidence/2026-09-28-g195/g195d-oracle-output.txt` (a
/// host run of real Portage 3.0.82.2 on these same inputs via
/// `g195d-oracle-probe.py`). Each row's expectation is the oracle's
/// `arm` column — the parent arm's composed end-to-end outcome, not
/// just the in-isolation `conditional` hit: `SKIP-missing-iuse` rows
/// (invalid defaultless conditionals, which real never calls the
/// function for — `depgraph.py:6721-6727`) and `FAIL-gate` rows both
/// expect `[]`, exactly what the port returns through its poison and
/// concrete-gate paths.
use crate::*;

fn set_195d(flags: &[&str]) -> HashSet<String> {
    flags.iter().map(|s| s.to_string()).collect()
}

/// Arguments of [`cell_195d`].
struct Cell195dArgs<'a> {
    token: &'a str,
    valid: bool,
    p: bool,
    c: bool,
    hit: bool,
}

/// One single-token oracle cell: `token` is the full `[...]` entry
/// (e.g. `"foo(+)?"`), `valid` whether `foo` is in child IUSE, `p` /
/// `c` the parent/child USE bits. `hit` is the oracle `arm` column
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
    // Real `Package.py:728-731` filters child USE to valid IUSE, so
    // an invalid flag is never set -- the same contract
    // `violated_parent_flags` documents.
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
/// 1-72; invalid rows only carry C=0, since filtered child USE
/// never holds an invalid flag).
#[test]
fn violated_parent_flags_matches_portage_across_the_oracle_matrix() {
    // `flag?` -- real `:1525-1531`. Valid: violated iff P && !C.
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
    // Invalid: parent-lacks is always satisfied; parent-has
    // violates only for `(-)` (defaultless poisons via the
    // `missing_iuse`-equivalent path).
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
    // `flag=` -- real `:1532-1549`. Valid: violated iff P != C.
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
    // parent lacks the flag (the `missing_enabled` sub-branch);
    // `(-)` violates only when the parent has it.
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
    // `flag!=` -- real `:1550-1567`.
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
    // Invalid `(+)`: parent-has-child-lacks violates (the review's
    // corner: real's `missing_enabled` sub-branch); parent-lacks-
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
    // `!flag?` -- real `:1568-1576`.
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

/// Arguments of [`gate_195d`].
struct Gate195dArgs<'a> {
    cond_tok: &'a str,
    uncond_tok: &'a str,
    bar_valid: bool,
    bar_use: bool,
    hit: bool,
}

/// The concrete gate on two-token atoms (oracle gate cells G1-G10):
/// a valid parent-active conditional plus one unconditional token.
/// `bar_valid` / `bar_use` mirror the oracle's columns; `hit` is
/// the oracle `arm` column.
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
    // hit alone (oracle-verified `[foo]`, gate PASS).
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
    // fail real's gate (G3, G4, G9, G10); the port's valid-only
    // gate used to let them through.
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
    // Invalid defaultless unconditionals poison via `required`
    // (G7, G8 -- real skips the arm via `missing_iuse`).
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
