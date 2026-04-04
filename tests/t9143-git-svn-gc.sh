#!/bin/sh
#
# Upstream: t9143-git-svn-gc.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn gc basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup directories and test repo' '
	false
'

test_expect_failure 'checkout working copy from svn' '
	false
'

test_expect_failure 'set some properties to create an unhandled.log file' '
	false
'

test_expect_failure 'Setup repo' '
	false
'

test_expect_failure 'Fetch repo' '
	false
'

test_expect_failure 'make backup copy of unhandled.log' '
	false
'

test_expect_failure 'create leftover index' '
	false
'

test_expect_failure 'git svn gc runs' '
	false
'

test_expect_failure 'git svn index removed' '
	false
'

test_expect_failure 'git svn gc produces a valid gzip file' '
	false
'

test_expect_failure 'git svn gc does not change unhandled.log files' '
	false
'

test_done
