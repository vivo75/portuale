#!/bin/bash
# b-compare.sh -- pairwise portage-vs-portuale report over b- cells.
# Normalizes volatile staging paths so real divergences stand out.
# Usage: b-compare.sh (runs in the evidence b/ directory, report on stdout).
set -u
EVDIR=/home/vivo/repo/PORTUALE/portuale/LLM/evidence/2026-10-12-348-s0/b
cd "$EVDIR"
TMPD=$(mktemp -d /var/tmp/t348-b/cmp.XXXXXX)
trap 'rm -rf "$TMPD"' EXIT

norm() {
	sed -e 's|/var/tmp/t348-b/tmp-[a-z]*|${BDIR}|g' \
		-e 's|/var/tmp/t348-b/root-[a-z]*|${EROOT}|g' \
		-e 's|/var/tmp/t348-b/cfg-[a-z]*|${CFG}|g' \
		-e 's|/var/tmp/portage|${BDIR0}|g' \
		-e 's|/var/cache/distfiles|${ADIST}|g' \
		-e 's|/var/tmp/t348-b/distfiles|${DIST}|g' "$1" >"$2"
}

# Strip the Portage-only pre-resolution global-updates block (up to and
# including the "Dependency resolution took" line) and the news tail.
# portuale implements neither; both are independent of EAPI behaviour.
# Only applied when the markers are present; otherwise the file is copied.
normalize_stdout() {
	if grep -q "^Dependency resolution took" "$1"; then
		sed -e '1,/^Dependency resolution took.*$/d' "$1" >"$2.tmp"
		mv "$2.tmp" "$2"
	else
		cp "$1" "$2"
	fi
	if grep -q "news items need reading" "$2"; then
		sed -e '/^ \* IMPORTANT: .* news items need reading/,$d' "$2" >"$2.tmp"
		mv "$2.tmp" "$2"
	fi
}

# Drop deterministic Portage-only config noise (fixture overlay lacks
# masters/repo_name; fixture news items are intentionally invalid).
noise_filter() {
	grep -v "missing masters attribute" "$1" \
		| grep -v "Set 'masters = testrepo'" \
		| grep -v "missing repo_name entries" \
		| grep -v "repo_name entry should be" \
		| grep -v "unique name for the repository" \
		| grep -v "fixtures/overlay/profiles/repo_name" \
		| grep -v "^!!! Invalid news item:" \
		| grep -v "^!!!   line 7: Display-If-Installed:" \
		| grep -v "Update type not recognized" \
		| grep -v "^$" >"$2" || true
}

for rc in *-portage-*.rc; do
	[[ -e $rc ]] || continue
	base=${rc%.rc}
	po=${base/-portage-/-portuale-}
	[[ -f $po.rc ]] || {
		echo "### $base : NO PORTUALE COUNTERPART"
		continue
	}
	stem=${base/-portage-/-X-}
	echo "### $stem"
	prc=$(cat "$rc")
	qrc=$(cat "$po.rc")
	[[ $prc == "$qrc" ]] && echo "rc: same ($prc)" || echo "rc: DIFF portage=$prc portuale=$qrc"
	normalize_stdout "${base}.stdout" "$TMPD/p.out"
	normalize_stdout "${po}.stdout" "$TMPD/q.out"
	if cmp -s "$TMPD/p.out" "$TMPD/q.out"; then
		echo "stdout: same"
	else
		n=$(diff "$TMPD/p.out" "$TMPD/q.out" | wc -l)
		echo "stdout: DIFF (${n} diff lines)"
	fi
	noise_filter "${base}.stderr" "$TMPD/p.err"
	noise_filter "${po}.stderr" "$TMPD/q.err"
	if cmp -s "$TMPD/p.err" "$TMPD/q.err"; then
		echo "stderr: same"
	else
		n=$(diff "$TMPD/p.err" "$TMPD/q.err" | wc -l)
		echo "stderr: DIFF (${n} diff lines)"
	fi
	for f in "$base".stage-*; do
		[[ -e $f ]] || continue
		g=${f/$base/$po}
		bn=$(basename "$f")
		if [[ ! -f $g ]]; then
			echo "stage $bn: MISSING in portuale cell"
			continue
		fi
		norm "$f" "$TMPD/p.stage"
		norm "$g" "$TMPD/q.stage"
		if cmp -s "$TMPD/p.stage" "$TMPD/q.stage"; then
			echo "stage $bn: same"
		else
			n=$(diff "$TMPD/p.stage" "$TMPD/q.stage" | wc -l)
			echo "stage $bn: DIFF (${n} diff lines)"
		fi
	done
	for g in "$po".stage-*; do
		[[ -e $g ]] || continue
		f=${g/$po/$base}
		[[ -f $f ]] || echo "stage $(basename "$g"): EXTRA in portuale cell"
	done
	echo
done
