#!/bin/sh
#
# Upstream: t9501-gitweb-standalone-http-status.sh
# Requires gitweb/Perl CGI — ported as test_expect_failure stubs.
#

test_description='gitweb as standalone script (http status tests).

This test runs gitweb (git web interface) as a CGI script from the
commandline, and checks that it returns the expected HTTP status
code and message.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='gitweb/Perl CGI not available in grit'
test_done
