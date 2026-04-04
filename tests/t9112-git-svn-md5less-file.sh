#!/bin/sh
#
# Upstream: t9112-git-svn-md5less-file.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='test that git handles an svn repository with missing md5sums'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Subversion not available in grit ---

test_expect_failure 'load svn dumpfile' '
	false
'

test_expect_failure 'initialize git svn' '
	false
'

test_expect_failure 'fetch revisions from svn' '
	false
'

test_done
