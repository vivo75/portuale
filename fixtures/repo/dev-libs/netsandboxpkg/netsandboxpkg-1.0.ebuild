EAPI=8
DESCRIPTION="fixture package: FEATURES=network-sandbox (SCOPE_BACKLOG Part 2.D) -- its src_compile records its own network namespace, the interfaces it can see, and the result of an outbound TCP connect, so a test can prove the phase ran isolated"
SLOT="0"
KEYWORDS="amd64"

src_compile() {
	local ifaces netns connect
	netns=$(readlink /proc/self/ns/net 2>/dev/null || echo "unknown")
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

	einfo "netsandboxpkg: netns=${netns} ifaces='${ifaces}' connect='${connect}'"
	{
		echo "netns=${netns}"
		echo "ifaces=${ifaces}"
		echo "connect=${connect}"
	} > "${T}/netsandbox-probe" || die
}

src_install() {
	insinto /usr/share/${PN}
	doins "${T}/netsandbox-probe"
}
