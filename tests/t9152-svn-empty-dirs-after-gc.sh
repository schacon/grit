#!/bin/sh
#
# Upstream: t9152-svn-empty-dirs-after-gc.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn creates empty directories, calls git gc, makes sure they are still empty'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'clone' '
	false
'

test_expect_failure 'git svn gc runs' '
	false
'

test_expect_failure 'git svn mkdirs recreates empty directories after git svn gc' '
	false
'

test_done
