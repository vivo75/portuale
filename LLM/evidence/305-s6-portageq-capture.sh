#!/bin/bash
# Capture portageq behavior on various test atoms

PQ="/usr/bin/portageq"
EROOT="/"

echo "Test 1: Simple installed package (sys-libs/glibc)"
echo "Command: has_version $EROOT sys-libs/glibc"
$PQ has_version $EROOT sys-libs/glibc
echo "Exit code: $?"
echo "---"

echo "Test 2: Missing package"
echo "Command: has_version $EROOT nonexistent/package"
$PQ has_version $EROOT nonexistent/package 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 3: Versioned atom"
echo "Command: has_version $EROOT '>=sys-libs/glibc-2.0'"
$PQ has_version $EROOT '>=sys-libs/glibc-2.0' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 4: Slot atom"
echo "Command: has_version $EROOT 'sys-libs/glibc:2.2'"
$PQ has_version $EROOT 'sys-libs/glibc:2.2' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 5: USE-dep atom (positive)"
echo "Command: has_version $EROOT 'sys-libs/glibc[multilib]'"
USE="multilib" $PQ has_version $EROOT 'sys-libs/glibc[multilib]' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 6: USE-dep atom (negative)"
echo "Command: has_version $EROOT 'sys-libs/glibc[-multilib]'"
USE="multilib" $PQ has_version $EROOT 'sys-libs/glibc[-multilib]' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 7: USE-conditional atom with matching USE"
echo "Command: has_version $EROOT 'dev-lang/python[sqlite?]' (with USE='sqlite')"
USE="sqlite" $PQ has_version $EROOT 'dev-lang/python[sqlite?]' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 8: USE-conditional atom without matching USE"
echo "Command: has_version $EROOT 'dev-lang/python[sqlite?]' (with USE='')"
USE="" $PQ has_version $EROOT 'dev-lang/python[sqlite?]' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 9: Invalid atom without EBUILD_PHASE"
echo "Command: has_version $EROOT 'garbage-atom'"
$PQ has_version $EROOT 'garbage-atom' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 10: Invalid atom with EBUILD_PHASE (strict mode)"
echo "Command: EBUILD_PHASE=setup EAPI=8 has_version $EROOT 'garbage-atom'"
EBUILD_PHASE=setup EAPI=8 PORTAGE_BIN_PATH=/usr/lib/portage/bin $PQ has_version $EROOT 'garbage-atom' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 11: best_version for installed package"
echo "Command: best_version $EROOT sys-libs/glibc"
$PQ best_version $EROOT sys-libs/glibc 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 12: best_version for missing package"
echo "Command: best_version $EROOT nonexistent/package"
$PQ best_version $EROOT nonexistent/package 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 13: best_version for multi-slot package"
echo "Command: best_version $EROOT dev-lang/python"
$PQ best_version $EROOT dev-lang/python 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 14: Repository atom"
echo "Command: has_version $EROOT 'sys-libs/glibc::gentoo'"
$PQ has_version $EROOT 'sys-libs/glibc::gentoo' 2>&1
echo "Exit code: $?"
echo "---"

echo "Test 15: Wrong number of arguments"
echo "Command: has_version $EROOT"
$PQ has_version $EROOT 2>&1
echo "Exit code: $?"
echo "---"
