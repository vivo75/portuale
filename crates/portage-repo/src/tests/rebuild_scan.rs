use super::*;

/// Backlog #303: under `--update` a provider the pass *downgraded*
/// (held back by an installed consumer's built `:=` pin) is a missed
/// update. Real's first pass makes the same downgrade, then
/// `_slot_operator_update_probe` (`depgraph.py:3131`) finds the
/// installed instance and backtracks to replace the consumer. The scan
/// therefore treats the downgrade's `from` slot/sub-slot as the
/// provider's new slot and flags the stale consumer. Expected value:
/// host real Portage on `-uDN sys-apps/systemd` (libfido2 `rRg`,
/// libcbor kept at the installed 0.14), see
/// `LLM/backlog-tasks-2026-10.md` #303.
#[test]
fn slot_operator_rebuild_scan_treats_a_pin_forced_downgrade_as_a_missed_update() {
    let dir = TempDir::new("portage-repo-slotop-303").keep();
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    // `bar` 2.0 is installed at sub-slot 2; `holder` was built
    // against the older sub-slot 1 and pins it.
    mk("bar-2.0", "0/2", "");
    mk("holder-1.0", "0", "dev-libs/bar:0/1=");
    let bar_downgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Downgrade {
            from: "2.0".into(),
            to: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("1".into()),
        ..graph_entry("dev-libs", "bar", "1.0")
    };
    let reach: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "holder".to_string())]);
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let scan = |update: bool, top: &HashSet<(String, String)>| {
        slot_operator_rebuild_scan(
            &dir,
            &[],
            std::slice::from_ref(&bar_downgrade),
            &reach,
            &empty,
            &empty,
            true,
            update,
            top,
            &HashSet::new(),
            &[],
            &test_config(),
        )
        .0
    };
    assert_eq!(
        scan(true, &HashSet::new()),
        BTreeSet::from([("dev-libs".to_string(), "holder".to_string())]),
        "the pin-forced downgrade schedules the stale consumer's rebuild"
    );
    assert!(
        scan(false, &HashSet::new()).is_empty(),
        "without --update a downgrade is not a missed update"
    );
    let requested: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "bar".to_string())]);
    assert!(
        scan(true, &requested).is_empty(),
        "an explicitly requested downgrade is not a missed update"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_operator_rebuild_scan_flags_only_the_stale_bindings() {
    // A throwaway vdb: `foo` (unused here), a `stale` consumer bound
    // `bar:2/2=`, a `fresh` consumer bound `bar:2/9=`, an
    // `otherslot` consumer bound `bar:1/1=`, and a `nonop` consumer
    // with a plain (no `=`) `bar:2` dep.
    let dir = TempDir::new("portage-repo-slotop").keep();
    let mk = |name: &str, slot: &str, rdepend: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    };
    mk("stale-1.0", "0", "dev-libs/bar:2/2=");
    mk("fresh-1.0", "0", "dev-libs/bar:2/9=");
    mk("otherslot-1.0", "0", "dev-libs/bar:1/1=");
    mk("nonop-1.0", "0", "dev-libs/bar:2");

    // `bar` is being upgraded to slot 2 sub-slot 9 this run.
    let bar_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    // Every consumer is "reachable" here (the gating is tested
    // separately in `required_set_reachable_cps_*`); empty repos ->
    // the rebuild slot falls back to the vdb `SLOT` (no cascade).
    let reach: HashSet<(String, String)> = ["stale", "fresh", "otherslot", "nonop"]
        .iter()
        .map(|p| ("dev-libs".to_string(), (*p).to_string()))
        .collect();
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_upgrade),
        &reach,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled,
        BTreeSet::from([("dev-libs".to_string(), "stale".to_string())]),
        "only the stale :2/2= binding is scheduled for a rebuild"
    );
    assert_eq!(
        abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )]
    );

    // Nothing changing `bar` -> no rebuilds.
    let (empty_sched, empty_abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        &[],
        &reach,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(empty_sched.is_empty() && empty_abi.is_empty());

    // Not reachable -> the post-walk vdb scan is suppressed entirely
    // (real: no slot-op rebuild for a consumer outside the required
    // sets). The S5 probe is not complete-mode gated, but `bar`'s
    // upgrade carries no unbuilt `:=` dep, so nothing fires here.
    let (none_sched, none_abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_upgrade),
        &HashSet::new(),
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(none_sched.is_empty() && none_abi.is_empty());

    // #24 S3: a cp already in `already` stays reported even once it is
    // in the graph as the rebuild this scan asked for last pass --
    // that is how `abi_rebuilds` survives to the settling pass. A
    // *different* in-graph cp is still skipped.
    let stale_rebuild = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "stale".into(),
        outcome: PretendOutcome::Reinstall {
            version: "1.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: true,
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "stale", "1.0")
    };
    let already = BTreeSet::from([("dev-libs".to_string(), "stale".to_string())]);
    let (again, again_abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        &[bar_upgrade.clone(), stale_rebuild],
        &reach,
        &already,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(again, already, "the set is stable -- no second restart");
    assert_eq!(
        again_abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )],
        "the causing-rebuilds pair survives into the settling pass"
    );

    // The `--solver=` bridges keep the pre-S3 synthesiser; it must
    // still build the one entry, flagged `slot_operator_rebuild`.
    let (out, _) = slot_operator_rebuild_entries(SlotOperatorRebuildEntriesArgs {
        root: &dir,
        repos: &[],
        entries: std::slice::from_ref(&bar_upgrade),
        reachable: &reach,
        with_bdeps: true,
        excluded: &[],
        update: false,
        top_level_cps: &HashSet::new(),
        top_level_atoms: &HashSet::new(),
        config: &test_config(),
    });
    let names: Vec<&str> = out.iter().map(|e| e.package.as_str()).collect();
    assert_eq!(names, vec!["stale"]);
    assert!(matches!(
        out[0].outcome,
        PretendOutcome::Reinstall {
            slot_operator_rebuild: true,
            ..
        }
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #211 (v2 `#24b`): real `_slot_operator_update_probe`'s
/// new-child-slot arm (`depgraph.py:3121-3126`, bug 486580) -- a
/// provider merged into a fresh slot schedules the installed
/// consumers bound to the old slot, even with an empty `reachable`
/// set, as long as they were walked this pass. Guards: a superseded
/// installed child in the bound slot, a higher (or equal) version,
/// `--update` (or a directly-requested provider), and a visible tree
/// candidate for the consumer.
#[test]
fn slot_operator_rebuild_scan_schedules_consumers_of_a_slot_moving_provider() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-newslot").keep();
    // vdb: provider `massb-1` in slot 1, consumer `massc-1` bound
    // `massb:1/1=`.
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    // Tree: `massb-2.0` at slot 2/2, plus a `massc-1.0` ebuild (the
    // replacement parent must exist).
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"211 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "2.0", "2/2");
    write_pkg("massc", "1.0", "0");
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
    // `massb-2.0` merged into the fresh slot 2; the consumer is walked
    // (AlreadyInstalled) but `reachable` is empty -- the empty-world
    // conflict-mass shape.
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "massc", "1.0")
    };
    let entries = vec![fresh.clone(), consumer.clone()];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled,
        BTreeSet::from([massc.clone()]),
        "the consumer bound to the abandoned slot rebuilds"
    );
    // #269: real's `_slot_operator_update_backtrack`
    // (`depgraph.py:2442`) files the child (provider) reinstall only
    // when `new_child_slot is None` -- the new-slot arm records the
    // consumer alone, so it contributes no `(provider, consumer)`
    // display pair (real's `_forced_rebuilds` stays empty: no
    // provider `r`, no `causing rebuilds` block).
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    // No `--update` and provider not directly requested: withheld,
    // mirroring real's `want_update` gate.
    let (withheld, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(withheld.is_empty(), "without --update the probe withholds");
    // ...unless the provider itself is directly requested (real's
    // arg-chain disjunct).
    let top = HashSet::from([("dev-libs".to_string(), "massb".to_string())]);
    let top_atoms = HashSet::from(["dev-libs/massb".to_string()]);
    let (via_arg, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        false,
        &top,
        &top_atoms,
        &[],
        &test_config(),
    );
    assert_eq!(via_arg, BTreeSet::from([massc.clone()]));
    // #252: with `--update` and the provider directly requested,
    // real's greedy request-argument expansion pins the installed
    // slot (`massb:1`), and the update probe's reverse-deps gate
    // refuses every slot-2 candidate against it -- the replace set
    // stays empty (probed on real 3.0.82.2: the `mmprov` shape
    // merges only the provider upgrade). Same call as `via_arg`
    // above, only with `update` on.
    let top_atoms = HashSet::from(["dev-libs/massb".to_string()]);
    let (greedy_pinned, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &top,
        &top_atoms,
        &[],
        &test_config(),
    );
    assert!(
        greedy_pinned.is_empty(),
        "a requested provider below the highest slot probes nothing"
    );
    // A lower-version fresh slot is a downgrade real never probes --
    // but only when nothing higher is available: with `massb-2.0` on
    // the tree the probe fires through it (real ranges over every
    // available package, highest acceptable wins, #269), and the
    // graph's downgrade entry does not veto that.
    let older = GraphEntry {
        outcome: PretendOutcome::New {
            version: "0.9".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "0.9")
    };
    let (downgraded, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &[older.clone(), consumer.clone()],
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        downgraded,
        BTreeSet::from([massc.clone()]),
        "a graph downgrade does not veto the available upgrade"
    );
    // Pure downgrade: no higher version anywhere, so nothing probes.
    let old_base = TempDir::new("portage-repo-slotop-downgrade").keep();
    let old_repo = old_base.join("repo");
    let write_old = |pkg: &str, pv: &str, slot: &str| {
        let dir = old_repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"211 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = old_repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_old("massb", "0.9", "2/2");
    write_old("massc", "1.0", "0");
    let old_repos = vec![RepoConfig {
        name: "testrepo".to_string(),
        location: old_repo.clone(),
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
    // The vdb half lives on `base` (installed `massb-1` + `massc-1`);
    // only the tree is downgrade-only here.
    let (pure_downgrade, ..) = slot_operator_rebuild_scan(
        &base,
        &old_repos,
        &[older, consumer.clone()],
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(pure_downgrade.is_empty(), "downgrades never probe");
    let _ = fs::remove_dir_all(&old_base);
    // No tree candidate for the consumer: no replacement parent, no
    // schedule (real finds nothing and does nothing).
    let (nocand, ..) = slot_operator_rebuild_scan(
        &base,
        &[],
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(nocand.is_empty(), "no candidate means no schedule");
    // Neither walked nor reachable: outside the probe's population.
    let (unwalked, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        std::slice::from_ref(&fresh),
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(unwalked.is_empty(), "unwalked consumers never schedule");
    let _ = fs::remove_dir_all(&base);
}

/// Backlog #268: the two cuts of the `#252` pin gate, at scan level.
/// Scratch `massb`/`massc` shape with a twist: the tree's highest
/// package `massb-2.0` (`2/2`) soft-blocks the bound slot
/// (`RDEPEND="!dev-libs/massb:1"`), so real's greedy revision with
/// blocker lookahead discards the `massb:1` pin and the probe fires;
/// and the request atoms are explicit (`massb:2`,
/// `=massb-1.0`-style), which span a single slot and pin nothing.
/// Every call below schedules the consumer alone (no display pair --
/// the new-slot arm, #269).
#[test]
fn slot_operator_rebuild_scan_pin_gate_cuts_probe() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-pingate").keep();
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str, rdepend: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let mut body =
            format!("EAPI=8\nDESCRIPTION=\"268 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        if !rdepend.is_empty() {
            body.push_str(&format!("RDEPEND=\"{rdepend}\"\n"));
        }
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=268 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64").unwrap();
        if !rdepend.is_empty() {
            writeln!(entry, "RDEPEND={rdepend}").unwrap();
        }
        writeln!(entry, "SLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "1.0", "1/1", "");
    write_pkg("massb", "2.0", "2/2", "!dev-libs/massb:1");
    write_pkg("massc", "1.0", "0", "");
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
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", "massc", "1.0")
    };
    let entries = vec![fresh, consumer];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massb = ("dev-libs".to_string(), "massb".to_string());
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let top = HashSet::from([massb.clone()]);
    let scan = |atoms: &HashSet<String>| {
        slot_operator_rebuild_scan(
            &base,
            &repos,
            &entries,
            &HashSet::new(),
            &empty,
            &empty,
            true,
            true,
            &top,
            atoms,
            &[],
            &test_config(),
        )
    };
    // Cut (b): the bare request's pin is discarded by the blocker
    // lookahead, so the probe fires.
    let bare = HashSet::from(["dev-libs/massb".to_string()]);
    let (discarded, abi, ..) = scan(&bare);
    assert_eq!(
        discarded,
        BTreeSet::from([massc.clone()]),
        "a blocker-discarded pin refuses nothing"
    );
    assert!(abi.is_empty(), "new-slot arm: consumer alone");
    // Cut (a): an explicit slot atom spans one slot and pins nothing.
    let slot_atom = HashSet::from(["dev-libs/massb:2".to_string()]);
    let (slotted, ..) = scan(&slot_atom);
    assert_eq!(
        slotted,
        BTreeSet::from([massc.clone()]),
        "an explicit slot atom pins nothing"
    );
    // Cut (a): an exact-version atom spans one slot and takes no
    // greedy pin -- but since #288 it is an argument *parent* of its
    // cp, and real's update probe refuses every candidate it does not
    // match (`=massb-1.0` vetoes the new-slot `massb-2.0`; real
    // 3.0.82.2 merges nothing for the `mmprov` twin, probe `D-arg-pin`).
    let ver_atom = HashSet::from(["=dev-libs/massb-1.0".to_string()]);
    let (versioned, ..) = scan(&ver_atom);
    assert!(
        versioned.is_empty(),
        "the exact-version argument vetoes the new-slot candidate: {versioned:?}"
    );
    let _ = fs::remove_dir_all(&base);
}

/// Backlog #211 R2: real `_slot_operator_check_reverse_dependencies`
/// (`depgraph.py:2472-2573`, gates `:2622`/`:2738`) refuses the whole
/// replacement when the fresh candidate violates another parent's
/// atom -- on the new-slot arm and the same-slot arm alike. A parent
/// whose pin the candidate still satisfies (built `:S/SS=`, relaxed
/// to `:=`) never vetoes; neither does the consumer itself (real's
/// `replacement_parent` skip), an out-of-scope parent real never
/// walked, a parent this run replaces, or an excluded one.
#[test]
fn slot_operator_update_probe_refusal_vetoes_both_arms() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-refusal").keep();
    // vdb: provider `massb-1` in slot 1, consumer `massc-1` bound
    // `massb:1/1=`, veto parent `massv-1` pinning `<massb-2`.
    for (name, slot, rdepend) in [
        ("massb-1", "1", ""),
        ("massc-1", "0", "dev-libs/massb:1/1="),
        ("massv-1", "0", "<dev-libs/massb-2"),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!("EAPI=8\nDESCRIPTION=\"211 R2\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 R2\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("massb", "2.0", "2/2");
    write_pkg("massc", "1.0", "0");
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
    let fresh = GraphEntry {
        slot: Some("2".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let walked_entry = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let consumer = walked_entry("massc");
    let veto_parent = walked_entry("massv");
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massc = ("dev-libs".to_string(), "massc".to_string());
    let scan = |entries: &[GraphEntry], excluded: &[String]| {
        slot_operator_rebuild_scan(
            &base,
            &repos,
            entries,
            &HashSet::new(),
            &empty,
            &empty,
            true,
            true,
            &HashSet::new(),
            &HashSet::new(),
            excluded,
            &test_config(),
        )
    };
    // The veto parent walked: the fresh candidate violates its pin,
    // so the whole replacement is refused -- nothing schedules.
    let (refused, ..) = scan(&[fresh.clone(), consumer.clone(), veto_parent.clone()], &[]);
    assert!(refused.is_empty(), "a vetoed replacement schedules nothing");
    // Out of scope (neither walked nor reachable): real never walks
    // the parent, so there is no veto.
    let (unscoped, ..) = scan(&[fresh.clone(), consumer.clone()], &[]);
    assert_eq!(unscoped, BTreeSet::from([massc.clone()]));
    // Excluded: real's `excluded_pkgs` skip.
    let (excluded_out, ..) = scan(
        &[fresh.clone(), consumer.clone(), veto_parent.clone()],
        &["dev-libs/massv".to_string()],
    );
    assert_eq!(excluded_out, BTreeSet::from([massc.clone()]));
    // Same-slot arm: an in-slot upgrade the veto parent rejects is
    // refused the same way.
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massb", "2.0")
    };
    let reach_massc: HashSet<(String, String)> =
        HashSet::from([massc.clone(), ("dev-libs".to_string(), "massv".to_string())]);
    let (same_refused, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &[upgrade.clone(), consumer.clone(), veto_parent.clone()],
        &reach_massc,
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(same_refused.is_empty(), "the same-slot arm refuses too");
    let (same_kept, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &[upgrade, consumer, veto_parent],
        &reach_massc,
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &["dev-libs/massv".to_string()],
        &test_config(),
    );
    assert_eq!(same_kept, BTreeSet::from([massc]));
    let _ = fs::remove_dir_all(&base);
}

/// Backlog #211 item 2: the new-child-slot arm also probes
/// `Upgrade`/`Downgrade`/`Reinstall` entries in a slot the consumer
/// is not bound to. Real's candidate loop (`_iter_similar_available`,
/// `depgraph.py:2660-2695`) ranges over every available package, not
/// only fresh-slot merges; entries at the bound slot stay with the
/// same-slot arm and never leak into the new arm.
#[test]
fn slot_operator_new_slot_arm_probes_bound_slot_mismatched_entries() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-mismatch").keep();
    // vdb: provider `massm` installed in slots 0 (`1`) and 1 (`2`),
    // consumer `massn-1` bound `massm:0/1=`, consumer `masso-1`
    // bound `massm:1/1=`.
    for (name, slot, rdepend) in [
        ("massm-1", "0", ""),
        ("massm-2", "1", ""),
        ("massn-1", "0", "dev-libs/massm:0/1="),
        ("masso-1", "0", "dev-libs/massm:1/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    for pkg in ["massn", "masso"] {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = "EAPI=8\nDESCRIPTION=\"211 mismatch\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\n";
        std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=211 mismatch\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT=0\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-1.0")), entry).unwrap();
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
    // `massm-3.0` upgrades the slot-1 instance; the consumer is bound
    // to slot 0 -- a bound-slot mismatch on an `Upgrade` entry, with
    // no `New` entry anywhere.
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massm", "3.0")
    };
    let walked_consumer = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let massn = ("dev-libs".to_string(), "massn".to_string());
    // Both consumers walked, neither reachable: the same-slot arm is
    // out of scope. `massn` (bound to slot 0) probes the slot-1
    // upgrade; `masso` (bound to the entry's own slot 1) must not
    // leak that entry into the new arm.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &[upgrade, walked_consumer("massn"), walked_consumer("masso")],
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(scheduled, BTreeSet::from([massn]));
    // #269: the new-slot arm records the consumer alone (real's
    // `_slot_operator_update_backtrack`, `depgraph.py:2442`), so the
    // bound-slot-mismatched `Upgrade` entry schedules `massn` with no
    // `(provider, consumer)` display pair.
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    let _ = fs::remove_dir_all(&base);
}

/// Backlog #304: the new-slot arm needs a replacement parent whose own
/// atom accepts the new slot (real `_slot_operator_update_probe`,
/// `depgraph.py:2667-2760`). `pinned`'s current ebuild says
/// `dev-libs/massm:0=`, so a slot-1 `massm` can never be pulled in by
/// rebuilding it (host: `gnome-keyring`'s `gcr:0/1=` vs the installed
/// `gcr:4`, expected value from real Portage -- no rebuild); `open`'s
/// says `dev-libs/massm:=` and does rebuild.
#[test]
fn slot_operator_new_slot_arm_needs_a_replacement_atom_that_accepts_the_new_slot() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-304").keep();
    for (name, slot, rdepend) in [
        ("massm-1", "0", ""),
        ("massm-2", "1", ""),
        ("pinned-1", "0", "dev-libs/massm:0/1="),
        ("open-1", "0", "dev-libs/massm:0/1="),
    ] {
        let d = base.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    let repo = base.join("repo");
    for (pkg, rdepend) in [
        ("pinned", "dev-libs/massm:0="),
        ("open", "dev-libs/massm:="),
    ] {
        let dir = repo.join("dev-libs").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "EAPI=8\nDESCRIPTION=\"304\"\nSLOT=\"0\"\nKEYWORDS=\"amd64\"\nRDEPEND=\"{rdepend}\"\n"
        );
        std::fs::write(dir.join(format!("{pkg}-1.0.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=304\nEAPI=8\n".to_string();
        writeln!(
            entry,
            "KEYWORDS=amd64\nRDEPEND={rdepend}\nSLOT=0\n_md5_={md5}"
        )
        .unwrap();
        let cachedir = repo.join("metadata/md5-cache/dev-libs");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-1.0")), entry).unwrap();
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
    let upgrade = GraphEntry {
        outcome: PretendOutcome::Upgrade {
            from: "2".into(),
            to: "3.0".into(),
        },
        slot: Some("1".into()),
        sub_slot: Some("2".into()),
        ..graph_entry("dev-libs", "massm", "3.0")
    };
    let walked_consumer = |package: &str| GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("dev-libs", package, "1.0")
    };
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let (scheduled, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &[upgrade, walked_consumer("pinned"), walked_consumer("open")],
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled,
        BTreeSet::from([("dev-libs".to_string(), "open".to_string())]),
        "only the consumer whose replacement atom accepts slot 1 rebuilds"
    );
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn slot_operator_rebuild_scan_honours_with_bdeps() {
    // #65 S1: an installed consumer's DEPEND/BDEPEND are read only
    // when `with_bdeps` is true (real empties them otherwise,
    // `_emerge/depgraph.py:4194-4247`); RDEPEND/PDEPEND/IDEPEND are
    // runtime/install-time keys and stay unconditional.
    let dir = TempDir::new("portage-repo-slotop-bdeps").keep();
    let mk = |name: &str, key: &str| {
        let d = dir.join("var/db/pkg/dev-libs").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
        fs::write(d.join("SLOT"), "0\n").unwrap();
        fs::write(d.join(key), "dev-libs/bar:2/2=\n").unwrap();
    };
    mk("rdep-1.0", "RDEPEND");
    mk("dep-1.0", "DEPEND");
    mk("bdep-1.0", "BDEPEND");
    mk("pdep-1.0", "PDEPEND");
    mk("idep-1.0", "IDEPEND");

    // `bar` moves to slot 2 sub-slot 9 this run.
    let bar_upgrade = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Upgrade {
            from: "1.0".into(),
            to: "2.0".into(),
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    let reach: HashSet<(String, String)> = ["rdep", "dep", "bdep", "pdep", "idep"]
        .iter()
        .map(|p| ("dev-libs".to_string(), (*p).to_string()))
        .collect();
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let cp = |name: &str| ("dev-libs".to_string(), name.to_string());

    let runtime_only = BTreeSet::from([cp("rdep"), cp("pdep"), cp("idep")]);
    let all_five = BTreeSet::from([cp("rdep"), cp("dep"), cp("bdep"), cp("pdep"), cp("idep")]);
    // `walked` entries: what the pass actually visited (an
    // `AlreadyInstalled` node per consumer, the @world shape).
    let walked_entry = |name: &str| GraphEntry {
        discovery: 0,
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1.0".into(),
        },
        ..graph_entry("dev-libs", name, "1.0")
    };

    // bdeps off, consumers only reachable: runtime/install-time keys.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_upgrade),
        &reach,
        &empty,
        &empty,
        false,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled, runtime_only,
        "DEPEND/BDEPEND are skipped without with_bdeps"
    );
    assert!(
        !abi.iter()
            .any(|(_, c)| c == "dev-libs/dep-1.0" || c == "dev-libs/bdep-1.0")
    );

    // bdeps on (--with-bdeps=y / the default without --usepkg) but
    // consumers only *reachable*, not walked: S2 keeps the build-time
    // keys off. Real's complete-mode-only reachability never probes
    // an installed parent's optional build-time dep
    // (depgraph.py:3114,3802-3806,4277/4287).
    let (scheduled, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_upgrade),
        &reach,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled, runtime_only,
        "build-time keys need a walked consumer even with bdeps on"
    );

    // bdeps on and every consumer walked (@world complete-mode
    // seeds): every key is scanned, as before this slice.
    let mut walked: Vec<GraphEntry> = vec![bar_upgrade.clone()];
    for name in ["rdep", "dep", "bdep", "pdep", "idep"] {
        walked.push(walked_entry(name));
    }
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        &walked,
        &reach,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(scheduled, all_five);
    assert_eq!(
        abi,
        vec![
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/bdep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/dep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/idep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/pdep-1.0".to_string()
            ),
            (
                "dev-libs/bar-2.0".to_string(),
                "dev-libs/rdep-1.0".to_string()
            ),
        ]
    );

    // The S1 gate beats the S2 walk test: even a walked consumer's
    // build-time keys stay out when bdeps is off.
    let (scheduled, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        &walked,
        &reach,
        &empty,
        &empty,
        false,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(scheduled, runtime_only);

    // The slot-change probe (`slot_operator_slot_change_probe`) reads
    // merge-bound parents' tree metadata, not the vdb, and must not
    // gain a with_bdeps gate -- covered by its own test above.
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #161 S1: a `Reinstall` parent counts toward the scan's
/// `new_slot` map (the `Upgrade | Downgrade | Reinstall` arm), and
/// the skip predicate's clauses each suppress independently: an
/// `undone` consumer, and a consumer outside `reachable` while the
/// reachable set is non-empty.
#[test]
fn slot_operator_rebuild_scan_reinstall_parent_and_skip_clauses() {
    let dir = slotundo_temp_dir("161-scan");
    let d = dir.join("var/db/pkg/dev-libs/stale-1.0");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("CATEGORY"), "dev-libs\n").unwrap();
    fs::write(d.join("SLOT"), "0\n").unwrap();
    fs::write(d.join("RDEPEND"), "dev-libs/bar:2/2=\n").unwrap();
    let bar_reinstall = GraphEntry {
        discovery: 0,
        category: "dev-libs".into(),
        package: "bar".into(),
        outcome: PretendOutcome::Reinstall {
            version: "2.0".into(),
            changed_flags: Vec::new(),
            deps_changed: false,
            slot_changed: false,
            rebuilt_binary: false,
            new_repo: false,
            slot_operator_rebuild: false,
        },
        slot: Some("2".into()),
        sub_slot: Some("9".into()),
        ..graph_entry("dev-libs", "bar", "2.0")
    };
    let stale = ("dev-libs".to_string(), "stale".to_string());
    let reach: HashSet<(String, String)> = HashSet::from([stale.clone()]);
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    // The Reinstall parent feeds `new_slot`, so the stale `:2/2=`
    // binding schedules a rebuild.
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_reinstall),
        &reach,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert_eq!(scheduled, BTreeSet::from([stale.clone()]));
    assert_eq!(
        abi,
        vec![(
            "dev-libs/bar-2.0".to_string(),
            "dev-libs/stale-1.0".to_string()
        )]
    );
    // An `undone` consumer is skipped even though it is reachable.
    let undone = BTreeSet::from([stale.clone()]);
    let (done_sched, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_reinstall),
        &reach,
        &empty,
        &undone,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(done_sched.is_empty(), "`undone` suppresses the rebuild");
    // A reachable set that omits the consumer suppresses the scan --
    // the gate itself is non-empty, so the loop runs and skips.
    let elsewhere: HashSet<(String, String)> =
        HashSet::from([("dev-libs".to_string(), "other".to_string())]);
    let (far_sched, ..) = slot_operator_rebuild_scan(
        &dir,
        &[],
        std::slice::from_ref(&bar_reinstall),
        &elsewhere,
        &empty,
        &empty,
        true,
        false,
        &HashSet::new(),
        &HashSet::new(),
        &[],
        &test_config(),
    );
    assert!(far_sched.is_empty(), "unreachable consumers never schedule");
    let _ = fs::remove_dir_all(&dir);
}

/// Backlog #269: the new-slot arm ranges over every *available*
/// package, not just the scheduled entries. The `mmprov`/`mmcons`
/// shape from the #253 recipe with only `mmcons` requested: installed
/// `mmprov-1` (`0/1`) + `mmprov-2` (`1/1`) + `mmcons-1` bound
/// `mmprov:0/1=`, tree `mmprov-3` (`1/2`). Real 3.0.82.2 (container
/// probe 2026-10-01, `emerge --ignore-default-opts --pretend --color=n
/// --backtrack=20 --update --deep app-misc/mmcons
/// `dev-libs/slotconflictoldconsumer`
/// `dev-libs/slotconflictnewconsumer`) merges `mmprov-3` bare plus
/// the `mmcons-1` rebuild (`rR`) with no `causing rebuilds` block:
/// the update probe (`_slot_operator_update_probe` with
/// `new_child_slot=True`, `depgraph.py:3121-3126`) finds `mmprov-3`
/// among the *available* packages (`_iter_similar_available`) even
/// though no pass scheduled it, and the backtrack
/// (`_slot_operator_update_backtrack`, `depgraph.py:2442`) records
/// the consumer alone. The pre-#269 scan only ranged over the
/// scheduled `New`/`Upgrade` entries, so with no provider entry it
/// scheduled nothing at all here.
#[test]
fn slot_operator_rebuild_scan_newslot_arm_fires_without_a_scheduled_provider() {
    use md5::Digest as _;
    use std::fmt::Write as _;
    let base = TempDir::new("portage-repo-slotop-269").keep();
    // vdb: provider instances in both slots, consumer bound to slot 0.
    for (name, slot, rdepend) in [
        ("mmprov-1", "0/1", ""),
        ("mmprov-2", "1/1", ""),
        ("mmcons-1", "0", "app-misc/mmprov:0/1="),
    ] {
        let d = base.join("var/db/pkg/app-misc").join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CATEGORY"), "app-misc\n").unwrap();
        fs::write(d.join("SLOT"), format!("{slot}\n")).unwrap();
        fs::write(d.join("repository"), "testrepo\n").unwrap();
        if !rdepend.is_empty() {
            fs::write(d.join("RDEPEND"), format!("{rdepend}\n")).unwrap();
        }
    }
    // Tree: `mmprov-3` at slot 1/2, plus the consumer's own ebuild
    // (the replacement parent must exist).
    let repo = base.join("repo");
    let write_pkg = |pkg: &str, pv: &str, slot: &str| {
        let dir = repo.join("app-misc").join(pkg);
        std::fs::create_dir_all(&dir).unwrap();
        let body =
            format!("EAPI=8\nDESCRIPTION=\"269 probe\"\nSLOT=\"{slot}\"\nKEYWORDS=\"amd64\"\n");
        std::fs::write(dir.join(format!("{pkg}-{pv}.ebuild")), &body).unwrap();
        let md5 = format!("{:x}", md5::Md5::digest(body.as_bytes()));
        let mut entry = "DEFINED_PHASES=-\nDESCRIPTION=269 probe\nEAPI=8\n".to_string();
        writeln!(entry, "KEYWORDS=amd64\nSLOT={slot}\n_md5_={md5}").unwrap();
        let cachedir = repo.join("metadata/md5-cache/app-misc");
        std::fs::create_dir_all(&cachedir).unwrap();
        std::fs::write(cachedir.join(format!("{pkg}-{pv}")), entry).unwrap();
    };
    write_pkg("mmprov", "3", "1/2");
    write_pkg("mmcons", "1", "0");
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
    // No provider entry at all (real's pass 0 schedules none either):
    // the consumer sits in the graph as walked/AlreadyInstalled.
    let consumer = GraphEntry {
        outcome: PretendOutcome::AlreadyInstalled {
            version: "1".into(),
        },
        slot: Some("0".into()),
        sub_slot: Some("0".into()),
        ..graph_entry("app-misc", "mmcons", "1")
    };
    let entries = vec![consumer.clone()];
    let empty: BTreeSet<(String, String)> = BTreeSet::new();
    let mmcons = ("app-misc".to_string(), "mmcons".to_string());
    let mmprov = ("app-misc".to_string(), "mmprov".to_string());
    // Only `mmcons` requested (never the provider): the #252 greedy
    // pin stays out, and `want_update` rides on `--update`.
    let top = HashSet::from([mmcons.clone()]);
    let top_atoms = HashSet::from(["app-misc/mmcons".to_string()]);
    let (scheduled, abi, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &top,
        &top_atoms,
        &[],
        &test_config(),
    );
    assert_eq!(
        scheduled,
        BTreeSet::from([mmcons.clone()]),
        "the available slot move rebuilds the consumer"
    );
    assert!(
        abi.is_empty(),
        "the new-slot arm records the consumer alone: {abi:?}"
    );
    // The #252 shape (provider requested too) still probes nothing.
    let mut both = top.clone();
    both.insert(mmprov.clone());
    let both_atoms = HashSet::from(["app-misc/mmcons".to_string(), "app-misc/mmprov".to_string()]);
    let (greedy_pinned, ..) = slot_operator_rebuild_scan(
        &base,
        &repos,
        &entries,
        &HashSet::new(),
        &empty,
        &empty,
        true,
        true,
        &both,
        &both_atoms,
        &[],
        &test_config(),
    );
    assert!(
        greedy_pinned.is_empty(),
        "a requested provider below the highest slot probes nothing"
    );
    let _ = fs::remove_dir_all(&base);
}
