#!/bin/sh
#
# Upstream: t9148-git-svn-propset.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn propset tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'setup propset via import' '
	false
'

test_expect_failure 'initialize git svn' '
	false
'

test_expect_failure 'fetch revisions from svn' '
	false
'

test_expect_failure 'add props top level' '
	false
'

test_expect_failure 'add multiple props' '
	false
'

test_expect_failure 'add props subdir' '
	false
'

test_expect_failure 'add props relative' '
	false
'

test_done
