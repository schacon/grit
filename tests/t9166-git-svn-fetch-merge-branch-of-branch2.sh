#!/bin/sh
#
# Upstream: t9166-git-svn-fetch-merge-branch-of-branch2.sh
# Requires Subversion — ported as test_expect_failure stubs.
#

test_description='git svn merge detection'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Subversion not available in grit'
test_done
