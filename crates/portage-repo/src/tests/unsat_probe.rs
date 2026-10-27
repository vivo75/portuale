use super::*;

/// Dead-end pass whose `NoVisibleCandidate` entry is an installed
/// owner's unsatisfiable built `:=` dep seeds that owner when a
/// replacement parent carries a satisfiable live `:=` on the same child.
/// Portage: lib/_emerge/depgraph.py, _slot_operator_unsatisfied_probe
#[test]
fn slot_operator_unsatisfied_probe_seeds_a_replacement_parent() {
    use md5::Digest as _;
    // One scratch root per shape: vdb owner(s) plus a repo holding
    // `unsatprov-2` and the requested parent ebuilds.
    fn unsat_base(
        tag: &str,
        vdb: &[(&str, &str)],
        pars: &[(&str, &str, &str)],
    ) -> (TempDir, Vec<RepoConfig>) {
        let base = TempDir::new(&format!("portage-repo-slotop-unsat-{tag}"));
        for (pkg, rdepend) in vdb {
            // EAPI-bearing vdb, like the bed fragment.
            let d = base.join(format!("var/db/pkg/app-misc/{pkg}-1"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("CATEGORY"), "app-misc\n").unwrap();
            fs::write(d.join("SLOT"), "0\n").unwrap();
            fs::write(d.join("repository"), "testrepo\n").unwrap();
            fs::write(d.join("EAPI"), "8\n").unwrap();
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
        let repo = base.join("repo");
        let write_pkg = |pkg: &str, pv: &str, slot: &str, rdepend: &str| {
            let dir = repo.join("app-misc").join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            let body = format!(
                "EAPI=8\nDESCRIPTION=\"215 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"{rdepend}\"\n"
            );
            std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
            let md5 = portage_util::hex_lower(&md5::Md5::digest(body.as_bytes()));
            let entry = format!(
                "DEFINED_PHASES=-\nDESCRIPTION=215 probe\nEAPI=8\nKEYWORDS=amd64\nRDEPEND={rdepend}\nSLOT={slot}\n_md5_={md5}\n"
            );
            let cachedir = repo.join("metadata/md5-cache/app-misc");
            std::fs::create_dir_all(&cachedir).unwrap();
            std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
        };
        write_pkg("unsatprov", "2", "2/2", "");
        for (pkg, pv, slot) in pars {
            write_pkg(pkg, pv, slot, "app-misc/unsatprov:=");
        }
        let repos = vec![RepoConfig {
            name: "testrepo".to_string(),
            location: repo.clone(),
            priority: 0,
            is_main: true,
            masters: vec![],
            profile_formats: vec![],
            cache_formats: vec![],
            aliases: vec![],
            sync_type: None,
            sync_uri: None,
            volatile: false,
            module_specific_options: vec![],
        }];
        (base, repos)
    }
    // The dead-end pass: the provider cp resolves to
    // `NoVisibleCandidate`, pulled by the installed parent's built want.
    type UnsatInputs = (
        Vec<GraphEntry>,
        HashMap<(String, String), Vec<String>>,
        SlotPullers,
    );
    fn unsat_inputs(owners: &[(&str, &str)]) -> UnsatInputs {
        let mut nvc = graph_entry("app-misc", "unsatprov", "2");
        nvc.outcome = PretendOutcome::NoVisibleCandidate;
        nvc.required_by = owners
            .iter()
            .map(|(pkg, _)| ("app-misc".to_string(), (*pkg).to_string()))
            .collect();
        let prov = ("app-misc".to_string(), "unsatprov".to_string());
        let mut slot_want: HashMap<(String, String), Vec<String>> = HashMap::new();
        slot_want.insert(
            prov.clone(),
            owners.iter().map(|(_, atom)| (*atom).to_string()).collect(),
        );
        let mut pullers: SlotPullers = HashMap::new();
        pullers.insert(
            prov,
            owners
                .iter()
                .map(|(pkg, atom)| {
                    (
                        "app-misc".to_string(),
                        (*pkg).to_string(),
                        "1".to_string(),
                        (*atom).to_string(),
                    )
                })
                .collect(),
        );
        (vec![nvc], slot_want, pullers)
    }
    let config = test_config();
    let masked: HashMap<(String, String), Vec<MaskEntry>> = HashMap::new();
    let par = ("app-misc".to_string(), "unsatpar".to_string());
    let built = ">=app-misc/unsatprov-1:0/1=";
    let plain = "app-misc/unsatprov:=";
    // Positive: the replacement parent's live `:=` resolves, so the
    // installed owner seeds.
    let (base, repos) = unsat_base("base", &[("unsatpar", built)], &[("unsatpar", "1", "0")]);
    let (entries, slot_want, pullers) = unsat_inputs(&[("unsatpar", built)]);
    assert_eq!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos),
        }),
        BTreeSet::from([par.clone()]),
        "installed owner with an unsatisfiable built dep and a live replacement seeds"
    );
    // A co-owner pulling the same cp via a plain atom never seeds,
    // even installed with its own live replacement.
    let (base_x, repos_x) = unsat_base(
        "cross",
        &[("unsatpar", built), ("unsatcona", plain)],
        &[("unsatpar", "1", "0"), ("unsatcona", "1", "0")],
    );
    let (entries_x, slot_want_x, pullers_x) =
        unsat_inputs(&[("unsatcona", plain), ("unsatpar", built)]);
    assert_eq!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_x,
            entries: &entries_x,
            slot_want: &slot_want_x,
            pullers: &pullers_x,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_x),
        }),
        BTreeSet::from([par.clone()]),
        "only the owner whose own want is the built atom seeds"
    );
    // A plain (unbuilt) `:=` never qualifies, even on the same edge.
    let (_, _, pullers_plain) = unsat_inputs(&[("unsatpar", plain)]);
    let mut plain_want = slot_want.clone();
    plain_want.insert(
        ("app-misc".to_string(), "unsatprov".to_string()),
        vec![plain.to_string()],
    );
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base,
            entries: &entries,
            slot_want: &plain_want,
            pullers: &pullers_plain,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos),
        })
        .is_empty(),
        "unbuilt atoms are not probe fuel"
    );
    // An already-masked installed instance stays dead (else the
    // restart loops).
    let mut masked_owner = masked.clone();
    masked_owner.insert(
        par.clone(),
        vec![MaskEntry {
            neg: "!=app-misc/unsatpar-1".to_string(),
            reason: MaskReason::SlotConflict { parents: vec![] },
        }],
    );
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked_owner,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos),
        })
        .is_empty(),
        "masked owners never re-seed"
    );
    // A masked replacement candidate yields no seed; masking only
    // one version still seeds through the other.
    let (base_m, repos_m) = unsat_base(
        "maskedcand",
        &[("unsatpar", built)],
        &[("unsatpar", "1", "0"), ("unsatpar", "2", "0")],
    );
    let mut masked_cand = masked.clone();
    masked_cand.insert(
        par.clone(),
        vec![MaskEntry {
            neg: "!=app-misc/unsatpar-2".to_string(),
            reason: MaskReason::SlotConflict { parents: vec![] },
        }],
    );
    assert_eq!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_m,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked_cand,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_m),
        }),
        BTreeSet::from([par.clone()]),
        "masking one candidate version still seeds through the other"
    );
    // ... but with the only same-slot candidate masked, the pass
    // stays a dead end even though the owner instance is unmasked.
    let (base_m2, repos_m2) = unsat_base(
        "maskedonly",
        &[("unsatpar", built)],
        &[("unsatpar", "2", "0")],
    );
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_m2,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked_cand,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_m2),
        })
        .is_empty(),
        "a masked replacement yields no seed"
    );
    // An `--exclude`d owner refuses.
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked,
            with_bdeps: false,
            excluded: &["app-misc/unsatpar".to_string()],
            config: &config,
            gate: crate::test_gate(&repos),
        })
        .is_empty(),
        "excluded owners refuse the probe"
    );
    // With no replacement ebuild the pass stays a dead end.
    let (base_n, repos_n) = unsat_base("norepl", &[("unsatpar", built)], &[]);
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_n,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_n),
        })
        .is_empty(),
        "no replacement stays a dead end"
    );
    // A replacement in a different slot is not a replacement.
    let (base_s, repos_s) = unsat_base(
        "diffslot",
        &[("unsatpar", built)],
        &[("unsatpar", "2", "1")],
    );
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_s,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_s),
        })
        .is_empty(),
        "a different-slot replacement stays a dead end"
    );
    // An owner with no installed instance is not a parent at all.
    let (base_u, repos_u) = unsat_base("uninstalled", &[], &[("unsatpar", "1", "0")]);
    assert!(
        slot_operator_unsatisfied_probe(SlotOperatorUnsatisfiedProbeArgs {
            root: &base_u,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_u),
        })
        .is_empty(),
        "uninstalled owners never seed"
    );
    // A non-installed built (binary) parent is masked, not seeded
    // as a reinstall.
    let (base_b, repos_b) = unsat_base("binary", &[], &[("unsatpar", "1", "0")]);
    let (mut entries_b, slot_want_b, pullers_b) = unsat_inputs(&[("unsatpar", built)]);
    let mut bin_par = graph_entry("app-misc", "unsatpar", "1");
    bin_par.source = CandidateSource::Binary;
    bin_par.slot = Some("0".to_string());
    entries_b.push(bin_par);
    let (seeds_b, masks_b) =
        slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
            root: &base_b,
            entries: &entries_b,
            slot_want: &slot_want_b,
            pullers: &pullers_b,
            masked: &masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_b),
        });
    assert!(
        seeds_b.is_empty(),
        "a binary parent is never a reinstall seed"
    );
    assert_eq!(
        masks_b,
        BTreeSet::from([(
            "app-misc".to_string(),
            "unsatpar".to_string(),
            "1".to_string()
        )]),
        "the stale binary parent is masked"
    );
    // An already-masked binary stops the loop.
    let mut masked_b = masked.clone();
    masked_b.insert(
        par.clone(),
        vec![MaskEntry {
            neg: "!=app-misc/unsatpar-1[binary]".to_string(),
            reason: MaskReason::SlotOperatorBuilt,
        }],
    );
    assert_eq!(
        slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
            root: &base_b,
            entries: &entries_b,
            slot_want: &slot_want_b,
            pullers: &pullers_b,
            masked: &masked_b,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos_b),
        }),
        (BTreeSet::new(), BTreeSet::new())
    );
    let only = |_unused: Option<&(String, String)>, masked| {
        slot_operator_unsatisfied_probe_full(SlotOperatorUnsatisfiedProbeFullArgs {
            root: &base,
            entries: &entries,
            slot_want: &slot_want,
            pullers: &pullers,
            masked,
            with_bdeps: false,
            excluded: &[],
            config: &config,
            gate: crate::test_gate(&repos),
        })
        .0
    };
    assert_eq!(only(None, &masked), BTreeSet::from([par.clone()]));
    // A replacement child the search has masked is no replacement
    // (`_select_package` honours `runtime_pkg_mask`).
    let masked_child: HashMap<(String, String), Vec<MaskEntry>> = HashMap::from([(
        ("app-misc".to_string(), "unsatprov".to_string()),
        vec![MaskEntry {
            neg: "!=app-misc/unsatprov-2".to_string(),
            reason: MaskReason::SlotConflict { parents: vec![] },
        }],
    )]);
    assert!(
        only(None, &masked_child).is_empty(),
        "a masked replacement child leaves the parent alone"
    );
    for dir in [
        &base, &base_x, &base_m, &base_m2, &base_n, &base_s, &base_u, &base_b,
    ] {
        let _ = fs::remove_dir_all(dir);
    }
}
