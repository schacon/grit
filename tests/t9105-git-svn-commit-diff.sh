#!/bin/sh
#
# Upstream: t9105-git-svn-commit-diff.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn commit-diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
