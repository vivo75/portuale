//! The diagram as Graphviz DOT text.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::levels::{DEPEND, Edges, Levels, RDEPEND};

/// The set a package is drawn as a member of, most important first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Set {
    System,
    Profile,
    World,
    Other,
}

impl Set {
    pub const ALL: [Set; 4] = [Set::System, Set::Profile, Set::World, Set::Other];

    fn name(self) -> &'static str {
        match self {
            Set::System => "@system",
            Set::Profile => "@profile",
            Set::World => "@world",
            Set::Other => "other",
        }
    }

    /// Fill colour plus a border difference that survives a grey printout.
    fn attrs(self) -> &'static str {
        match self {
            Set::System => "fillcolor=\"#f8b4b4\", peripheries=2",
            Set::Profile => "fillcolor=\"#fde2a7\", style=\"filled,rounded\"",
            Set::World => "fillcolor=\"#b7e4c7\", penwidth=2",
            Set::Other => "fillcolor=white",
        }
    }
}

/// Name and look of each dependency kind, in `portage_world::DEP_KEYS` bit order.
const KINDS: [(&str, &str); 5] = [
    ("DEPEND", "color=\"#1f77b4\""),
    ("RDEPEND", "color=\"#2ca02c\", arrowhead=empty"),
    ("PDEPEND", "color=\"#9467bd\", style=dotted"),
    ("BDEPEND", "color=\"#d62728\", style=dashed"),
    ("IDEPEND", "color=\"#ff7f0e\", style=dashed, arrowhead=odot"),
];

/// What `DEPEND` and `RDEPEND` on the same pair draw as: one arrow of two
/// parallel lines, one in each colour. Every other kind, `BDEPEND` included,
/// keeps its own arrow.
const BOTH: (&str, &str) = (
    "DEPEND + RDEPEND",
    "color=\"#1f77b4:#2ca02c\", arrowhead=empty",
);

/// The arrows of one edge, as `(name, look)`: the merged one when both
/// `DEPEND` and `RDEPEND` name the pair, then one per remaining kind.
fn arrows(kinds: u8) -> Vec<(&'static str, &'static str)> {
    let both = kinds & (DEPEND | RDEPEND) == DEPEND | RDEPEND;
    let mut out = Vec::new();
    if both {
        out.push(BOTH);
    }
    for (bit, kind) in KINDS.iter().enumerate() {
        let merged = both && (1 << bit) & (DEPEND | RDEPEND) != 0;
        if kinds & (1 << bit) != 0 && !merged {
            out.push(*kind);
        }
    }
    out
}

/// One package of the diagram.
pub struct Node {
    pub label: String,
    pub set: Set,
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The whole diagram: dependents above their dependencies, one row per
/// build level, one arrow per dependency kind. `nslimit` bounds dot's
/// horizontal placement, which otherwise takes minutes on a few hundred
/// packages.
///
/// With `bundle`, the arrows of one kind that reach the same package from
/// two or more packages join in a small dot just above it and go on as one
/// arrow, so a package everybody needs has one long arrow, not a fan.
pub fn render(
    nodes: &BTreeMap<String, Node>,
    edges: &Edges,
    levels: &Levels,
    bundle: bool,
) -> String {
    let mut out = String::from(
        "digraph dependencies {\n  rankdir=TB;\n  newrank=true;\n  nslimit=4;\n  \
         node [shape=box, style=filled, fontname=\"Helvetica\", fontsize=10];\n  \
         edge [fontname=\"Helvetica\", fontsize=9];\n",
    );
    for (id, node) in nodes {
        let _ = writeln!(
            out,
            "  {} [label={}, {}];",
            quoted(id),
            quoted(&node.label),
            node.set.attrs()
        );
    }
    let mut rows: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
    for (id, level) in &levels.level {
        rows.entry(*level).or_default().insert(id);
    }
    for (level, row) in rows.iter().rev() {
        let ids: Vec<String> = row.iter().map(|id| quoted(id)).collect();
        let _ = writeln!(
            out,
            "  {{ rank=same; /* level {level} */ {}; }}",
            ids.join("; ")
        );
    }
    // (target, arrow name, constraint-free) -> (look, packages arriving).
    let mut arriving: BTreeMap<(&str, &str, bool), (&str, Vec<&str>)> = BTreeMap::new();
    for ((from, to), kinds) in edges {
        let loose = levels.loose.contains(&(from.clone(), to.clone()));
        for (name, look) in arrows(*kinds) {
            arriving
                .entry((to, name, loose))
                .or_insert((look, Vec::new()))
                .1
                .push(from);
        }
    }
    for ((to, name, loose), (look, froms)) in &arriving {
        let loose = if *loose { ", constraint=false" } else { "" };
        if bundle && froms.len() > 1 {
            // Rank: the dot sits level with the lowest of the packages
            // joining it, so the one arrow left is the long part.
            let dot = quoted(&format!("join {to} {name} {loose}"));
            let _ = writeln!(
                out,
                "  {dot} [label=\"\", shape=point, width=0.06, color=\"{}\"];",
                first_color(look)
            );
            for from in froms {
                let _ = writeln!(
                    out,
                    "  {} -> {dot} [{look}{loose}, arrowhead=none, minlen=0];",
                    quoted(from)
                );
            }
            let _ = writeln!(out, "  {dot} -> {} [{look}{loose}];", quoted(to));
        } else {
            for from in froms {
                let _ = writeln!(out, "  {} -> {} [{look}{loose}];", quoted(from), quoted(to));
            }
        }
    }
    legend(&mut out);
    out.push_str("}\n");
    out
}

/// The first colour of a look (`color="#1f77b4:#2ca02c"` gives `#1f77b4`).
fn first_color(look: &str) -> &str {
    look.split("color=\"")
        .nth(1)
        .and_then(|rest| rest.split(['"', ':']).next())
        .unwrap_or("black")
}

fn legend(out: &mut String) {
    out.push_str(
        "  subgraph cluster_legend {\n    label=\"Legend\";\n    fontname=\"Helvetica\";\n",
    );
    for set in Set::ALL {
        let _ = writeln!(
            out,
            "    \"legend {0}\" [label=\"{0}\", {1}];",
            set.name(),
            set.attrs()
        );
    }
    for (name, look) in KINDS.into_iter().chain([BOTH]) {
        let _ = writeln!(
            out,
            "    \"legend {name} from\" [label=\"\", shape=point, width=0.01, style=invis];\n    \
             \"legend {name} to\" [label=\"\", shape=point, width=0.01, style=invis];\n    \
             \"legend {name} from\" -> \"legend {name} to\" [label=\"{name}\", {look}];"
        );
    }
    out.push_str("  }\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::{BDEPEND, DEPEND, PDEPEND, RDEPEND, levels};

    fn diagram() -> String {
        let nodes: BTreeMap<String, Node> = [
            ("a:0", Set::World),
            ("b:0", Set::System),
            ("c:0", Set::Other),
        ]
        .into_iter()
        .map(|(id, set)| {
            (
                id.to_string(),
                Node {
                    label: id.to_string(),
                    set,
                },
            )
        })
        .collect();
        let edges: Edges = [
            ("a:0", "b:0", DEPEND | RDEPEND),
            ("a:0", "c:0", BDEPEND),
            ("b:0", "a:0", PDEPEND),
        ]
        .into_iter()
        .map(|(a, b, k)| ((a.to_string(), b.to_string()), k))
        .collect();
        let l = levels(&nodes.keys().cloned().collect(), &edges);
        render(&nodes, &edges, &l, false)
    }

    #[test]
    fn packages_of_one_level_share_a_row_and_dependents_come_first() {
        let dot = diagram();
        let top = dot.find("/* level 1 */ \"a:0\";").expect("level 1 row");
        let bottom = dot
            .find("/* level 0 */ \"b:0\"; \"c:0\";")
            .expect("level 0 row");
        assert!(top < bottom);
    }

    #[test]
    fn the_other_kinds_each_get_their_own_arrow() {
        let dot = diagram();
        assert!(dot.contains("\"a:0\" -> \"c:0\" [color=\"#d62728\", style=dashed];"));
        assert!(
            dot.contains("\"b:0\" -> \"a:0\" [color=\"#9467bd\", style=dotted, constraint=false];")
        );
    }

    #[test]
    fn depend_and_rdepend_on_one_pair_merge_into_one_two_colour_arrow() {
        let dot = diagram();
        let arrow = "\"a:0\" -> \"b:0\" [color=\"#1f77b4:#2ca02c\", arrowhead=empty];";
        assert_eq!(dot.matches(arrow).count(), 1);
        assert!(!dot.contains("\"a:0\" -> \"b:0\" [color=\"#1f77b4\"];"));
        assert!(!dot.contains("\"a:0\" -> \"b:0\" [color=\"#2ca02c\", arrowhead=empty];"));
    }

    #[test]
    fn depend_and_bdepend_stay_separate_arrows_with_different_colours() {
        let nodes: BTreeMap<String, Node> = ["a:0", "b:0"]
            .into_iter()
            .map(|id| {
                (
                    id.to_string(),
                    Node {
                        label: id.to_string(),
                        set: Set::Other,
                    },
                )
            })
            .collect();
        let edges: Edges = [(("a:0".to_string(), "b:0".to_string()), DEPEND | BDEPEND)].into();
        let dot = render(
            &nodes,
            &edges,
            &levels(&nodes.keys().cloned().collect(), &edges),
            false,
        );
        assert!(dot.contains("\"a:0\" -> \"b:0\" [color=\"#1f77b4\"];"));
        assert!(dot.contains("\"a:0\" -> \"b:0\" [color=\"#d62728\", style=dashed];"));
    }

    #[test]
    fn bundling_joins_the_arrows_of_one_kind_into_one_package_and_only_those() {
        let nodes: BTreeMap<String, Node> = ["a:0", "b:0", "c:0"]
            .into_iter()
            .map(|id| {
                let node = Node {
                    label: id.to_string(),
                    set: Set::Other,
                };
                (id.to_string(), node)
            })
            .collect();
        // `a` and `b` both build-depend on `c`; `a` also runs `b`.
        let edges: Edges = [
            (("a:0".to_string(), "c:0".to_string()), BDEPEND),
            (("b:0".to_string(), "c:0".to_string()), BDEPEND),
            (("a:0".to_string(), "b:0".to_string()), RDEPEND),
        ]
        .into();
        let l = levels(&nodes.keys().cloned().collect(), &edges);
        let plain = render(&nodes, &edges, &l, false);
        let bundled = render(&nodes, &edges, &l, true);
        assert!(!plain.contains("join"));
        // Two arrows meet in a dot, which has the single arrow on to `c`.
        assert_eq!(bundled.matches("-> \"join c:0 BDEPEND \" [").count(), 2);
        assert_eq!(
            bundled.matches("\"join c:0 BDEPEND \" -> \"c:0\"").count(),
            1
        );
        // A kind with one arrow is left alone.
        assert!(bundled.contains("\"a:0\" -> \"b:0\" [color=\"#2ca02c\""));
    }

    #[test]
    fn a_package_is_drawn_in_the_style_of_its_set() {
        let dot = diagram();
        assert!(dot.contains("\"b:0\" [label=\"b:0\", fillcolor=\"#f8b4b4\", peripheries=2];"));
        assert!(dot.contains("\"c:0\" [label=\"c:0\", fillcolor=white];"));
    }
}
