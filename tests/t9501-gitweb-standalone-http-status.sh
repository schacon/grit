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

# --- gitweb/Perl CGI not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'snapshots: good tree-ish id' '
	false
'

test_expect_failure 'snapshots: bad tree-ish id' '
	false
'

test_expect_failure 'snapshots: bad tree-ish id (tagged object)' '
	false
'

test_expect_failure 'snapshots: good object id' '
	false
'

test_expect_failure 'snapshots: bad object id' '
	false
'

test_expect_failure 'modification: feed last-modified' '
	false
'

test_expect_failure 'modification: feed if-modified-since (modified)' '
	false
'

test_expect_failure 'modification: feed if-modified-since (unmodified)' '
	false
'

test_expect_failure 'modification: snapshot last-modified' '
	false
'

test_expect_failure 'modification: snapshot if-modified-since (modified)' '
	false
'

test_expect_failure 'modification: snapshot if-modified-since (unmodified)' '
	false
'

test_expect_failure 'modification: tree snapshot' '
	false
'

test_expect_failure 'load checking: load too high (default action)' '
	false
'

test_expect_failure 'invalid arguments: invalid regexp (in project search)' '
	false
'

test_done
