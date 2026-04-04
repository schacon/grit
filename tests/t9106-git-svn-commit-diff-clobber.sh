#!/bin/sh
#
# Upstream: t9106-git-svn-commit-diff-clobber.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn commit-diff clobber'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
