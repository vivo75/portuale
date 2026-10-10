# crates/portage-repo/src/tests/repo_config.rs
`find_repos` config-layer pins: masters tiers, repo-name canonicalization, sync fields.

## fn find_repos_defaults_masters_to_the_main_repo_alone_when_unset
- Portage config.py's own default (RepoConfigLoader, lib/portage/repository/config.py:1229-1260): no explicit `masters =` key -- every non-main repo implicitly masters the main repo alone; the main repo is never its own master.

## fn find_repos_resolves_an_explicit_masters_key_to_the_named_repos_own_locations
- Portage explicit `masters = name1 name2` overrides the default outright, resolved to each named repo's location in declared order; an unknown name is silently dropped (Portage config.py only warns, never a hard error). An explicit key fully replaces the default: the main repo is not implicitly included.

## fn find_repos_treats_an_explicit_empty_masters_key_as_no_masters_at_all
- Portage explicit `masters = ` (present but empty) is genuinely different from the key being absent: "no masters", not the main-repo-alone default.

## fn find_repos_reads_layout_conf_masters_repo_name_and_profile_formats
- Portage three-tier masters (config.py:237-245/484-490): with no repos.conf `masters` key, the repo's own `metadata/layout.conf` `masters =` is the middle tier before the implicit main-repo default. `repo-name` overrides the section name; `profile-formats` is captured verbatim.
- A repos.conf `masters` key (even an empty one) still wins over layout.conf's.

## fn find_repos_reads_profiles_repo_name_and_drops_a_mismatched_section
- Portage `_read_repo_name` plus the config.py:1121 mismatch check. `dropped` mismatches with no alias; `aliased` mismatches but survives via its alias.

## fn reverse_dependency_constraints_enforces_a_runtime_pin_but_ignores_a_build_time_pin
- Backlog #91 S1, on the committed fixtures: `whtarget-1.0` records a runtime-key `~whblocker-1.0` pin (the hermetic host-qemu shape) while `rdctarget-1.0` records the build-time-key `~rdcblocker-1.0` pin (D1, #79). Both consumers reachable, both upgrades otherwise selected.
