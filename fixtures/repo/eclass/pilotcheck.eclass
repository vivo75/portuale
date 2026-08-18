# Copyright fixture-only. Not a real Gentoo eclass -- exists purely to
# prove real inherit()/PORTAGE_ECLASS_LOCATIONS resolution end-to-end
# (see ebuild_phases.rs's own eclass_locations_value doc comment):
# real, unmodified bin/ebuild.sh's own inherit() function finds and
# sources this file via <repo_root>/eclass/pilotcheck.eclass, and the
# function it defines below is really callable afterward.

pilotcheck_hello() {
	echo "hello from pilotcheck.eclass"
}
