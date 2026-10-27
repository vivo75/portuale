# Upstream case-6 shape, but L's atom also carries a conditional flag (bar?) that L has off.
# _check_configuration marks bar "cond" (it is in violated_atom.use.required), the first
# candidate forces bar "disabled" on K-1, K-1 already has bar off, so _force_flag_for_package
# leaves "" and the candidate is invalid; the second candidate forces "enabled".
# EAPIs as upstream (1/2) so the package USE never goes empty (see Package._init_use).
ebuilds = {
    "sci-libs/K-1": {"IUSE": "+foo bar", "EAPI": 1},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo,bar?]", "IUSE": "bar", "EAPI": 2},
    "sci-libs/M-1": {"DEPEND": "sci-libs/K[foo=]", "IUSE": "+foo", "EAPI": 2},
}
installed = {
    "sci-libs/K-1": {"IUSE": "foo bar", "USE": ""},
    "sci-libs/L-1": {"DEPEND": "sci-libs/K[-foo,bar?]", "IUSE": "bar", "USE": ""},
}
requests = [(["sci-libs/L", "sci-libs/M"], {"--backtrack": 0})]
