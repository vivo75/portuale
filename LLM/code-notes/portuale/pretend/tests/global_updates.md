# crates/portuale/src/pretend/tests/global_updates.rs
Global-updates AUTOCLEAN force pins (#562).

## fn updates_test_repo and force pins
- The applied-updates AUTOCLEAN force (#562). Portage runs `_global_updates` over every repo's `profiles/updates/` before the resolve (`lib/_emerge/actions.py:3944-3964`) and forces `AUTOCLEAN=yes` iff a changed updates file yields >=1 valid directive (`lib/portage/_global_updates.py:139`); files with parse errors are applied but their mtime stays unrecorded (`:131-135`).
- The S0 oracle (`task-3-report.md`, probe-merge.sh `14-merge-match`) shows the fresh-ROOT tail with AUTOCLEAN set nowhere.
