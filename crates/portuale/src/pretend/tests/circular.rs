use super::*;

#[test]
fn circular_node_text_matches_portage_package_str() {
    // Backlog #206 S1: real `_emerge/Package.py::Package.__str__`
    // for a merge-bound ebuild — `(cpv:slot/sub::repo, ebuild
    // scheduled for merge)` — byte for byte, grounded on the live
    // n206 probe (`=dev-libs/cyc0b-1`, `dev-libs/hardcyclea`,
    // `dev-libs/usecyclea`, all `ROOT=$FX` with no `to` suffix).
    let nc = Colorizer::new(false);
    assert_eq!(
        circular_node_text(CircularNodeTextArgs {
            cpv: "dev-libs/cyc0b-1",
            slot: "0",
            sub_slot: "0",
            repo: "testrepo",
            type_name: "ebuild",
            color: &nc
        }),
        "(dev-libs/cyc0b-1:0/0::testrepo, ebuild scheduled for merge)"
    );
    // A non-0 slot renders `:slot/sub_slot`, like real
    // `_append_slot`-less `__str__` (which always prints both).
    assert_eq!(
        circular_node_text(CircularNodeTextArgs {
            cpv: "dev-libs/slotted-2.0",
            slot: "1",
            sub_slot: "2",
            repo: "testrepo",
            type_name: "ebuild",
            color: &nc
        }),
        "(dev-libs/slotted-2.0:1/2::testrepo, ebuild scheduled for merge)"
    );
    // A binary merge says `binary`, like real `type_name`.
    assert_eq!(
        circular_node_text(CircularNodeTextArgs {
            cpv: "dev-libs/binpkg-1.0",
            slot: "0",
            sub_slot: "0",
            repo: "testrepo",
            type_name: "binary",
            color: &nc
        }),
        "(dev-libs/binpkg-1.0:0/0::testrepo, binary scheduled for merge)"
    );
    // Under `--color y` real wraps the `cpv:slot/sub::repo` head in
    // `PKG_MERGE` (`PKG_BINARY_MERGE` for a binary merge); the
    // `, <type> scheduled for merge)` tail stays plain.
    let yc = Colorizer::new(true);
    assert_eq!(
        circular_node_text(CircularNodeTextArgs {
            cpv: "dev-libs/cyc0b-1",
            slot: "0",
            sub_slot: "0",
            repo: "testrepo",
            type_name: "ebuild",
            color: &yc
        }),
        "(\x1b[32mdev-libs/cyc0b-1:0/0::testrepo\x1b[39;49;00m, ebuild scheduled for merge)"
    );
    assert_eq!(
        circular_node_text(CircularNodeTextArgs {
            cpv: "dev-libs/binpkg-1.0",
            slot: "0",
            sub_slot: "0",
            repo: "testrepo",
            type_name: "binary",
            color: &yc
        }),
        "(\x1b[35mdev-libs/binpkg-1.0:0/0::testrepo\x1b[39;49;00m, binary scheduled for merge)"
    );
}

#[test]
fn edge_use_tail_colours_flags_like_portage_3_0_83() {
    // Portage `_edge_description`: enabled flags red, disabled ones blue
    // with a `-`, sorted, after `, USE=` (shapes 07 and 09).
    let flags = vec![("nls".to_string(), true), ("system-b".to_string(), false)];
    assert_eq!(
        edge_use_tail(&flags, &Colorizer::new(false)),
        ", USE=nls -system-b"
    );
    assert_eq!(
        edge_use_tail(&flags, &Colorizer::new(true)),
        ", USE=\x1b[31;01mnls\x1b[39;49;00m \x1b[34;01m-system-b\x1b[39;49;00m"
    );
    assert_eq!(edge_use_tail(&[], &Colorizer::new(true)), "");
}

/// Python's `json.dumps(report, indent=2)`: empty containers print inline,
/// nested ones indent by two, non-ASCII is escaped, booleans are lower case.
#[test]
fn circular_json_prints_like_python_json_dumps() {
    let report = portage_repo::CycleReport {
        cycles: vec![vec!["a/b-1".into(), "a/c-1".into()], vec![]],
        shortest_cycle: vec![portage_repo::CycleReportEdge {
            parent: "a/c-1".into(),
            child: "a/b-1".into(),
            priority: "buildtime".into(),
            affecting_use: vec!["x".into()],
        }],
        solutions: vec![(
            "a/c-1".into(),
            vec![vec![("x".into(), false), ("y".into(), true)]],
        )],
        test_dep_parents: vec![],
        masked_alternatives: vec![("a/b-1".into(), vec!["a/\u{e9}\"-1".into()])],
        search_truncated: vec![],
        max_affecting_use: 10,
    };
    let expected = r#"{
  "cycles": [
    [
      "a/b-1",
      "a/c-1"
    ],
    []
  ],
  "shortest_cycle": [
    {
      "parent": "a/c-1",
      "child": "a/b-1",
      "priority": "buildtime",
      "affecting_use": [
        "x"
      ]
    }
  ],
  "solutions": [
    {
      "package": "a/c-1",
      "use_changes": [
        {
          "x": false,
          "y": true
        }
      ]
    }
  ],
  "test_dep_parents": [],
  "masked_alternatives": [
    {
      "package": "a/b-1",
      "alternatives": [
        "a/\u00e9\"-1"
      ]
    }
  ],
  "search_truncated": [],
  "max_affecting_use": 10
}
"#;
    assert_eq!(render_circular_json(&report), expected);
}
