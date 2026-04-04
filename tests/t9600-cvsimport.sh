#!/bin/sh
#
# Upstream: t9600-cvsimport.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'setup cvsroot environment' '
	false
'

test_expect_failure 'setup cvsroot' '
	false
'

test_expect_failure 'setup a cvs module' '
	false
'

test_expect_failure 'import a trivial module' '
	false
'

test_expect_failure 'pack refs' '
	false
'

test_expect_failure 'initial import has correct .git/cvs-revisions' '
	false
'

test_expect_failure 'update cvs module' '
	false
'

test_expect_failure 'update git module' '
	false
'

test_expect_failure 'update has correct .git/cvs-revisions' '
	false
'

test_expect_failure 'update cvs module' '
	false
'

test_expect_failure 'cvsimport.module config works' '
	false
'

test_expect_failure 'second update has correct .git/cvs-revisions' '
	false
'

test_expect_failure 'import from a CVS working tree' '
	false
'

test_expect_failure 'no .git/cvs-revisions created by default' '
	false
'

test_expect_failure 'test entire HEAD' '
	false
'

test_done
