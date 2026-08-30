#!/bin/bash
# The container image ships STABLE portage, but this repo's PORTING/ work
# is ported against ~amd64 =sys-apps/portage-3.0.82.2. Install that first
# so every later script's real `emerge` matches the version whose source
# portuale mirrors.
export ACCEPT_KEYWORDS=~amd64
/usr/bin/emerge -tv =sys-apps/portage-3.0.82.2
