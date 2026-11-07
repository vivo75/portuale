# Cross-reference: Gentoo Portage bug list (2026-11-07) vs portuale backlog and quirks

Source: `LLM/portage_bug_list_2026-11-07.html` (1827 rows parsed, 1108 with product `Portage`). Titles only, no network.
"needs bug body" = the title is compatible but only the report text can settle it; URL `https://bugs.gentoo.org/<id>`.
Component names are shortened as in the list (`Core - D` = Core - Dependency Resolution, `Core - I` = Core - Interface, `Binary p` = Binary packages).
Confidence is that the bug describes the SAME Portage behaviour, judged from the title alone. Most titles are weaker than the table suggests.
Status: CONF = CONFIRMED, UNCO = UNCONFIRMED, IN_P = IN_PROGRESS.

## Quirks (portage-quirks.md)

| Quirk | Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|---|
| Q1 silent skip, installed node reused | 892832 | CONF | Core - D | emerge -pvuDN world silently skips tiff update | medium | same symptom (update silently absent); cause not stated, needs bug body |
| Q1 | 984929 | CONF | Core - D | Portage skips openssl-4.1.0 upgrade with nvidia-drivers `\|\| ( .. openssl:0/3 )` dep | medium | an installed pick via an any-of dep blocks an update, as with edk2-bin; needs bug body |
| Q1 | 872227 | CONF | Core - D | 3.0.36 fails to choose alternative dependency and prefers to keep old/no longer existing version installed | medium | `\|\|` choice keeps installed; needs bug body |
| Q1 | 848849 | UNCO | Core - D | silently skips rebuilding a package when upgrading @world and package has unmet REQUIRED_USE | low | silent skip, different cause |
| Q2 binhost timestamp | 963736 | CONF | Core - D | 3.0.69.3 keeps reinstalling the same nlohmann_json binary package on every @world upgrade | low-medium | binary-vs-installed twin handling; needs bug body |
| Q2 | 915494 | CONF | Binary p | 3.0.52 getbinpkg stuck in infinite rebuilding loop | low | same area (binpkg vs installed), different symptom |
| Q3/Q4/Q6/Q11/Q24 hash-order output | 656074 | CONF | Core - D | emerge list non-deterministic when @world has 2 atoms matching the same package | low-medium | non-determinism from set iteration; Q24 is similar in spirit but a different trigger. No bug names PYTHONHASHSEED/frozenset ordering |
| Q12 cgroup | 602664 | CONF | Core | FEATURES=cgroup disables itself silently | high | Q12's "silent when the cgroup is not writable" (by design); Gentoo files it as a bug |
| Q19 interned atoms leak a `\|\|` choice | 828136 | CONF | Core - D | mishandles ebuilds containing two identical `\|\|` blocks | low-medium | equal `\|\|` groups are shared objects; Q19 is the identity-vs-equality leak. Needs bug body |
| Q25 libc atom / dynamic deps | 516612 | CONF | Core - D | please disable dynamic-deps by default | low | dynamic deps read the vdb string once the ebuild is gone; not the libc atom itself |
| Q25 | 753500 | CONF | Binary p | support dynamic symbol version dependencies (glibc compat symbols) | low | the origin of `_inject_libc_dep`, not the merge-order effect |
| Q20-Q22 slot-conflict handler | 612486 | CONF | Core - I | --verbose-conflicts does not show all conflicts | low | same output family, unrelated detail |
| Q20-Q22 | 698232 | CONF | Core - I | 2.3.76 strange keyword suggestion after slot conflict | low | handler suggestion oddities; no evidence of the same oddity |
| Q28 lowest-version node reused | 506214 | CONF | Core - D | 2.2.10 fails to resolve a slot conflict with itself due to a repo change | low | slot conflict between two nodes of one slot; weak |
| Q7 depclean cycle wording | 644260 | UNCO | Core - D | will not depclean a package satisfied by `\|\| ( )` alternatives | low | depclean, other aspect |
| Q5, Q8, Q9, Q10, Q13-Q18, Q23, Q26, Q27 | - | - | - | no title matches | - | Q13/Q14: only 815814 (egencache ignores /etc/portage/categories, UNCO, low) is near. Q10: no tar/hardlink bug on extraction. Q26: no bug on `!>` blockers or `-r999` |

## Backlog items

### #658 (perl:0 skip-block parent list; complete-graph holders; inline virtual expansion)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 731150 | CONF | Core - D | does not enforce dependencies on installed non-@world packages | medium | Portage lists installed `dev-perl/*` holders that no request walks: installed packages enter only via the complete graph; needs bug body |
| 598503 | IN_P | Core - I | dependency calculation failures could filter out noise from conflicts involving built slot operator deps | low-medium | the skip block is noisy with built slot-operator parents (69 rows) |
| 467928 | CONF | Core - D | excluding package that needs rebuild due to sub-slot change triggers unsolved slot conflicts | low | subslot/slot-conflict family |
| 622270 | IN_P | Core - I | endless subslot rebuilds triggered by installed package with unavailable ebuild | low | installed holders of `:=` |
| 550742 | UNCO | Core - D | icu:= rdep is causing portage to require a specific subslot | low | built slot-operator atoms |
| 828136 | CONF | Core - D | two identical `\|\|` blocks | low | see Q19 |

### #617 (installed linux-headers edge goes to perl-5.44.0, Portage keeps 5.42.2)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 477544 | CONF | Core - D | Portage using dependencies from installed package, possibly related to a slot-operator dep | low-medium | an edge to the installed instance where the new one is expected; needs bug body |
| 731150 | CONF | Core - D | does not enforce dependencies on installed non-@world packages | low | installed-consumer edges |
No bug mentions "lowest version of a slot" (Q28).

### #648 (consumer-side virtual expansion: installed version without tree ebuild; flat-RDEPEND versions)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 174509 | CONF | Core - D | If all ebuilds are gone Portage doesn't consider installed stuff as providing a virtual | medium | shape (a): installed version with no tree ebuild |
| 609472 | CONF | Core - I | package.provided does not work for virtuals unless the version corresponds to an available ebuild | low | same ebuild-must-exist assumption |
| 977755 | UNCO | Core - D | forcing/assuming the SLOT of the latest version for unslotted deps in any-of groups | low | any-of expansion picks by latest |
| 608546 | CONF | Core - D | prefer dependency choices with unsatisfied USE over those with missing IUSE | low | `\|\|` choice ranking |

### #596 (`_unsatisfied_system_deps` gate, no capture)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 698400 | CONF | Core - D | @system set scheduling constraints leak from BROOT to SYSROOT | low-medium | the same @system scheduling code in the scheduler; a ROOT variant |
| 985128 | UNCO | Core - D | acct-user/systemd-* merge before dependency's satisfied | low | merge scheduling; needs bug body |

### #611 (masked-dependency display gaps; parts b, d)
| Bug | Part | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|---|
| 693514 | (d) | CONF | Core - I | "installed packages are masked" warning should include keyword masked packages | medium-high | it is that very block; Gentoo's own view is that the block is incomplete |
| 698232 | (b) | CONF | Core - I | strange keyword suggestion after slot conflict | low | keyword suggestion, other context |
| 796635 | (b) | CONF | Core - I | --autounmask-continue may display irrelevant mask messages | low | autounmask/mask display |
| 391359 | (a) | CONF | Core - D | Masked package message misleading (due to backtracking) | low | "masked" messages unreliable |
| 758467 | (a) | CONF | Core | "no ebuilds to satisfy gdbm:0/1.13=[berkdb]" message confusing for unsatisfied `:=` deps | low | which "no ebuilds" text Portage prints |
| 419381 | (e) | CONF | Core - I | autounmask comment shows confusing "required by" graph | low | chain text of the autounmask comment |

### #666 (`force_reinstall` argument atoms) - no match. Nearest: 913197 (CONF, force downgrade, low).

### #667 (blank lines around the `--verbose-conflicts` NOTE) - no match. 612486 is the only title on the option (low).

### #668 (`||` group, both branches missing, prints both)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 460816 | UNCO | Core - D | aborts resolution due to one issue but reports unrelated issues | low | which unsatisfied dep gets reported |
| 814335 | UNCO | Core - D | won't select a candidate in a `\|\|` dep, confusing blockers | low | `\|\|` pop/selection |
| 666746 | CONF | Core - I | show errors other than slot/subslot conflicts as late as possible | low | problem report ordering |

### #669 (downgrade printed `N` instead of `UD`)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 573628 | UNCO | Unclassi | Portage calculates unnecessary downgrade | low | downgrade logic, not its flag |
| 509662 | CONF | Core - D | --usepkg omits some downgrades in @world | low | downgrade handling |

### #670 (backtrack count) - no match (582840 and 304631 mention backtracking, low).

### #671 / #673 (upgrade breaks a consumer's `[use]` dependency)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 499370 | CONF | Core - D | keeps installed non-world packages when USE dependencies are no longer satisfied | medium | installed-consumer `[use]` break; consistent with #673 (portuale drops the upgrade, Portage keeps it and reports); needs bug body |
| 350230 | CONF | Core - D | emerge silently ignores packages with conflicting use settings | low-medium | silent handling of a USE conflict |
| 354787 | CONF | Core - D | does not reinstall lower SLOTs when USE flags have changed | low | USE-driven reinstall |

### #672 (ROOT suffix in `Multiple package instances`) - no match (563034, ROOT != /, CONF, low).

### #674 (`--complete-graph` backtrack count) - 938932 (UNCO, "full complete graph feature request", low) only.

### #675 ("causes rebuilds" block for a consumer upgraded anyway)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 956678 | CONF | Unclassi | --buildpkgonly causes "The following packages are causing rebuilds" on top | low-medium | same block, wrong in another context; needs bug body |
| 486968 | CONF | Unclassi | emerge libpciaccess wants to rebuild zlib for no apparent reason | low | unexplained rebuild line |
| 475850 | CONF | Core | always rebuilding packages that cannot upgrade due to conflict | low | rebuild listed for a package that is not rebuilt in the end |

### #676 (installed consumer reached only through a merge-bound package, complete mode)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 731150 | CONF | Core - D | does not enforce dependencies on installed non-@world packages | medium | the same: installed packages outside the world scope are evaluated only through the complete graph |
| 559224 | CONF | Core - D | incomplete dependency resolution on emerge -uDN @world | low | partial scope of installed packages |
| 389735 | UNCO | Core - D | fails to do --deep correctly | low | old |

### #677 (`--tree` does not print `[nomerge]` owner chain)
| Bug | Status | Comp | Summary | Conf. | Why |
|---|---|---|---|---|---|
| 444894 | CONF | Core - I | emerge --tree should display the most relevant parent | low-medium | tree parents are chosen by Portage in a way users find odd; this is a portuale gap, not a Portage bug |

### #678 (pytest cwd trap) - not a Portage behaviour; no match.

## Verdict

**Divergence probably is a Portage bug (CONFIRMED match, candidate to park or record as a quirk), all pending a body check:**
- #611(d): 693514 (CONF) says the "installed packages are masked" block is incomplete. portuale not printing it is the same family; low priority either way.
- #673 / #671 residue: 499370 (CONF) "keeps installed packages when USE deps are no longer satisfied". Portage's keep-and-report on #673 may be this bug. Weak, since Portage here reports a conflict.
- #658 parent list and #676: 731150 (CONF) is the Gentoo-side description that installed packages outside @world join the graph only through the complete graph. Portage's 69-row list and its #676 behaviour may be an accepted result of that. This is a "Portage is surprising" signal, not a fix; portuale still has to copy it to match.
- Q12: 602664 (CONF) confirms the silent cgroup behaviour.
- Q1: 892832, 984929 and 872227 (all CONF) are plausible reports of the silent skip. Check the bodies first; Q1 already says "suspect", so this could raise it to "confirmed".

**No match in the list (nothing to park on Gentoo evidence):**
#666, #667, #668, #670, #672, #674, #678, #596 (only the weak 698400), #648 (only the weak 174509), #617 (and so Q28), Q3/Q4/Q6/Q11/Q24 (no hash-order bug), Q5, Q9, Q10, Q13-Q18, Q23, Q26, Q27.

Overall the list is dominated by feature requests and old EAPI-era resolver bugs; a title-only match for the narrow output-format items (#667, #672, #675) is unlikely.

## Bug bodies read (2026-11-07, via `curl https://bugs.gentoo.org/rest/bug/N/comment`)

The title-only matches above were checked against the bug text. Verdicts replace the earlier confidence guesses.

| Bug | Status | Same behaviour as | Verdict |
|---|---|---|---|
| 693514 | CONFIRMED (wish) | #611(d) | Not a bug in Portage's view. zmedico (2019-09-04): keyword masks of installed packages are "largely ignored by emerge" on purpose; the report only asks for a nicer warning. Supports keeping keywords out of #611(d). |
| 731150 | CONFIRMED (open, last dup 2026-10-14) | background of #658/#676 | Describes the limit of the complete graph: dependencies of installed non-@world packages are not enforced unless `--deep`/`--complete-graph` applies (zmedico 2020-07-07). It does not describe the 69-parent skip block, so it is context only, not a match. |
| 602664 | CONFIRMED (wish) | Q12 | Different thing: the bug is a silent fallback when cgroup setup fails; Q12 is about resource summaries needing kernel accounting. Not a match. |
| 984929 | CONFIRMED (2026-10-26, patch in progress) | Q1 | Different mechanism: `_slot_operator_check_reverse_dependencies` rejects the `|| ( openssl-compat openssl:0/3 )` candidate (nvidia-drivers). Same theme (`||` held by an installed parent blocks an upgrade), not the Q1 find_existing_node path. Not a match. |
| 174509 | CONFIRMED (2009: contested) | #648(a) | Same shape: installed package with no ebuild in the tree. zmedico (2009-02-02): the behaviour conflicts with bug 252167, which treats packages without ebuilds as undesirable, so Portage needs an option to suit both tastes. The behaviour is a known open design conflict, not a clear bug. |
| 499370 | CONFIRMED (2016) | related to #671/#673 | zmedico: this is why emerge suggests `--depclean` after deep @world updates. It concerns non-@world packages; #671/#673 concern world consumers. Related, not the same. |

Net: no backlog item turns out to be a copy of a confirmed Portage bug. Two items gain a grounded reason to stay as they are (#611(d) keywords, #648(a)); Q1 and Q12 get no bug citation.
