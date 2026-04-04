#!/bin/sh
#
# Upstream: t9102-git-svn-deep-rmdir.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn rmdir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'initialize repo' '
	false
'

test_expect_failure 'mirror via git svn' '
	false
'

test_expect_failure 'Try a commit on rmdir' '
	false
'

test_done
