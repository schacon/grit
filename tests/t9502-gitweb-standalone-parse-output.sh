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

# --- gitweb/Perl CGI not available in grit ---

test_expect_failure 'snapshot: full sha1' '
	false
'

test_expect_failure 'snapshot: shortened sha1' '
	false
'

test_expect_failure 'snapshot: almost full sha1' '
	false
'

test_expect_failure 'snapshot: HEAD' '
	false
'

test_expect_failure 'snapshot: short branch name (main)' '
	false
'

test_expect_failure 'snapshot: short tag name (first)' '
	false
'

test_expect_failure 'snapshot: full branch name (refs/heads/main)' '
	false
'

test_expect_failure 'snapshot: full tag name (refs/tags/first)' '
	false
'

test_expect_failure 'snapshot: hierarchical branch name (xx/test)' '
	false
'

test_expect_failure 'forks: setup' '
	false
'

test_expect_failure 'forks: not skipped unless "forks" feature enabled' '
	false
'

test_expect_failure 'enable forks feature' '
	false
'

test_expect_failure 'forks: forks skipped if "forks" feature enabled' '
	false
'

test_expect_failure 'forks: "forks" action for forked repository' '
	false
'

test_expect_failure 'forks: can access forked repository' '
	false
'

test_expect_failure 'forks: project_index lists all projects (incl. forks)' '
	false
'

test_expect_failure 'xss checks' '
	false
'

test_expect_failure 'no http-equiv="content-type" in XHTML' '
	false
'

test_expect_failure 'Proper DOCTYPE with entity declarations' '
	false
'

test_done
