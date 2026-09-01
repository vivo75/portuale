EAPI=8
DESCRIPTION="fixture package: FEATURES={network,ipc,mount,pid}-sandbox (SCOPE_BACKLOG Part 2.D) -- its src_compile records the namespace ids it runs in, the interfaces it can see, and an outbound TCP connect, so a test can prove the phase ran isolated"
SLOT="0"
KEYWORDS="amd64"

src_compile() {
	local ifaces connect
	# /proc/net/dev is per-network-namespace (unlike /sys/class/net,
	# whose sysfs mount still reflects the host netns here); a fresh
	# netns has only "lo".
	ifaces=$(awk -F: 'NR > 2 { gsub(/[ \t]/, "", $1); printf "%s ", $1 }' /proc/net/dev)
	ifaces=${ifaces% }
	: "${ifaces:=unknown}"
	# TEST-NET-2 (RFC 5737, 198.51.100.0/24) -- never routable. `timeout`
	# keeps this from hanging when the host has a default route that just
	# blackholes it; under FEATURES=network-sandbox the fresh netns has no
	# route at all, so `connect` fails immediately with ENETUNREACH.
	connect=$(timeout 5 bash -c 'exec 3<>/dev/tcp/198.51.100.7/80' 2>&1)
	: "${connect:=connected-or-timed-out}"

	{
		local ns
		for ns in net ipc mnt pid; do
			echo "${ns}ns=$(readlink /proc/self/ns/${ns} 2>/dev/null || echo unknown)"
		done
		echo "ifaces=${ifaces}"
		echo "connect=${connect}"
		# A fresh PID namespace makes this process pid 1 and hides every
		# host process from /proc.
		echo "procs=$(ls -d /proc/[0-9]* 2>/dev/null | wc -l)"
	} > "${T}/netsandbox-probe" || die
	einfo "netsandboxpkg: $(tr '\n' ' ' < "${T}/netsandbox-probe")"
}

src_install() {
	insinto /usr/share/${PN}
	doins "${T}/netsandbox-probe"
}
