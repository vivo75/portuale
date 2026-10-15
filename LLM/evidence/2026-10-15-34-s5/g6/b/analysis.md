# G6b — `-pve kde-apps/kdecore-meta` openssh/shadow chain

Verdict: **engine** (downstream of G5's `||`-choice, same parent as G6a).
No bridge patch, no fixture/test/fix — allowlist + crate-gap text at the
bottom (shared with G6a).

## The rows (bed `-pve l0-20261015T094939Z`; real 535 rows / portuale 535
## rows — same total, different composition)

Portuale-only (all installed in the VDB; real has no rows for them):

```
[ebuild  R    ] acct-group/sshd-0-r3::gentoo                          (line 53)
[ebuild  R    ] acct-user/sshd-0-r3::gentoo                           (line 83)
[ebuild  R    ] virtual/openssh-0-r1::gentoo  USE="ssl"                (line 119)
[ebuild  R    ] sys-apps/shadow-4.19.4:0/5::gentoo  USE="acl nls pam systemd …"   (line 425)
[ebuild  R    ] net-misc/openssh-10.4_p1-r1::gentoo  USE="pam ssl …"   (line 454)
```

## Chain (established from the `PORTUALE_PG_TRACE=1 -pve` run, `../cmd.txt`)

Trace edges (`pg-pve-trace.txt`), child ← candidate parents (all versions
in closure), intersected with the actual solution (`pg-plan` set, 544
entries — only in-solution parents can force a pick):

- `virtual/openssh` ← {asyncssh, twisted, **gnome-keyring-48.0-r1**
  (DEPEND+RDEPEND), gvfs, torque, virtual/ssh}. **In solution: ONLY
  gnome-keyring.** (asyncssh/twisted/gvfs/torque/virtual-ssh all absent.)
  Container-tree atom (`gnome-keyring-48.0-r1.ebuild:30`):
  `ssh-agent? ( virtual/openssh )`, in RDEPEND; plan USE has `ssh-agent`
  ON (both the kdecore plan and the control probe:
  `USE="pam ssh-agent systemd …"`).
- `net-misc/openssh` ← `virtual/openssh-0-r1` (RDEPEND:
  `|| ( >=net-misc/openssh-9.3_p1-r1[ssl?] … )`,
  `virtual/openssh-0-r1.ebuild:12-17`).
- `acct-group/sshd`, `acct-user/sshd` ← `net-misc/openssh-10.4_p1-r1`
  (DEPEND+RDEPEND).
- `sys-apps/shadow` ← `net-misc/openssh-10.4_p1-r1` (RDEPEND
  `!prefix? ( sys-apps/shadow )`, container
  `openssh-10.4_p1-r1.ebuild:64-69` — unconditional on this profile).

Full chain: root kdecore-meta → … → `virtual/secret-service ||` →
gnome-keyring-48.0-r1 (the G5 first-listed pick; Portage takes in-graph
kwallet-runtime instead) → `ssh-agent? virtual/openssh` →
net-misc/openssh-10.4_p1-r1 → {shadow-4.19.4, acct-group/sshd,
acct-user/sshd}.

## Why Portage has no rows

Portage's `-pve` closure never contains gnome-keyring, so `virtual/openssh`
is never required by anything in it; openssh/shadow/sshd stay out of the
plan entirely. (Real's lone `[ebuild R] acct-group/shadow-0` line 50 is a
different cp — the shadow group's own entry, pulled independently — not a
G6b row.)

## Control: Portage agrees downstream of the parent

Real `emerge -pve gnome-base/gnome-keyring` (control probe, `../cmd.txt`)
prints ALL FIVE rows with identical versions/actions/USE:

```
[ebuild  R    ] acct-group/sshd-0-r3
[ebuild  R    ] acct-user/sshd-0-r3
[ebuild  R    ] virtual/openssh-0-r1  USE="ssl"
[ebuild  R    ] sys-apps/shadow-4.19.4:0/5  USE="acl nls pam systemd …"
[ebuild  R    ] net-misc/openssh-10.4_p1-r1  USE="pam ssl …"
```

(rc 1 is the UD-portage row + the G4a autounmask gate after the list; the
rows themselves match portuale's kdecore extras exactly. The same probe
also prints the G6a `NS docbook-xml-dtd-4.3` row.) So given gnome-keyring
in-graph, both PMs derive the identical downstream closure — the sole
divergence is the `||` parent pick, owned by the engine (mechanism cited
in sibling file a/).

## Disposition

`owner: engine`, same entry as G6a (one `||`-choice, one parent, six
downstream rows across two modes). Proposed allowlist text: see a/.
No fixture/test/fix per the task stop rule (a bridge re-solve constraining
`||` branches to the prior solution would patch around the engine).
