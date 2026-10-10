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
    /// Real `output.py`'s own in-slot `best()` check (around line 750):
    /// the resolved candidate's version is *not* `best([to, from])` --
    /// i.e. `to` is actually older than what's installed, typically
    /// because a newer version got masked/removed from the tree since
    /// `from` was merged. Real portage flags this with a distinct
    /// `attr_display.downgrade` column alongside its own `U` column
    /// (both set together); portuale's simplified one-letter-per-
    /// outcome scheme (see `Upgrade`/`Reinstall`'s own `r`/`U`) uses a
    /// single dedicated `D` instead of stacking two.
    Downgrade {
        from: String,
        to: String,
    },
    AlreadyInstalled {
        version: String,
    },
    /// `--newuse`/`--changed-use` and/or `--changed-deps` and/or
    /// `--changed-slot` and/or `--rebuilt-binaries` and/or `--newrepo`:
    /// already installed at this exact version, but this package's
    /// currently-effective USE differs from what the vdb recorded at
    /// merge time (see `reinstall_flags_for_use_change`), and/or its own
    /// vdb-recorded dependency strings differ from the repo's current
    /// ebuild (see `deps_changed`), and/or its own vdb-recorded `SLOT`
    /// differs from the repo's current ebuild (see `slot_changed`),
    /// and/or a binary candidate at this same version has a different
    /// `BUILD_TIME` than the vdb's own recorded one (see
    /// `rebuilt_binary_changed`), and/or its own vdb-recorded
    /// `repository` differs from the repo that currently provides this
    /// version (see `new_repo_changed`) -- real portage treats these as
    /// independent, freely-combinable reinstall reasons, not mutually
    /// exclusive ones. `changed_flags` is the sorted set of flag names
    /// that triggered the USE-based reason (real depgraph's own
    /// `_reinstall_for_flags` return value, kept here purely for
    /// display, matching `Upgrade`'s own `from`/`to` pattern) -- empty
    /// when it didn't trigger this outcome. At least one of
    /// `changed_flags`/`deps_changed`/`slot_changed`/`rebuilt_binary`/
    /// `new_repo`/`slot_operator_rebuild` is always non-empty/`true`; a
    /// `Reinstall` with none of the six is never constructed.
    ///
    /// `slot_operator_rebuild` is real depgraph's own
    /// `_slot_operator_trigger_reinstalls` / the
    /// `@__auto_slot_operator_replace_installed__` set: this installed
    /// package's vdb `*DEPEND` carries a *built* slot-operator atom
    /// (`cat/pkg:S/SS=`) whose bound `S/SS` no longer matches how this
    /// run leaves `cat/pkg` in that slot, so it must be rebuilt against
    /// the new ABI (`slot_operator_rebuild_entries`, a single-pass v1 --
    /// see its own doc comment). Never combined with the other five in
    /// portuale: the post-pass only ever constructs it standalone.
    Reinstall {
        version: String,
        changed_flags: Vec<String>,
        deps_changed: bool,
        slot_changed: bool,
        rebuilt_binary: bool,
        new_repo: bool,
        slot_operator_rebuild: bool,
    },
    /// #68/#72 B3: a blocker resolved by *removing* an installed package
    /// -- real `_validate_blockers`' `operation="uninstall"` task
    /// (`depgraph.py:9226-9246`), part of the merge list (`mylist`) and
    /// counted by `counters.uninst` (`output.py:619-621`). The `version`
    /// is the installed version being removed; `required_by` carries the
    /// blocker owner so `build_digraph`'s fallback edge orders the row
    /// before it (`serialize_merge_order`). Never a merge target or an
    /// execution unit: #72 B4 renders the `[uninstall     ]` row, every
    /// scheduling/merge site skips it (execution is a documented
    /// non-goal, `LLM/02.072-uninstall_merge_rows.md` §7).
    Uninstall {
        version: String,
    },
}
