//! The outcome of resolving one atom.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PretendOutcome {
    NoVisibleCandidate,
    New {
        version: String,
    },
    Upgrade {
        from: String,
        to: String,
    },
    /// The resolved candidate is older than what's installed (a newer version was
    /// masked/removed since `from` merged). Rendered as a single dedicated `D`,
    /// where Portage stacks its downgrade column alongside `U`.
    Downgrade {
        from: String,
        to: String,
    },
    AlreadyInstalled {
        version: String,
    },
    /// Already installed at this version, but scheduled for reinstall: effective USE,
    /// recorded deps, recorded slot, binary build time, providing repo, or built
    /// slot-operator ABI binding changed. The reasons combine freely; `changed_flags`
    /// is display-only. At least one of the six is always set; a `Reinstall` with
    /// none is never constructed. `slot_operator_rebuild` is only ever standalone.
    Reinstall {
        version: String,
        changed_flags: Vec<String>,
        deps_changed: bool,
        slot_changed: bool,
        rebuilt_binary: bool,
        new_repo: bool,
        slot_operator_rebuild: bool,
    },
    /// A blocker resolved by *removing* an installed package: `version` is the
    /// installed version being removed; `required_by` carries the blocker owner so the
    /// merge-order fallback edge orders the row before it. Part of the merge list and
    /// its counters. Never a merge target or an execution unit.
    Uninstall {
        version: String,
    },
}
