#!/bin/sh
#
# Upstream: t9502-gitweb-standalone-parse-output.sh
# Requires gitweb/Perl CGI — ported as test_expect_failure stubs.
#

test_description='gitweb as standalone script (parsing script output).

This test runs gitweb (git web interface) as a CGI script from the
commandline, and checks that it produces the correct output, either
in the HTTP header or the actual script output.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='gitweb/Perl CGI not available in grit'
test_done
