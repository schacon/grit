#!/bin/sh
#
# Upstream: t9502-gitweb-standalone-parse-output.sh
# Requires gitweb/Perl CGI — not available in grit.
#

test_description='gitweb output parsing tests (requires gitweb CGI)'

. ./test-lib.sh

skip_all='skipping gitweb tests, requires gitweb CGI infrastructure'
test_done
