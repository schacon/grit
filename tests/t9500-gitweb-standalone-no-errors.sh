#!/bin/sh
#
# Upstream: t9500-gitweb-standalone-no-errors.sh
# Requires gitweb/Perl CGI — ported as test_expect_failure stubs.
#

test_description='gitweb as standalone script (basic tests).

This test runs gitweb (git web interface) as CGI script from
commandline, and checks that it would not write any errors
or warnings to log.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- gitweb/Perl CGI not available in grit ---

test_expect_failure 'setup typechange commits' '
	false
'

test_expect_failure 'setup incomplete lines' '
	false
'

test_expect_failure 'commitdiff(1): addition of incomplete line' '
	false
'

test_expect_failure 'commitdiff(1): incomplete line as context line' '
	false
'

test_expect_failure 'commitdiff(1): change incomplete line' '
	false
'

test_expect_failure 'commitdiff(1): removal of incomplete line' '
	false
'

test_expect_failure 'side-by-side: addition of incomplete line' '
	false
'

test_expect_failure 'side-by-side: incomplete line as context line' '
	false
'

test_expect_failure 'side-by-side: changed incomplete line' '
	false
'

test_expect_failure 'side-by-side: removal of incomplete line' '
	false
'

test_expect_failure 'side-by-side: merge commit' '
	false
'

test_expect_failure 'setup' '
	false
'

test_expect_failure '
	cat >>gitweb_config.perl <<-\EOF
	our $highlight_bin = "highlight";
	$feature{"highlight"}{"override"} = 1;
	EOF
' '
	false
'

test_done
