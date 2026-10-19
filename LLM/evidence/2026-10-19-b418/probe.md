# #418 probe (2026-10-19, host Portage 3.0.82.2)

Root: `LLM/evidence/2026-10-19-b415/setup.sh` plus `usepkg-exclude = >=dev-libs/binaryonlypkg-1` in the repo's
`repos.conf` section; command `emerge -pf --usepkg dev-util/fetchprobe`; count of
`atoms are invalid in usepkg-exclude` blocks on stderr.

| options | blocks |
|---|---|
| (none) | 2 |
| `--package-moves=n` | 1 |
| fixture tree of `out373/c373-bad.txt` (one `profiles/updates` file) | 3 |

The count follows how many times Portage reloads its configuration (the global-updates pass reloads it), so
there is no fixed "three" to copy. Closed without code.
