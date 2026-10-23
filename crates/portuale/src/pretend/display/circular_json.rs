//! `--circular-deps-report=json`: the circular-dependency report as one JSON
//! document on stdout.

/// A JSON value, printed the way Python's `json.dumps(value, indent=2)` does.
enum Json {
    Str(String),
    Int(usize),
    Bool(bool),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

/// `json.dumps`' string escaping (`ensure_ascii`).
fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
        }
    }
    out.push('"');
}

impl Json {
    fn write(&self, depth: usize, out: &mut String) {
        let pad = |n: usize| "  ".repeat(n);
        match self {
            Json::Str(s) => escape(s, out),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    item.write(depth + 1, out);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push(']');
            }
            Json::Obj(fields) if fields.is_empty() => out.push_str("{}"),
            Json::Obj(fields) => {
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    escape(key, out);
                    out.push_str(": ");
                    value.write(depth + 1, out);
                    out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push('}');
            }
        }
    }
}

fn kv(key: &str, value: Json) -> (String, Json) {
    (key.to_string(), value)
}

fn strings(items: &[String]) -> Json {
    Json::Arr(items.iter().cloned().map(Json::Str).collect())
}

/// The JSON document of `report`, ending in a newline.
// Portage: depgraph.py, _show_circular_deps_json
pub(crate) fn render_circular_json(report: &portage_repo::CycleReport) -> String {
    let edge = |e: &portage_repo::CycleReportEdge| {
        Json::Obj(vec![
            kv("parent", Json::Str(e.parent.clone())),
            kv("child", Json::Str(e.child.clone())),
            kv("priority", Json::Str(e.priority.clone())),
            kv("affecting_use", strings(&e.affecting_use)),
        ])
    };
    let solution = |changes: &Vec<(String, bool)>| {
        Json::Obj(Vec::from_iter(
            changes.iter().map(|(flag, on)| kv(flag, Json::Bool(*on))),
        ))
    };
    let doc = Json::Obj(vec![
        kv(
            "cycles",
            Json::Arr(report.cycles.iter().map(|c| strings(c)).collect()),
        ),
        kv(
            "shortest_cycle",
            Json::Arr(report.shortest_cycle.iter().map(edge).collect()),
        ),
        kv(
            "solutions",
            Json::Arr(
                report
                    .solutions
                    .iter()
                    .map(|(pkg, sols)| {
                        Json::Obj(vec![
                            kv("package", Json::Str(pkg.clone())),
                            kv(
                                "use_changes",
                                Json::Arr(sols.iter().map(solution).collect()),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        kv("test_dep_parents", strings(&report.test_dep_parents)),
        kv(
            "masked_alternatives",
            Json::Arr(
                report
                    .masked_alternatives
                    .iter()
                    .map(|(pkg, alts)| {
                        Json::Obj(vec![
                            kv("package", Json::Str(pkg.clone())),
                            kv("alternatives", strings(alts)),
                        ])
                    })
                    .collect(),
            ),
        ),
        kv("search_truncated", strings(&report.search_truncated)),
        kv("max_affecting_use", Json::Int(report.max_affecting_use)),
    ]);
    let mut out = String::new();
    doc.write(0, &mut out);
    out.push('\n');
    out
}
