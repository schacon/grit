#!/bin/sh
#
# Upstream: t9501-gitweb-standalone-http-status.sh
# Requires gitweb/Perl CGI — not available in grit.
#

test_description='gitweb HTTP status tests (requires gitweb CGI)'

. ./test-lib.sh

skip_all='skipping gitweb tests, requires gitweb CGI infrastructure'
test_done
