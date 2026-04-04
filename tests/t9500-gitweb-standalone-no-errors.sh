#!/bin/sh
#
# Upstream: t9500-gitweb-standalone-no-errors.sh
# Requires gitweb/Perl CGI — not available in grit.
#

test_description='gitweb standalone tests (requires gitweb CGI)'

. ./test-lib.sh

if ! test_have_prereq PERL; then
	skip_all='skipping gitweb tests, Perl not available'
	test_done
fi

skip_all='skipping gitweb tests, requires gitweb CGI infrastructure'
test_done
