EAPI=8
DESCRIPTION=x
SLOT=0
KEYWORDS=amd64
S=${WORKDIR}
pkg_pretend() { echo pretend >> LOGFILE; }
pkg_setup()   { echo setup >> LOGFILE; }
src_unpack()  { echo unpack >> LOGFILE; }
